//! GOG : bibliothèque via le registre de GOG Galaxy / des installateurs, catalogue via l'API publique.

use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;

use crate::{http, reg, Art, Game, Launch, Offer, Store};

pub struct Gog;

const GAMES: &str = r"SOFTWARE\WOW6432Node\GOG.com\Games";

/// Les URLs d'images GOG peuvent être sans schéma (`//images…`).
fn absolute(u: &str) -> String {
    if u.starts_with("//") {
        format!("https:{u}")
    } else {
        u.to_string()
    }
}

impl Store for Gog {
    fn name(&self) -> &'static str {
        "GOG"
    }

    fn detected(&self) -> bool {
        !reg::subkeys(HKEY_LOCAL_MACHINE, GAMES).is_empty() || std::path::Path::new(r"C:\Program Files (x86)\GOG Galaxy\GalaxyClient.exe").exists()
    }

    fn installed(&self) -> Vec<Game> {
        let mut games: Vec<Game> = reg::subkeys(HKEY_LOCAL_MACHINE, GAMES)
            .into_iter()
            .filter_map(|id| {
                let key = format!(r"{GAMES}\{id}");
                let get = |n: &str| reg::string(HKEY_LOCAL_MACHINE, &key, n);
                let title = get("gameName")?;
                let exe = get("exe").or_else(|| get("launchCommand"))?;
                let args = get("launchParam").map(|a| a.split_whitespace().map(String::from).collect()).unwrap_or_default();
                // Les images viennent de l'API du produit, résolues au téléchargement (cf. art.rs).
                let art = Art { cover: vec![format!("gog-api:{id}:cover")], hero: vec![format!("gog-api:{id}:hero")], logo: vec![] };
                Some(Game { id: format!("gog-{id}"), store: "GOG", title, launch: Launch::Exe { path: exe, args }, art })
            })
            .collect();
        games.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        games
    }

    fn featured(&self) -> Vec<Offer> {
        catalog("order=desc:trending")
    }

    fn search(&self, query: &str) -> Vec<Offer> {
        catalog(&format!("query=like:{}&order=desc:score", crate::art::encode(query)))
    }
}

fn catalog(params: &str) -> Vec<Offer> {
    let url = format!("https://catalog.gog.com/v1/catalog?limit=24&{params}&productType=in:game,pack&locale=fr-FR&countryCode=FR&currencyCode=EUR");
    let Some(j) = http::get_json(&url, &[]) else { return vec![] };
    j["products"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let title = p["title"].as_str()?.to_string();
            let images: Vec<String> = ["coverVertical", "coverHorizontal"].iter().filter_map(|k| p[*k].as_str().map(String::from)).collect();
            if images.is_empty() {
                return None;
            }
            let amount = p["price"]["finalMoney"]["amount"].as_str().and_then(|a| a.parse::<f64>().ok());
            let price = match amount {
                Some(v) if v == 0.0 => "Gratuit".to_string(),
                Some(v) => format!("{v:.2} €").replace('.', ","),
                None => String::new(),
            };
            let url = p["storeLink"].as_str().unwrap_or("https://www.gog.com/fr").to_string();
            let heroes = p["coverHorizontal"].as_str().map(|h| vec![h.to_string()]).unwrap_or_default();
            let genres: Vec<&str> = p["genres"].as_array().into_iter().flatten().filter_map(|g| g["name"].as_str()).take(3).collect();
            let mut meta = vec![];
            if !genres.is_empty() {
                meta.push(genres.join(", "));
            }
            if let Some(d) = p["releaseDate"].as_str() {
                meta.push(d.to_string());
            }
            Some(Offer { title, price, cents: amount.map(|v| (v * 100.0).round() as i64), store: "GOG".into(), images, heroes, url, meta: meta.join("  ·  "), ..Default::default() })
        })
        .collect()
}

/// Résout `gog-api:<id>:cover|hero` en URL d'image (API publique des produits).
pub fn resolve(spec: &str) -> Option<String> {
    let mut p = spec.strip_prefix("gog-api:")?.split(':');
    let (id, kind) = (p.next()?, p.next()?);
    let j = http::get_json(&format!("https://api.gog.com/products/{id}?expand=description"), &[])?;
    let img = &j["images"];
    let url = match kind {
        "hero" => img["background"].as_str()?,
        _ => img["logo2x"].as_str().or_else(|| img["logo"].as_str())?,
    };
    Some(absolute(url))
}
