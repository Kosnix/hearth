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

pub use art::{asset, ensure_art, fetch_first, save_art};

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
    title.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Fusionne les listes de plusieurs boutiques : alternance des sources, doublons regroupés
/// (même titre) avec la boutique la moins chère pour le lien d'achat.
fn merge(lists: Vec<Vec<Offer>>) -> Vec<Offer> {
    let mut out: Vec<Offer> = vec![];
    let mut keys: Vec<String> = vec![];
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..longest {
        for list in &lists {
            let Some(o) = list.get(i) else { continue };
            let k = norm(&o.title);
            match keys.iter().position(|x| *x == k) {
                Some(p) => {
                    let e = &mut out[p];
                    e.store = format!("{} · {}", e.store, o.store);
                    if o.cents.is_some_and(|c| e.cents.is_none_or(|ec| c < ec)) {
                        e.cents = o.cents;
                        e.price = o.price.clone();
                        e.url = o.url.clone();
                    }
                }
                None => {
                    keys.push(k);
                    out.push(o.clone());
                }
            }
        }
    }
    out
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

/// Enregistre une jaquette (première URL téléchargeable) sous `id`, pour les jeux possédés non installés.
pub fn save_cover(id: &str, urls: &[String]) -> bool {
    let offer = Offer { images: urls.to_vec(), ..Default::default() };
    save_art(id, &offer)
}
