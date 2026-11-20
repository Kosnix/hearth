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

/// Visuels SteamGridDB d'un jeu : identifiant trouvé par appid Steam (exact) ou par titre, puis jaquette, héros et logo.
/// `Err(())` : réseau indisponible ; `Ok(None)` : jeu inconnu de SteamGridDB.
struct Sgdb {
    id: i64,
    auth: String,
}

fn sgdb_find(id: &str, title: &str, key: &str) -> Result<Option<Sgdb>, ()> {
    let auth = format!("Bearer {key}");
    let h = [("Authorization", auth.as_str())];
    let steam = id.strip_prefix("owned-steam-").or_else(|| id.strip_prefix("steam-")).filter(|a| !a.is_empty() && a.bytes().all(|b| b.is_ascii_digit()));
    if let Some(app) = steam {
        if let Some(j) = http::get_json(&format!("https://www.steamgriddb.com/api/v2/games/steam/{app}"), &h) {
            if let Some(i) = j["data"]["id"].as_i64() {
                return Ok(Some(Sgdb { id: i, auth }));
            }
        }
    }
    let s = http::get_json(&format!("https://www.steamgriddb.com/api/v2/search/autocomplete/{}", encode(title)), &h).ok_or(())?;
    Ok(s["data"][0]["id"].as_i64().map(|i| Sgdb { id: i, auth }))
}

impl Sgdb {
    fn url(&self, kind: &str) -> Option<String> {
        let (path, dims) = match kind {
            "cover" => ("grids", "&dimensions=600x900,342x482,660x930"),
            "hero" => ("heroes", "&dimensions=1920x620,3840x1240"),
            _ => ("logos", ""),
        };
        let h = [("Authorization", self.auth.as_str())];
        let j = http::get_json(&format!("https://www.steamgriddb.com/api/v2/{path}/game/{}?mimes=image/png,image/jpeg{dims}", self.id), &h)?;
        j["data"][0]["url"].as_str().map(String::from)
    }
}

/// Recherche de jeux par titre sur SteamGridDB (« Identifier le jeu ») : nom exact, jaquette et héros. Vide sans clé ou sans réseau.
pub fn search_sgdb(query: &str, key: &str) -> Vec<crate::Offer> {
    let auth = format!("Bearer {key}");
    let h = [("Authorization", auth.as_str())];
    let Some(s) = http::get_json(&format!("https://www.steamgriddb.com/api/v2/search/autocomplete/{}", encode(query)), &h) else { return vec![] };
    let mut out = vec![];
    for g in s["data"].as_array().into_iter().flatten().take(8) {
        let (Some(id), Some(name)) = (g["id"].as_i64(), g["name"].as_str()) else { continue };
        let sg = Sgdb { id, auth: auth.clone() };
        out.push(crate::Offer {
            title: name.to_string(),
            store: "SteamGridDB".into(),
            images: sg.url("cover").into_iter().collect(),
            heroes: sg.url("hero").into_iter().collect(),
            ..Default::default()
        });
    }
    out
}

/// Jaquette de SteamGridDB pour un titre (jeux sans aucun visuel).
fn steamgriddb(id: &str, title: &str, key: &str) -> Option<String> {
    sgdb_find(id, title, key).ok()??.url("cover")
}

fn download(url: &str, limit: (u32, u32)) -> Option<(Vec<u8>, &'static str)> {
    let data = http::get(url, &[])?;
    let ext = extension(&data)?;
    Some(shrink(data, ext, limit))
}

/// Visuels trop lourds (jaquettes Epic de plusieurs Mo) : réduits à la taille d'affichage et réencodés (JPEG, ou PNG s'il y a de la transparence).
fn shrink(data: Vec<u8>, ext: &'static str, (w, h): (u32, u32)) -> (Vec<u8>, &'static str) {
    if data.len() < 250_000 {
        return (data, ext);
    }
    let Ok(img) = image::load_from_memory(&data) else { return (data, ext) };
    let img = if img.width() > w || img.height() > h { img.resize(w, h, image::imageops::FilterType::Triangle) } else { img };
    let mut out = std::io::Cursor::new(Vec::new());
    let ok = if img.color().has_alpha() {
        img.write_to(&mut out, image::ImageFormat::Png).is_ok()
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88).encode_image(&img.to_rgb8()).is_ok()
    };
    let new_ext = if img.color().has_alpha() { "png" } else { "jpg" };
    let bytes = out.into_inner();
    if ok && bytes.len() < data.len() {
        (bytes, new_ext)
    } else {
        (data, ext)
    }
}

/// Télécharge les visuels manquants du jeu. Renvoie vrai si au moins un fichier a été ajouté (bloquant).
pub fn ensure_art(game: &Game, sgdb_key: Option<&str>) -> bool {
    ensure_art_for(&game.id, &game.title, &game.art, sgdb_key)
}

/// Idem pour un identifiant et des URLs quelconques (jeux possédés non installés).
/// Avec une clé SteamGridDB, ses visuels remplacent une fois ceux de la boutique (qui restent en repli) ; marqueur `.sgdb` par jeu.
pub fn ensure_art_for(id: &str, title: &str, art: &crate::Art, sgdb_key: Option<&str>) -> bool {
    let key = sgdb_key.filter(|k| !k.trim().is_empty());
    let dir = assets_dir().join(id);
    let marker = dir.join(".sgdb");
    let mut sg = None;
    let mut redo = false;
    if let Some(k) = key {
        if !marker.exists() {
            // Réseau indisponible : on réessaiera plus tard (pas de marqueur).
            if let Ok(found) = sgdb_find(id, title, k) {
                redo = found.is_some();
                sg = found;
                if !redo && std::fs::create_dir_all(&dir).is_ok() {
                    let _ = std::fs::write(&marker, "none");
                }
            }
        }
    }
    let mut added = false;
    for (kind, urls) in [("cover", &art.cover), ("hero", &art.hero), ("logo", &art.logo)] {
        if !redo && asset(id, kind).is_some() {
            continue;
        }
        let mut candidates: Vec<String> = vec![];
        if redo {
            candidates.extend(sg.as_ref().and_then(|s| s.url(kind)));
        }
        candidates.extend(urls.iter().filter_map(|u| if u.starts_with("gog-api:") { gog::resolve(u) } else { Some(u.clone()) }));
        if kind == "cover" && urls.is_empty() && !redo {
            candidates.extend(key.and_then(|k| steamgriddb(id, title, k)));
        }
        for url in candidates {
            if let Some((data, ext)) = download(&url, match kind { "hero" => (1920, 1080), "logo" => (1000, 400), _ => (720, 1080) }) {
                if std::fs::create_dir_all(&dir).is_ok() {
                    for old in ["png", "jpg", "jpeg"] {
                        let _ = std::fs::remove_file(dir.join(format!("{kind}.{old}")));
                    }
                    if std::fs::write(dir.join(format!("{kind}.{ext}")), data).is_ok() {
                        added = true;
                        break;
                    }
                }
            }
        }
    }
    if redo {
        let _ = std::fs::write(&marker, "ok");
    }
    added
}

/// Première image téléchargeable parmi les URLs candidates (mise en cache) ; chemin local.
pub fn fetch_first(urls: &[String]) -> Option<PathBuf> {
    urls.iter().find_map(|u| fetch_image(u))
}

/// Images du catalogue : conservées 24 h, puis retéléchargées (prix, promotions et visuels de la boutique changent).
const OFFER_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

fn fresh(p: &std::path::Path) -> bool {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e < OFFER_TTL)
}

/// Supprime les images du catalogue plus vieilles que 24 h (appelé au démarrage, en tâche de fond).
pub fn purge_offers() {
    if let Ok(rd) = std::fs::read_dir(assets_dir().join("_offers")) {
        for e in rd.flatten() {
            if !fresh(&e.path()) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Image distante (offre du catalogue) mise en cache ; renvoie son chemin local.
fn fetch_image(url: &str) -> Option<PathBuf> {
    let hash = url.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let dir = assets_dir().join("_offers");
    for ext in ["jpg", "png"] {
        let p = dir.join(format!("{hash:016x}.{ext}"));
        if p.exists() && fresh(&p) {
            return Some(p);
        }
    }
    let (data, ext) = download(url, (420, 630))?;
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
            if let Some((data, ext)) = download(url, if kind == "hero" { (1920, 1080) } else { (720, 1080) }) {
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

/// Dossier des visuels d'un jeu (créé au besoin par l'appelant).
pub fn asset_dir(id: &str) -> PathBuf {
    assets_dir().join(id)
}
