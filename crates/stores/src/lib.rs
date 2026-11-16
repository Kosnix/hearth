//! Connecteurs de boutiques : bibliothèque installée (lecture locale), visuels, catalogue « à la une ».
//! Visuels mis en cache dans `%LOCALAPPDATA%\Hearth\assets\<id>\{cover,hero,logo}.<ext>`.

mod art;
mod epic;
mod gog;
mod http;
mod reg;
mod steam;

pub mod qr;

pub use art::{asset, ensure_art, fetch_image};

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
    pub image: String,
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
}

pub fn all() -> Vec<Box<dyn Store>> {
    vec![Box::new(steam::Steam), Box::new(epic::Epic), Box::new(gog::Gog)]
}

/// Tous les jeux installés, toutes boutiques confondues.
pub fn installed() -> Vec<Game> {
    all().iter().flat_map(|s| s.installed()).collect()
}
