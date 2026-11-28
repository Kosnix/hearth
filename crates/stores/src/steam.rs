//! Steam : bibliothèque installée via les manifestes locaux, catalogue via l'API publique de la boutique.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

use crate::{http, reg, Art, Game, Launch, Offer, Store};

pub struct Steam;

/// Paires `"clé" "valeur"` d'un fichier VDF/ACF (suffisant pour libraryfolders et appmanifest).
fn pairs(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let parts: Vec<&str> = l.split('"').collect();
            // ["\t", clé, "\t\t", valeur, ""]
            (parts.len() >= 5).then(|| (parts[1].to_string(), parts[3].replace("\\\\", "\\")))
        })
        .collect()
}

fn root() -> Option<PathBuf> {
    // HKCU n'existe qu'après le premier lancement de Steam : on se rabat sur l'installation système.
    reg::string(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath")
        .filter(|p| !p.is_empty())
        .or_else(|| reg::string(HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"))
        .map(PathBuf::from)
        .filter(|p| p.join("steam.exe").exists() || p.join("Steam.exe").exists())
}

/// Dossier d'installation du client Steam.
pub fn root_dir() -> Option<PathBuf> {
    root()
}

fn libraries(root: &Path) -> Vec<PathBuf> {
    let mut v = vec![root.to_path_buf()];
    if let Ok(t) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        for (k, val) in pairs(&t) {
            if k == "path" {
                let p = PathBuf::from(val);
                if !v.contains(&p) {
                    v.push(p);
                }
            }
        }
    }
    v
}

pub fn art(id: &str) -> Art {
    let base = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/");
    Art {
        cover: ["library_600x900_2x.jpg", "library_600x900.jpg", "header.jpg"].iter().map(|f| format!("{base}{f}")).collect(),
        hero: ["library_hero.jpg", "page_bg_generated_v6b.jpg"].iter().map(|f| format!("{base}{f}")).collect(),
        logo: vec![format!("{base}logo.png")],
    }
}

fn price(cents: i64, currency: &str) -> String {
    let sym = match currency {
        "EUR" => "€",
        "USD" => "$",
        "GBP" => "£",
        other => other,
    };
    format!("{},{:02} {sym}", cents / 100, cents % 100)
}

impl Store for Steam {
    fn name(&self) -> &'static str {
        "Steam"
    }

    fn detected(&self) -> bool {
        root().is_some_and(|r| r.exists())
    }

    fn installed(&self) -> Vec<Game> {
        let mut libs: Vec<(PathBuf, bool)> = root().map(|r| libraries(&r)).unwrap_or_default().into_iter().map(|p| (p, false)).collect();
        libs.push((crate::steamcmd::dir(), true));
        let mut games: Vec<Game> = vec![];
        for (lib, cmd) in libs {
            let Ok(dir) = std::fs::read_dir(lib.join("steamapps")) else { continue };
            for e in dir.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
                let get = |k: &str| pairs(&text).into_iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, v)| v);
                let (Some(id), Some(title)) = (get("appid"), get("name")) else { continue };
                let installed = get("StateFlags").and_then(|f| f.parse::<u32>().ok()).is_some_and(|f| f & 4 != 0);
                // Outils et environnements d'exécution : pas des jeux.
                let tool = id == "228980" || ["Steamworks", "Steam Linux Runtime", "Proton", "Steam Controller"].iter().any(|p| title.starts_with(p));
                if !installed || tool || games.iter().any(|g| g.id == format!("steam-{id}")) {
                    continue;
                }
                let launch = if cmd {
                    // Installé par SteamCMD : le client ne le connaît pas, on lance l'exécutable directement.
                    let Some(exe) = get("installdir").and_then(|d| crate::steamcmd::main_exe(&lib.join("steamapps").join("common").join(d))) else { continue };
                    Launch::Exe { path: exe.to_string_lossy().into_owned(), args: vec![] }
                } else {
                    Launch::Uri(format!("steam://rungameid/{id}"))
                };
                games.push(Game { art: art(&id), launch, id: format!("steam-{id}"), store: "Steam", title });
            }
        }
        games.sort_by_key(|a| a.title.to_lowercase());
        games
    }

    fn featured(&self, limit: usize) -> Vec<Offer> {
        let Some(j) = http::get_json(&format!("https://store.steampowered.com/api/featuredcategories?cc=fr&l={}", crate::steam_lang()), &[]) else { return vec![] };
        let mut seen = vec![];
        let mut out = vec![];
        for section in ["specials", "top_sellers", "new_releases"] {
            for it in j[section]["items"].as_array().into_iter().flatten() {
                let Some(id) = it["id"].as_i64() else { continue };
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                let name = it["name"].as_str().unwrap_or("");
                let fin = it["final_price"].as_i64().unwrap_or(0);
                // type 1 = « package » (édition groupée) : son id n'est pas un appid, image et lien
                // seraient faux. Le matériel Valve (Steam Deck, Steam Machine…) est listé comme une appli
                // de type 0 dans les meilleures ventes : on le reconnaît à son nom et à son prix.
                if it["type"].as_i64().unwrap_or(0) != 0 || (name.starts_with("Steam ") && fin >= 15_000) {
                    continue;
                }
                let cur = it["currency"].as_str().unwrap_or("EUR");
                let disc = it["discount_percent"].as_i64().unwrap_or(0);
                out.push(offer(id, name, fin, disc, cur));
                if out.len() >= limit {
                    return out;
                }
            }
        }
        out
    }

    // L'API non-officielle `storesearch` ne pagine pas (`page`/`start` ignorés, vérifié) : un seul appel,
    // borné à ce qu'elle rend elle-même. "Voir plus" ne fera donc plus grandir Steam au-delà de ça.
    fn search(&self, query: &str, limit: usize) -> Vec<Offer> {
        let url = format!("https://store.steampowered.com/api/storesearch/?term={}&l={}&cc=FR", crate::art::encode(query), crate::steam_lang());
        let Some(j) = http::get_json(&url, &[]) else { return vec![] };
        j["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|it| {
                let id = it["id"].as_i64()?;
                let fin = it["price"]["final"].as_i64().unwrap_or(0);
                let initial = it["price"]["initial"].as_i64().unwrap_or(fin);
                let disc = if initial > fin && initial > 0 { (initial - fin) * 100 / initial } else { 0 };
                Some(offer(id, it["name"].as_str()?, fin, disc, it["price"]["currency"].as_str().unwrap_or("EUR")))
            })
            .take(limit)
            .collect()
    }
}

fn offer(id: i64, title: &str, fin: i64, disc: i64, cur: &str) -> Offer {
    let p = if fin == 0 {
        "Gratuit".to_string()
    } else if disc > 0 {
        format!("-{disc} % {}", price(fin, cur))
    } else {
        price(fin, cur)
    };
    let base = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/");
    Offer {
        title: title.into(),
        price: p,
        cents: Some(fin),
        store: "Steam".into(),
        images: vec![format!("{base}library_600x900_2x.jpg"), format!("{base}library_600x900.jpg"), format!("{base}header.jpg")],
        heroes: vec![format!("{base}library_hero.jpg"), format!("{base}page_bg_generated_v6b.jpg")],
        url: format!("https://store.steampowered.com/app/{id}"),
        ..Default::default()
    }
}

/// Description, genres, date et captures depuis l'API publique de la fiche.
pub fn details(id: &str, d: &mut crate::Details) {
    let Some(j) = http::get_json(&format!("https://store.steampowered.com/api/appdetails?appids={id}&l={}&cc=fr", crate::steam_lang()), &[]) else { return };
    let data = &j[id]["data"];
    if let Some(t) = data["short_description"].as_str() {
        d.desc = t.to_string();
    }
    let genres: Vec<&str> = data["genres"].as_array().into_iter().flatten().filter_map(|g| g["description"].as_str()).take(3).collect();
    let mut meta: Vec<String> = vec![];
    if !genres.is_empty() {
        meta.push(genres.join(", "));
    }
    if let Some(r) = data["release_date"]["date"].as_str().filter(|r| !r.is_empty()) {
        meta.push(r.to_string());
    }
    if let Some(dev) = data["developers"][0].as_str() {
        meta.push(dev.to_string());
    }
    if !meta.is_empty() {
        d.meta = meta.join("  ·  ");
    }
    let shots: Vec<&serde_json::Value> = data["screenshots"].as_array().into_iter().flatten().filter(|s| s["path_thumbnail"].is_string()).take(4).collect();
    d.shots = shots.iter().filter_map(|s| s["path_thumbnail"].as_str().map(String::from)).collect();
    d.shots_full = shots.iter().map(|s| s["path_full"].as_str().or(s["path_thumbnail"].as_str()).unwrap_or("").to_string()).collect();
}

/// Compte connecté dans le client Steam (0 : client arrêté ou déconnecté).
pub fn client_user() -> u32 {
    reg::dword(HKEY_CURRENT_USER, r"Software\Valve\Steam\ActiveProcess", "ActiveUser").unwrap_or(0)
}

/// Démarre le client Steam (réduit dans la zone de notification si `silent`).
pub fn client_start(silent: bool) -> bool {
    let Some(r) = root() else { return false };
    let mut c = std::process::Command::new(r.join("steam.exe"));
    if silent {
        c.arg("-silent");
    }
    c.spawn().is_ok()
}

fn steam_running() -> bool {
    std::process::Command::new("tasklist")
        .args(["/fi", "imagename eq steam.exe", "/nh"])
        .creation_flags(0x0800_0000)
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("steam.exe"))
}

/// Désactive la superposition Steam en jeu (elle s'ouvre avec le bouton guide en même temps que le menu de Hearth).
/// Steam réécrit sa configuration en se fermant : on l'arrête proprement, on modifie, l'appelant le relance. Bloquant.
/// Renvoie faux si aucun compte n'a encore de configuration locale.
pub fn disable_overlay() -> bool {
    let Some(r) = root() else { return false };
    // Compte actif uniquement (capturé avant l'arrêt de Steam, qui remet le registre à 0) : sinon la
    // coupure de la synchro cloud toucherait aussi les comptes d'autres personnes ayant déjà utilisé
    // Steam sur cette session Windows, sans lien avec le jeu qu'on lance. 0 (Steam non lancé/déconnecté
    // au moment de l'appel) : on retombe sur tous les comptes locaux, comme avant.
    let active = client_user();
    let files: Vec<PathBuf> = std::fs::read_dir(r.join("userdata"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| active == 0 || e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) == Some(active))
        .map(|e| e.path().join("config").join("localconfig.vdf"))
        .filter(|p| p.exists())
        .collect();
    if files.is_empty() {
        return false;
    }
    if steam_running() {
        let _ = std::process::Command::new(r.join("steam.exe")).arg("-shutdown").creation_flags(0x0800_0000).status();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if !steam_running() {
                break;
            }
        }
        // Steam encore vivant après l'attente : il réécrira son propre état en se fermant et
        // écraserait silencieusement le patch qu'on s'apprête à appliquer — abandonner plutôt.
        if steam_running() {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        // Superposition coupée ; synchronisation cloud coupée (ses fenêtres de conflit bloquent le lancement des jeux).
        let step1 = patch_overlay(&text);
        let step2 = patch_cloud(step1.as_deref().unwrap_or(&text));
        if let Some(new) = step2.or(step1) {
            let _ = std::fs::copy(&f, f.with_extension("vdf.hearth-bak"));
            let _ = std::fs::write(&f, new);
        }
    }
    let cfg = r.join("config").join("config.vdf");
    if let Ok(text) = std::fs::read_to_string(&cfg) {
        if let Some(new) = patch_input(&text) {
            let _ = std::fs::copy(&cfg, cfg.with_extension("vdf.hearth-bak"));
            let _ = std::fs::write(&cfg, new);
        }
    }
    true
}

/// Ajoute/force `"EnableGameOverlay" "0"` dans le bloc `"system"` de localconfig.vdf.
fn patch_overlay(text: &str) -> Option<String> {
    if let Some(i) = text.find("\"EnableGameOverlay\"") {
        let line_end = text[i..].find('\n').map_or(text.len(), |n| i + n);
        let line = &text[i..line_end];
        if line.trim_end().ends_with("\"0\"") {
            return None;
        }
        return Some(format!("{}\"EnableGameOverlay\"\t\t\"0\"{}", &text[..i], &text[line_end..]));
    }
    // Recherche sur le texte d'origine (une copie en minuscules peut changer la longueur de certains
    // caractères, et ses positions tomber au milieu d'un caractère : plantage).
    let key = ["\"system\"", "\"System\"", "\"SYSTEM\""].iter().filter_map(|k| text.find(k)).min()?;
    let brace = key + text[key..].find('{')?;
    Some(format!("{}\n\t\t\"EnableGameOverlay\"\t\t\"0\"{}", &text[..=brace], &text[brace + 1..]))
}


/// Désactive Steam Input pour les manettes Xbox et PlayStation : sinon Steam accapare le bouton guide
/// (grande fenêtre Steam) en même temps que Hearth, et traduit la manette à la place des jeux.
fn patch_input(text: &str) -> Option<String> {
    const KEYS: [&str; 4] = ["SteamController_PSSupport", "SteamController_XBoxSupport", "SteamController_GenericGBSupport", "SteamController_SwitchSupport"];
    let mut out: Vec<String> = text.lines().map(String::from).collect();
    let mut changed = false;
    for l in out.iter_mut() {
        for k in KEYS {
            if l.contains(&format!("\"{k}\"")) && !l.trim_end().ends_with("\"0\"") {
                *l = format!("\t\t\t\t\"{k}\"\t\t\"0\"");
                changed = true;
            }
        }
    }
    let missing: Vec<&str> = KEYS.iter().copied().filter(|k| !text.contains(&format!("\"{k}\""))).collect();
    if !missing.is_empty() {
        let i = out.iter().position(|l| l.trim() == "\"Steam\"")?;
        let brace = i + out[i..].iter().position(|l| l.trim() == "{")?;
        for k in missing.iter().rev() {
            out.insert(brace + 1, format!("\t\t\t\t\"{k}\"\t\t\"0\""));
        }
        changed = true;
    }
    changed.then(|| out.join("\n") + "\n")
}

/// Désactive la synchronisation Steam Cloud (`"cloudenabled" "0"` dans le bloc `"Steam"` de localconfig.vdf).
fn patch_cloud(text: &str) -> Option<String> {
    let mut out: Vec<String> = text.lines().map(String::from).collect();
    if let Some(l) = out.iter_mut().find(|l| l.to_lowercase().contains("\"cloudenabled\"")) {
        if l.trim_end().ends_with("\"0\"") {
            return None;
        }
        *l = "\t\t\t\t\"cloudenabled\"\t\t\"0\"".to_string();
        return Some(out.join("\n") + "\n");
    }
    let i = out.iter().position(|l| l.trim() == "\"Steam\"")?;
    let brace = i + out[i..].iter().position(|l| l.trim() == "{")?;
    out.insert(brace + 1, "\t\t\t\t\"cloudenabled\"\t\t\"0\"".to_string());
    Some(out.join("\n") + "\n")
}
