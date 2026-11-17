//! Comptes des boutiques, connexion depuis Hearth (QR code / code appareil), jetons chiffrés (DPAPI),
//! bibliothèque possédée et installation.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::http;

/// Session enregistrée d'une boutique.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub account_id: String,
    pub access: String,
    pub refresh: String,
}

/// Connexion en cours : à afficher sous forme de QR code (`url`) avec un code éventuel.
#[derive(Clone, Debug)]
pub struct Challenge {
    pub store: &'static str,
    pub url: String,
    pub code: String,
    /// Secondes entre deux interrogations.
    pub interval: u64,
    /// Epic : l'utilisateur doit saisir le code affiché par la page de connexion.
    pub needs_code: bool,
    state: String,
}

pub enum Poll {
    Pending,
    Done(Session),
    Failed(String),
}

/// Jeu possédé (peut ne pas être installé).
#[derive(Clone, Debug)]
pub struct Owned {
    pub store: &'static str,
    pub title: String,
    /// Identifiant pour l'installation (appid Steam, `ns:catalog:app` Epic).
    pub key: String,
    pub images: Vec<String>,
}

// ---------- Stockage chiffré ----------

fn file() -> PathBuf {
    crate::profile_dir().join("accounts.dat")
}

mod dpapi {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};

    fn run(data: &[u8], protect: bool) -> Option<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            if protect {
                CryptProtectData(&input, None, None, None, None, 0, &mut out).ok()?;
            } else {
                CryptUnprotectData(&input, None, None, None, None, 0, &mut out).ok()?;
            }
            let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
            Some(v)
        }
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        run(data, true)
    }

    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        run(data, false)
    }
}

fn load_all() -> std::collections::BTreeMap<String, Session> {
    std::fs::read(file()).ok().and_then(|b| dpapi::unprotect(&b)).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_all(m: &std::collections::BTreeMap<String, Session>) {
    if let Some(enc) = serde_json::to_vec(m).ok().and_then(|b| dpapi::protect(&b)) {
        let _ = std::fs::write(file(), enc);
    }
}

pub fn session(store: &str) -> Option<Session> {
    load_all().remove(store)
}

pub fn store_session(store: &str, s: Session) {
    let mut m = load_all();
    m.insert(store.into(), s);
    save_all(&m);
}

pub fn logout(store: &str) {
    let mut m = load_all();
    m.remove(store);
    save_all(&m);
}

// ---------- Utilitaires ----------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn b64_decode(s: &str) -> Vec<u8> {
    let mut out = vec![];
    let mut acc = 0u32;
    let mut bits = 0;
    for ch in s.chars() {
        let v = match ch {
            'A'..='Z' => ch as u32 - 'A' as u32,
            'a'..='z' => ch as u32 - 'a' as u32 + 26,
            '0'..='9' => ch as u32 - '0' as u32 + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            _ => continue,
        };
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xFF) as u8);
        }
    }
    out
}

fn enc(s: &str) -> String {
    crate::art::encode(s)
}

// ---------- Epic Games (code appareil) ----------

const EPIC_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
const EPIC_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";
const EPIC_OAUTH: &str = "https://account-public-service-prod03.ol.epicgames.com/account/api/oauth";

fn epic_basic() -> String {
    format!("Basic {}", b64_encode(format!("{EPIC_ID}:{EPIC_SECRET}").as_bytes()))
}

/// Epic n'accepte pas le code appareil pour ce client : connexion sur téléphone (QR code), puis saisie du
/// code d'autorisation affiché par la page.
pub fn epic_begin() -> Option<Challenge> {
    let redirect = format!("https://www.epicgames.com/id/api/redirect?clientId={EPIC_ID}&responseType=code");
    Some(Challenge {
        store: "Epic Games",
        url: format!("https://www.epicgames.com/id/login?redirectUrl={}", enc(&redirect)),
        code: String::new(),
        interval: 0,
        needs_code: true,
        state: String::new(),
    })
}

/// Échange le code d'autorisation saisi contre une session.
pub fn exchange(store: &str, code: &str) -> Result<Session, String> {
    if store != "Epic Games" {
        return Err("boutique non prise en charge".into());
    }
    let basic = epic_basic();
    let code = code.trim().trim_matches('"');
    let (st, j) = http::post_form_status(&format!("{EPIC_OAUTH}/token"), &[("Authorization", &basic)], &format!("grant_type=authorization_code&code={}&token_type=eg1", enc(code))).ok_or("réseau indisponible")?;
    if st != 200 {
        return Err(j["errorMessage"].as_str().unwrap_or("code refusé").to_string());
    }
    let s = epic_session(&j).ok_or("réponse inattendue")?;
    store_session("Epic Games", s.clone());
    Ok(s)
}

fn epic_session(j: &serde_json::Value) -> Option<Session> {
    Some(Session {
        name: j["displayName"].as_str().unwrap_or("Epic").to_string(),
        account_id: j["account_id"].as_str()?.to_string(),
        access: j["access_token"].as_str()?.to_string(),
        refresh: j["refresh_token"].as_str().unwrap_or("").to_string(),
    })
}

fn epic_refresh(s: &Session) -> Option<Session> {
    let basic = epic_basic();
    let (st, j) = http::post_form_status(&format!("{EPIC_OAUTH}/token"), &[("Authorization", &basic)], &format!("grant_type=refresh_token&refresh_token={}", enc(&s.refresh)))?;
    (st == 200).then(|| epic_session(&j)).flatten()
}

fn epic_library(s: &mut Session) -> Vec<Owned> {
    let base = "https://library-service.live.use1a.on.epicgames.com/library/api/public/items?includeMetadata=true&platform=Windows";
    let mut records: Vec<serde_json::Value> = vec![];
    let mut cursor = String::new();
    for _ in 0..8 {
        let url = if cursor.is_empty() { base.to_string() } else { format!("{base}&cursor={cursor}") };
        let mut res = http::get_status(&url, &[("Authorization", &format!("bearer {}", s.access))]);
        if matches!(res, Some((401, _))) {
            if let Some(n) = epic_refresh(s) {
                *s = n;
                store_session("Epic Games", s.clone());
                res = http::get_status(&url, &[("Authorization", &format!("bearer {}", s.access))]);
            }
        }
        let Some((200, j)) = res else { break };
        records.extend(j["records"].as_array().cloned().unwrap_or_default());
        cursor = j["responseMetadata"]["nextCursor"].as_str().unwrap_or("").to_string();
        if cursor.is_empty() {
            break;
        }
    }
    // Jeux publics (pas les outils Unreal ni les doublons de plateforme), un par produit.
    let mut seen = std::collections::HashSet::new();
    let games: Vec<(String, String, String)> = records
        .iter()
        .filter(|r| r["sandboxType"] == "PUBLIC" && r["recordType"] == "APPLICATION" && r["namespace"] != "ue")
        .filter_map(|r| {
            let (ns, cat, app) = (r["namespace"].as_str()?, r["catalogItemId"].as_str()?, r["appName"].as_str()?);
            seen.insert(ns.to_string()).then(|| (ns.to_string(), cat.to_string(), app.to_string()))
        })
        .collect();
    let token = s.access.clone();
    let mut out: Vec<Option<Owned>> = vec![None; games.len()];
    std::thread::scope(|sc| {
        for (chunk, slots) in games.chunks(8).zip(out.chunks_mut(8)) {
            let token = &token;
            sc.spawn(move || {
                for ((ns, cat, app), slot) in chunk.iter().zip(slots) {
                    let meta = http::get_json(&format!("https://catalog-public-service-prod06.ol.epicgames.com/catalog/api/shared/namespace/{ns}/bulk/items?id={cat}&country=FR&locale=fr"), &[("Authorization", &format!("bearer {token}"))]);
                    let Some(item) = meta.as_ref().and_then(|m| m[cat.as_str()].as_object()) else { continue };
                    let title = item.get("title").and_then(|t| t.as_str()).unwrap_or(app).to_string();
                    let images: Vec<String> = ["DieselGameBoxTall", "OfferImageTall", "Thumbnail"]
                        .iter()
                        .filter_map(|t| item.get("keyImages")?.as_array()?.iter().find(|k| k["type"] == *t)?["url"].as_str().map(String::from))
                        .collect();
                    *slot = Some(Owned { store: "Epic Games", title, key: format!("{ns}:{cat}:{app}"), images });
                }
            });
        }
    });
    out.into_iter().flatten().collect()
}

// ---------- Steam (connexion par QR code) ----------

const STEAM_API: &str = "https://api.steampowered.com/IAuthenticationService";

fn varint(mut n: u64, out: &mut Vec<u8>) {
    while n >= 0x80 {
        out.push((n & 0x7F) as u8 | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
}

pub fn steam_begin() -> Option<Challenge> {
    // CAuthentication_BeginAuthSessionViaQR_Request { device_friendly_name = "Hearth", platform_type = 1 (client Steam) }
    let mut p = vec![0x0A, 6];
    p.extend_from_slice(b"Hearth");
    p.extend_from_slice(&[0x10, 1]);
    let (st, j) = http::post_form_status(&format!("{STEAM_API}/BeginAuthSessionViaQR/v1"), &[], &format!("input_protobuf_encoded={}&format=json", enc(&b64_encode(&p))))?;
    let r = &j["response"];
    if st != 200 {
        return None;
    }
    let client_id = r["client_id"].as_str().map(String::from).or_else(|| r["client_id"].as_u64().map(|n| n.to_string()))?;
    Some(Challenge {
        store: "Steam",
        url: r["challenge_url"].as_str()?.to_string(),
        code: String::new(),
        interval: r["interval"].as_f64().unwrap_or(5.0).max(2.0) as u64,
        needs_code: false,
        state: format!("{client_id}|{}", r["request_id"].as_str()?),
    })
}

fn jwt_sub(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let j: serde_json::Value = serde_json::from_slice(&b64_decode(payload)).ok()?;
    j["sub"].as_str().map(String::from)
}

fn steam_poll(c: &Challenge) -> Poll {
    let Some((cid, rid)) = c.state.split_once('|') else { return Poll::Failed("état invalide".into()) };
    let mut p = vec![0x08];
    varint(cid.parse().unwrap_or(0), &mut p);
    let rid = b64_decode(rid);
    p.push(0x12);
    varint(rid.len() as u64, &mut p);
    p.extend_from_slice(&rid);
    match http::post_form_status(&format!("{STEAM_API}/PollAuthSessionStatus/v1"), &[], &format!("input_protobuf_encoded={}&format=json", enc(&b64_encode(&p)))) {
        Some((200, j)) => {
            let r = &j["response"];
            match (r["access_token"].as_str(), r["refresh_token"].as_str()) {
                (Some(a), Some(rf)) => Poll::Done(Session {
                    name: r["account_name"].as_str().unwrap_or("Steam").to_string(),
                    account_id: jwt_sub(a).or_else(|| jwt_sub(rf)).unwrap_or_default(),
                    access: a.to_string(),
                    refresh: rf.to_string(),
                }),
                _ => Poll::Pending,
            }
        }
        Some((_, _)) => Poll::Failed("connexion refusée ou expirée".into()),
        None => Poll::Pending,
    }
}

fn steam_library(s: &Session) -> Vec<Owned> {
    let url = format!(
        "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/?access_token={}&steamid={}&include_appinfo=1&include_played_free_games=1&format=json",
        enc(&s.access),
        enc(&s.account_id)
    );
    let Some(j) = http::get_json(&url, &[]) else { return vec![] };
    j["response"]["games"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let id = g["appid"].as_i64()?;
            let base = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/");
            Some(Owned { store: "Steam", title: g["name"].as_str()?.to_string(), key: id.to_string(), images: vec![format!("{base}library_600x900_2x.jpg"), format!("{base}library_600x900.jpg"), format!("{base}header.jpg")] })
        })
        .collect()
}

// ---------- API commune ----------

pub fn begin(store: &str) -> Option<Challenge> {
    match store {
        "Epic Games" => epic_begin(),
        "Steam" => steam_begin(),
        _ => None,
    }
}

/// Interroge une connexion en cours ; enregistre la session quand l'utilisateur a validé.
pub fn poll(c: &Challenge) -> Poll {
    let r = match c.store {
        "Epic Games" => Poll::Pending,
        "Steam" => steam_poll(c),
        _ => Poll::Failed("boutique non prise en charge".into()),
    };
    if let Poll::Done(s) = &r {
        store_session(c.store, s.clone());
    }
    r
}

/// Jeux possédés d'une boutique connectée (réseau, bloquant).
pub fn library(store: &str) -> Vec<Owned> {
    let Some(mut s) = session(store) else { return vec![] };
    match store {
        "Epic Games" => epic_library(&mut s),
        "Steam" => steam_library(&s),
        _ => vec![],
    }
}

/// Demande l'installation d'un jeu possédé au client de la boutique (qui doit être installé et connecté).
/// Visuels d'un jeu possédé (Steam : jaquette, héros et logo ; Epic : jaquette de la liste).
pub fn owned_art(o: &Owned) -> crate::Art {
    match o.store {
        "Steam" => crate::steam_art(&o.key),
        _ => crate::Art { cover: o.images.clone(), ..Default::default() },
    }
}

pub fn install_uri(o: &Owned) -> Option<String> {
    match o.store {
        "Steam" => Some(format!("steam://install/{}", o.key)),
        "Epic Games" => {
            let mut p = o.key.splitn(3, ':');
            let (ns, cat, app) = (p.next()?, p.next()?, p.next()?);
            Some(format!("com.epicgames.launcher://apps/{ns}%3A{cat}%3A{app}?action=install"))
        }
        _ => None,
    }
}


#[doc(hidden)]
pub fn epic_debug() -> String {
    let Some(s) = session("Epic Games") else { return "pas de session".into() };
    let url = "https://library-service.live.use1a.on.epicgames.com/library/api/public/items?includeMetadata=true&platform=Windows";
    match http::get_status(url, &[("Authorization", &format!("bearer {}", s.access))]) {
        Some((st, j)) => format!("HTTP {st}: {}", j.to_string().chars().take(600).collect::<String>()),
        None => "pas de réponse".into(),
    }
}

/// Code d'échange a usage unique pour ouvrir une session dans un autre client (Legendary), sans nouvelle saisie.
pub fn epic_exchange_code() -> Option<String> {
    let mut s = session("Epic Games")?;
    let url = format!("{EPIC_OAUTH}/exchange");
    let mut res = http::get_status(&url, &[("Authorization", &format!("bearer {}", s.access))]);
    if !matches!(res, Some((200, _))) {
        if let Some(n) = epic_refresh(&s) {
            s = n;
            store_session("Epic Games", s.clone());
            res = http::get_status(&url, &[("Authorization", &format!("bearer {}", s.access))]);
        }
    }
    let Some((200, j)) = res else { return None };
    j["code"].as_str().map(String::from)
}
