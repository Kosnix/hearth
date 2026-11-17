//! SteamCMD : installation et mise à jour de jeux Steam sans le client (identifiant + mot de passe saisis dans Hearth).
//! Les jeux vont dans `%LOCALAPPDATA%\Hearth\steamcmd\steamapps\common`.

use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const NO_WINDOW: u32 = 0x0800_0000;
const ZIP: &str = "https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip";

pub fn dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth").join("steamcmd")
}

fn exe() -> PathBuf {
    dir().join("steamcmd.exe")
}

/// Résultat d'une tentative de connexion.
pub enum Login {
    Ok,
    /// Code Steam Guard (e-mail ou application) à fournir.
    NeedCode,
    Failed(String),
}

fn run(args: &[&str]) -> Result<String, String> {
    let out = Command::new(exe()).args(args).current_dir(dir()).creation_flags(NO_WINDOW).stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Télécharge et prépare SteamCMD au besoin (première utilisation : mise à jour automatique de l'outil).
pub fn ensure() -> Result<(), String> {
    if exe().exists() && dir().join("package").exists() {
        return Ok(());
    }
    let d = dir();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    if !exe().exists() {
        let zip = crate::download(ZIP).ok_or("téléchargement de SteamCMD impossible")?;
        let zp = d.join("steamcmd.zip");
        std::fs::write(&zp, zip).map_err(|e| e.to_string())?;
        let ps = format!("Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force", zp.display(), d.display());
        let st = Command::new("powershell").args(["-NoProfile", "-Command", &ps]).creation_flags(NO_WINDOW).status().map_err(|e| e.to_string())?;
        if !st.success() || !exe().exists() {
            return Err("décompression de SteamCMD impossible".into());
        }
    }
    // Premier lancement : SteamCMD se met à jour tout seul (code de sortie variable, sans importance).
    let _ = run(&["+quit"]);
    Ok(())
}

fn failure(out: &str) -> String {
    let l = out.to_lowercase();
    if l.contains("invalid password") || l.contains("invalidpassword") {
        "mot de passe incorrect".into()
    } else if l.contains("no subscription") {
        "jeu non possédé par ce compte".into()
    } else if l.contains("rate limit") {
        "trop de tentatives, réessayez plus tard".into()
    } else if l.contains("two-factor") || l.contains("steam guard") || l.contains("twofactor") {
        "code Steam Guard requis".into()
    } else if l.contains("not logged on") || l.contains("login failure") || l.contains("no cached credentials") {
        "connexion requise".into()
    } else if l.contains("no space") || l.contains("disk write failure") {
        "espace disque insuffisant".into()
    } else {
        out.lines().rev().find(|x| x.trim().len() > 3).unwrap_or("échec").trim().to_string()
    }
}

/// Connexion (SteamCMD garde ensuite la session : les appels suivants n'ont besoin que de l'identifiant).
pub fn login(user: &str, pass: &str, code: Option<&str>) -> Login {
    if let Err(e) = ensure() {
        return Login::Failed(e);
    }
    let mut args = vec!["+login", user, pass];
    if let Some(c) = code {
        args.push(c);
    }
    args.push("+quit");
    match run(&args) {
        Ok(out) if out.contains("Logged in OK") => Login::Ok,
        Ok(out) => {
            let l = out.to_lowercase();
            if code.is_none() && (l.contains("steam guard") || l.contains("two-factor") || l.contains("twofactor")) {
                Login::NeedCode
            } else {
                Login::Failed(failure(&out))
            }
        }
        Err(e) => Login::Failed(e),
    }
}

/// Session mémorisée par SteamCMD pour cet identifiant ?
pub fn session_ok(user: &str) -> bool {
    run(&["+login", user, "+quit"]).is_ok_and(|o| o.contains("Logged in OK"))
}

/// Installe ou met à jour un jeu (bloquant, long).
pub fn install(user: &str, appid: &str) -> Result<(), String> {
    ensure()?;
    let out = run(&["+@NoPromptForPassword", "1", "+login", user, "+app_update", appid, "validate", "+quit"])?;
    if out.contains("fully installed") {
        Ok(())
    } else {
        Err(failure(&out))
    }
}

/// Numéro de build public le plus récent de chaque jeu.
pub fn latest_builds(user: &str, ids: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if ids.is_empty() || ensure().is_err() {
        return map;
    }
    let mut args: Vec<&str> = vec!["+@NoPromptForPassword", "1", "+login", user, "+app_info_update", "1"];
    for id in ids {
        args.push("+app_info_print");
        args.push(id);
    }
    args.push("+quit");
    let Ok(out) = run(&args) else { return map };
    for block in out.split("AppID : ").skip(1) {
        let id: String = block.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Some(b) = block.find("\"branches\"") else { continue };
        let rest = &block[b..];
        let Some(p) = rest.find("\"public\"") else { continue };
        let rest = &rest[p..];
        let Some(k) = rest.find("\"buildid\"") else { continue };
        let value: Vec<&str> = rest[k + 9..].lines().next().unwrap_or("").split('"').collect();
        if let Some(v) = value.get(1).filter(|v| !v.is_empty()) {
            map.insert(id, v.to_string());
        }
    }
    map
}

fn manifest(appid: &str) -> Option<String> {
    std::fs::read_to_string(dir().join("steamapps").join(format!("appmanifest_{appid}.acf"))).ok()
}

fn field(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let p: Vec<&str> = l.split('"').collect();
        (p.len() >= 5 && p[1].eq_ignore_ascii_case(key)).then(|| p[3].to_string())
    })
}

/// Build installé localement.
pub fn local_build(appid: &str) -> Option<String> {
    field(&manifest(appid)?, "buildid")
}

/// Identifiants des jeux installés par SteamCMD.
pub fn installed_ids() -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir().join("steamapps")) else { return vec![] };
    rd.flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.strip_prefix("appmanifest_")?.strip_suffix(".acf").map(String::from)
        })
        .collect()
}

/// Supprime un jeu installé par SteamCMD.
pub fn uninstall(appid: &str) -> bool {
    let Some(m) = manifest(appid) else { return false };
    if let Some(d) = field(&m, "installdir").filter(|d| !d.is_empty() && !d.contains(['\\', '/', ':'])) {
        let _ = std::fs::remove_dir_all(dir().join("steamapps").join("common").join(d));
    }
    std::fs::remove_file(dir().join("steamapps").join(format!("appmanifest_{appid}.acf"))).is_ok()
}

/// Exécutable principal d'un dossier de jeu : le plus gros .exe qui n'est pas un installeur ou un utilitaire.
pub fn main_exe(root: &Path) -> Option<PathBuf> {
    fn walk(d: &Path, depth: u32, best: &mut Option<(u64, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if p.is_dir() {
                if depth < 3 && !["redist", "_commonredist", "directx", "crashreport"].iter().any(|b| name.contains(b)) {
                    walk(&p, depth + 1, best);
                }
            } else if name.ends_with(".exe") && !["unins", "crash", "redist", "setup", "vc_", "dxsetup", "dotnet", "easyanticheat", "uninstall", "launcher_helper"].iter().any(|b| name.contains(b)) {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                if best.as_ref().is_none_or(|(s, _)| size > *s) {
                    *best = Some((size, p));
                }
            }
        }
    }
    let mut best = None;
    walk(root, 0, &mut best);
    best.map(|(_, p)| p)
}
