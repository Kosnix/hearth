//! Epic Games sans le lanceur : Legendary (outil libre, github.com/legendary-gl/legendary) pour installer et lancer.
//! La session vient de Hearth (code d'échange) : aucune saisie au clavier.

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const NO_WINDOW: u32 = 0x0800_0000;
const URL: &str = "https://github.com/legendary-gl/legendary/releases/download/0.21.1/legendary_windows_x64.exe";

pub fn dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth").join("legendary")
}

pub fn config() -> PathBuf {
    // Une session Legendary par profil (le profil « default » garde l'ancien dossier).
    match std::env::var("HEARTH_PROFILE_ID").ok().filter(|p| p != "default" && !p.is_empty()) {
        Some(p) => dir().join(format!("config-{p}")),
        None => dir().join("config"),
    }
}

pub fn exe() -> PathBuf {
    dir().join("legendary.exe")
}

fn cmd(args: &[&str]) -> Command {
    let mut c = Command::new(exe());
    c.args(args).env("LEGENDARY_CONFIG_PATH", config()).current_dir(dir()).creation_flags(NO_WINDOW);
    c
}

/// Télécharge Legendary au besoin.
pub fn ensure() -> Result<(), String> {
    if exe().exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let data = crate::download(URL).ok_or("téléchargement de Legendary impossible")?;
    if !data.starts_with(b"MZ") {
        return Err("fichier Legendary invalide".into());
    }
    std::fs::write(exe(), data).map_err(|e| e.to_string())
}

pub fn logged_in() -> bool {
    config().join("user.json").exists()
}

/// Ouvre la session Legendary avec la session Epic de Hearth.
pub fn auth() -> Result<(), String> {
    ensure()?;
    if logged_in() {
        return Ok(());
    }
    let code = crate::account::epic_exchange_code().ok_or("session Epic expirée : reconnectez-vous dans Réglages > Comptes")?;
    let out = cmd(&["auth", "--token", &code]).output().map_err(|e| e.to_string())?;
    if logged_in() {
        Ok(())
    } else {
        let t = String::from_utf8_lossy(&out.stderr).into_owned();
        Err(t.lines().rev().find(|l| l.trim().len() > 3).unwrap_or("connexion refusée").trim().to_string())
    }
}

/// Déconnexion de Legendary (à la déconnexion du compte Epic dans Hearth).
pub fn logout() {
    if exe().exists() {
        let _ = cmd(&["auth", "--delete"]).output();
    }
}

fn games_dir() -> PathBuf {
    dir().join("games")
}

/// Installe un jeu (bloquant, long). `progress(pourcentage, octets/s, secondes restantes)`.
pub fn install(app: &str, pid: &AtomicU32, progress: impl Fn(f32, f64, Option<u64>)) -> Result<(), String> {
    auth()?;
    let base = games_dir();
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let mut c = cmd(&["install", app, "--base-path", &base.to_string_lossy(), "-y", "--skip-sdl"]);
    c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).stdin(std::process::Stdio::null());
    let mut child = c.spawn().map_err(|e| e.to_string())?;
    pid.store(child.id(), Ordering::SeqCst);
    let text = Arc::new(Mutex::new(String::new()));
    let pipes: Vec<Box<dyn Read + Send>> = [
        child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
        child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .collect();
    for mut pipe in pipes {
        let t = text.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if let Ok(mut s) = t.lock() {
                    s.push_str(&String::from_utf8_lossy(&buf[..n]));
                    // Ne garde que la fin : seul le dernier point d'avancement compte.
                    if s.len() > 20_000 {
                        let mut cut = s.len() - 10_000;
                        while !s.is_char_boundary(cut) {
                            cut += 1;
                        }
                        s.drain(..cut);
                    }
                }
            }
        });
    }
    loop {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let s = text.lock().map(|s| s.clone()).unwrap_or_default();
        if let Some(i) = s.rfind("Progress: ") {
            let line = s[i..].lines().next().unwrap_or("");
            let pct: f32 = line["Progress: ".len()..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect::<String>().parse().unwrap_or(0.0);
            let eta = line.find("ETA: ").and_then(|k| {
                let t: Vec<u64> = line[k + 5..].chars().take_while(|c| c.is_ascii_digit() || *c == ':').collect::<String>().split(':').filter_map(|x| x.parse().ok()).collect();
                (t.len() == 3).then(|| t[0] * 3600 + t[1] * 60 + t[2])
            });
            let speed = s
                .rfind("Download\t- ")
                .or_else(|| s.rfind("Download - "))
                .and_then(|k| s[k..].split_whitespace().find_map(|w| w.parse::<f64>().ok()))
                .map_or(0.0, |m| m * 1_048_576.0);
            progress(pct, speed, eta);
        }
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
    }
    pid.store(0, Ordering::SeqCst);
    if installed_json().get(app).is_some() {
        Ok(())
    } else {
        let t = text.lock().map(|s| s.clone()).unwrap_or_default();
        Err(t.lines().rev().find(|l| l.trim().len() > 3).unwrap_or("échec").trim().to_string())
    }
}

pub fn uninstall(app: &str) -> bool {
    ensure().is_ok() && cmd(&["uninstall", app, "-y"]).output().is_ok_and(|o| o.status.success())
}

pub fn cancel(pid: u32) {
    if pid != 0 {
        let _ = Command::new("taskkill").args(["/pid", &pid.to_string(), "/t", "/f"]).creation_flags(NO_WINDOW).status();
    }
}

fn installed_json() -> serde_json::Value {
    std::fs::read_to_string(config().join("installed.json")).ok().and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()).unwrap_or_default()
}

/// Jeux installés par Legendary, avec visuels tirés de ses métadonnées.
pub fn installed() -> Vec<crate::Game> {
    let j = installed_json();
    let Some(map) = j.as_object() else { return vec![] };
    let mut out = vec![];
    for (app, g) in map {
        let title = g["title"].as_str().unwrap_or(app).to_string();
        let meta: serde_json::Value = std::fs::read_to_string(config().join("metadata").join(format!("{app}.json"))).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let pick = |types: &[&str]| -> Vec<String> {
            types.iter().filter_map(|t| meta["metadata"]["keyImages"].as_array()?.iter().find(|k| k["type"] == *t)?["url"].as_str().map(String::from)).collect()
        };
        let art = crate::Art { cover: pick(&["DieselGameBoxTall", "OfferImageTall", "Thumbnail"]), hero: pick(&["DieselGameBox", "OfferImageWide", "DieselStoreFrontWide"]), logo: pick(&["DieselGameBoxLogo"]) };
        out.push(crate::Game {
            id: format!("epic-{app}"),
            store: "Epic Games",
            title,
            launch: crate::Launch::Exe { path: exe().to_string_lossy().into_owned(), args: vec!["launch".into(), app.clone()] },
            art,
        });
    }
    out
}
