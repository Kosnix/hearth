//! Programme d'installation de Hearth : un seul exécutable qui embarque le superviseur et la console,
//! demande confirmation, puis exécute `hearth install` (copie dans %LOCALAPPDATA%\Hearth\app, shell Windows, raccourcis).
#![windows_subsystem = "windows"]

use std::process::Command;

use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONINFORMATION, MB_ICONQUESTION, MB_YESNO, MB_OK, MB_ICONERROR};

const SUPERVISOR: &[u8] = include_bytes!("../../../target/release/hearth.exe");
const SHELL: &[u8] = include_bytes!("../../../target/release/hearth-shell.exe");

fn french() -> bool {
    let mut buf = [0u16; 85];
    let n = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n.saturating_sub(1)]).to_lowercase().starts_with("fr")
}

fn message(text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) -> bool {
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("Hearth"), flags) == IDYES }
}

fn main() {
    let fr = french();
    let ask = if fr {
        "Installer Hearth ?\n\nHearth remplace l'explorateur Windows par une interface de console de jeu pour ce compte utilisateur (effet à la prochaine ouverture de session).\n\nPour revenir en arrière : « Hearth (bureau) » ou la commande « hearth.exe uninstall »."
    } else {
        "Install Hearth?\n\nHearth replaces the Windows desktop with a game-console interface for this user account (effective at next sign-in).\n\nTo undo: use \"Hearth (desktop)\" or run \"hearth.exe uninstall\"."
    };
    if !message(ask, MB_YESNO | MB_ICONQUESTION) {
        return;
    }
    match run() {
        Ok(()) => {
            let done = if fr { "Hearth est installé. Fermez votre session puis rouvrez-la pour démarrer la console." } else { "Hearth is installed. Sign out and back in to start the console." };
            message(done, MB_OK | MB_ICONINFORMATION);
        }
        Err(e) => {
            message(&format!("{} {e}", if fr { "Échec de l'installation :" } else { "Installation failed:" }), MB_OK | MB_ICONERROR);
        }
    }
}

fn run() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("hearth-setup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("hearth.exe"), SUPERVISOR).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("hearth-shell.exe"), SHELL).map_err(|e| e.to_string())?;
    // Une ancienne version en cours d'exécution verrouille ses fichiers.
    for p in ["hearth.exe", "hearth-shell.exe"] {
        let _ = Command::new("taskkill").args(["/F", "/IM", p]).output();
    }
    std::thread::sleep(std::time::Duration::from_millis(800));
    let st = Command::new(dir.join("hearth.exe")).arg("install").status().map_err(|e| e.to_string());
    let _ = std::fs::remove_dir_all(&dir);
    if st?.success() { Ok(()) } else { Err("hearth.exe install".into()) }
}
