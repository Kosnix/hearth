//! Fenêtres des jeux : lancement dans un Job Object, plein écran sans bordure, premier plan.
//! Aucune injection : uniquement des API Win32 publiques appliquées aux fenêtres d'autres processus.

pub mod compat;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::JobObjects::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::*;

pub use windows::Win32::Foundation::HWND;

/// Taille minimale d'une fenêtre « principale » : en dessous (splash, dialogues de launcher), on n'y touche pas.
const MIN_W: i32 = 640;
const MIN_H: i32 = 480;

/// Un jeu lancé par le shell : tous ses processus enfants sont dans le même job.
pub struct Session {
    job: HANDLE,
    dir: PathBuf,
}

impl Session {
    pub fn launch(exe: &Path, args: &[String]) -> windows::core::Result<Session> {
        let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut line = quote(&exe.to_string_lossy());
        for a in args {
            line.push(' ');
            line.push_str(&quote(a));
        }
        let mut line: Vec<u16> = line.encode_utf16().chain([0]).collect();
        let cwd = HSTRING::from(dir.as_os_str());
        let cwd = if dir.as_os_str().is_empty() { PCWSTR::null() } else { PCWSTR(cwd.as_ptr()) };
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null())?;
            let si = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, ..Default::default() };
            let mut pi = PROCESS_INFORMATION::default();
            // Suspendu le temps de l'ajouter au job, pour que ses enfants y soient aussi.
            CreateProcessW(PCWSTR::null(), Some(PWSTR(line.as_mut_ptr())), None, None, false, CREATE_SUSPENDED, None, cwd, &si, &mut pi)?;
            let _ = AssignProcessToJobObject(job, pi.hProcess);
            ResumeThread(pi.hThread);
            let _ = CloseHandle(pi.hThread);
            let _ = CloseHandle(pi.hProcess);
            Ok(Session { job, dir })
        }
    }

    pub fn alive(&self) -> bool {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let ok = unsafe {
            QueryInformationJobObject(
                Some(self.job),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as _,
                size_of_val(&info) as u32,
                None,
            )
        };
        ok.is_ok() && info.ActiveProcesses > 0
    }

    /// Fenêtre d'un processus du job, ou d'un exécutable du dossier du jeu (relancé par un launcher).
    pub fn owns(&self, hwnd: HWND) -> bool {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let Ok(p) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return false };
        let mut in_job = Default::default();
        let _ = unsafe { IsProcessInJob(p, Some(self.job), &mut in_job) };
        let owned = in_job.as_bool()
            || (!self.dir.as_os_str().is_empty() && image_path(p).is_some_and(|x| x.starts_with(&self.dir)));
        unsafe {
            let _ = CloseHandle(p);
        }
        owned
    }

    /// Fenêtres principales du jeu, la plus haute dans l'ordre Z en premier.
    pub fn windows(&self) -> Vec<HWND> {
        top_windows().into_iter().filter(|h| is_main_window(*h) && self.owns(*h)).collect()
    }

    pub fn main_window(&self) -> Option<HWND> {
        self.windows().into_iter().next()
    }

    /// Demande polie (WM_CLOSE) : le jeu peut afficher sa propre confirmation.
    pub fn close(&self) {
        for h in self.windows() {
            unsafe {
                let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn kill(&self) {
        unsafe {
            let _ = TerminateJobObject(self.job, 1);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.job);
        }
    }
}

fn quote(s: &str) -> String {
    if !s.is_empty() && !s.contains([' ', '\t', '"']) {
        return s.to_string();
    }
    format!("\"{}\"", s.replace('"', "\\\""))
}

fn image_path(p: HANDLE) -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
}

fn top_windows() -> Vec<HWND> {
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> windows::core::BOOL {
        (*(l.0 as *mut Vec<HWND>)).push(h);
        true.into()
    }
    let mut v: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

/// Visible, sans propriétaire, pas une fenêtre d'outil.
pub fn is_main_window(h: HWND) -> bool {
    unsafe {
        IsWindowVisible(h).as_bool()
            && GetWindow(h, GW_OWNER).is_err()
            && GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 == 0
    }
}

fn monitor_rect(h: HWND) -> RECT {
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mut mi);
    }
    mi.rcMonitor
}

/// Plein écran fenêtré sans bordure sur l'écran de la fenêtre. Sans effet si c'est déjà le cas.
pub fn make_borderless(h: HWND) {
    unsafe {
        let mut r = RECT::default();
        let _ = GetWindowRect(h, &mut r);
        if r.right - r.left < MIN_W || r.bottom - r.top < MIN_H {
            return;
        }
        let m = monitor_rect(h);
        let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
        let frame = (WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0;
        let same = (r.left, r.top, r.right, r.bottom) == (m.left, m.top, m.right, m.bottom);
        if style & frame == 0 && same {
            return;
        }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let ex_frame = (WS_EX_DLGMODALFRAME | WS_EX_CLIENTEDGE | WS_EX_STATICEDGE | WS_EX_WINDOWEDGE).0;
        SetWindowLongPtrW(h, GWL_STYLE, (style & !frame) as isize);
        SetWindowLongPtrW(h, GWL_EXSTYLE, (ex & !ex_frame) as isize);
        let _ = SetWindowPos(
            h,
            None,
            m.left,
            m.top,
            m.right - m.left,
            m.bottom - m.top,
            SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOOWNERZORDER,
        );
    }
}

/// Un jeu Direct3D est en plein écran exclusif : ni modification ni overlay possibles.
pub fn exclusive_fullscreen() -> bool {
    unsafe { SHQueryUserNotificationState() }.is_ok_and(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN)
}

pub fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

// ponytail: recherche par titre exact ; suffisant tant que le shell n'a que deux fenêtres.
pub fn find_window(title: &str) -> Option<HWND> {
    unsafe { FindWindowW(PCWSTR::null(), &HSTRING::from(title)) }.ok()
}

/// Windows refuse SetForegroundWindow à un processus en arrière-plan, sauf s'il vient
/// d'injecter une entrée : on maintient Alt le temps de l'appel.
pub fn focus(h: HWND) {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_MENU, dwFlags: flags, ..Default::default() } },
    };
    unsafe {
        SendInput(&[key(KEYBD_EVENT_FLAGS(0))], size_of::<INPUT>() as i32);
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h);
        SendInput(&[key(KEYEVENTF_KEYUP)], size_of::<INPUT>() as i32);
    }
}

pub fn minimize(h: HWND) {
    unsafe {
        let _ = ShowWindow(h, SW_MINIMIZE);
    }
}

thread_local! {
    static ON_WINDOW: RefCell<Option<Box<dyn Fn(HWND)>>> = RefCell::new(None);
}

/// Appelle `f` pour chaque fenêtre principale d'un autre processus qui apparaît ou passe au premier plan.
/// À appeler depuis le thread de l'UI (sa boucle de messages délivre les événements).
pub fn watch(f: impl Fn(HWND) + 'static) {
    ON_WINDOW.with(|c| *c.borrow_mut() = Some(Box::new(f)));
    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    unsafe {
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_event), 0, 0, flags);
        SetWinEventHook(EVENT_OBJECT_SHOW, EVENT_OBJECT_SHOW, None, Some(on_event), 0, 0, flags);
    }
}

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, _: u32, h: HWND, obj: i32, child: i32, _: u32, _: u32) {
    if obj != OBJID_WINDOW.0 || child != 0 || !is_main_window(h) {
        return;
    }
    ON_WINDOW.with(|c| {
        if let Ok(c) = c.try_borrow() {
            if let Some(f) = c.as_ref() {
                f(h);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn quoting() {
        assert_eq!(super::quote("a.exe"), "a.exe");
        assert_eq!(super::quote(r"C:\Program Files\x.exe"), r#""C:\Program Files\x.exe""#);
        assert_eq!(super::quote(""), r#""""#);
    }
}
