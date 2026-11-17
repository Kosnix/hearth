//! Client HTTPS minimal sur WinHTTP (certificats et proxy du système, aucune dépendance TLS).

use std::ffi::c_void;

use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// GET bloquant (à appeler hors du thread UI). `headers` : lignes « Nom: valeur ».
pub fn get(url: &str, headers: &[(&str, &str)]) -> Option<Vec<u8>> {
    request("GET", url, headers, None)
}

pub fn post_json(url: &str, headers: &[(&str, &str)], body: &str) -> Option<serde_json::Value> {
    let mut h = headers.to_vec();
    h.push(("Content-Type", "application/json"));
    serde_json::from_slice(&request("POST", url, &h, Some(body.as_bytes()))?).ok()
}

fn request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Option<Vec<u8>> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
    unsafe {
        let session = WinHttpOpen(PCWSTR(wide("Hearth").as_ptr()), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
        if session.is_null() {
            return None;
        }
        let _ = WinHttpSetTimeouts(session, 5000, 5000, 10000, 20000);
        let out = (|| {
            let conn = WinHttpConnect(session, PCWSTR(wide(host).as_ptr()), 443, 0);
            if conn.is_null() {
                return None;
            }
            let req = WinHttpOpenRequest(conn, PCWSTR(wide(method).as_ptr()), PCWSTR(wide(&path).as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE);
            let body = (|| {
                if req.is_null() {
                    return None;
                }
                let hdr: Vec<u16> = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect::<String>().encode_utf16().collect();
                let len = body.map_or(0, |b| b.len() as u32);
                WinHttpSendRequest(req, if hdr.is_empty() { None } else { Some(&hdr) }, body.map(|b| b.as_ptr().cast::<c_void>()), len, len, 0).ok()?;
                WinHttpReceiveResponse(req, std::ptr::null_mut()).ok()?;
                // Code HTTP : on n'accepte que 200.
                let mut status = 0u32;
                let mut len = size_of::<u32>() as u32;
                WinHttpQueryHeaders(req, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut status as *mut u32 as *mut c_void), &mut len, std::ptr::null_mut()).ok()?;
                if status != 200 {
                    return None;
                }
                let mut data = Vec::new();
                loop {
                    let mut avail = 0u32;
                    WinHttpQueryDataAvailable(req, &mut avail).ok()?;
                    if avail == 0 {
                        break;
                    }
                    let mut buf = vec![0u8; avail as usize];
                    let mut read = 0u32;
                    WinHttpReadData(req, buf.as_mut_ptr().cast(), avail, &mut read).ok()?;
                    data.extend_from_slice(&buf[..read as usize]);
                }
                Some(data)
            })();
            if !req.is_null() {
                let _ = WinHttpCloseHandle(req);
            }
            let _ = WinHttpCloseHandle(conn);
            body
        })();
        let _ = WinHttpCloseHandle(session);
        out
    }
}

pub fn get_json(url: &str, headers: &[(&str, &str)]) -> Option<serde_json::Value> {
    serde_json::from_slice(&get(url, headers)?).ok()
}
