//! Connecteurs de boutiques (phase 5) : squelette. Chaque boutique implémente `Store`.
//! Les visuels d'un jeu vivent dans `%LOCALAPPDATA%\Hearth\assets\<id>\{cover,hero,logo}.png`.

use std::path::PathBuf;

/// Jeu connu d'une boutique (bibliothèque ou catalogue).
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub installed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Pas encore implémenté.
    Soon,
    /// Implémenté mais pas connecté (connexion par QR code).
    SignedOut,
    SignedIn,
}

pub trait Store {
    fn name(&self) -> &'static str;
    fn status(&self) -> Status;
    /// Jeux possédés par l'utilisateur.
    fn library(&self) -> Vec<Entry> {
        vec![]
    }
    /// Résultats du catalogue de la boutique.
    fn search(&self, _query: &str) -> Vec<Entry> {
        vec![]
    }
}

struct Stub(&'static str);

impl Store for Stub {
    fn name(&self) -> &'static str {
        self.0
    }
    fn status(&self) -> Status {
        Status::Soon
    }
}

pub fn all() -> Vec<Box<dyn Store>> {
    ["Steam", "Epic Games", "GOG", "Xbox"].into_iter().map(|n| Box::new(Stub(n)) as Box<dyn Store>).collect()
}

fn assets_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth").join("assets")
}

/// Chemin d'un visuel (`cover`, `hero` ou `logo`) s'il existe.
pub fn asset(id: &str, kind: &str) -> Option<PathBuf> {
    if id.is_empty() {
        return None;
    }
    ["png", "jpg", "jpeg"].iter().map(|ext| assets_dir().join(id).join(format!("{kind}.{ext}"))).find(|p| p.exists())
}
