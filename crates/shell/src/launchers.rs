//! Lanceurs de boutique dont certains jeux ont besoin pour démarrer (authentification, DRM, SDK
//! en ligne) : démarrés réduits juste avant le jeu s'ils ne tournent pas déjà. Steam a son propre
//! circuit (`scmd::steam_ready` : il doit en plus être connecté).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{with_shell, Game, Shell};

pub struct Launcher {
    exe: PathBuf,
    /// Nom des processus dont on réduit les fenêtres tant que le lanceur démarre.
    procs: &'static [&'static str],
    args: &'static [&'static str],
}

fn first_existing(paths: &[&str]) -> Option<PathBuf> {
    paths.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Le lanceur dont ce jeu a besoin, s'il en a un et qu'il est installé.
pub fn needed(g: &Game) -> Option<Launcher> {
    let exe = Path::new(&g.exe);
    // Jeux installés par le lanceur Epic (pas ceux de Legendary, qui gère lui-même l'authentification) :
    // beaucoup refusent de démarrer, ou démarrent hors ligne, sans le lanceur.
    if g.id.starts_with("epic-") && exe != stores::epiccmd::exe() {
        let exe = first_existing(&[
            r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe",
            r"C:\Program Files\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe",
            r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win32\EpicGamesLauncher.exe",
            r"C:\Program Files\Epic Games\Launcher\Portal\Binaries\Win32\EpicGamesLauncher.exe",
        ])?;
        return Some(Launcher { exe, procs: &["epicgameslauncher.exe"], args: &["-silent"] });
    }
    // GOG : seuls les jeux qui embarquent le SDK Galaxy (succès, multijoueur) en dépendent ; les autres
    // tournent très bien seuls, inutile d'ouvrir Galaxy pour eux.
    if g.store == "GOG" {
        let dir = exe.parent()?;
        if !["Galaxy64.dll", "Galaxy.dll"].iter().any(|d| dir.join(d).exists()) {
            return None;
        }
        let exe = first_existing(&[r"C:\Program Files (x86)\GOG Galaxy\GalaxyClient.exe", r"C:\Program Files\GOG Galaxy\GalaxyClient.exe"])?;
        return Some(Launcher { exe, procs: &["galaxyclient.exe"], args: &[] });
    }
    None
}

impl Launcher {
    fn running(&self) -> bool {
        self.exe.file_name().and_then(|n| n.to_str()).is_some_and(wm::process_running)
    }
}

impl Shell {
    /// Vrai si le jeu peut partir tout de suite. Sinon démarre son lanceur réduit, attend qu'il soit
    /// prêt (en arrière-plan) puis relance le jeu ; l'appelant abandonne ce lancement-ci.
    pub fn launcher_ready(&self, i: usize, g: &Game) -> bool {
        let Some(l) = needed(g) else { return true };
        if l.running() {
            return true;
        }
        if self.launcher_wait.replace(true) {
            return false;
        }
        // Aucune notification : l'écran de lancement du jeu est déjà affiché, le lanceur démarre derrière.
        let mut c = std::process::Command::new(&l.exe);
        c.args(l.args);
        if c.spawn().is_err() {
            // Impossible à démarrer : tenter le jeu quand même plutôt que de bloquer.
            self.launcher_wait.set(false);
            return true;
        }
        let procs = l.procs;
        std::thread::spawn(move || {
            // Prêt = processus présent puis quelques secondes pour sa connexion. Ses fenêtres (Epic en
            // ouvre une malgré `-silent` au tout premier démarrage) sont réduites au passage.
            let mut seen = None;
            for step in 0..120 {
                std::thread::sleep(Duration::from_millis(250));
                wm::minimize_windows_of(procs);
                let up = procs.iter().any(|p| wm::process_running(p));
                if up && seen.is_none() {
                    seen = Some(step);
                }
                if seen.is_some_and(|s| step - s >= 24) {
                    break;
                }
            }
            let _ = slint::invoke_from_event_loop(move || {
                with_shell(|s| {
                    s.launcher_wait.set(false);
                    s.launch(i);
                })
            });
        });
        false
    }
}
