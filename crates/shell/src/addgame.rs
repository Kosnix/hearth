//! « Ajouter un jeu » : navigation dans les dossiers et déduction d'un titre depuis l'exécutable.

use std::path::{Path, PathBuf};

pub struct Entry {
    pub label: String,
    pub sub: String,
    /// Icône Segoe MDL2.
    pub glyph: &'static str,
    pub path: PathBuf,
    pub is_dir: bool,
}

fn user_dir(name: &str) -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("USERPROFILE")?).join(name);
    p.exists().then_some(p)
}

/// Racine : disques et dossiers usuels.
pub fn roots() -> Vec<Entry> {
    let mut v: Vec<Entry> = system::storage::drives()
        .into_iter()
        .map(|d| Entry { label: d.name.clone(), sub: if d.removable { "USB".into() } else { String::new() }, glyph: "\u{EDA2}", path: PathBuf::from(&d.name), is_dir: true })
        .collect();
    for (label, dir) in [("Bureau", "Desktop"), ("Téléchargements", "Downloads"), ("Documents", "Documents")] {
        if let Some(path) = user_dir(dir) {
            v.push(Entry { label: label.into(), sub: String::new(), glyph: "\u{E838}", path, is_dir: true });
        }
    }
    v
}

/// Sous-dossiers puis exécutables (.exe) d'un dossier.
pub fn list_dir(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut dirs = vec![];
    let mut exes = vec![];
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('$') || name.starts_with('.') {
            continue;
        }
        let path = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => dirs.push(Entry { label: name, sub: String::new(), glyph: "\u{E838}", path, is_dir: true }),
            Ok(_) if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) => {
                let size = e.metadata().map(|m| m.len() / 1_000_000).unwrap_or(0);
                exes.push(Entry { label: name, sub: format!("{size} Mo"), glyph: "\u{E7FC}", path, is_dir: false });
            }
            _ => {}
        }
    }
    dirs.sort_by_key(|e| e.label.to_lowercase());
    exes.sort_by_key(|e| e.label.to_lowercase());
    dirs.extend(exes);
    dirs.truncate(500);
    dirs
}

/// Titre probable d'un jeu : nom de l'exécutable, ou du dossier s'il est trop générique.
/// Recherche pour « Identifier le jeu » : SteamGridDB (clé enregistrée), sinon les catalogues des boutiques.
pub fn identify(title: &str) -> Vec<stores::Offer> {
    let key = crate::i18n::pref("steamgriddb_key").or_else(|| std::env::var("HEARTH_SGDB_KEY").ok()).filter(|k| !k.trim().is_empty());
    let found = key.map(|k| stores::search_sgdb(title, k.trim())).unwrap_or_default();
    if found.is_empty() { stores::search(title) } else { found }
}

pub fn guess_title(exe: &Path) -> String {
    let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let generic = ["launcher", "game", "start", "run", "play", "main", "app", "bin", "client", "shipping", "win64", "x64"];
    let parent = exe.ancestors().skip(1).filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).find(|n| !generic.contains(&n.to_lowercase().as_str()) && !n.eq_ignore_ascii_case("binaries"));
    let base = if generic.iter().any(|g| stem.to_lowercase().contains(g)) && parent.is_some() { parent.unwrap_or_default() } else { stem };
    let mut t: String = base.chars().map(|c| if matches!(c, '_' | '-' | '.') { ' ' } else { c }).collect();
    for junk in ["x64", "win64", "Win64", "Shipping", "shipping"] {
        t = t.replace(junk, " ");
    }
    // Sépare les mots collés (« SuperTuxKart » -> « Super Tux Kart » donnerait une mauvaise recherche : on garde tel quel).
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}
