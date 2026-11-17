//! Steam sans le client : connexion et téléchargements par SteamCMD, file d'attente, mises à jour.

use std::sync::atomic::Ordering;
use std::time::Duration;

use slint::Model;
use stores::steamcmd::{self, Login, Pending};

use crate::i18n::{self, tr};
use crate::{with_shell, DlRow, Shell};

fn appid_of(game_id: &str) -> Option<&str> {
    game_id.strip_prefix("owned-steam-").or_else(|| game_id.strip_prefix("steam-"))
}

#[derive(Clone, PartialEq)]
pub enum DlState {
    Queued,
    Active,
    Done,
    Failed(String),
}

#[derive(Clone)]
pub struct Dl {
    pub appid: String,
    pub name: String,
    pub pct: f32,
    pub state: DlState,
}

/// Action d'une ligne de l'écran Téléchargements.
#[derive(Clone)]
pub enum RowAct {
    Search,
    UpdateAll,
    Item(String),
    Outdated(String),
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

    /// Clavier fermé sans valider pendant la connexion.
    pub fn scmd_cancel(&self) {
        self.scmd_stage.set(0);
        *self.scmd_pass.borrow_mut() = String::new();
        *self.scmd_pending.borrow_mut() = None;
        if let Some(p) = self.scmd_proc.borrow_mut().take() {
            steamcmd::abandon(p);
        }
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
                self.scmd_keyboard(4, "Code Steam Guard de l'application Steam (vide : validation sur le téléphone)");
            }
            4 => {
                self.ui.set_zone(0);
                self.scmd_stage.set(0);
                let user = self.scmd_user_tmp.borrow().clone();
                let pass = std::mem::take(&mut *self.scmd_pass.borrow_mut());
                let code = Some(text.trim().to_string()).filter(|c| !c.is_empty());
                self.push_notif("Steam", &tr("Connexion à Steam… (si besoin, validez sur votre téléphone, 1 minute)"));
                std::thread::spawn(move || {
                    let (r, p) = steamcmd::login(&user, &pass, code.as_deref());
                    let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.scmd_login_done(user, r, p)));
                });
            }
            _ => {
                self.ui.set_zone(0);
                self.scmd_stage.set(0);
                let user = self.scmd_user_tmp.borrow().clone();
                let Some(p) = self.scmd_proc.borrow_mut().take() else { return };
                self.push_notif("Steam", &tr("Vérification du code…"));
                let code = text.trim().to_string();
                std::thread::spawn(move || {
                    let r = steamcmd::submit_code(p, &code);
                    let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.scmd_login_done(user, r, None)));
                });
            }
        }
    }

    fn scmd_login_done(&self, user: String, r: Login, proc: Option<Pending>) {
        match r {
            Login::Ok => {
                i18n::set_pref("steamcmd_user", &user);
                self.push_notif("Steam", &format!("{} {user}", tr("Connecté à Steam :")));
                if self.ui.get_zone() == 3 && self.ui.get_cat() == 12 {
                    self.open_cat(12);
                }
                if let Some(id) = self.scmd_pending.borrow_mut().take() {
                    self.steam_install(&id);
                }
                self.check_updates(false);
            }
            Login::NeedCode => {
                *self.scmd_proc.borrow_mut() = proc;
                self.push_notif("Steam", &tr("Saisissez le code Steam Guard (ou validez sur votre téléphone)"));
                self.scmd_keyboard(3, "Code Steam Guard, ou Valider (vide) si approuvé sur le téléphone");
            }
            Login::Failed(e) => {
                self.push_notif("Steam", &format!("{} : {e}", tr("Connexion refusée")));
            }
        }
    }

    fn name_of(&self, appid: &str) -> String {
        self.games.borrow().iter().find(|g| appid_of(&g.id) == Some(appid)).map(|g| g.name.clone()).unwrap_or_else(|| appid.to_string())
    }

    /// Met un jeu Steam dans la file de téléchargement (connexion demandée au besoin).
    pub fn steam_install(&self, game_id: &str) {
        let Some(appid) = appid_of(game_id).map(String::from) else { return };
        if self.scmd_user().is_none() {
            self.scmd_begin(Some(game_id.to_string()));
            return;
        }
        if self.dl.borrow().iter().any(|d| d.appid == appid && matches!(d.state, DlState::Queued | DlState::Active)) {
            self.push_notif(&tr("Téléchargements"), &tr("Déjà dans la file"));
            return;
        }
        self.dl.borrow_mut().retain(|d| d.appid != appid);
        let name = self.name_of(&appid);
        self.dl.borrow_mut().push(Dl { appid, name: name.clone(), pct: 0.0, state: DlState::Queued });
        self.push_notif(&tr("Téléchargements"), &format!("{name} : {}", tr("ajouté à la file")));
        self.dl_next();
        self.dl_refresh();
    }

    /// Lance le prochain téléchargement de la file (un seul à la fois).
    fn dl_next(&self) {
        if self.dl.borrow().iter().any(|d| d.state == DlState::Active) {
            return;
        }
        let Some(user) = self.scmd_user() else { return };
        let next = {
            let mut dl = self.dl.borrow_mut();
            let Some(d) = dl.iter_mut().find(|d| d.state == DlState::Queued) else { return };
            d.state = DlState::Active;
            d.appid.clone()
        };
        let pid = self.dl_pid.clone();
        std::thread::spawn(move || {
            let id = next.clone();
            let r = steamcmd::install(&user, &next, &pid, move |p| {
                let id = id.clone();
                let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.dl_progress(&id, p)));
            });
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.dl_done(&next, r)));
        });
    }

    fn dl_progress(&self, appid: &str, pct: f32) {
        if let Some(d) = self.dl.borrow_mut().iter_mut().find(|d| d.appid == appid && d.state == DlState::Active) {
            d.pct = pct;
        }
        self.dl_refresh();
    }

    fn dl_done(&self, appid: &str, r: Result<(), String>) {
        let name = self.name_of(appid);
        // Annulé par l'utilisateur : déjà retiré de la liste.
        let known = self.dl.borrow().iter().any(|d| d.appid == appid);
        if known {
            let state = match &r {
                Ok(()) => DlState::Done,
                Err(e) => DlState::Failed(e.clone()),
            };
            if let Some(d) = self.dl.borrow_mut().iter_mut().find(|d| d.appid == appid) {
                d.state = state;
                d.pct = 100.0;
            }
            match &r {
                Ok(()) => self.push_notif(&tr("Téléchargements"), &format!("{name} : {}", tr("installé"))),
                Err(e) => {
                    self.push_notif(&tr("Téléchargements"), &format!("{name} : {} ({e})", tr("échec")));
                    if e.contains("connexion requise") || e.contains("mot de passe") {
                        self.scmd_begin(Some(format!("steam-{appid}")));
                    }
                }
            }
        }
        if r.is_ok() {
            self.updates.borrow_mut().retain(|a| a != appid);
            self.poll_installed();
        }
        self.dl_next();
        self.dl_refresh();
    }

    /// Cherche les mises à jour des jeux installés par SteamCMD (en tâche de fond).
    pub fn check_updates(&self, manual: bool) {
        let Some(user) = self.scmd_user() else {
            if manual {
                self.push_notif(&tr("Téléchargements"), &tr("Connectez-vous à Steam (Réglages > Comptes) pour gérer les mises à jour"));
            }
            return;
        };
        let ids = steamcmd::installed_ids();
        if ids.is_empty() {
            if manual {
                self.push_notif(&tr("Téléchargements"), &tr("Aucun jeu installé par Hearth à mettre à jour"));
            }
            return;
        }
        if manual {
            self.push_notif(&tr("Téléchargements"), &tr("Recherche des mises à jour…"));
        }
        std::thread::spawn(move || {
            let latest = steamcmd::latest_builds(&user, &ids);
            let old: Vec<String> = ids
                .into_iter()
                .filter(|id| match (steamcmd::local_build(id), latest.get(id)) {
                    (Some(l), Some(n)) => &l != n,
                    _ => false,
                })
                .collect();
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.updates_found(old, manual)));
        });
    }

    fn updates_found(&self, old: Vec<String>, manual: bool) {
        let fresh = old.iter().filter(|a| !self.updates.borrow().contains(a)).count();
        *self.updates.borrow_mut() = old.clone();
        if manual && old.is_empty() {
            self.push_notif(&tr("Téléchargements"), &tr("Tous les jeux sont à jour"));
        } else if fresh > 0 {
            self.push_notif(&tr("Téléchargements"), &format!("{fresh} {}", tr("mise(s) à jour disponible(s)")));
        }
        self.dl_refresh();
    }

    /// Appelé chaque seconde : propose une mise à jour quand la console est libre (pas en jeu, pas dans un menu).
    pub fn maybe_prompt_update(&self) {
        if self.updates.borrow().is_empty() || !matches!(self.ui.get_zone(), 0 | 1) || !self.running.borrow().is_empty() {
            return;
        }
        if !wm::find_window(crate::MAIN_TITLE).is_some_and(|h| h == wm::foreground()) {
            return;
        }
        let busy = |a: &String| self.dl.borrow().iter().any(|d| d.appid == *a && matches!(d.state, DlState::Queued | DlState::Active));
        let Some(appid) = self.updates.borrow().iter().find(|a| !self.prompted.borrow().contains(*a) && !busy(a)).cloned() else { return };
        self.prompted.borrow_mut().insert(appid.clone());
        let gid = self.games.borrow().iter().position(|g| appid_of(&g.id) == Some(appid.as_str()) && self.is_scmd_game(&g.exe));
        if let Some(gid) = gid {
            self.update_menu(gid);
        }
    }

    /// Première recherche 90 s après le démarrage.
    pub fn schedule_updates() {
        slint::Timer::single_shot(Duration::from_secs(90), || with_shell(|s| s.check_updates(false)));
    }

    // --- Écran Téléchargements ---

    /// Reconstruit les lignes de l'écran et le badge de la barre du haut.
    pub fn dl_refresh(&self) {
        let dl = self.dl.borrow().clone();
        let busy = |a: &String| dl.iter().any(|d| d.appid == *a && matches!(d.state, DlState::Queued | DlState::Active));
        let outdated: Vec<String> = self.updates.borrow().iter().filter(|a| !busy(a)).cloned().collect();
        let mut rows: Vec<(DlRow, RowAct)> = vec![];
        let row = |name: String, status: String, pct: i32, kind: i32| DlRow { name: name.into(), status: status.into(), pct, kind };
        rows.push((row(tr("Rechercher les mises à jour"), String::new(), -1, 3), RowAct::Search));
        if outdated.len() > 1 {
            rows.push((row(format!("{} ({})", tr("Tout mettre à jour"), outdated.len()), String::new(), -1, 3), RowAct::UpdateAll));
        }
        for d in dl.iter().filter(|d| matches!(d.state, DlState::Active | DlState::Queued)) {
            let (status, pct) = if d.state == DlState::Active { (format!("{:.0} %", d.pct), d.pct as i32) } else { (tr("En attente"), -1) };
            rows.push((row(d.name.clone(), status, pct, 0), RowAct::Item(d.appid.clone())));
        }
        for a in &outdated {
            rows.push((row(self.name_of(a), tr("Mise à jour disponible"), -1, 1), RowAct::Outdated(a.clone())));
        }
        for d in dl.iter().filter(|d| matches!(d.state, DlState::Done | DlState::Failed(_))) {
            let status = match &d.state {
                DlState::Failed(e) => format!("{} : {e}", tr("échec")),
                _ => tr("Terminé"),
            };
            rows.push((row(d.name.clone(), status, -1, 2), RowAct::Item(d.appid.clone())));
        }
        let badge = dl.iter().filter(|d| matches!(d.state, DlState::Active | DlState::Queued)).count() + outdated.len();
        self.ui.set_dl_badge(badge as i32);
        *self.dl_acts.borrow_mut() = rows.iter().map(|(_, a)| a.clone()).collect();
        self.dl_rows.set_vec(rows.into_iter().map(|(r, _)| r).collect::<Vec<_>>());
        let max = self.dl_rows.row_count().saturating_sub(1) as i32;
        if self.ui.get_dlsel() > max {
            self.ui.set_dlsel(max);
        }
    }

    pub fn dl_pick(&self, i: usize) {
        let Some(act) = self.dl_acts.borrow().get(i).cloned() else { return };
        match act {
            RowAct::Search => self.check_updates(true),
            RowAct::UpdateAll => {
                let all = self.updates.borrow().clone();
                for a in all {
                    self.steam_install(&format!("steam-{a}"));
                }
            }
            RowAct::Outdated(a) => self.steam_install(&format!("steam-{a}")),
            RowAct::Item(a) => {
                let state = self.dl.borrow().iter().find(|d| d.appid == a).map(|d| d.state.clone());
                match state {
                    Some(DlState::Active) => {
                        self.dl.borrow_mut().retain(|d| d.appid != a);
                        steamcmd::cancel(self.dl_pid.load(Ordering::SeqCst));
                        self.push_notif(&tr("Téléchargements"), &tr("Téléchargement annulé"));
                    }
                    _ => self.dl.borrow_mut().retain(|d| d.appid != a),
                }
            }
        }
        self.dl_refresh();
    }
}
