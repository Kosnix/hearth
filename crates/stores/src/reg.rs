//! Lecture du registre (chaînes et sous-clés).

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::System::Registry::*;

pub fn string(root: HKEY, key: &str, name: &str) -> Option<String> {
    let (k, n) = (HSTRING::from(key), HSTRING::from(name));
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(root, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) };
    r.is_ok().then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
}

pub fn subkeys(root: HKEY, key: &str) -> Vec<String> {
    let k = HSTRING::from(key);
    let mut h = HKEY::default();
    if unsafe { RegOpenKeyExW(root, PCWSTR(k.as_ptr()), None, KEY_READ, &mut h) }.is_err() {
        return vec![];
    }
    let mut out = vec![];
    for i in 0.. {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        if unsafe { RegEnumKeyExW(h, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) }.is_err() {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    let _ = unsafe { RegCloseKey(h) };
    out
}

pub fn dword(root: HKEY, key: &str, name: &str) -> Option<u32> {
    let (k, n) = (HSTRING::from(key), HSTRING::from(name));
    let mut v = 0u32;
    let mut len = 4u32;
    let r = unsafe { RegGetValueW(root, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_DWORD, None, Some((&mut v as *mut u32).cast()), Some(&mut len)) };
    r.is_ok().then_some(v)
}
