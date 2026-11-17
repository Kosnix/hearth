//! Actions administrateur sans invite UAC à l'usage : une tâche planifiée « privilèges maximum »
//! (créée une seule fois, avec un seul consentement UAC) exécute `hearth-helper.exe elevated`,
//! qui n'accepte qu'une liste fermée d'opérations demandées par un fichier.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const TASK: &str = "HearthElevated";
const NO_WINDOW: u32 = 0x0800_0000;

fn dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth").join("elevated")
}

/// L'autorisation administrateur a-t-elle déjà été accordée ?
pub fn enabled() -> bool {
    Command::new("schtasks").args(["/query", "/tn", TASK]).creation_flags(NO_WINDOW).output().is_ok_and(|o| o.status.success())
}

/// Demande l'autorisation une fois (invite UAC de Windows) et attend la fin de la configuration.
pub fn setup() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("hearth.exe");
    let ps = format!("try {{ $p = Start-Process -FilePath '{}' -ArgumentList 'setup-admin' -Verb RunAs -Wait -PassThru; exit $p.ExitCode }} catch {{ exit 1223 }}", exe.display());
    let st = Command::new("powershell").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps]).creation_flags(NO_WINDOW).status().map_err(|e| e.to_string())?;
    if st.success() && enabled() {
        Ok(())
    } else {
        Err("autorisation administrateur refusée".into())
    }
}

/// Envoie une opération (`winget <id>`) à l'assistant élevé et attend son résultat (bloquant).
pub fn request(op: &str) -> Result<(), String> {
    let d = dir();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(d.join("result.txt"));
    std::fs::write(d.join("request.txt"), op).map_err(|e| e.to_string())?;
    let st = Command::new("schtasks").args(["/run", "/tn", TASK]).creation_flags(NO_WINDOW).output().map_err(|e| e.to_string())?;
    if !st.status.success() {
        return Err("assistant administrateur indisponible".into());
    }
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(1800) {
        std::thread::sleep(Duration::from_secs(2));
        if let Ok(r) = std::fs::read_to_string(d.join("result.txt")) {
            let r = r.trim_start_matches('\u{feff}').trim().to_string();
            return if r == "ok" { Ok(()) } else { Err(r.trim_start_matches("error: ").to_string()) };
        }
    }
    Err("délai dépassé".into())
}
