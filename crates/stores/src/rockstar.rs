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
        std::path::Path::new(LAUNCHER).exists() || !reg::subkeys(HKEY_LOCAL_MACHINE, ROOT).is_empty()
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
