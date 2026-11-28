//! Comptes des boutiques : connexion (QR code), bibliothèque possédée, installation des clients et des jeux.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use slint::Model;
use stores::account::{self, Owned, Poll};

use crate::i18n::tr;
use crate::{with_shell, Game, Shell};

static LOGIN_GEN: AtomicU32 = AtomicU32::new(0);
const STORES: [&str; 3] = ["Steam", "Epic Games", "GOG"];

impl Shell {
    /// Recalcule la détection des clients et le nombre de jeux installés par boutique.
    pub fn refresh_store_status(&self) {
        *self.store_status.borrow_mut() =
            stores::all().iter().map(|s| (s.name().to_string(), s.detected(), self.games.borrow().iter().filter(|g| g.store == s.name() && !g.install).count())).collect();
    }

    /// Installe le client d'une boutique via winget (en tâche de fond, notifications de progression).
    pub fn account_install_client(&self, store: String) {
        let Some(id) = system::install::client_id(&store) else { return };
        // Déjà en cours pour cette boutique (validé deux fois) : une seule installation.
        thread_local! {
            static INSTALLING: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        if INSTALLING.with(|l| l.borrow().contains(&store)) {
            return;
        }
        INSTALLING.with(|l| l.borrow_mut().push(store.clone()));
        self.push_notif(&tr("Installation"), &format!("{} {store}…", tr("Installation du client")));
        std::thread::spawn(move || {
            // Hors du fil de l'interface (`schtasks`) : prévient seulement si l'invite Windows va s'afficher.
            if !system::admin::enabled() {
                let _ = slint::invoke_from_event_loop(|| {
                    with_shell(|s| s.push_notif(&tr("Autorisation administrateur"), &tr("Autorisation administrateur requise : acceptez la fenêtre Windows (une seule fois)")))
                });
            }
            let r = system::install::winget(id);
            let _ = slint::invoke_from_event_loop(move || {
                INSTALLING.with(|l| l.borrow_mut().retain(|x| *x != store));
                with_shell(|s| {
                    match r {
                        Ok(()) => {
                            s.push_notif(&tr("Installation"), &format!("{store} : {}", tr("client installé")));
                            if store == "GOG" {
                                s.open_client(&store);
                            }
                        }
                        Err(e) => s.push_notif(&tr("Installation"), &format!("{store} : {} ({e})", tr("échec de l'installation du client"))),
                    }
                    s.refresh_store_status();
                    if s.ui.get_zone() == 3 && s.ui.get_cat() == 12 {
                        s.open_cat(12);
                    }
                })
            });
        });
    }

    pub fn begin_login(&self, store: String) {
        let gen = LOGIN_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        self.push_notif(&tr("Connexion"), &format!("{} {store}…", tr("Connexion à")));
        std::thread::spawn(move || {
            let ch = account::begin(&store);
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.login_started(gen, store, ch)));
        });
    }

    fn login_started(&self, gen: u32, store: String, ch: Option<account::Challenge>) {
        if LOGIN_GEN.load(Ordering::SeqCst) != gen {
            return;
        }
        let Some(ch) = ch else {
            self.push_notif(&tr("Connexion"), &format!("{store} : {}", tr("service indisponible")));
            return;
        };
        let Some((w, h, px)) = stores::qr::rgb(&ch.url, 8) else { return };
        let buf = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(&px, w, h);
        self.ui.set_qr(slint::Image::from_rgb8(buf));
        self.ui.set_qr_title(format!("{} — {store}", tr("Connexion")).into());
        self.ui.set_qr_url(if ch.needs_code { "epicgames.com".into() } else { ch.code.clone() }.into());
        if ch.needs_code {
            self.ui.set_qr_hint(tr("Connectez-vous sur votre téléphone, puis saisissez le code d'autorisation affiché").into());
            self.ui.set_qr_action(tr("Saisir le code").into());
        } else {
            self.ui.set_qr_hint(tr("Scannez avec l'application Steam sur votre téléphone puis validez").into());
            self.ui.set_qr_action("".into());
        }
        self.desktop_show_login(&format!("{} — {store}", tr("Connexion")), self.ui.get_qr(), &self.ui.get_qr_hint(), ch.needs_code);
        self.ui.set_qr_return(self.home_zone());
        self.ui.set_zone(5);
        *self.login.borrow_mut() = Some(ch.clone());
        if !ch.needs_code {
            let mut ch = ch;
            std::thread::spawn(move || {
                for _ in 0..(300 / ch.interval.max(2)) {
                    std::thread::sleep(Duration::from_secs(ch.interval.max(2)));
                    if LOGIN_GEN.load(Ordering::SeqCst) != gen {
                        return;
                    }
                    match account::poll(&mut ch) {
                        Poll::Pending => {}
                        // QR code renouvelé par Steam : le nouveau remplace l'ancien à l'écran (et sur le bureau).
                        Poll::Renewed => {
                            let url = ch.url.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                with_shell(|s| {
                                    if LOGIN_GEN.load(Ordering::SeqCst) != gen {
                                        return;
                                    }
                                    if let Some((w, h, px)) = stores::qr::rgb(&url, 8) {
                                        let buf = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(&px, w, h);
                                        s.ui.set_qr(slint::Image::from_rgb8(buf));
                                        s.desktop_show_login(&s.ui.get_qr_title(), s.ui.get_qr(), &s.ui.get_qr_hint(), false);
                                    }
                                })
                            });
                        }
                        Poll::Done(session) => {
                            let store = ch.store.to_string();
                            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.login_done(&store, &session.name)));
                            return;
                        }
                        Poll::Failed(e) => {
                            let store = ch.store.to_string();
                            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.push_notif(&tr("Connexion"), &format!("{store} : {e}"))));
                            return;
                        }
                    }
                }
            });
        }
    }

    /// Epic : saisie du code d'autorisation au clavier virtuel.
    pub fn login_code(&self) {
        self.kb_login.set(true);
        self.settings.kb_open("");
        self.kb_refresh();
        self.ui.set_kbx(0);
        self.ui.set_kby(0);
        self.ui.set_kb_title(tr("Code d'autorisation").into());
        self.ui.set_zone(4);
    }

    pub fn login_exchange(&self, code: String) {
        let Some(store) = self.login.borrow().as_ref().map(|c| c.store.to_string()) else { return };
        std::thread::spawn(move || {
            let r = account::exchange(&store, &code);
            let _ = slint::invoke_from_event_loop(move || {
                with_shell(|s| match r {
                    Ok(session) => s.login_done(&store, &session.name),
                    Err(e) => {
                        s.ui.set_zone(5);
                        s.push_notif(&tr("Connexion"), &format!("{store} : {e}"));
                    }
                })
            });
        });
    }

    pub fn login_cancel(&self) {
        LOGIN_GEN.fetch_add(1, Ordering::SeqCst);
        *self.login.borrow_mut() = None;
        // Le QR code est aussi affiché dans la version bureau : il disparaît des deux côtés.
        self.desktop_login_close();
    }

    fn login_done(&self, store: &str, name: &str) {
        LOGIN_GEN.fetch_add(1, Ordering::SeqCst);
        *self.login.borrow_mut() = None;
        if self.ui.get_zone() == 5 {
            self.ui.set_zone(3);
        }
        self.push_notif(&tr("Connexion"), &format!("{} {store} : {name}", tr("Connecté à")));
        self.desktop_login_close();
        if self.ui.get_zone() == 3 && self.ui.get_cat() == 12 {
            self.open_cat(12);
        }
        if store == "Steam" && self.scmd_user().is_none() {
            // Étape 2 : identifiants de téléchargement (SteamCMD), une seule fois, dans la foulée.
            self.push_notif("Steam", &tr("Étape 2 : identifiant Steam pour télécharger les jeux"));
            self.scmd_begin(None);
        }
        if store == "Epic Games" {
            self.epic_prepare();
        }
        self.refresh_owned();
    }

    pub fn logout(&self, store: &str) {
        account::logout(store);
        if store == "Steam" {
            crate::i18n::set_pref(&crate::scmd::user_key(), "");
        }
        if store == "Epic Games" {
            std::thread::spawn(stores::epiccmd::logout);
        }
        self.push_notif(&tr("Connexion"), &format!("{store} : {}", tr("déconnecté")));
        self.refresh_owned();
    }

    /// Jeux possédés des boutiques connectées : onglet Bibliothèque (badge « À installer »).
    pub fn refresh_owned(&self) {
        self.refresh_onboard();
        // Sessions, bibliothèques distantes et lecture des jeux installés : tout hors du thread de l'interface.
        self.refresh_owned_try(0);
    }

    /// Un échec réseau (fréquent au démarrage du PC, Wi-Fi pas encore connecté) ne vide jamais la liste mémorisée : on la garde et on réessaie.
    fn refresh_owned_try(&self, attempt: u32) {
        std::thread::spawn(move || {
            let old = account::owned_cache_load();
            let connected: Vec<&'static str> = ["Steam", "Epic Games"].into_iter().filter(|s| account::session(s).is_some()).collect();
            let mut items: Vec<(Owned, String)> = vec![];
            let mut failed = false;
            for store in connected {
                // Toute la bibliothèque (plus de plafond à 300 jeux : les jaquettes ne sont chargées
                // qu'à l'affichage). Échec de lecture (réseau, session) ≠ compte réellement sans jeu.
                match account::library(store) {
                    Some(list) => items.extend(list.into_iter().map(|o| {
                        let slug = if store == "Steam" { "steam" } else { "epic" };
                        let id = format!("owned-{slug}-{}", o.key.replace([':', '/', '\\'], "_"));
                        (o, id)
                    })),
                    None => {
                        items.extend(old.iter().filter(|(o, _)| o.store == store).cloned());
                        // Session refusée (jetons expirés) : on prévient, une fois, au lieu de réessayer.
                        if account::take_expired(store) {
                            let _ = slint::invoke_from_event_loop(move || {
                                with_shell(|s| s.push_notif(store, &tr("Session expirée : reconnectez-vous (Réglages > Comptes)")))
                            });
                        } else {
                            failed = true;
                        }
                    }
                }
            }
            if !failed {
                account::owned_cache_save(&items);
            }
            let installed = stores::installed();
            let _ = slint::invoke_from_event_loop(move || {
                with_shell(|s| {
                    s.owned_ready(items, installed);
                    if failed && attempt < 20 {
                        slint::Timer::single_shot(Duration::from_secs(20), move || with_shell(|s| s.refresh_owned_try(attempt + 1)));
                    }
                })
            });
        });
    }

    fn owned_ready(&self, items: Vec<(Owned, String)>, installed: Vec<stores::Game>) {
        *self.owned.borrow_mut() = items;
        self.reload_games(installed);
        self.start_art();
        self.mark_owned_offers();
    }

    /// Titres (normalisés) déjà dans la bibliothèque : achetés ou installés.
    pub fn owned_titles(&self) -> std::collections::HashSet<String> {
        let mut set: std::collections::HashSet<String> = self.owned.borrow().iter().map(|(o, _)| stores::norm_title(&o.title)).collect();
        set.extend(self.games.borrow().iter().filter(|g| !g.install && !g.store.is_empty()).map(|g| stores::norm_title(&g.name)));
        set
    }

    /// Met à jour la mention « Dans la bibliothèque » des offres déjà affichées.
    pub fn mark_owned_offers(&self) {
        let set = self.owned_titles();
        for i in 0..self.offers.row_count() {
            let Some(mut row) = self.offers.row_data(i) else { continue };
            let own = set.contains(&stores::norm_title(&row.title));
            if row.owned != own {
                row.owned = own;
                self.offers.set_row_data(i, row);
            }
        }
    }

    /// Ajoute à la liste les jeux possédés qui ne sont pas (encore) installés.
    pub fn append_owned(&self, games: &mut Vec<Game>) {
        for (o, id) in self.owned.borrow().iter() {
            let installed = games.iter().any(|g| {
                !g.install
                    && match o.store {
                        "Steam" => g.id == format!("steam-{}", o.key),
                        _ => g.store == o.store && g.name.eq_ignore_ascii_case(&o.title),
                    }
            });
            if installed {
                continue;
            }
            let Some(uri) = account::install_uri(o) else { continue };
            let mut game = Game { name: o.title.clone(), id: id.clone(), uri: Some(uri), store: o.store.into(), install: true, ..Default::default() };
            if let Some(ov) = self.overrides.borrow().get(id) {
                game.hidden = ov.hidden;
            }
            games.push(game);
        }
    }

    /// Installe un jeu possédé via le client de sa boutique (installé au besoin).
    pub fn install_owned(&self, g: &Game) {
        if g.id.starts_with("owned-epic-") {
            self.epic_install(&g.id);
            return;
        }
        if g.id.starts_with("owned-steam-") {
            self.steam_install(&g.id);
            return;
        }
        let detected = self.store_status.borrow().iter().any(|(n, d, _)| *n == g.store && *d);
        if !detected {
            self.account_install_client(g.store.clone());
            return;
        }
        if let Some(uri) = &g.uri {
            system::open_uri(uri);
            self.push_notif(&tr("Installation"), &format!("{} ({}) : {}", g.name, g.store, tr("à valider dans la fenêtre du client")));
        }
    }
}

impl Shell {
    /// Boutiques sans compte connecté (affichées comme invitations dans l'onglet Boutique).
    pub fn refresh_onboard(&self) {
        let label: Vec<slint::SharedString> = if self.missing_stores().is_empty() { vec![] } else { vec![tr("Ajouter une boutique").into()] };
        self.ui.set_onboard(std::rc::Rc::new(slint::VecModel::from(label)).into());
        self.ui.set_obsel(0);
    }

    /// Indices (dans `STORES`) des boutiques pas encore ajoutées : compte non connecté (Steam, Epic) ou client absent (GOG).
    fn missing_stores(&self) -> Vec<usize> {
        let st = self.store_status.borrow();
        (0..STORES.len())
            .filter(|&i| match STORES[i] {
                "Steam" | "Epic Games" => account::session(STORES[i]).is_none(),
                n => !st.iter().any(|(s, d, _)| s == n && *d),
            })
            .collect()
    }

    pub fn onboard_pick(&self, _i: usize) {
        let items: Vec<(String, crate::menus::Act)> = self
            .missing_stores()
            .into_iter()
            .map(|i| (STORES[i].to_string(), crate::menus::Act::ConnectStore(i)))
            .chain([(tr("Annuler"), crate::menus::Act::Cancel)])
            .collect();
        self.menu_show(tr("Ajouter une boutique"), items);
    }

    pub fn connect_store(&self, i: usize) {
        let Some(&store) = STORES.get(i) else { return };
        match store {
            "Steam" | "Epic Games" => {
                self.ui.set_qr_return(0);
                self.begin_login(store.to_string());
            }
            _ => {
                if self.open_client(store) {
                    self.refresh_store_status();
                    self.refresh_onboard();
                } else {
                    self.account_install_client(store.to_string());
                }
            }
        }
    }

    /// Ouvre le client GOG (connexion au compte dans sa fenêtre) ; faux s'il n'est pas installé.
    fn open_client(&self, store: &str) -> bool {
        let exe = [r"C:\Program Files (x86)\GOG Galaxy\GalaxyClient.exe", r"C:\Program Files\GOG Galaxy\GalaxyClient.exe"].into_iter().map(std::path::PathBuf::from).find(|p| p.exists());
        let Some(exe) = exe else { return false };
        let _ = std::process::Command::new(exe).spawn();
        self.push_notif(store, &tr("Connectez-vous dans la fenêtre du client"));
        true
    }
}
