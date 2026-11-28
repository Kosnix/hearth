//! SteamCMD : installation et mise à jour de jeux Steam sans le client (identifiant + mot de passe saisis dans Hearth).
//! Les jeux vont dans `%LOCALAPPDATA%\Hearth\steamcmd\steamapps\common`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const NO_WINDOW: u32 = 0x0800_0000;
const ZIP: &str = "https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip";

/// Un seul SteamCMD à la fois sur ce dossier : la recherche de mises à jour en fond (`latest_builds`,
/// toutes les 6 h) et un téléchargement actif (`install`) sont deux threads indépendants qui, sans ce
/// verrou, pouvaient lancer `steamcmd.exe` en parallèle sur la même config. Un verrou maison (pas
/// `std::sync::Mutex`) : sa prise doit pouvoir voyager dans `Pending` d'un thread à l'autre pendant
/// l'attente d'un code Steam Guard (`MutexGuard` n'est pas `Send`, un jeton vide l'est).
static LOCK_TAKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub struct Lock;
impl Lock {
    fn acquire() -> Self {
        while LOCK_TAKEN.swap(true, Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Lock
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        LOCK_TAKEN.store(false, Ordering::SeqCst);
    }
}

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
    if l.contains("error (timeout)") {
        "délai dépassé (approbation non reçue, ou code Steam Guard requis)".into()
    } else if l.contains("account logon denied") || l.contains("invalid login auth code") {
        "code Steam Guard (e-mail) requis ou incorrect".into()
    } else if l.contains("twofactorcodemismatch") || l.contains("two-factor code mismatch") {
        "code Steam Guard incorrect ou expiré".into()
    } else if l.contains("invalid password") || l.contains("invalidpassword") {
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

/// Processus SteamCMD interactif en attente d'un code Steam Guard.
pub struct Pending {
    child: std::process::Child,
    out: Arc<Mutex<String>>,
    offset: u64,
    _lock: Lock,
}

fn spawn_piped(args: &[&str]) -> Result<(std::process::Child, Arc<Mutex<String>>), String> {
    let mut child = Command::new(exe())
        .args(args)
        .current_dir(dir())
        .creation_flags(NO_WINDOW)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let out = Arc::new(Mutex::new(String::new()));
    if let Some(mut so) = child.stdout.take() {
        let o = out.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = so.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if let Ok(mut s) = o.lock() {
                    s.push_str(&String::from_utf8_lossy(&buf[..n]));
                }
            }
        });
    }
    Ok((child, out))
}

fn console_log() -> PathBuf {
    dir().join("logs").join("console_log.txt")
}

fn console_len() -> u64 {
    std::fs::metadata(console_log()).map(|m| m.len()).unwrap_or(0)
}

/// Journal de SteamCMD depuis `offset` : SteamCMD ne vide sa sortie standard qu'à l'arrêt, le journal est la seule trace en direct.
fn console_since(offset: u64) -> String {
    let Ok(bytes) = std::fs::read(console_log()) else { return String::new() };
    let start = (offset as usize).min(bytes.len());
    String::from_utf8_lossy(&bytes[start..]).to_lowercase()
}

fn snapshot(out: &Arc<Mutex<String>>, offset: u64) -> String {
    let o = out.lock().map(|s| s.to_lowercase()).unwrap_or_default();
    format!("{o}\n{}", console_since(offset))
}

/// Attend la réussite, un échec, ou une demande de code (jusqu'à 3 minutes : l'utilisateur peut valider sur son téléphone).
/// `lock` est conservé dans le `Pending` en cas d'attente de code : il ne sera relâché qu'à `submit_code`/`abandon`.
fn await_login(mut child: std::process::Child, out: Arc<Mutex<String>>, code_sent: bool, offset: u64, lock: Lock) -> Login_ {
    let t0 = std::time::Instant::now();
    while t0.elapsed() < std::time::Duration::from_secs(180) {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let s = snapshot(&out, offset);
        // « Waiting for user info » n'apparaît qu'une fois connecté ; la session est alors mémorisée par SteamCMD.
        if s.contains("logged in ok") || s.contains("waiting for user info") {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let _ = child.kill();
            let _ = child.wait();
            return Login_::Done(Login::Ok);
        }
        if s.contains("error (") {
            let msg = failure(&s);
            let _ = child.kill();
            return Login_::Done(Login::Failed(msg));
        }
        if !code_sent && (s.contains("steam guard code") || s.contains("two-factor code") || s.contains("enter the current code") || s.contains("guard code")) {
            return Login_::Wait(Pending { child, out, offset, _lock: lock });
        }
        if matches!(child.try_wait(), Ok(Some(_))) {
            return Login_::Done(Login::Failed(failure(&s)));
        }
    }
    let _ = child.kill();
    Login_::Done(Login::Failed("délai dépassé".into()))
}

enum Login_ {
    Done(Login),
    Wait(Pending),
}

/// Connexion (SteamCMD garde ensuite la session : plus de mot de passe à saisir). Renvoie le processus en attente si un code est demandé.
pub fn login(user: &str, pass: &str, code: Option<&str>) -> (Login, Option<Pending>) {
    let lock = Lock::acquire();
    if let Err(e) = ensure() {
        return (Login::Failed(e), None);
    }
    let mut args = vec!["+@NoPromptForPassword", "0", "+login", user, pass];
    if let Some(c) = code {
        args.push(c);
    }
    let offset = console_len();
    let (child, out) = match spawn_piped(&args) {
        Ok(x) => x,
        Err(e) => return (Login::Failed(e), None),
    };
    match await_login(child, out, code.is_some(), offset, lock) {
        Login_::Done(l) => (l, None),
        Login_::Wait(p) => (Login::NeedCode, Some(p)),
    }
}

/// Envoie le code Steam Guard au même processus (même session : pas de second essai avec le mot de passe).
pub fn submit_code(mut p: Pending, code: &str) -> Login {
    // Code vide : l'utilisateur valide la connexion sur son téléphone, on attend simplement.
    if let Some(si) = p.child.stdin.as_mut().filter(|_| !code.is_empty()) {
        let _ = si.write_all(format!("{code}\n").as_bytes());
        let _ = si.flush();
    }
    match await_login(p.child, p.out, true, p.offset, p._lock) {
        Login_::Done(l) => l,
        Login_::Wait(mut p) => {
            let _ = p.child.kill();
            Login::Failed("code Steam Guard incorrect".into())
        }
    }
}

pub fn abandon(mut p: Pending) {
    let _ = p.child.kill();
}

/// Session mémorisée par SteamCMD pour cet identifiant ?
pub fn session_ok(user: &str) -> bool {
    let _lock = Lock::acquire();
    run(&["+@NoPromptForPassword", "1", "+login", user, "+quit"]).is_ok_and(|o| o.contains("Logged in OK") || o.contains("Waiting for user info"))
}

/// Dernière ligne « …progress: 42.57 (123456 / 290000)… » du journal SteamCMD, si présente : c'est la
/// seule source qui avance en continu (le manifeste `.acf`, lui, ne se met à jour qu'à des étapes,
/// parfois seulement au tout début et à la toute fin — d'où une barre qui semblait sauter de 0 à 100 %).
fn last_progress(log: &str) -> Option<(f64, f64)> {
    let rest = &log[log.rfind("progress: ")? + "progress: ".len()..];
    let inner = &rest[rest.find('(')? + 1..rest.find(')')?];
    let (done, total) = inner.split_once('/')?;
    Some((done.trim().parse().ok()?, total.trim().parse().ok()?))
}

/// Installe ou met à jour un jeu (bloquant, long). `pid` reçoit le processus (pour annuler), `progress` le pourcentage.
pub fn install(user: &str, appid: &str, pid: &AtomicU32, progress: impl Fn(f64, f64)) -> Result<(), String> {
    let _lock = Lock::acquire();
    ensure()?;
    let offset = console_len();
    let (mut child, out) = spawn_piped(&["+@NoPromptForPassword", "1", "+login", user, "+app_update", appid, "validate", "+quit"])?;
    pid.store(child.id(), Ordering::SeqCst);
    let mut last = -1.0f64;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let s = console_since(offset);
        // Progression en direct depuis le journal ; repli sur le manifeste si le journal n'en dit rien
        // encore (début de partie, ou jeu déjà entièrement validé sans retéléchargement).
        let found = last_progress(&s).or_else(|| {
            let m = manifest(appid)?;
            let n = |k: &str| field(&m, k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            let (total, done) = (n("BytesToDownload"), n("BytesDownloaded"));
            (total > 0.0).then_some((done, total))
        });
        if let Some((done, total)) = found {
            if done != last {
                last = done;
                progress(done, total);
            }
        }
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        // Connexion refusée en cours de route : inutile d'attendre.
        if s.contains("error (invalid password") || s.contains("error (no cached") {
            let _ = child.kill();
            break;
        }
    }
    pid.store(0, Ordering::SeqCst);
    let text = format!("{}\n{}", out.lock().map(|s| s.clone()).unwrap_or_default(), console_since(offset));
    let installed = manifest(appid).and_then(|m| field(&m, "StateFlags")).and_then(|f| f.parse::<u32>().ok()).is_some_and(|f| f & 4 != 0);
    if installed || text.to_lowercase().contains("fully installed") {
        Ok(())
    } else {
        Err(failure(&text.to_lowercase()))
    }
}

/// Interrompt un téléchargement en cours.
pub fn cancel(pid: u32) {
    if pid != 0 {
        let _ = Command::new("taskkill").args(["/pid", &pid.to_string(), "/t", "/f"]).creation_flags(NO_WINDOW).status();
    }
}

/// Numéro de build public le plus récent de chaque jeu.
pub fn latest_builds(user: &str, ids: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if ids.is_empty() {
        return map;
    }
    let _lock = Lock::acquire();
    if ensure().is_err() {
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
            n.strip_prefix("appmanifest_")?.strip_suffix(".acf").map(String::from).filter(|id| id != "228980")
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
fn main_exe_scan(root: &Path) -> Option<PathBuf> {
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

/// Exécutable principal d'un dossier de jeu (résultat mis en cache : le parcours du dossier est coûteux).
pub fn main_exe(root: &Path) -> Option<PathBuf> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(Some(p)) = cache.lock().ok().and_then(|c| c.get(root).cloned()) {
        if p.exists() {
            return Some(p);
        }
    }
    let r = main_exe_scan(root);
    if let Ok(mut c) = cache.lock() {
        c.insert(root.to_path_buf(), r.clone());
    }
    r
}
