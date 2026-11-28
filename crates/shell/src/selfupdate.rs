//! Mises à jour de Hearth : dernière version publiée sur GitHub (releases), installée en silence après confirmation.

use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use slint::Model;

use crate::i18n::tr;
use crate::menus::Act;
use crate::{with_shell, Notif, Shell};

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
            let resp = stores::fetch_json(API);
            let reached = resp.is_some();
            let found = resp.and_then(|j| {
                let tag = j["tag_name"].as_str()?.to_string();
                let url = j["assets"].as_array()?.iter().find(|a| a["name"] == ASSET)?["browser_download_url"].as_str()?.to_string();
                (url.starts_with(ASSET_PREFIX) && newer(&tag, env!("HEARTH_VERSION"))).then_some((tag.trim_start_matches('v').to_string(), url))
            });
            let _ = slint::invoke_from_event_loop(move || {
                with_shell(|s| {
                    s.hearth_checked.set(Some(reached));
                    // « À jour » seulement si GitHub a vraiment répondu (pas en cas d'échec réseau).
                    if manual && !reached {
                        s.push_notif("Hearth", &tr("Impossible de vérifier les mises à jour (réseau ?)"));
                    } else if manual && found.is_none() {
                        s.push_notif("Hearth", &tr("Hearth est à jour"));
                    }
                    if s.ui.get_zone() == 3 && s.ui.get_cat() == 6 {
                        s.open_cat_keep(6);
                    }
                    if let Some(f) = found {
                        let fresh = s.hearth_update.borrow().as_ref().is_none_or(|o| o.0 != f.0);
                        if fresh {
                            s.hearth_prompted.set(false);
                        }
                        *s.hearth_update.borrow_mut() = Some(f.clone());
                        if manual {
                            s.hearth_prompted.set(false);
                        }
                        // Notification épinglée (ne se supprime pas, déclenche l'installation) : reste
                        // affichée tant que la mise à jour n'est pas faite, pas juste le temps d'un toast.
                        if fresh || !s.notifs.iter().any(|n| n.pinned) {
                            s.push_hearth_update_notif(&f.0);
                        }
                    }
                });
                // Une seule chaîne de vérifications : une recherche manuelle n'en relance pas une autre.
                if !manual {
                    Shell::schedule_next_hearth_check();
                }
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
        // Fenêtre qui s'ouvre toute seule : un A destiné à autre chose ne doit pas installer (et redémarrer).
        self.ui.set_msel(1);
    }

    /// Après confirmation (menu ou notification épinglée) : télécharge l'installateur — progression
    /// visible dans Téléchargements, comme un jeu — puis le lance en mode silencieux (il redémarre le
    /// PC une fois l'installation faite ; Hearth, shell de la session, se relance tout seul après).
    pub fn hearth_update_install(&self) {
        if self.hearth_dl.borrow().is_some() {
            return; // déjà en cours
        }
        let Some((_, url)) = self.hearth_update.borrow().clone() else { return };
        let counters = Arc::new((AtomicU64::new(0), AtomicU64::new(0)));
        *self.hearth_dl.borrow_mut() = Some(counters.clone());
        self.dl_refresh();
        self.hearth_dl_timer.start(slint::TimerMode::Repeated, Duration::from_millis(200), || with_shell(|s| s.dl_refresh()));
        std::thread::spawn(move || {
            let path = std::env::temp_dir().join("Hearth-Setup-update.exe");
            let c2 = counters.clone();
            let bytes = stores::download_with_progress(&url, move |done, total| {
                c2.0.store(done, Ordering::Relaxed);
                c2.1.store(total.unwrap_or(0), Ordering::Relaxed);
            });
            let ok = bytes.is_some_and(|b| b.len() > 1_000_000 && b.starts_with(b"MZ") && std::fs::write(&path, b).is_ok());
            let started = ok && std::process::Command::new(&path).arg("/S").creation_flags(0x0000_0008 | 0x0800_0000).spawn().is_ok();
            let _ = slint::invoke_from_event_loop(move || {
                with_shell(|s| {
                    s.hearth_dl_timer.stop();
                    *s.hearth_dl.borrow_mut() = None;
                    if started {
                        s.push_notif("Hearth", &tr("Installation en cours… (redémarrage à la fin)"));
                        // La notification épinglée a fait son office : l'installateur silencieux prend le relais.
                        s.notifs.set_vec(s.notifs.iter().filter(|n| !n.pinned).collect::<Vec<Notif>>());
                        s.update_unread();
                    } else {
                        s.push_notif("Hearth", &tr("Échec de la mise à jour"));
                    }
                    s.dl_refresh();
                })
            });
        });
    }

    /// Progression 0..100 du téléchargement en cours de la mise à jour de Hearth, s'il y en a un.
    pub fn hearth_dl_pct(&self) -> Option<f32> {
        let g = self.hearth_dl.borrow();
        let c = g.as_ref()?;
        let total = c.1.load(Ordering::Relaxed);
        (total > 0).then(|| (c.0.load(Ordering::Relaxed) as f32 / total as f32 * 100.0).min(100.0))
    }

    /// Notification épinglée (ne se supprime pas tant que la mise à jour n'est pas faite) : son geste
    /// de suppression déclenche l'installation à la place (voir `notif_action`).
    fn push_hearth_update_notif(&self, ver: &str) {
        let keep: Vec<Notif> = self.notifs.iter().filter(|n| !n.pinned).collect();
        self.notifs.set_vec(keep);
        self.push_notif_ex("Hearth", &format!("{} {ver}", tr("Mise à jour disponible :")), true);
    }
}
