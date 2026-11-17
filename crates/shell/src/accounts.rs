//! Comptes des boutiques : connexion (QR code), bibliothèque possédée, installation des clients et des jeux.

use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use stores::account::{self, Owned, Poll};

use crate::i18n::tr;
use crate::{with_shell, Game, Shell};

static LOGIN_GEN: AtomicU32 = AtomicU32::new(0);

impl Shell {
    /// Recalcule la détection des clients et le nombre de jeux installés par boutique.
    pub fn refresh_store_status(&self) {
        *self.store_status.borrow_mut() =
            stores::all().iter().map(|s| (s.name().to_string(), s.detected(), self.games.borrow().iter().filter(|g| g.store == s.name() && !g.install).count())).collect();
    }

    /// Installe le client d'une boutique via winget (en tâche de fond, notifications de progression).
    pub fn account_install_client(&self, store: String) {
        let Some(id) = system::install::client_id(&store) else { return };
        self.push_notif(&tr("Installation"), &format!("{} {store}…", tr("Installation du client")));
        if !system::admin::enabled() {
            self.push_notif(&tr("Autorisation administrateur"), &tr("Autorisation administrateur requise : acceptez la fenêtre Windows (une seule fois)"));
        }
        std::thread::spawn(move || {
            let r = system::install::winget(id);
            let _ = slint::invoke_from_event_loop(move || {
                with_shell(|s| {
                    match r {
                        Ok(()) => s.push_notif(&tr("Installation"), &format!("{store} : {}", tr("client installé"))),
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
        self.ui.set_qr_return(3);
        self.ui.set_zone(5);
        *self.login.borrow_mut() = Some(ch.clone());
        if !ch.needs_code {
            std::thread::spawn(move || {
                for _ in 0..(300 / ch.interval.max(2)) {
                    std::thread::sleep(Duration::from_secs(ch.interval.max(2)));
                    if LOGIN_GEN.load(Ordering::SeqCst) != gen {
                        return;
                    }
                    match account::poll(&ch) {
                        Poll::Pending => {}
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
    }

    fn login_done(&self, store: &str, name: &str) {
        LOGIN_GEN.fetch_add(1, Ordering::SeqCst);
        *self.login.borrow_mut() = None;
        if self.ui.get_zone() == 5 {
            self.ui.set_zone(3);
        }
        self.push_notif(&tr("Connexion"), &format!("{} {store} : {name}", tr("Connecté à")));
        if self.ui.get_zone() == 3 && self.ui.get_cat() == 12 {
            self.open_cat(12);
        }
        self.refresh_owned();
    }

    pub fn logout(&self, store: &str) {
        account::logout(store);
        self.push_notif(&tr("Connexion"), &format!("{store} : {}", tr("déconnecté")));
        self.refresh_owned();
    }

    /// Jeux possédés des boutiques connectées : onglet Bibliothèque (badge « À installer »).
    pub fn refresh_owned(&self) {
        let connected: Vec<&'static str> = ["Steam", "Epic Games"].into_iter().filter(|s| account::session(s).is_some()).collect();
        if connected.is_empty() {
            self.owned_ready(vec![]);
            return;
        }
        std::thread::spawn(move || {
            let mut items: Vec<(Owned, String)> = vec![];
            for store in connected {
                for o in account::library(store).into_iter().take(300) {
                    let slug = if store == "Steam" { "steam" } else { "epic" };
                    let id = format!("owned-{slug}-{}", o.key.replace([':', '/', '\\'], "_"));
                    items.push((o, id));
                }
            }
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.owned_ready(items)));
        });
    }

    fn owned_ready(&self, items: Vec<(Owned, String)>) {
        *self.owned.borrow_mut() = items;
        self.reload_games(stores::installed());
        self.start_art();
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
            let _ = Command::new("cmd").args(["/c", "start", "", uri]).spawn();
            self.push_notif(&tr("Installation"), &format!("{} ({}) : {}", g.name, g.store, tr("à valider dans la fenêtre du client")));
        }
    }
}
