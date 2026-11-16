//! Steam : bibliothèque installée via les manifestes locaux, catalogue via l'API publique de la boutique.

use std::path::PathBuf;

use windows::Win32::System::Registry::HKEY_CURRENT_USER;

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
    reg::string(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath").map(PathBuf::from)
}

fn libraries(root: &PathBuf) -> Vec<PathBuf> {
    let mut v = vec![root.clone()];
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

fn art(id: &str) -> Art {
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
        let Some(root) = root() else { return vec![] };
        let mut games = vec![];
        for lib in libraries(&root) {
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
                if installed && !tool {
                    games.push(Game { art: art(&id), launch: Launch::Uri(format!("steam://rungameid/{id}")), id: format!("steam-{id}"), store: "Steam", title });
                }
            }
        }
        games.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        games
    }

    fn featured(&self) -> Vec<Offer> {
        let Some(j) = http::get_json("https://store.steampowered.com/api/featuredcategories?cc=fr&l=french", &[]) else { return vec![] };
        let mut seen = vec![];
        let mut out = vec![];
        for section in ["specials", "top_sellers", "new_releases"] {
            for it in j[section]["items"].as_array().into_iter().flatten() {
                let Some(id) = it["id"].as_i64() else { continue };
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                let cur = it["currency"].as_str().unwrap_or("EUR");
                let fin = it["final_price"].as_i64().unwrap_or(0);
                let disc = it["discount_percent"].as_i64().unwrap_or(0);
                let p = if fin == 0 {
                    "Gratuit".to_string()
                } else if disc > 0 {
                    format!("-{disc} % {}", price(fin, cur))
                } else {
                    price(fin, cur)
                };
                out.push(Offer {
                    title: it["name"].as_str().unwrap_or("").into(),
                    price: p,
                    image: format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/library_600x900.jpg"),
                    url: format!("https://store.steampowered.com/app/{id}"),
                });
                if out.len() >= 24 {
                    return out;
                }
            }
        }
        out
    }
}
