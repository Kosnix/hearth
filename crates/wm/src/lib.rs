//! Fenêtres des jeux : lancement dans un Job Object, plein écran sans bordure, premier plan.
//! Aucune injection : uniquement des API Win32 publiques appliquées aux fenêtres d'autres processus.

pub mod compat;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromWindow, DEVMODEW, ENUM_CURRENT_SETTINGS, ENUM_REGISTRY_SETTINGS,
    MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
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
        // Seules la barre de titre et la bordure comptent : un jeu plein écran garde souvent
        // WS_MINIMIZEBOX/WS_SYSMENU sur une fenêtre WS_POPUP, et il ne faut pas y toucher.
        let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
        if style & (WS_CAPTION | WS_THICKFRAME).0 == 0 {
            return;
        }
        let frame = (WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0;
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let ex_frame = (WS_EX_DLGMODALFRAME | WS_EX_CLIENTEDGE | WS_EX_STATICEDGE | WS_EX_WINDOWEDGE).0;
        SetWindowLongPtrW(h, GWL_STYLE, (style & !frame) as isize);
        SetWindowLongPtrW(h, GWL_EXSTYLE, (ex & !ex_frame) as isize);
        fit(h, SWP_FRAMECHANGED);
    }
}

/// Cale la fenêtre sur tout son écran (sans toucher à son style).
pub fn fit_to_monitor(h: HWND) {
    fit(h, SET_WINDOW_POS_FLAGS(0));
}

fn fit(h: HWND, extra: SET_WINDOW_POS_FLAGS) {
    let m = monitor_rect(h);
    unsafe {
        let _ = SetWindowPos(
            h,
            None,
            m.left,
            m.top,
            m.right - m.left,
            m.bottom - m.top,
            extra | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Le jeu a pris l'écran pour lui : Direct3D exclusif, ou résolution de l'écran changée
/// (OpenGL/SDL « plein écran » classique). Ni modification ni overlay dans ce cas.
pub fn exclusive_fullscreen() -> bool {
    unsafe { SHQueryUserNotificationState() }.is_ok_and(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN) || display_mode_changed()
}

fn display_mode_changed() -> bool {
    let mode = |which| {
        let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
        unsafe { EnumDisplaySettingsW(PCWSTR::null(), which, &mut dm) }.as_bool().then_some((dm.dmPelsWidth, dm.dmPelsHeight))
    };
    match (mode(ENUM_CURRENT_SETTINGS), mode(ENUM_REGISTRY_SETTINGS)) {
        (Some(cur), Some(reg)) => cur != reg,
        _ => false,
    }
}

pub fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

/// Fenêtre de notre propre processus portant ce titre.
pub fn find_window(title: &str) -> Option<HWND> {
    let me = std::process::id();
    top_windows().into_iter().find(|&h| {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        let mut buf = [0u16; 128];
        let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
        pid == me && String::from_utf16_lossy(&buf[..n]) == title
    })
}

/// Overlay : style d'une fenêtre qui ne prend jamais le focus et n'apparaît pas dans Alt+Tab.
pub fn make_overlay(h: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(h, GWL_EXSTYLE, (ex | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST).0) as isize);
    }
}

/// Affiche l'overlay par-dessus tout sans voler le focus au jeu.
pub fn show_overlay(h: HWND, x: i32, y: i32, w: i32, height: i32) {
    unsafe {
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, y, w, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

pub fn hide(h: HWND) {
    unsafe {
        let _ = ShowWindow(h, SW_HIDE);
    }
}

pub fn is_visible(h: HWND) -> bool {
    unsafe { IsWindowVisible(h).as_bool() }
}

/// Rectangle de l'écran principal (pixels physiques).
pub fn primary_monitor() -> (i32, i32, i32, i32) {
    let m = monitor_rect(unsafe { GetDesktopWindow() });
    (m.left, m.top, m.right - m.left, m.bottom - m.top)
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

/// Nom de l'exécutable (minuscules) du processus qui possède la fenêtre.
fn exe_name(h: HWND) -> String {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    let Ok(p) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return String::new() };
    let mut buf = [0u16; 512];
    let mut n = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n) }.is_ok();
    let _ = unsafe { CloseHandle(p) };
    if !ok {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize]).rsplit('\\').next().unwrap_or("").to_lowercase()
}

/// Réduit sans prendre le focus les fenêtres visibles des processus donnés (ex. client Steam). Renvoie le nombre réduit.
pub fn minimize_windows_of(exes: &[&str]) -> usize {
    let mut count = 0;
    for h in top_windows() {
        unsafe {
            if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
                continue;
            }
        }
        if exes.contains(&exe_name(h).as_str()) {
            unsafe {
                let _ = ShowWindow(h, SW_SHOWMINNOACTIVE);
            }
            count += 1;
        }
    }
    count
}
