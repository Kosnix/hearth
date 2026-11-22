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

/// GET bloquant avec progression (octets reçus, total si connu via `Content-Length`) : pour un gros
/// téléchargement (installateur) qu'on veut montrer à l'écran pendant qu'il avance.
pub fn get_with_progress(url: &str, headers: &[(&str, &str)], on_progress: impl FnMut(u64, Option<u64>)) -> Option<Vec<u8>> {
    let mut on_progress = on_progress;
    let (status, data) = request_raw_progress("GET", url, headers, None, Some(&mut on_progress))?;
    (status == 200).then_some(data)
}

pub fn post_json(url: &str, headers: &[(&str, &str)], body: &str) -> Option<serde_json::Value> {
    let mut h = headers.to_vec();
    h.push(("Content-Type", "application/json"));
    serde_json::from_slice(&request("POST", url, &h, Some(body.as_bytes()))?).ok()
}

fn request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Option<Vec<u8>> {
    let (status, data) = request_raw(method, url, headers, body)?;
    (status == 200).then_some(data)
}

/// Requête avec code HTTP (les flux de connexion répondent 400 tant que l'utilisateur n'a pas validé).
fn request_raw(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Option<(u32, Vec<u8>)> {
    request_raw_progress(method, url, headers, body, None)
}

fn request_raw_progress(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    mut progress: Option<&mut dyn FnMut(u64, Option<u64>)>,
) -> Option<(u32, Vec<u8>)> {
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
                let mut total = 0u32;
                let mut tlen = size_of::<u32>() as u32;
                let total = WinHttpQueryHeaders(
                    req,
                    WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER,
                    PCWSTR::null(),
                    Some(&mut total as *mut u32 as *mut c_void),
                    &mut tlen,
                    std::ptr::null_mut(),
                )
                .is_ok()
                .then_some(total as u64);
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
                    if let Some(cb) = progress.as_deref_mut() {
                        cb(data.len() as u64, total);
                    }
                }
                Some((status, data))
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


/// POST de formulaire, renvoie le code HTTP et le JSON (même en cas d'erreur).
pub fn post_form_status(url: &str, headers: &[(&str, &str)], form: &str) -> Option<(u32, serde_json::Value)> {
    let mut h = headers.to_vec();
    h.push(("Content-Type", "application/x-www-form-urlencoded"));
    let (status, data) = request_raw("POST", url, &h, Some(form.as_bytes()))?;
    Some((status, serde_json::from_slice(&data).unwrap_or(serde_json::Value::Null)))
}

/// GET avec code HTTP et JSON.
pub fn get_status(url: &str, headers: &[(&str, &str)]) -> Option<(u32, serde_json::Value)> {
    let (status, data) = request_raw("GET", url, headers, None)?;
    Some((status, serde_json::from_slice(&data).unwrap_or(serde_json::Value::Null)))
}
