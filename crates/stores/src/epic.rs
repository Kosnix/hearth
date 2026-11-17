//! Epic Games : bibliothèque via les manifestes du lanceur, catalogue via les promotions publiques.

use std::path::Path;

use crate::{http, Art, Game, Launch, Offer, Store};

pub struct Epic;

const MANIFESTS: &str = r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests";

impl Store for Epic {
    fn name(&self) -> &'static str {
        "Epic Games"
    }

    fn detected(&self) -> bool {
        Path::new(MANIFESTS).exists()
    }

    fn installed(&self) -> Vec<Game> {
        let Ok(dir) = std::fs::read_dir(MANIFESTS) else { return vec![] };
        let mut games: Vec<Game> = dir
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "item"))
            .filter_map(|e| {
                let j: serde_json::Value = serde_json::from_str(std::fs::read_to_string(e.path()).ok()?.trim_start_matches('\u{feff}')).ok()?;
                let title = j["DisplayName"].as_str()?.to_string();
                let app = j["AppName"].as_str()?.to_string();
                // Compléments (DLC) et jeux sans exécutable : ignorés.
                let exe = j["LaunchExecutable"].as_str().filter(|x| !x.is_empty())?;
                let dir = j["InstallLocation"].as_str()?;
                let path = Path::new(dir).join(exe).to_string_lossy().into_owned();
                let args = j["LaunchCommand"].as_str().unwrap_or("").split_whitespace().map(String::from).collect();
                Some(Game { id: format!("epic-{app}"), store: "Epic Games", title, launch: Launch::Exe { path, args }, art: Art::default() })
            })
            .collect();
        games.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        games
    }

    fn featured(&self) -> Vec<Offer> {
        let url = "https://store-site-backend-static.ak.epicgames.com/freeGamesPromotions?locale=fr&country=FR&allowCountries=FR";
        let Some(j) = http::get_json(url, &[]) else { return vec![] };
        j["data"]["Catalog"]["searchStore"]["elements"].as_array().into_iter().flatten().filter_map(offer).collect()
    }

    fn search(&self, query: &str) -> Vec<Offer> {
        let body = serde_json::json!({
            "query": "query q($k:String,$c:String!,$l:String){Catalog{searchStore(keywords:$k,country:$c,locale:$l,count:20,category:\"games/edition/base|bundles/games|editors\"){elements{title productSlug urlSlug keyImages{type url} price(country:$c){totalPrice{discountPrice}}}}}}",
            "variables": { "k": query, "c": "FR", "l": "fr" }
        });
        let Some(j) = http::post_json("https://store.epicgames.com/graphql", &[], &body.to_string()) else { return vec![] };
        j["data"]["Catalog"]["searchStore"]["elements"].as_array().into_iter().flatten().filter_map(offer).collect()
    }
}

fn offer(e: &serde_json::Value) -> Option<Offer> {
    let title = e["title"].as_str()?.to_string();
    let slug = e["productSlug"].as_str().or_else(|| e["urlSlug"].as_str()).unwrap_or("");
    let images: Vec<String> = ["OfferImageTall", "DieselStoreFrontTall", "Thumbnail", "OfferImageWide"]
        .iter()
        .filter_map(|t| e["keyImages"].as_array()?.iter().find(|k| k["type"] == *t)?["url"].as_str().map(String::from))
        .collect();
    if images.is_empty() {
        return None;
    }
    let fin = e["price"]["totalPrice"]["discountPrice"].as_i64();
    let price = match fin {
        Some(0) => "Gratuit".to_string(),
        Some(n) if n > 0 => format!("{},{:02} €", n / 100, n % 100),
        _ => String::new(),
    };
    let url = if slug.is_empty() { "https://store.epicgames.com/fr/".into() } else { format!("https://store.epicgames.com/fr/p/{}", slug.trim_end_matches("/home")) };
    let heroes: Vec<String> = ["OfferImageWide", "DieselStoreFrontWide", "featuredMedia"]
        .iter()
        .filter_map(|t| e["keyImages"].as_array()?.iter().find(|k| k["type"] == *t)?["url"].as_str().map(String::from))
        .collect();
    Some(Offer { title, price, cents: fin, store: "Epic".into(), images, heroes, url })
}
