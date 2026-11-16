//! Superviseur : déclaré comme shell Windows, lance et relance hearth-shell.exe.
//! `hearth install|uninstall` bascule le shell de l'utilisateur courant ; `hearth console` quitte le bureau.
#![windows_subsystem = "windows"]

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, REG_SZ};
use windows::Win32::System::Threading::{CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE};
use windows::Win32::UI::WindowsAndMessaging::GetShellWindow;

const EXIT_DESKTOP: i32 = 10; // cf. hearth-shell
const CONSOLE_EVENT: PCWSTR = w!(r"Local\HearthConsoleMode");
const WINLOGON: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon";
const SHELL_EXE: &str = "hearth-shell.exe";
const MAX_CRASHES: usize = 5; // en CRASH_WINDOW, avant repli sur le bureau
const CRASH_WINDOW: Duration = Duration::from_secs(60);

fn main() {
    match env::args().nth(1).as_deref() {
        Some("install") => cli(install()),
        Some("uninstall") => cli(uninstall()),
        Some("console") => unsafe {
            if let Ok(ev) = OpenEventW(EVENT_MODIFY_STATE, false, CONSOLE_EVENT) {
                let _ = SetEvent(ev);
            }
        },
        _ => supervise(),
    }
}

fn supervise() {
    apply_config();
    let shell = exe_dir().join(SHELL_EXE);
    let event = unsafe { CreateEventW(None, false, false, CONSOLE_EVENT) }.expect("CreateEventW");
    let mut crashes: Vec<Instant> = Vec::new();
    loop {
        let code = Command::new(&shell).status().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        if code == EXIT_DESKTOP {
            log("retour au bureau demandé");
            desktop_until_console(event);
            continue;
        }
        log(&format!("shell terminé (code {code}), relance"));
        crashes.retain(|t| t.elapsed() < CRASH_WINDOW);
        crashes.push(Instant::now());
        if crashes.len() >= MAX_CRASHES {
            log("trop de plantages : repli sur le bureau");
            crashes.clear();
            desktop_until_console(event);
            continue;
        }
        thread::sleep(Duration::from_millis(500 * crashes.len() as u64));
    }
}

/// Ouvre explorer et attend `hearth console`. Si un shell existe déjà (superviseur lancé
/// hors Winlogon, en test), on ne touche pas à explorer.
fn desktop_until_console(event: windows::Win32::Foundation::HANDLE) {
    let own_explorer = unsafe { GetShellWindow() }.is_invalid();
    if own_explorer {
        let _ = Command::new("explorer.exe").spawn();
    }
    unsafe { WaitForSingleObject(event, INFINITE) };
    if own_explorer {
        let _ = Command::new("taskkill").args(["/f", "/im", "explorer.exe"]).status();
    }
    log("retour en mode console");
}

/// Réappliquée à chaque démarrage : les mises à jour Windows peuvent la réactiver.
fn apply_config() {
    for (key, name) in [
        (r"Software\Microsoft\GameBar", "UseNexusForGameBarEnabled"), // bouton guide -> Game Bar
        (r"Software\Microsoft\Windows\CurrentVersion\GameDVR", "AppCaptureEnabled"),
        (r"System\GameConfigStore", "GameDVR_Enabled"),
    ] {
        if let Err(e) = set_dword(key, name, 0) {
            log(&format!("registre {key}\\{name} : {e}"));
        }
    }
}

fn install() -> Result<String, String> {
    let dst = data_dir().join("app");
    fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
    for f in ["hearth.exe", SHELL_EXE] {
        fs::copy(exe_dir().join(f), dst.join(f)).map_err(|e| format!("copie de {f} : {e}"))?;
    }
    let exe = dst.join("hearth.exe");
    set_string(WINLOGON, "Shell", &exe.to_string_lossy()).map_err(|e| e.to_string())?;
    let ps = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut([Environment]::GetFolderPath('Desktop')+'\\Mode console.lnk');\
         $s.TargetPath='{}';$s.Arguments='console';$s.Save()",
        exe.display()
    );
    let _ = Command::new("powershell").args(["-NoProfile", "-Command", &ps]).status();
    Ok(format!("Installé dans {}. Actif à la prochaine ouverture de session.", dst.display()))
}

fn uninstall() -> Result<String, String> {
    let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(WINLOGON), w!("Shell")) };
    r.ok().map_err(|e| e.to_string())?;
    let ps = "Remove-Item ([Environment]::GetFolderPath('Desktop')+'\\Mode console.lnk') -EA SilentlyContinue";
    let _ = Command::new("powershell").args(["-NoProfile", "-Command", ps]).status();
    Ok(format!(
        "Explorateur rétabli à la prochaine ouverture de session. Fichiers restants : {}",
        data_dir().display()
    ))
}

fn cli(r: Result<String, String>) {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
    match r {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("Erreur : {e}");
            std::process::exit(1);
        }
    }
}

fn set_dword(key: &str, name: &str, v: u32) -> windows::core::Result<()> {
    let bytes = v.to_le_bytes();
    set_value(key, name, REG_DWORD.0, &bytes)
}

fn set_string(key: &str, name: &str, v: &str) -> windows::core::Result<()> {
    let wide: Vec<u8> = v.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    set_value(key, name, REG_SZ.0, &wide)
}

fn set_value(key: &str, name: &str, kind: u32, data: &[u8]) -> windows::core::Result<()> {
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            kind,
            Some(data.as_ptr().cast()),
            data.len() as u32,
        )
        .ok()
    }
}

fn exe_dir() -> PathBuf {
    env::current_exe().ok().and_then(|p| p.parent().map(Into::into)).unwrap_or_default()
}

fn data_dir() -> PathBuf {
    PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth")
}

fn log(msg: &str) {
    let _ = fs::create_dir_all(data_dir());
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(data_dir().join("supervisor.log")) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{t} {msg}");
    }
}
