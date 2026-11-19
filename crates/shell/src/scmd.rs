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
    pub done: f64,
    pub total: f64,
    pub speed: f64,
    pub eta: Option<u64>,
    pub t: Option<std::time::Instant>,
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
        i18n::pref(&user_key())
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
                i18n::set_pref(&user_key(), &user);
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
        if let Some(d) = self.dl.borrow().iter().find(|d| d.appid == appid) {
            return d.name.clone();
        }
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
        self.dl.borrow_mut().push(Dl { appid, name: name.clone(), pct: 0.0, done: 0.0, total: 0.0, speed: 0.0, eta: None, t: None, state: DlState::Queued });
        self.push_notif(&tr("Téléchargements"), &format!("{name} : {}", tr("ajouté à la file")));
        self.dl_next();
        self.dl_refresh();
    }

    /// Lance le prochain téléchargement de la file (un seul à la fois).
    fn dl_next(&self) {
        if self.dl.borrow().iter().any(|d| d.state == DlState::Active) {
            return;
        }
        let next = {
            let mut dl = self.dl.borrow_mut();
            let Some(d) = dl.iter_mut().find(|d| d.state == DlState::Queued) else { return };
            // Steam exige une session SteamCMD ; Epic utilise Legendary (session de Hearth).
            if !d.appid.starts_with("epic:") && self.scmd_user().is_none() {
                return;
            }
            d.state = DlState::Active;
            d.appid.clone()
        };
        let pid = self.dl_pid.clone();
        if let Some(app) = next.strip_prefix("epic:").map(String::from) {
            std::thread::spawn(move || {
                let id = next.clone();
                let r = stores::epiccmd::install(&app, &pid, move |pct, speed, eta| {
                    let id = id.clone();
                    let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.dl_progress_epic(&id, pct, speed, eta)));
                });
                let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.dl_done(&next, r)));
            });
            return;
        }
        let Some(user) = self.scmd_user() else { return };
        std::thread::spawn(move || {
            let id = next.clone();
            let r = steamcmd::install(&user, &next, &pid, move |done, total| {
                let id = id.clone();
                let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.dl_progress(&id, done, total)));
            });
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.dl_done(&next, r)));
        });
    }

    fn dl_progress_epic(&self, appid: &str, pct: f32, speed: f64, eta: Option<u64>) {
        if let Some(d) = self.dl.borrow_mut().iter_mut().find(|d| d.appid == appid && d.state == DlState::Active) {
            d.pct = pct.min(100.0);
            d.speed = speed;
            d.eta = eta;
        }
        self.dl_refresh();
    }

    /// Met un jeu Epic dans la file (Legendary, session de Hearth : aucune saisie).
    pub fn epic_install(&self, game_id: &str) {
        let owned = self.owned.borrow().iter().find(|(_, id)| id == game_id).map(|(o, _)| (o.key.rsplit(':').next().unwrap_or("").to_string(), o.title.clone()));
        let Some((app, title)) = owned.filter(|(a, _)| !a.is_empty()) else { return };
        if stores::account::session("Epic Games").is_none() {
            self.push_notif(&tr("Téléchargements"), &tr("Connectez-vous à Epic (Réglages > Comptes)"));
            return;
        }
        let key = format!("epic:{app}");
        if self.dl.borrow().iter().any(|d| d.appid == key && matches!(d.state, DlState::Queued | DlState::Active)) {
            self.push_notif(&tr("Téléchargements"), &tr("Déjà dans la file"));
            return;
        }
        self.dl.borrow_mut().retain(|d| d.appid != key);
        self.dl.borrow_mut().push(Dl { appid: key, name: title.clone(), pct: 0.0, done: 0.0, total: 0.0, speed: 0.0, eta: None, t: None, state: DlState::Queued });
        self.push_notif(&tr("Téléchargements"), &format!("{title} : {}", tr("ajouté à la file")));
        self.dl_next();
        self.dl_refresh();
    }

    /// Ouvre la session Legendary après la connexion Epic de Hearth (en tâche de fond).
    pub fn epic_prepare(&self) {
        std::thread::spawn(|| {
            let r = stores::epiccmd::auth();
            if let Err(e) = r {
                let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.push_notif("Epic", &format!("{} ({e})", tr("Préparation de l'installation impossible")))));
            }
        });
    }

    fn dl_progress(&self, appid: &str, done: f64, total: f64) {
        if let Some(d) = self.dl.borrow_mut().iter_mut().find(|d| d.appid == appid && d.state == DlState::Active) {
            let now = std::time::Instant::now();
            if let Some(t) = d.t {
                let dt = now.duration_since(t).as_secs_f64();
                if dt > 0.5 && done >= d.done {
                    let inst = (done - d.done) / dt;
                    d.speed = if d.speed == 0.0 { inst } else { 0.7 * d.speed + 0.3 * inst };
                }
            }
            d.t = Some(now);
            d.done = done;
            d.total = total;
            d.pct = (done / total.max(1.0) * 100.0).min(100.0) as f32;
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
        let row = |name: String, status: String, pct: i32, kind: i32| DlRow { name: name.into(), status: status.into(), pct, kind, cancel: false };
        rows.push((row(tr("Rechercher les mises à jour"), String::new(), -1, 3), RowAct::Search));
        if outdated.len() > 1 {
            rows.push((row(format!("{} ({})", tr("Tout mettre à jour"), outdated.len()), String::new(), -1, 3), RowAct::UpdateAll));
        }
        for d in dl.iter().filter(|d| matches!(d.state, DlState::Active | DlState::Queued)) {
            let (status, pct) = if d.state == DlState::Active { (active_status(d), d.pct as i32) } else { (tr("En attente"), -1) };
            let mut r = row(d.name.clone(), status, pct, 0);
            r.cancel = true;
            rows.push((r, RowAct::Item(d.appid.clone())));
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
                let busy = self.dl.borrow().iter().any(|d| d.appid == a && matches!(d.state, DlState::Active | DlState::Queued));
                if busy {
                    self.dl_cancel_ask(i);
                    return;
                }
                self.dl.borrow_mut().retain(|d| d.appid != a);
            }
        }
        self.dl_refresh();
    }
}

/// « 42 % · 12,3 Mo\/s · 8 min restantes ».
fn active_status(d: &Dl) -> String {
    let mut s = format!("{:.0} %", d.pct);
    if d.speed > 1.0 {
        let mbs = d.speed / 1_048_576.0;
        s.push_str(&format!("  ·  {} {}", format!("{mbs:.1}").replace('.', i18n::decimal_sep()), tr("Mo/s")));
        let left = d.eta.unwrap_or_else(|| ((d.total - d.done) / d.speed).max(0.0) as u64);
        let eta = if left < 60 {
            format!("{left} s")
        } else if left < 3600 {
            format!("{} min", left.div_ceil(60))
        } else {
            format!("{} h {:02} min", left / 3600, (left % 3600) / 60)
        };
        s.push_str(&format!("  ·  {eta} {}", tr("restant")));
    }
    s
}

impl Shell {
    /// Les jeux installés par SteamCMD utilisent l'API Steam : ils plantent si le client n'est pas lancé et connecté.
    /// Renvoie vrai si on peut lancer tout de suite ; sinon prépare Steam (démarrage, superposition désactivée une fois)
    /// et relance le jeu quand il est prêt.
    pub fn steam_ready(&self, i: usize, g: &crate::Game) -> bool {
        if let (Some(id), Some(dir)) = (g.id.strip_prefix("steam-"), std::path::Path::new(&g.exe).parent()) {
            let f = dir.join("steam_appid.txt");
            if !f.exists() {
                let _ = std::fs::write(f, id);
            }
        }
        let overlay_done = i18n::pref("steam_overlay_off").is_some() && i18n::pref("steam_input_off").is_some();
        if stores::steam_client_user() != 0 && overlay_done {
            self.steam_keep(30);
            return true;
        }
        if self.steam_wait.replace(true) {
            return false;
        }
        self.push_notif("Steam", &tr("Préparation de Steam…"));
        self.steam_keep(180);
        std::thread::spawn(move || {
            // Une fois : on coupe la superposition Steam (elle s'ouvre avec le bouton guide en même temps que le menu de Hearth).
            let patched = !overlay_done && stores::steam_disable_overlay();
            if stores::steam_client_user() == 0 {
                stores::steam_client_start(true);
            }
            let ok = (0..40).any(|_| {
                std::thread::sleep(Duration::from_secs(1));
                stores::steam_client_user() != 0
            });
            let _ = slint::invoke_from_event_loop(move || {
                with_shell(|s| {
                    s.steam_wait.set(false);
                    if patched {
                        i18n::set_pref("steam_overlay_off", "1");
                        i18n::set_pref("steam_input_off", "1");
                    }
                    if ok {
                        s.launch(i);
                    } else {
                        stores::steam_client_start(false);
                        s.steam_keep(900);
                        s.push_notif("Steam", &tr("Connectez-vous dans la fenêtre Steam (une seule fois), puis relancez le jeu"));
                    }
                })
            });
        });
        false
    }
}


impl Shell {
    /// Pendant `secs` secondes, réduit la fenêtre du client Steam dès qu'il est connecté (il prend sinon le focus du jeu).
    pub fn steam_keep(&self, secs: u64) {
        self.steam_until.set(Some(std::time::Instant::now() + Duration::from_secs(secs)));
        if !self.steam_timer.running() {
            self.steam_timer.start(slint::TimerMode::Repeated, Duration::from_millis(300), || with_shell(|s| s.steam_keep_tick()));
        }
    }

    fn steam_keep_tick(&self) {
        match self.steam_until.get() {
            Some(t) if std::time::Instant::now() < t => {
                if stores::steam_client_user() != 0 {
                    wm::minimize_windows_of(&["steam.exe", "steamwebhelper.exe"]);
                }
            }
            _ => {
                self.steam_until.set(None);
                self.steam_timer.stop();
            }
        }
    }
}

/// Clé de l'identifiant SteamCMD dans settings.json : une par profil.
pub fn user_key() -> String {
    match std::env::var("HEARTH_PROFILE_ID").ok().filter(|p| p != "default" && !p.is_empty()) {
        Some(p) => format!("steamcmd_user_{p}"),
        None => "steamcmd_user".into(),
    }
}

impl Shell {
    /// Demande confirmation avant d'annuler un téléchargement (bouton Annuler de la ligne).
    pub fn dl_cancel_ask(&self, i: usize) {
        let Some(RowAct::Item(a)) = self.dl_acts.borrow().get(i).cloned() else { return };
        let name = self.dl.borrow().iter().find(|d| d.appid == a).map(|d| d.name.clone()).unwrap_or_default();
        self.menu_show(
            format!("{} : {name} ?", tr("Annuler le téléchargement")),
            vec![(tr("Oui, annuler"), crate::menus::Act::CancelDl(i)), (tr("Non, continuer"), crate::menus::Act::Cancel)],
        );
    }

    /// Annule réellement (après confirmation) : arrête le processus s'il est actif, ou retire l'élément de la file.
    pub fn dl_cancel(&self, i: usize) {
        let Some(RowAct::Item(a)) = self.dl_acts.borrow().get(i).cloned() else { return };
        let active = self.dl.borrow().iter().any(|d| d.appid == a && d.state == DlState::Active);
        self.dl.borrow_mut().retain(|d| d.appid != a);
        if active {
            if a.starts_with("epic:") {
                stores::epiccmd::cancel(self.dl_pid.load(Ordering::SeqCst));
            } else {
                steamcmd::cancel(self.dl_pid.load(Ordering::SeqCst));
            }
        }
        self.push_notif(&tr("Téléchargements"), &tr("Téléchargement annulé"));
        self.ui.set_dlbtn(false);
        self.dl_refresh();
    }
}
