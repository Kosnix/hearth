//! Rockstar Games Launcher : jeux installés lus dans le registre (le lanceur n'a pas d'API publique).

use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;

use crate::{reg, Art, Game, Launch, Offer, Store};

pub struct Rockstar;

const ROOT: &str = r"SOFTWARE\WOW6432Node\Rockstar Games";
const LAUNCHER: &str = r"C:\Program Files\Rockstar Games\Launcher\Launcher.exe";

impl Store for Rockstar {
    fn name(&self) -> &'static str {
        "Rockstar Games"
    }

    fn detected(&self) -> bool {
        launcher().is_some() || !reg::subkeys(HKEY_LOCAL_MACHINE, ROOT).is_empty()
    }

    fn installed(&self) -> Vec<Game> {
        let mut games = vec![];
        for key in reg::subkeys(HKEY_LOCAL_MACHINE, ROOT) {
            let Some(folder) = reg::string(HKEY_LOCAL_MACHINE, &format!(r"{ROOT}\{key}"), "InstallFolder") else { continue };
            let dir = std::path::PathBuf::from(folder.trim_end_matches('\\'));
            if !dir.exists() {
                continue;
            }
            // Exécutables connus ; sinon le plus gros .exe du dossier.
            let known = ["PlayGTAV.exe", "RDR2.exe", "LANoire.exe", "MaxPayne3.exe", "GTAIV.exe", "GTAVLauncher.exe"];
            let exe = known.iter().map(|k| dir.join(k)).find(|p| p.exists()).or_else(|| crate::steamcmd::main_exe(&dir));
            let Some(exe) = exe else { continue };
            let title = key.trim_start_matches("Rockstar Games ").to_string();
            let id = format!("rockstar-{}", title.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>());
            games.push(Game { id, store: "Rockstar Games", title, launch: Launch::Exe { path: exe.to_string_lossy().into_owned(), args: vec![] }, art: Art::default() });
        }
        games
    }

    fn featured(&self) -> Vec<Offer> {
        vec![]
    }

    fn search(&self, _query: &str) -> Vec<Offer> {
        vec![]
    }
}

/// Jeux Rockstar du lanceur (titre affiché, identifiant court) : proposés dans la Bibliothèque quand le lanceur est installé.
pub const CATALOG: [(&str, &str); 5] = [
    ("Grand Theft Auto V", "gtav"),
    ("Red Dead Redemption 2", "rdr2"),
    ("L.A. Noire", "lanoire"),
    ("Max Payne 3", "maxpayne3"),
    ("Grand Theft Auto IV", "gtaiv"),
];

/// Exécutable du Rockstar Games Launcher, s'il est installé.
pub fn launcher() -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(LAUNCHER);
    if p.exists() {
        return Some(p);
    }
    for root in [r"SOFTWARE\WOW6432Node\Rockstar Games\Launcher", r"SOFTWARE\Rockstar Games\Launcher"] {
        if let Some(d) = reg::string(HKEY_LOCAL_MACHINE, root, "InstallFolder") {
            let p = std::path::PathBuf::from(d.trim_end_matches('\\')).join("Launcher.exe");
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Le titre est-il l'un des jeux du catalogue Rockstar ? (comparaison sans casse ni ponctuation)
pub fn same_title(a: &str, b: &str) -> bool {
    crate::norm_title(a) == crate::norm_title(b)
}
