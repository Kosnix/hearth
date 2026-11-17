//! Steam sans le client : connexion et installation par SteamCMD, recherche des mises à jour.

use std::time::Duration;

use stores::steamcmd::{self, Login};

use crate::i18n::{self, tr};
use crate::{with_shell, Shell};

fn appid_of(game_id: &str) -> Option<&str> {
    game_id.strip_prefix("owned-steam-").or_else(|| game_id.strip_prefix("steam-"))
}

impl Shell {
    pub fn scmd_user(&self) -> Option<String> {
        i18n::pref("steamcmd_user")
    }

    /// Le jeu a-t-il été installé par SteamCMD ?
    pub fn is_scmd_game(&self, exe: &str) -> bool {
        !exe.is_empty() && std::path::Path::new(exe).starts_with(steamcmd::dir())
    }

    fn scmd_keyboard(&self, stage: u8, title: &str) {
        self.scmd_stage.set(stage);
        self.settings.kb_open("");
        self.kb_refresh();
        self.ui.set_kbx(0);
        self.ui.set_kby(0);
        self.ui.set_kb_title(tr(title).into());
        self.ui.set_zone(4);
    }

    /// Début de la connexion (identifiant puis mot de passe, puis code Steam Guard si besoin).
    pub fn scmd_begin(&self, pending: Option<String>) {
        *self.scmd_pending.borrow_mut() = pending;
        self.scmd_keyboard(1, "Identifiant Steam");
    }

    /// Texte validé au clavier virtuel pendant la connexion.
    pub fn scmd_text(&self, text: String) {
        match self.scmd_stage.get() {
            1 => {
                *self.scmd_user_tmp.borrow_mut() = text.trim().to_string();
                self.scmd_keyboard(2, "Mot de passe Steam");
            }
            2 => {
                *self.scmd_pass.borrow_mut() = text;
                self.ui.set_zone(0);
                self.scmd_stage.set(0);
                self.scmd_login(None);
            }
            _ => {
                self.ui.set_zone(0);
                self.scmd_stage.set(0);
                self.scmd_login(Some(text.trim().to_string()));
            }
        }
    }

    fn scmd_login(&self, code: Option<String>) {
        let (user, pass) = (self.scmd_user_tmp.borrow().clone(), self.scmd_pass.borrow().clone());
        self.push_notif("Steam", &tr("Connexion à Steam…"));
        std::thread::spawn(move || {
            let r = steamcmd::login(&user, &pass, code.as_deref());
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.scmd_login_done(user, r)));
        });
    }

    fn scmd_login_done(&self, user: String, r: Login) {
        match r {
            Login::Ok => {
                *self.scmd_pass.borrow_mut() = String::new();
                i18n::set_pref("steamcmd_user", &user);
                self.push_notif("Steam", &format!("{} {user}", tr("Connecté à Steam :")));
                if self.ui.get_zone() == 3 && self.ui.get_cat() == 12 {
                    self.open_cat(12);
                }
                if let Some(id) = self.scmd_pending.borrow_mut().take() {
                    self.steam_install(&id);
                }
                self.check_updates();
            }
            Login::NeedCode => {
                self.push_notif("Steam", &tr("Saisissez le code Steam Guard"));
                self.scmd_keyboard(3, "Code Steam Guard");
            }
            Login::Failed(e) => {
                *self.scmd_pass.borrow_mut() = String::new();
                self.push_notif("Steam", &format!("{} : {e}", tr("Connexion refusée")));
            }
        }
    }

    /// Installe (ou met à jour) un jeu Steam par SteamCMD ; connexion demandée au besoin.
    pub fn steam_install(&self, game_id: &str) {
        let Some(appid) = appid_of(game_id).map(String::from) else { return };
        let Some(user) = self.scmd_user() else {
            self.scmd_begin(Some(game_id.to_string()));
            return;
        };
        if !self.scmd_busy.borrow_mut().insert(appid.clone()) {
            self.push_notif(&tr("Installation"), &tr("Déjà en cours"));
            return;
        }
        let name = self.games.borrow().iter().find(|g| appid_of(&g.id) == Some(appid.as_str())).map(|g| g.name.clone()).unwrap_or_else(|| appid.clone());
        self.push_notif(&tr("Installation"), &format!("{name} : {}", tr("téléchargement en cours (plusieurs minutes)…")));
        let gid = game_id.to_string();
        std::thread::spawn(move || {
            let r = steamcmd::install(&user, &appid);
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.steam_install_done(&gid, &appid, &name, r)));
        });
    }

    fn steam_install_done(&self, game_id: &str, appid: &str, name: &str, r: Result<(), String>) {
        self.scmd_busy.borrow_mut().remove(appid);
        match r {
            Ok(()) => {
                self.push_notif(&tr("Installation"), &format!("{name} : {}", tr("installé")));
                self.updates.borrow_mut().retain(|a| a != appid);
                self.poll_installed();
            }
            Err(e) => {
                self.push_notif(&tr("Installation"), &format!("{name} : {} ({e})", tr("échec")));
                if e.contains("connexion requise") || e.contains("mot de passe") {
                    self.scmd_begin(Some(game_id.to_string()));
                }
            }
        }
    }

    /// Cherche les mises à jour des jeux installés par SteamCMD (en tâche de fond).
    pub fn check_updates(&self) {
        let Some(user) = self.scmd_user() else { return };
        let ids = steamcmd::installed_ids();
        if ids.is_empty() {
            return;
        }
        std::thread::spawn(move || {
            let latest = steamcmd::latest_builds(&user, &ids);
            let old: Vec<String> = ids.into_iter().filter(|id| match (steamcmd::local_build(id), latest.get(id)) {
                (Some(l), Some(n)) => &l != n,
                _ => false,
            }).collect();
            if !old.is_empty() {
                let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.updates_found(old)));
            }
        });
    }

    fn updates_found(&self, ids: Vec<String>) {
        let declined = self.declined.borrow();
        let mut upd = self.updates.borrow_mut();
        for id in ids {
            if !declined.contains(&id) && !upd.contains(&id) {
                upd.push(id);
            }
        }
    }

    /// Appelé chaque seconde : propose une mise à jour quand la console est libre (pas en jeu, pas dans un menu).
    pub fn maybe_prompt_update(&self) {
        if self.updates.borrow().is_empty() || !matches!(self.ui.get_zone(), 0 | 1) || !self.running.borrow().is_empty() {
            return;
        }
        if !wm::find_window(crate::MAIN_TITLE).is_some_and(|h| h == wm::foreground()) {
            return;
        }
        let Some(appid) = self.updates.borrow().first().cloned() else { return };
        let gid = self.games.borrow().iter().position(|g| appid_of(&g.id) == Some(appid.as_str()) && self.is_scmd_game(&g.exe));
        let Some(gid) = gid else {
            self.updates.borrow_mut().retain(|a| *a != appid);
            return;
        };
        self.updates.borrow_mut().retain(|a| *a != appid);
        self.declined.borrow_mut().insert(appid);
        self.update_menu(gid);
    }

    /// Récupère périodiquement : première recherche après 90 s, puis toutes les 6 h.
    pub fn schedule_updates() {
        slint::Timer::single_shot(Duration::from_secs(90), || with_shell(|s| s.check_updates()));
    }
}
