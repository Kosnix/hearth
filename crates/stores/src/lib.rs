//! Connecteurs de boutiques : bibliothèque installée (lecture locale), visuels, catalogue « à la une ».
//! Visuels mis en cache dans `%LOCALAPPDATA%\Hearth\assets\<id>\{cover,hero,logo}.<ext>`.

mod art;
mod epic;
mod gog;
mod http;
mod reg;
mod steam;

pub mod account;
pub mod qr;

pub use art::{asset, clear_art, ensure_art, ensure_art_for, fetch_first, save_art};

/// Façon de lancer un jeu.
#[derive(Clone, Debug)]
pub enum Launch {
    /// Exécutable direct : le gestionnaire de fenêtres peut l'encapsuler.
    Exe { path: String, args: Vec<String> },
    /// Lancement par le client de la boutique (`steam://…`) : pas d'encapsulation.
    Uri(String),
}

/// URLs candidates des visuels, essayées dans l'ordre.
#[derive(Clone, Debug, Default)]
pub struct Art {
    pub cover: Vec<String>,
    pub hero: Vec<String>,
    pub logo: Vec<String>,
}

/// Jeu installé.
#[derive(Clone, Debug)]
pub struct Game {
    /// Identifiant stable, aussi nom du dossier de visuels (`steam-440`, `gog-123`…).
    pub id: String,
    pub store: &'static str,
    pub title: String,
    pub launch: Launch,
    pub art: Art,
}


/// Jeu proposé par le catalogue d'une boutique.
#[derive(Clone, Debug, Default)]
pub struct Offer {
    pub title: String,
    /// Prix affiché (« 19,99 € », « -50 % 9,99 € », « Gratuit »).
    pub price: String,
    /// Prix final en centimes, pour comparer les boutiques.
    pub cents: Option<i64>,
    /// Boutique(s) qui le proposent (« Steam », « Steam · GOG »).
    pub store: String,
    /// URLs d'images candidates (jaquette), essayées dans l'ordre.
    pub images: Vec<String>,
    /// Visuels larges (héros), si la boutique en fournit.
    pub heroes: Vec<String>,
    /// Page de la boutique (encodée en QR code pour l'achat sur téléphone).
    pub url: String,
    /// Description et genres/date, quand la boutique les donne dans ses listes.
    pub desc: String,
    pub meta: String,
    /// Une entrée par boutique qui le propose (renseigné par la fusion), la moins chère d'abord.
    pub deals: Vec<Deal>,
}

/// Prix d'un jeu dans une boutique donnée.
#[derive(Clone, Debug, Default)]
pub struct Deal {
    pub store: String,
    pub price: String,
    pub cents: Option<i64>,
    pub url: String,
}

/// Fiche détaillée d'un jeu (récupérée à la demande).
#[derive(Clone, Debug, Default)]
pub struct Details {
    pub desc: String,
    pub meta: String,
    pub shots: Vec<String>,
}

pub trait Store: Send + Sync {
    fn name(&self) -> &'static str;
    /// Le client de la boutique est-il installé ?
    fn detected(&self) -> bool;
    fn installed(&self) -> Vec<Game>;
    /// Sélection mise en avant (réseau, bloquant).
    fn featured(&self) -> Vec<Offer>;
    /// Recherche dans le catalogue (réseau, bloquant).
    fn search(&self, query: &str) -> Vec<Offer>;
}

pub fn all() -> Vec<Box<dyn Store>> {
    vec![Box::new(steam::Steam), Box::new(epic::Epic), Box::new(gog::Gog)]
}

/// Tous les jeux installés, toutes boutiques confondues.
pub fn installed() -> Vec<Game> {
    all().iter().flat_map(|s| s.installed()).collect()
}

fn norm(title: &str) -> String {
    let mut t = title.to_lowercase();
    for w in ["standard edition", "deluxe edition", "ultimate edition", "gold edition", "complete edition", "definitive edition", "game of the year edition", "goty edition", "digital edition"] {
        t = t.replace(w, "");
    }
    t.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Fusionne les listes de plusieurs boutiques : alternance des sources, un seul résultat par jeu
/// (même titre) avec le détail du prix dans chaque boutique, la moins chère en premier.
fn merge(lists: Vec<Vec<Offer>>) -> Vec<Offer> {
    let mut out: Vec<Offer> = vec![];
    let mut keys: Vec<String> = vec![];
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..longest {
        for list in &lists {
            let Some(o) = list.get(i) else { continue };
            let deal = Deal { store: o.store.clone(), price: o.price.clone(), cents: o.cents, url: o.url.clone() };
            let k = norm(&o.title);
            match keys.iter().position(|x| *x == k) {
                Some(p) => {
                    let e = &mut out[p];
                    if e.deals.iter().any(|d| d.store == deal.store) {
                        continue;
                    }
                    e.deals.push(deal);
                    if e.heroes.is_empty() {
                        e.heroes = o.heroes.clone();
                    }
                    if e.desc.is_empty() {
                        e.desc = o.desc.clone();
                    }
                    if e.meta.is_empty() {
                        e.meta = o.meta.clone();
                    }
                }
                None => {
                    keys.push(k);
                    let mut e = o.clone();
                    e.deals = vec![deal];
                    out.push(e);
                }
            }
        }
    }
    for e in &mut out {
        e.deals.sort_by_key(|d| d.cents.unwrap_or(i64::MAX));
        if let Some(d) = e.deals.first() {
            e.price = d.price.clone();
            e.cents = d.cents;
            e.url = d.url.clone();
        }
        e.store = e.deals.iter().map(|d| d.store.as_str()).collect::<Vec<_>>().join(" · ");
    }
    out
}

/// Fiche détaillée : description, genres, captures (Steam à la demande ; sinon ce que la liste donnait).
pub fn details(o: &Offer) -> Details {
    let mut d = Details { desc: o.desc.clone(), meta: o.meta.clone(), shots: vec![] };
    for deal in &o.deals {
        if let Some(id) = deal.url.strip_prefix("https://store.steampowered.com/app/") {
            steam::details(id.trim_end_matches('/'), &mut d);
        }
    }
    d
}

/// Visuels Steam d'un jeu par son appid.
pub fn steam_art(appid: &str) -> Art {
    steam::art(appid)
}

fn collect(f: impl Fn(&dyn Store) -> Vec<Offer> + Sync) -> Vec<Offer> {
    let stores = all();
    let lists = std::thread::scope(|sc| {
        let hs: Vec<_> = stores.iter().map(|s| sc.spawn(|| f(s.as_ref()))).collect();
        hs.into_iter().map(|h| h.join().unwrap_or_default()).collect::<Vec<_>>()
    });
    merge(lists)
}

/// Catalogue unifié « à la une » de toutes les boutiques.
pub fn catalog() -> Vec<Offer> {
    collect(|s| s.featured())
}

/// Recherche unifiée dans toutes les boutiques.
pub fn search(query: &str) -> Vec<Offer> {
    collect(|s| s.search(query))
}
