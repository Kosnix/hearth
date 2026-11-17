//! Visuels des jeux : téléchargement et cache disque.

use std::path::PathBuf;

use crate::{gog, http, Game};

fn assets_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth").join("assets")
}

/// Supprime les visuels en cache d'un jeu (avant d'en télécharger de nouveaux).
pub fn clear_art(id: &str) {
    if id.is_empty() {
        return;
    }
    for kind in ["cover", "hero", "logo"] {
        for ext in ["png", "jpg", "jpeg"] {
            let _ = std::fs::remove_file(assets_dir().join(id).join(format!("{kind}.{ext}")));
        }
    }
}

/// Chemin d'un visuel (`cover`, `hero` ou `logo`) s'il est déjà en cache.
pub fn asset(id: &str, kind: &str) -> Option<PathBuf> {
    if id.is_empty() {
        return None;
    }
    ["png", "jpg", "jpeg"].iter().map(|ext| assets_dir().join(id).join(format!("{kind}.{ext}"))).find(|p| p.exists())
}

/// PNG ou JPEG : évite d'enregistrer une page d'erreur HTML.
fn extension(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if data.starts_with(&[0xFF, 0xD8]) {
        Some("jpg")
    } else {
        None
    }
}

pub(crate) fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Meilleure jaquette 600×900 de SteamGridDB (clé API dans `HEARTH_SGDB_KEY` ou `settings.json`).
fn steamgriddb(title: &str, key: &str) -> Option<String> {
    let auth = format!("Bearer {key}");
    let h = [("Authorization", auth.as_str())];
    let s = http::get_json(&format!("https://www.steamgriddb.com/api/v2/search/autocomplete/{}", encode(title)), &h)?;
    let id = s["data"][0]["id"].as_i64()?;
    let g = http::get_json(&format!("https://www.steamgriddb.com/api/v2/grids/game/{id}?dimensions=600x900"), &h)?;
    g["data"][0]["url"].as_str().map(String::from)
}

fn download(url: &str) -> Option<(Vec<u8>, &'static str)> {
    let data = http::get(url, &[])?;
    let ext = extension(&data)?;
    Some((data, ext))
}

/// Télécharge les visuels manquants du jeu. Renvoie vrai si au moins un fichier a été ajouté (bloquant).
pub fn ensure_art(game: &Game, sgdb_key: Option<&str>) -> bool {
    ensure_art_for(&game.id, &game.title, &game.art, sgdb_key)
}

/// Idem pour un identifiant et des URLs quelconques (jeux possédés non installés).
pub fn ensure_art_for(id: &str, title: &str, art: &crate::Art, sgdb_key: Option<&str>) -> bool {
    let mut added = false;
    for (kind, urls) in [("cover", &art.cover), ("hero", &art.hero), ("logo", &art.logo)] {
        if asset(id, kind).is_some() {
            continue;
        }
        let mut candidates: Vec<String> = urls.iter().filter_map(|u| if u.starts_with("gog-api:") { gog::resolve(u) } else { Some(u.clone()) }).collect();
        if kind == "cover" && candidates.is_empty() {
            candidates.extend(sgdb_key.and_then(|k| steamgriddb(title, k)));
        }
        for url in candidates {
            if let Some((data, ext)) = download(&url) {
                let dir = assets_dir().join(id);
                if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(dir.join(format!("{kind}.{ext}")), data).is_ok() {
                    added = true;
                    break;
                }
            }
        }
    }
    added
}

/// Première image téléchargeable parmi les URLs candidates (mise en cache) ; chemin local.
pub fn fetch_first(urls: &[String]) -> Option<PathBuf> {
    urls.iter().find_map(|u| fetch_image(u))
}

/// Image distante (offre du catalogue) mise en cache ; renvoie son chemin local.
fn fetch_image(url: &str) -> Option<PathBuf> {
    let hash = url.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let dir = assets_dir().join("_offers");
    for ext in ["jpg", "png"] {
        let p = dir.join(format!("{hash:016x}.{ext}"));
        if p.exists() {
            return Some(p);
        }
    }
    let (data, ext) = download(url)?;
    std::fs::create_dir_all(&dir).ok()?;
    let p = dir.join(format!("{hash:016x}.{ext}"));
    std::fs::write(&p, data).ok()?;
    Some(p)
}

/// Enregistre jaquette et héros d'une offre pour un jeu ajouté à la main (`id` = `custom-…`). Bloquant.
pub fn save_art(id: &str, offer: &crate::Offer) -> bool {
    let mut added = false;
    for (kind, urls) in [("cover", &offer.images), ("hero", &offer.heroes)] {
        if asset(id, kind).is_some() {
            continue;
        }
        for url in urls {
            if let Some((data, ext)) = download(url) {
                let dir = assets_dir().join(id);
                if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(dir.join(format!("{kind}.{ext}")), data).is_ok() {
                    added = true;
                    break;
                }
            }
        }
    }
    added
}
