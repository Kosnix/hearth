//! Mises à jour de Hearth : dernière version publiée sur GitHub (releases), installée en silence après confirmation.

use std::os::windows::process::CommandExt;
use std::time::Duration;

use crate::i18n::tr;
use crate::menus::Act;
use crate::{with_shell, Shell};

const API: &str = "https://api.github.com/repos/Kosnix/hearth/releases/latest";
const ASSET: &str = "Hearth-Setup.exe";
const ASSET_PREFIX: &str = "https://github.com/Kosnix/hearth/";

fn parts(v: &str) -> Vec<u32> {
    v.trim_start_matches('v').split('.').map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect()
}

/// `latest` est-elle strictement plus récente que `current` ?
pub fn newer(latest: &str, current: &str) -> bool {
    let (a, b) = (parts(latest), parts(current));
    (0..a.len().max(b.len())).map(|i| (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0))).find(|(x, y)| x != y).is_some_and(|(x, y)| x > y)
}

impl Shell {
    /// Première recherche 60 s après le démarrage, puis toutes les 6 h.
    pub fn schedule_hearth_updates() {
        slint::Timer::single_shot(Duration::from_secs(60), || {
            with_shell(|s| s.check_hearth_update(false));
        });
    }

    pub fn check_hearth_update(&self, manual: bool) {
        if manual {
            self.push_notif("Hearth", &tr("Recherche des mises à jour…"));
        }
        std::thread::spawn(move || {
            let found = stores::fetch_json(API).and_then(|j| {
                let tag = j["tag_name"].as_str()?.to_string();
                let url = j["assets"].as_array()?.iter().find(|a| a["name"] == ASSET)?["browser_download_url"].as_str()?.to_string();
                (url.starts_with(ASSET_PREFIX) && newer(&tag, env!("CARGO_PKG_VERSION"))).then_some((tag.trim_start_matches('v').to_string(), url))
            });
            let _ = slint::invoke_from_event_loop(move || {
                with_shell(|s| {
                    if manual && found.is_none() {
                        s.push_notif("Hearth", &tr("Hearth est à jour"));
                    }
                    if let Some(f) = found {
                        let fresh = s.hearth_update.borrow().as_ref().is_none_or(|o| o.0 != f.0);
                        if fresh {
                            s.hearth_prompted.set(false);
                        }
                        *s.hearth_update.borrow_mut() = Some(f);
                        if manual {
                            s.hearth_prompted.set(false);
                        }
                    }
                });
                Shell::schedule_next_hearth_check();
            });
        });
    }

    fn schedule_next_hearth_check() {
        slint::Timer::single_shot(Duration::from_secs(6 * 3600), || with_shell(|s| s.check_hearth_update(false)));
    }

    /// Appelé chaque seconde : propose la mise à jour quand la console est libre (pas en jeu, pas dans un menu).
    pub fn maybe_prompt_hearth_update(&self) {
        if self.hearth_prompted.get() || !matches!(self.ui.get_zone(), 0 | 1) || !self.running.borrow().is_empty() || self.oobe_active() {
            return;
        }
        let Some((ver, _)) = self.hearth_update.borrow().clone() else { return };
        if !wm::find_window(crate::MAIN_TITLE).is_some_and(|h| h == wm::foreground()) {
            return;
        }
        self.hearth_prompted.set(true);
        self.menu_show(
            format!("{} {ver}", tr("Mise à jour de Hearth disponible :")),
            vec![(tr("Installer la mise à jour (silencieux)"), Act::SelfUpdate), (tr("Plus tard"), Act::Cancel)],
        );
    }

    /// Après confirmation : télécharge l'installateur, le lance en mode silencieux (il relance Hearth).
    pub fn hearth_update_install(&self) {
        let Some((_, url)) = self.hearth_update.borrow().clone() else { return };
        self.push_notif("Hearth", &tr("Téléchargement de la mise à jour…"));
        std::thread::spawn(move || {
            let path = std::env::temp_dir().join("Hearth-Setup-update.exe");
            let ok = stores::download(&url).is_some_and(|b| b.len() > 1_000_000 && b.starts_with(b"MZ") && std::fs::write(&path, b).is_ok());
            let started = ok && std::process::Command::new(&path).arg("/S").creation_flags(0x0000_0008 | 0x0800_0000).spawn().is_ok();
            if !started {
                let _ = slint::invoke_from_event_loop(|| with_shell(|s| s.push_notif("Hearth", &tr("Échec de la mise à jour"))));
            }
        });
    }
}
