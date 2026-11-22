//! Programme d'installation de Hearth : un seul exécutable qui embarque le superviseur et la console,
//! demande confirmation, puis exécute `hearth install` (copie dans %LOCALAPPDATA%\Hearth\app, shell Windows, raccourcis).
//! Texte en français ou anglais selon la langue de Windows (ce petit programme n'a pas l'infrastructure de
//! traduction de la console : seulement ces deux langues, l'anglais restant le repli pour les autres).
#![windows_subsystem = "windows"]

use std::path::PathBuf;
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

fn installed_exe() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth").join("app").join("hearth.exe")
}

fn main() {
    // `/S` : mise à jour silencieuse (déjà confirmée dans Hearth) : ni question ni message, puis redémarrage
    // (repart sur une session propre ; Hearth, shell de la session, se relance tout seul à l'ouverture).
    if std::env::args().skip(1).any(|a| a.eq_ignore_ascii_case("/S") || a.eq_ignore_ascii_case("--silent")) {
        if run().is_ok() {
            let _ = Command::new("shutdown").args(["/r", "/t", "5"]).spawn();
        }
        return;
    }
    let fr = french();
    let ask = if fr {
        "Installer Hearth ?\n\nHearth remplace le bureau Windows par une interface de console de jeu pour ce compte (effectif à la prochaine connexion ; vous pouvez aussi la lancer tout de suite sans vous déconnecter).\n\nPour annuler : utilisez « Hearth (bureau) » ou lancez « hearth.exe uninstall »."
    } else {
        "Install Hearth?\n\nHearth replaces the Windows desktop with a game-console interface for this user account (effective at next sign-in; you can also launch it right away without signing out).\n\nTo undo: use \"Hearth (desktop)\" or run \"hearth.exe uninstall\"."
    };
    if !message(ask, MB_YESNO | MB_ICONQUESTION) {
        return;
    }
    match run() {
        Ok(()) => {
            // Lancée tout de suite, la console tourne par-dessus le bureau Windows sans fermer la session ;
            // elle deviendra le shell au prochain démarrage de session grâce à la clé Winlogon déjà posée.
            let launch = if fr { "Hearth est installé et démarrera automatiquement à la prochaine connexion.\n\nLe lancer maintenant ?" } else { "Hearth is installed and will start automatically at next sign-in.\n\nLaunch it now?" };
            if message(launch, MB_YESNO | MB_ICONINFORMATION) {
                let _ = Command::new(installed_exe()).spawn();
            }
        }
        Err(e) => {
            let fail = if fr { format!("Installation échouée : {e}") } else { format!("Installation failed: {e}") };
            message(&fail, MB_OK | MB_ICONERROR);
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
