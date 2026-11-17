#![windows_subsystem = "windows"]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use input::Nav;
use i18n::tr;
use slint::{ComponentHandle, Model, SharedString, VecModel};

mod addgame;
mod accounts;
mod addflow;
mod i18n;
mod menus;
mod scmd;
mod playtime;
mod prefs;
mod settings;

slint::include_modules!();

/// Code de sortie lu par le superviseur : ouvrir le bureau Windows.
const EXIT_DESKTOP: i32 = 10;
const MAIN_TITLE: &str = "Hearth";
const GUIDE_TITLE: &str = "Hearth Guide";
const SPLASH_TITLE: &str = "Hearth Splash";
const GUIDE_WIDTH: f32 = 460.0; // px logiques
const MAX_NOTES: usize = 5;

#[derive(serde::Deserialize, serde::Serialize, Clone, Default)]
struct Game {
    name: String,
    #[serde(default)]
    id: String,
    exe: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cover: String,
    #[serde(default)]
    anticheat: bool,
    #[serde(skip)]
    uri: Option<String>,
    #[serde(skip)]
    store: String,
    #[serde(skip)]
    install: bool,
    #[serde(skip)]
    hidden: bool,
}

/// Jeu en cours : `tile` est l'indice du jeu dans `Shell::games`.
struct Running {
    tile: usize,
    session: wm::Session,
    hands_off: bool, // anti-triche ou plein écran sans bordure désactivé
    closing: bool,
    started: std::time::Instant,
}

struct Shell {
    ui: App,
    guide: GuideMenu,
    splash: Splash,
    games: RefCell<Vec<Game>>,
    /// Onglet Jeux (installés, non cachés) et onglet Bibliothèque (possédés à installer).
    tiles: Rc<VecModel<Tile>>,
    library: Rc<VecModel<Tile>>,
    art_cache: RefCell<std::collections::HashMap<String, (slint::Image, slint::Image, slint::Image)>>,
    overrides: RefCell<std::collections::HashMap<String, prefs::Override>>,
    owned: RefCell<Vec<(stores::account::Owned, String)>>,
    menu_acts: RefCell<Vec<menus::Act>>,
    rename_id: RefCell<Option<String>>,
    scmd_stage: Cell<u8>,
    steam_wait: Cell<bool>,
    steam_timer: slint::Timer,
    steam_until: Cell<Option<std::time::Instant>>,
    scmd_user_tmp: RefCell<String>,
    scmd_pass: RefCell<String>,
    scmd_pending: RefCell<Option<String>>,
    dl: RefCell<Vec<scmd::Dl>>,
    dl_pid: std::sync::Arc<std::sync::atomic::AtomicU32>,
    dl_rows: Rc<VecModel<DlRow>>,
    dl_acts: RefCell<Vec<scmd::RowAct>>,
    scmd_proc: RefCell<Option<stores::steamcmd::Pending>>,
    updates: RefCell<Vec<String>>,
    prompted: RefCell<std::collections::HashSet<String>>,
    pick_ids: RefCell<Vec<String>>,
    identify_target: RefCell<Option<String>>,
    deals: Rc<VecModel<Deal>>,
    det_shots: Rc<VecModel<slint::Image>>,
    det_deals: RefCell<Vec<stores::Deal>>,
    det_title: RefCell<String>,
    det_gen: Cell<u32>,
    notes: Rc<VecModel<SharedString>>,
    running: RefCell<Vec<Running>>,
    guide_tile: Cell<usize>,
    settings: Rc<settings::Settings>,
    offers: Rc<VecModel<Offer>>,
    /// Offres du catalogue affiché (titres, URLs) et leurs images locales.
    offer_data: RefCell<Vec<stores::Offer>>,
    featured: RefCell<Option<(Vec<stores::Offer>, Vec<Option<PathBuf>>)>>,
    store_status: RefCell<Vec<(String, bool, usize)>>,
    catalog_started: Cell<bool>,
    catalog_gen: Cell<u32>,
    kb_search: Cell<bool>,
    kb_identify: Cell<bool>,
    kb_login: Cell<bool>,
    login: RefCell<Option<stores::account::Challenge>>,
    pick_mode: RefCell<addflow::PickMode>,
    pick_entries: RefCell<Vec<addgame::Entry>>,
    pick_offers: RefCell<Vec<stores::Offer>>,
    pick_gen: Cell<u32>,
    identify_exe: RefCell<Option<PathBuf>>,
    guess_title: RefCell<String>,
    notifs: Rc<VecModel<Notif>>,
    toast_gen: Cell<u32>,
    rebuild_pending: Cell<bool>,
    splash_timer: slint::Timer,
    splash_start: Cell<Option<std::time::Instant>>,
    splash_uri: Cell<bool>,
}

fn main() -> Result<(), slint::PlatformError> {
    // Le shell n'a pas de console : les paniques vont dans un fichier.
    std::panic::set_hook(Box::new(|info| {
        let _ = std::fs::write(games_file().with_file_name("panic.log"), info.to_string());
    }));
    // set_fullscreen() avant run() est ignoré par winit : plein écran dès la création.
    std::env::set_var("SLINT_FULLSCREEN", "1");
    // Legendary (jeux Epic) lit sa configuration ici, y compris quand il est lancé par le gestionnaire de fenêtres.
    std::env::set_var("LEGENDARY_CONFIG_PATH", stores::epiccmd::config());
    // ponytail: femtovg cesse de se redessiner avec ce design (constaté sur l'Optiplex) ; rendu logiciel.
    std::env::set_var("SLINT_BACKEND", std::env::var("HEARTH_BACKEND").unwrap_or_else(|_| "winit-skia".into()));
    let overrides = prefs::load();
    let games = load_games(stores::installed(), &overrides);
    let shell = Rc::new(Shell {
        ui: App::new()?,
        guide: GuideMenu::new()?,
        splash: Splash::new()?,
        games: RefCell::new(games),
        tiles: Rc::new(VecModel::default()),
        library: Rc::new(VecModel::default()),
        art_cache: RefCell::new(Default::default()),
        overrides: RefCell::new(overrides),
        owned: RefCell::new(vec![]),
        menu_acts: RefCell::new(vec![]),
        rename_id: RefCell::new(None),
        scmd_stage: Cell::new(0),
        steam_wait: Cell::new(false),
        steam_timer: slint::Timer::default(),
        steam_until: Cell::new(None),
        scmd_user_tmp: RefCell::new(String::new()),
        scmd_pass: RefCell::new(String::new()),
        scmd_pending: RefCell::new(None),
        dl: RefCell::new(vec![]),
        dl_pid: Default::default(),
        dl_rows: Rc::new(VecModel::default()),
        dl_acts: RefCell::new(vec![]),
        scmd_proc: RefCell::new(None),
        updates: RefCell::new(vec![]),
        prompted: RefCell::new(Default::default()),
        pick_ids: RefCell::new(vec![]),
        identify_target: RefCell::new(None),
        deals: Rc::new(VecModel::default()),
        det_shots: Rc::new(VecModel::default()),
        det_deals: RefCell::new(vec![]),
        det_title: RefCell::new(String::new()),
        det_gen: Cell::new(0),
        notes: Rc::new(VecModel::default()),
        running: RefCell::new(Vec::new()),
        guide_tile: Cell::new(0),
        settings: settings::Settings::new(),
        offers: Rc::new(VecModel::default()),
        offer_data: RefCell::new(vec![]),
        featured: RefCell::new(Default::default()),
        store_status: RefCell::new(vec![]),
        catalog_started: Cell::new(false),
        catalog_gen: Cell::new(0),
        kb_search: Cell::new(false),
        kb_identify: Cell::new(false),
        kb_login: Cell::new(false),
        login: RefCell::new(None),
        pick_mode: RefCell::new(addflow::PickMode::Root),
        pick_entries: RefCell::new(vec![]),
        pick_offers: RefCell::new(vec![]),
        pick_gen: Cell::new(0),
        identify_exe: RefCell::new(None),
        guess_title: RefCell::new(String::new()),
        notifs: Rc::new(VecModel::default()),
        toast_gen: Cell::new(0),
        rebuild_pending: Cell::new(false),
        splash_timer: slint::Timer::default(),
        splash_start: Cell::new(None),
        splash_uri: Cell::new(false),
    });
    shell.ui.set_games(shell.tiles.clone().into());
    shell.ui.set_library(shell.library.clone().into());
    shell.ui.set_deals(shell.deals.clone().into());
    shell.ui.set_dl_rows(shell.dl_rows.clone().into());
    shell.dl_refresh();
    let s = shell.clone();
    shell.ui.on_dl_pick(move |i| s.dl_pick(i as usize));
    let s = shell.clone();
    shell.ui.on_dl_open(move || s.dl_refresh());
    shell.ui.set_det_shots(shell.det_shots.clone().into());
    shell.rebuild();
    shell.guide.set_notes(shell.notes.clone().into());
    *shell.store_status.borrow_mut() = stores::all().iter().map(|s| (s.name().to_string(), s.detected(), shell.games.borrow().iter().filter(|g| g.store == s.name()).count())).collect();
    i18n::init();
    shell.refresh_lang();
    shell.ui.set_rows(shell.settings.rows.clone().into());
    shell.ui.set_offers(shell.offers.clone().into());
    shell.ui.set_notifs(shell.notifs.clone().into());
    let s = shell.clone();
    shell.ui.on_notifs_opened(move || s.notifs_opened());
    let s = shell.clone();
    shell.ui.on_notif_action(move |i| s.notif_action(i));
    shell.ui.set_user(std::env::var("USERNAME").unwrap_or_default().into());
    shell.set_clock();

    let s = shell.clone();
    shell.ui.on_open_cat(move|c| s.open_cat(c));
    let s = shell.clone();
    shell.ui.on_setting(move|c, r, d| s.setting(c, r, d));
    let s = shell.clone();
    shell.ui.on_kb_press(move |x, y| s.kb_press(x, y));

    let s = shell.clone();
    shell.ui.on_launch(move |i| s.launch(i as usize));
    let s = shell.clone();
    shell.ui.on_game_menu(move |i| s.game_menu(i as usize));
    let s = shell.clone();
    shell.ui.on_add_menu(move || s.add_menu());
    let s = shell.clone();
    shell.ui.on_menu_pick(move |i| s.menu_pick(i as usize));
    let s = shell.clone();
    shell.ui.on_deal_pick(move |i| s.deal_pick(i as usize));
    let s = shell.clone();
    shell.ui.on_kb_char(move |t| s.kb_char(t.as_str()));
    let s = shell.clone();
    shell.ui.on_pick(move |i| s.pick(i as usize));
    let s = shell.clone();
    shell.ui.on_pick_back(move || s.pick_back());
    let s = shell.clone();
    shell.ui.on_qr_act(move || s.login_code());
    let s = shell.clone();
    shell.ui.on_qr_cancel(move || s.login_cancel());
    shell.ui.on_power(|a| match a {
        0 => unsafe {
            let _ = windows::Win32::System::Power::SetSuspendState(false, false, false);
        },
        1 => drop(Command::new("shutdown").args(["/r", "/t", "0"]).spawn()),
        2 => drop(Command::new("shutdown").args(["/s", "/t", "0"]).spawn()),
        _ => {}
    });
    shell.ui.on_desktop(|| std::process::exit(EXIT_DESKTOP));
    // Alt+F4 / fermeture de la console : bureau Windows plutôt qu'un écran noir pendant la relance.
    shell.ui.window().on_close_requested(|| {
        if wm::ctrl_down() {
            return slint::CloseRequestResponse::KeepWindowShown;
        }
        std::process::exit(EXIT_DESKTOP)
    });
    let s = shell.clone();
    shell.ui.on_search_open(move || s.search_open());
    let s = shell.clone();
    shell.ui.on_show_offer(move |i| s.show_offer(i as usize));
    let s = shell.clone();
    shell.guide.on_action(move |item, delta| s.guide_action(item, delta));

    let s = shell.clone();
    wm::watch(move |h| {
        let Ok(running) = s.running.try_borrow() else { return };
        if running.iter().any(|r| !r.hands_off && r.session.owns(h)) && !wm::exclusive_fullscreen() {
            wm::make_borderless(h);
        }
    });

    let s = shell.clone();
    let tick = slint::Timer::default();
    tick.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || s.tick());

    let s = shell.clone();
    let poll = slint::Timer::default();
    poll.start(slint::TimerMode::Repeated, Duration::from_secs(20), move || s.poll_installed());

    let s = shell.clone();
    let phase = Cell::new(0.0f32);
    let anim = slint::Timer::default();
    anim.start(slint::TimerMode::Repeated, Duration::from_millis(160), move || s.animate(&phase));

    // Le menu guide est créé une fois puis affiché/masqué en Win32, sans jamais prendre le focus
    // (un jeu plein écran qui perd le focus se réduit et clignote).
    shell.guide.show()?;
    shell.splash.show()?;
    // Les fenêtres natives n'existent pas forcément dès le premier tour : on réessaie.
    let setup = Rc::new(slint::Timer::default());
    let t = setup.clone();
    setup.start(slint::TimerMode::Repeated, Duration::from_millis(50), move || {
        if let (Some(h), Some(sp)) = (wm::find_window(GUIDE_TITLE), wm::find_window(SPLASH_TITLE)) {
            wm::make_overlay(h);
            wm::hide(h);
            wm::make_overlay(sp);
            wm::hide(sp);
            if let Some(m) = wm::find_window(MAIN_TITLE) {
                wm::focus(m);
                // Dev : un ancien shell élevé peut masquer le nôtre ; HEARTH_DEV_TOPMOST le recouvre.
                if std::env::var_os("HEARTH_DEV_TOPMOST").is_some() {
                    let (x, y, w, h) = wm::primary_monitor();
                    wm::show_overlay(m, x, y, w, h);
                }
            }
            t.stop();
        }
    });

    // Rc n'est pas Send : le thread manette retrouve le Shell via un thread_local du thread UI.
    SHELL.with(|s| *s.borrow_mut() = Some(shell.clone()));
    input::spawn(|nav| {
        let _ = slint::invoke_from_event_loop(move || {
            SHELL.with(|s| {
                if let Some(s) = s.borrow().as_ref() {
                    s.on_nav(nav);
                }
            });
        });
    });

    Shell::schedule_updates();
    let s = shell.clone();
    let upd = slint::Timer::default();
    upd.start(slint::TimerMode::Repeated, Duration::from_secs(6 * 3600), move || s.check_updates(false));
    shell.start_art();
    shell.refresh_owned();
    shell.ui.run()
}

thread_local! {
    static SHELL: RefCell<Option<Rc<Shell>>> = const { RefCell::new(None) };
}

impl Shell {
    fn launch(&self, i: usize) {
        if let Some(h) = self.running.borrow().iter().find(|r| r.tile == i).and_then(|r| r.session.main_window()) {
            wm::focus(h);
            return;
        }
        let Some(g) = self.games.borrow().get(i).cloned() else { return };
        if g.install {
            self.install_owned(&g);
            return;
        }
        if self.is_scmd_game(&g.exe) && !self.steam_ready(i, &g) {
            return;
        }
        self.start_splash(i, g.uri.is_some());
        if let Some(uri) = &g.uri {
            // Lancement par le client de la boutique : le jeu n'est pas encapsulé.
            let _ = Command::new("cmd").args(["/c", "start", "", uri]).spawn();
            return;
        }
        let exe = Path::new(&g.exe);
        let compat = wm::compat::lookup(exe);
        compat.apply_ini();
        let args: Vec<String> = g.args.iter().chain(&compat.args).cloned().collect();
        match wm::Session::launch(exe, &args) {
            Ok(session) => {
                let hands_off = g.anticheat || compat.anticheat || !compat.borderless;
                self.running.borrow_mut().push(Running { tile: i, session, hands_off, closing: false, started: std::time::Instant::now() });
                self.rebuild();
            }
            Err(e) => {
                self.hide_splash();
                self.notify(format!("{} {} : {}", tr("Impossible de lancer"), g.name, e.message()));
            }
        }
    }

    fn on_nav(&self, nav: Nav) {
        if wm::find_window(SPLASH_TITLE).is_some_and(wm::is_visible) {
            if nav == Nav::Back {
                self.hide_splash();
            }
            return;
        }
        if wm::find_window(GUIDE_TITLE).is_some_and(wm::is_visible) {
            if nav == Nav::Guide {
                self.guide_action(-1, 0);
            } else {
                self.guide.invoke_nav(nav as i32);
            }
            return;
        }
        if nav != Nav::Guide {
            // La manette est lue même en arrière-plan : n'agir que si la console est visible
            // au premier plan, sinon on navigue à l'aveugle (jusqu'à « Veille »…).
            if wm::find_window(MAIN_TITLE).is_some_and(|h| h == wm::foreground()) {
                self.ui.invoke_nav(nav as i32);
            }
            return;
        }
        let fg = wm::foreground();
        let target = self.running.borrow().iter().find(|r| r.session.owns(fg)).map(|r| (r.tile, r.hands_off));
        match target {
            // Jeu protégé ou plein écran exclusif : pas d'overlay, retour propre au shell.
            Some((_, true)) => {
                wm::minimize(fg);
                self.home();
            }
            Some(_) if wm::exclusive_fullscreen() => {
                wm::minimize(fg);
                self.home();
            }
            Some((tile, false)) => self.open_guide(tile),
            None => {
                // Depuis la console, avec un jeu en arrière-plan : son menu (le seul moyen de
                // fermer un jeu en plein écran exclusif, qui n'a pas d'overlay).
                let in_console = wm::find_window(MAIN_TITLE).is_some_and(|h| h == fg);
                let last = self.running.borrow().last().map(|r| r.tile);
                match last {
                    Some(tile) if in_console => self.open_guide(tile),
                    _ => self.home(),
                }
            }
        }
    }

    fn open_cat(&self, cat: i32) {
        let hint = self.settings.open(cat);
        self.ui.set_shint(hint.into());
        if settings::needs_fetch(cat) {
            self.spawn_fetch(cat);
        }
    }

    /// Lecture WinRT hors du thread UI, résultat renvoyé au thread UI.
    fn spawn_fetch(&self, cat: i32) {
        std::thread::spawn(move || {
            let f = settings::fetch(cat);
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.fetched(cat, f)));
        });
    }

    fn fetched(&self, cat: i32, f: settings::Fetched) {
        // Un résultat lent ne doit pas écraser les données de la catégorie affichée à présent.
        if self.ui.get_cat() == cat {
            self.settings.apply(f);
            self.open_cat_keep(cat);
        }
    }

    fn setting(&self, cat: i32, row: i32, delta: i32) {
        match self.settings.act(cat, row, delta) {
            settings::Outcome::DisplayChanged => self.display_changed(),
            settings::Outcome::Toggle { wifi, on } => {
                self.settings.start_loading();
                std::thread::spawn(move || {
                    if wifi {
                        system::radio::set_wifi(on);
                    } else {
                        system::radio_ext::set_bluetooth(on);
                    }
                    let f = settings::fetch(cat);
                    let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.fetched(cat, f)));
                });
            }
            settings::Outcome::Password(ssid) => {
                self.settings.kb_open(&ssid);
                self.kb_refresh();
                self.ui.set_kbx(0);
                self.ui.set_kby(0);
                self.ui.set_kb_title(format!("{} {ssid}", tr("Mot de passe :")).into());
                self.ui.set_zone(4);
            }
            settings::Outcome::Connect(ssid) => self.connect(ssid, None),
            settings::Outcome::LangChanged => self.refresh_lang(),
            settings::Outcome::InstallClient(store) => self.account_install_client(store),
            settings::Outcome::Admin => {
                self.push_notif(&tr("Autorisation administrateur"), &tr("Autorisation administrateur requise : acceptez la fenêtre Windows (une seule fois)"));
                std::thread::spawn(|| {
                    let r = system::admin::setup();
                    let _ = slint::invoke_from_event_loop(move || {
                        with_shell(|s| {
                            match r {
                                Ok(()) => s.push_notif(&tr("Autorisation administrateur"), &tr("Activée")),
                                Err(e) => s.push_notif(&tr("Autorisation administrateur"), &e),
                            }
                            if s.ui.get_zone() == 3 && s.ui.get_cat() == 12 {
                                s.open_cat(12);
                            }
                        })
                    });
                });
            }
            settings::Outcome::SteamCmd => self.scmd_begin(None),
            settings::Outcome::Login(store) => self.begin_login(store),
            settings::Outcome::Logout(store) => self.logout(&store),
            settings::Outcome::BtScan => {
                self.settings.set_message(tr("Recherche d'appareils Bluetooth…"));
                self.open_cat_keep(3);
                std::thread::spawn(|| {
                    let found = system::bluetooth::devices(false);
                    let _ = slint::invoke_from_event_loop(move || {
                        with_shell(|s| {
                            s.settings.set_bt_found(found);
                            s.settings.set_message(String::new());
                            s.open_cat_keep(3);
                        })
                    });
                });
            }
            settings::Outcome::BtPair(id, name) => {
                self.push_notif("Bluetooth", &format!("{} {name}…", tr("Appairage de")));
                std::thread::spawn(move || {
                    let r = system::bluetooth::pair(&id);
                    let fresh = settings::fetch(3);
                    let _ = slint::invoke_from_event_loop(move || {
                        with_shell(|s| {
                            match r {
                                Ok(()) => s.push_notif("Bluetooth", &format!("{name} : {}", tr("appairé"))),
                                Err(e) => s.push_notif("Bluetooth", &format!("{name} : {} ({e})", tr("échec de l'appairage"))),
                            }
                            s.settings.set_bt_found(vec![]);
                            s.fetched(3, fresh);
                        })
                    });
                });
            }
            settings::Outcome::BtUnpair(id, name) => {
                std::thread::spawn(move || {
                    let r = system::bluetooth::unpair(&id);
                    let fresh = settings::fetch(3);
                    let _ = slint::invoke_from_event_loop(move || {
                        with_shell(|s| {
                            match r {
                                Ok(()) => s.push_notif("Bluetooth", &format!("{name} : {}", tr("oublié"))),
                                Err(e) => s.push_notif("Bluetooth", &format!("{name} : {e}")),
                            }
                            s.fetched(3, fresh);
                        })
                    });
                });
            }
            settings::Outcome::CheckUpdates(drivers) => {
                std::thread::spawn(move || {
                    let r = if drivers { system::updates::pending_drivers() } else { system::updates::pending() };
                    let _ = slint::invoke_from_event_loop(move || {
                        with_shell(|s| {
                            s.settings.set_update(drivers, r);
                            s.open_cat_keep(if drivers { 8 } else { 6 });
                        })
                    });
                });
            }
            settings::Outcome::None => {}
        }
        self.open_cat_keep(cat);
    }

    fn kb_refresh(&self) {
        let text = self.settings.kb_text();
        let shown = if self.scmd_stage.get() == 2 { "•".repeat(text.chars().count()) } else { text };
        self.ui.set_kb_text(shown.into());
        let keys: Vec<SharedString> = self.settings.kb_keys().into_iter().map(Into::into).collect();
        self.ui.set_kb_keys(Rc::new(VecModel::from(keys)).into());
    }

    fn kb_press(&self, x: i32, y: i32) {
        let k = self.settings.kb_press(x, y);
        self.kb_apply(k);
    }

    /// Saisie au clavier physique.
    fn kb_char(&self, t: &str) {
        let k = self.settings.kb_char(t);
        self.kb_apply(k);
    }

    fn kb_apply(&self, k: settings::Kb) {
        self.ui.set_kb_acc(self.settings.kb_acc());
        match k {
            settings::Kb::Continue => self.kb_refresh(),
            settings::Kb::Cancel => {
                let back = if self.kb_search.replace(false) {
                    0
                } else if self.kb_identify.replace(false) {
                    6
                } else if self.kb_login.replace(false) {
                    5
                } else if self.rename_id.borrow_mut().take().is_some() {
                    0
                } else if self.scmd_stage.get() != 0 {
                    self.scmd_cancel();
                    0
                } else {
                    3
                };
                self.ui.set_zone(back);
            }
            settings::Kb::Done(ssid, text) => {
                let renaming = self.rename_id.borrow_mut().take();
                if self.scmd_stage.get() != 0 {
                    self.scmd_text(text);
                } else if let Some(id) = renaming {
                    self.ui.set_zone(0);
                    self.rename_game(&id, text.trim());
                } else if self.kb_login.replace(false) {
                    self.ui.set_zone(5);
                    self.login_exchange(text);
                } else if self.kb_identify.replace(false) {
                    self.ui.set_zone(6);
                    if !text.trim().is_empty() {
                        self.run_identify(text.trim().to_string());
                    }
                } else if self.kb_search.replace(false) {
                    self.ui.set_zone(0);
                    self.ui.set_search_query(text.clone().into());
                    self.load_catalog(if text.trim().is_empty() { None } else { Some(text.trim().to_string()) });
                } else {
                    self.ui.set_zone(3);
                    self.connect(ssid, Some(text));
                }
            }
        }
    }

    fn connect(&self, ssid: String, password: Option<String>) {
        self.settings.set_message(format!("{} {ssid}…", tr("Connexion à")));
        self.open_cat_keep(2);
        std::thread::spawn(move || {
            let ok = system::wifi::connect(&ssid, password.as_deref());
            let _ = slint::invoke_from_event_loop(move || {
                with_shell(|s| {
                    s.settings.set_message(format!("{} {ssid}", tr(if ok { "Connecté à" } else { "Échec de la connexion à" })));
                    s.settings.start_loading();
                    s.open_cat_keep(2);
                    s.spawn_fetch(2);
                })
            });
        });
    }

    /// Rafraîchit les lignes sans recharger les données (garde les choix en cours).
    fn open_cat_keep(&self, cat: i32) {
        let hint = self.settings.refresh(cat);
        self.ui.set_shint(hint.into());
        let max = self.settings.row_count().saturating_sub(1) as i32;
        self.ui.set_srow(self.ui.get_srow().min(max));
    }

    fn display_changed(&self) {
        if let Some(h) = wm::find_window(MAIN_TITLE) {
            wm::fit_to_monitor(h);
        }
    }

    fn set_clock(&self) {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        self.ui.set_clock(format!("{:02}:{:02}", t.wHour, t.wMinute).into());
    }

    fn home(&self) {
        if let Some(h) = wm::find_window(MAIN_TITLE) {
            // Un jeu qui a changé la résolution a pu rétrécir notre fenêtre : on la recale.
            wm::fit_to_monitor(h);
            wm::focus(h);
        }
        self.ui.invoke_nav(Nav::Guide as i32);
    }

    fn open_guide(&self, tile: usize) {
        self.guide_tile.set(tile);
        let closing = self.running.borrow().iter().any(|r| r.tile == tile && r.closing);
        self.guide.set_game(self.games.borrow()[tile].name.as_str().into());
        self.guide.set_art(self.tile_for(tile).map(|t| t.cover).unwrap_or_default());
        self.guide.set_closing(closing);
        self.guide.set_sel(0);
        self.guide.set_volume(system::audio::volume().map_or(-1, |v| v as i32));
        if let Some(h) = wm::find_window(GUIDE_TITLE) {
            let (x, y, _, height) = wm::primary_monitor();
            let width = (GUIDE_WIDTH * self.ui.window().scale_factor()) as i32;
            wm::show_overlay(h, x, y, width, height);
            self.guide.window().request_redraw();
        }
        // DDC/CI et WinRT sont lents : lecture en arrière-plan.
        let g = self.guide.as_weak();
        std::thread::spawn(move || {
            let b = system::display::brightness().map_or(-1, |v| v as i32);
            let w = system::radio::wifi_on().map_or(-1, |on| on as i32);
            let _ = g.upgrade_in_event_loop(move |g| {
                g.set_brightness(b);
                g.set_wifi(w);
            });
        });
    }

    fn guide_action(&self, item: i32, delta: i32) {
        let tile = self.guide_tile.get();
        let game = || self.running.borrow().iter().find(|r| r.tile == tile).and_then(|r| r.session.main_window());
        match (item, delta) {
            (-1, _) | (0, 0) => {
                self.hide_guide();
                if let Some(h) = game() {
                    wm::focus(h);
                }
            }
            (1, 0) => {
                self.hide_guide();
                if let Some(h) = game() {
                    wm::minimize(h);
                }
                self.home();
            }
            (2, 0) => {
                self.hide_guide();
                let mut running = self.running.borrow_mut();
                if let Some(r) = running.iter_mut().find(|r| r.tile == tile) {
                    if r.closing {
                        r.session.kill();
                    } else {
                        r.closing = true;
                        r.session.close();
                    }
                }
                drop(running);
                // Laisse le jeu afficher sa propre confirmation éventuelle.
                if let Some(h) = game() {
                    wm::focus(h);
                }
            }
            (3, d) if d != 0 => {
                let v = (self.guide.get_volume() + d * 5).clamp(0, 100);
                system::audio::set_volume(v as u32);
                self.guide.set_volume(system::audio::volume().map_or(-1, |v| v as i32));
            }
            (4, d) if d != 0 && self.guide.get_brightness() >= 0 => {
                let b = (self.guide.get_brightness() + d * 10).clamp(0, 100);
                self.guide.set_brightness(b);
                std::thread::spawn(move || system::display::set_brightness(b as u32));
            }
            (5, _) if self.guide.get_wifi() >= 0 => {
                let on = self.guide.get_wifi() == 0;
                self.guide.set_wifi(on as i32);
                std::thread::spawn(move || system::radio::set_wifi(on));
            }
            _ => {}
        }
    }

    /// Chaque seconde : jeux terminés, et ré-application du sans-bordure (certains jeux le retirent).
    fn tick(&self) {
        self.set_clock();
        self.maybe_prompt_update();
        if self.settings.tick() {
            self.display_changed();
            self.open_cat_keep(self.ui.get_cat());
        } else if self.settings.confirming() {
            self.open_cat_keep(self.ui.get_cat());
        }
        let ended: Vec<(usize, u64)> = {
            let mut running = self.running.borrow_mut();
            let ended: Vec<(usize, u64)> = running.iter().filter(|r| !r.session.alive()).map(|r| (r.tile, r.started.elapsed().as_secs())).collect();
            running.retain(|r| r.session.alive());
            ended
        };
        for &(tile, secs) in &ended {
            let key = playtime_key(&self.games.borrow()[tile]);
            playtime::record(&key, secs);
            self.notify(format!("{} {}", self.games.borrow()[tile].name, tr("s'est fermé")));
        }
        if !ended.is_empty() {
            self.rebuild();
        }
        if !ended.is_empty() {
            if ended.iter().any(|(t, _)| *t == self.guide_tile.get()) {
                self.hide_guide();
            }
            let fg = wm::foreground();
            if !self.running.borrow().iter().any(|r| r.session.owns(fg)) {
                self.home();
            }
        }
        let fg = wm::foreground();
        let fix = self.running.borrow().iter().any(|r| !r.hands_off && r.session.owns(fg));
        if fix && wm::is_main_window(fg) && !wm::exclusive_fullscreen() {
            wm::make_borderless(fg);
        }
    }

    fn hide_guide(&self) {
        if let Some(h) = wm::find_window(GUIDE_TITLE) {
            wm::hide(h);
        }
    }

    /// Notification simple (titre « Hearth »).
    fn notify(&self, msg: String) {
        self.push_notif("Hearth", &msg);
    }

    /// Ajoute une notification au centre, au menu en jeu, et l'affiche en toast quelques secondes.
    fn push_notif(&self, title: &str, text: &str) {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        self.notifs.insert(0, Notif { title: title.into(), text: text.into(), time: format!("{:02}:{:02}", t.wHour, t.wMinute).into(), unread: true });
        if self.notifs.row_count() > 30 {
            self.notifs.remove(30);
        }
        self.update_unread();
        self.notes.insert(0, text.into());
        if self.notes.row_count() > MAX_NOTES {
            self.notes.remove(MAX_NOTES);
        }
        if !i18n::toasts() {
            return;
        }
        self.ui.set_toast_title(title.into());
        self.ui.set_toast_text(text.into());
        self.ui.set_toast_on(true);
        let gen = self.toast_gen.get() + 1;
        self.toast_gen.set(gen);
        slint::Timer::single_shot(Duration::from_secs(5), move || {
            with_shell(|s| {
                if s.toast_gen.get() == gen {
                    s.ui.set_toast_on(false);
                }
            })
        });
    }

    fn update_unread(&self) {
        self.ui.set_unread(self.notifs.iter().filter(|n| n.unread).count() as i32);
    }

    fn notifs_opened(&self) {
        for i in 0..self.notifs.row_count() {
            if let Some(mut n) = self.notifs.row_data(i) {
                n.unread = false;
                self.notifs.set_row_data(i, n);
            }
        }
        self.update_unread();
        self.ui.set_toast_on(false);
    }

    fn notif_action(&self, i: i32) {
        if i < 0 {
            self.notifs.set_vec(vec![]);
        } else if (i as usize) < self.notifs.row_count() {
            self.notifs.remove(i as usize);
        }
        self.update_unread();
    }
}

fn games_file() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").unwrap_or_default();
    Path::new(&base).join("Hearth").join("games.json")
}

/// Jeux manuels (games.json) puis jeux installés des boutiques, avec noms/masquage personnalisés.
fn load_games(installed: Vec<stores::Game>, overrides: &std::collections::HashMap<String, prefs::Override>) -> Vec<Game> {
    let mut games: Vec<Game> = std::fs::read_to_string(games_file())
        // Notepad et PowerShell ajoutent souvent un BOM, que serde_json refuse.
        .ok()
        .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    for g in &mut games {
        if g.id.is_empty() {
            g.id = format!("custom-{:08x}", name_hash(&g.exe));
        }
    }
    // Jeux installés des boutiques (Steam, Epic, GOG), sans doublon avec games.json.
    for g in installed {
        if games.iter().any(|m| m.id == g.id) {
            continue;
        }
        let (exe, args, uri) = match g.launch {
            stores::Launch::Exe { path, args } => (path, args, None),
            stores::Launch::Uri(u) => (String::new(), vec![], Some(u)),
        };
        games.push(Game { name: g.title, id: g.id, exe, args, uri, store: g.store.into(), ..Default::default() });
    }
    if games.is_empty() {
        games = ["notepad.exe", "calc.exe", "mspaint.exe"]
            .into_iter()
            .zip(["Bloc-notes", "Calculatrice", "Paint"])
            .map(|(exe, name)| Game { name: name.into(), id: format!("demo-{exe}"), exe: exe.into(), ..Default::default() })
            .collect();
    }
    for g in &mut games {
        if let Some(o) = overrides.get(&g.id) {
            if let Some(n) = o.name.as_ref().filter(|n| !n.is_empty()) {
                g.name = n.clone();
            }
            g.hidden = o.hidden;
        }
    }
    games
}

fn name_hash(s: &str) -> u32 {
    s.to_lowercase().bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3)) as u32
}

fn with_shell(f: impl FnOnce(&Shell)) {
    SHELL.with(|s| {
        if let Some(s) = s.borrow().as_ref() {
            f(s);
        }
    });
}

/// Visuel d'une tuile : `cover` du jeu, sinon assets\<id>\cover.* (cf. crate `stores`).
fn cover_of(g: &Game) -> slint::Image {
    let path = if g.cover.is_empty() { stores::asset(&g.id, "cover").unwrap_or_default() } else { PathBuf::from(&g.cover) };
    slint::Image::load_from_path(&path).unwrap_or_default()
}

impl Shell {
    /// Textes calculés en Rust après un changement de langue (les @tr de Slint suivent seuls).
    fn refresh_lang(&self) {
        let cats: Vec<SharedString> = settings::cats().into_iter().map(Into::into).collect();
        self.ui.set_cats(Rc::new(VecModel::from(cats)).into());
        // Pas avant la création de la fenêtre : COM multithread sur ce thread ferait échouer winit.
        if self.ui.get_zone() == 3 {
            self.open_cat_keep(self.ui.get_cat());
        }
    }

    /// Fond animé (arrêté quand la console n'est pas devant : économise le CPU en jeu) et pictogrammes.
    fn animate(&self, phase: &Cell<f32>) {
        if self.ui.get_tab() == 2 && !self.catalog_started.get() {
            self.load_catalog(None);
        }
        let pad = match std::env::var("HEARTH_PAD").as_deref() {
            Ok("ps") => 1,
            Ok("xbox") => 0,
            _ => input::last_pad() as i32,
        };
        if self.ui.global::<Pad>().get_kind() != pad {
            self.ui.global::<Pad>().set_kind(pad);
            self.guide.global::<Pad>().set_kind(pad);
        }
        if i18n::anim() && self.ui.get_zone() < 3 && wm::find_window(MAIN_TITLE).is_some_and(|h| h == wm::foreground()) {
            phase.set((phase.get() + 0.8) % 3600.0);
            self.ui.set_phase(phase.get());
        }
    }
}

/// Teinte stable par jeu (bleus/violets/turquoises, en harmonie avec le thème) pour les tuiles sans visuel.
fn hue_of(name: &str) -> slint::Color {
    let h = name.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let hue = 190.0 + (h % 120) as f32; // 190..310°
    let (s, v) = (0.55f32, 0.85f32);
    let c = v * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (hue / 60.0) as u32 {
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    slint::Color::from_rgb_u8(((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}

/// Grand visuel du jeu, s'il en existe un (sinon rien : une jaquette agrandie serait floue).
fn hero_of(g: &Game) -> slint::Image {
    stores::asset(&g.id, "hero").and_then(|p| slint::Image::load_from_path(&p).ok()).unwrap_or_default()
}

fn logo_of(g: &Game) -> slint::Image {
    stores::asset(&g.id, "logo").and_then(|p| slint::Image::load_from_path(&p).ok()).unwrap_or_default()
}

/// Images des offres, téléchargées en parallèle (mises en cache sur disque).
fn fetch_all(offers: &[stores::Offer]) -> Vec<Option<PathBuf>> {
    let mut out = vec![None; offers.len()];
    std::thread::scope(|sc| {
        for (o, slots) in offers.chunks(4).zip(out.chunks_mut(4)) {
            sc.spawn(move || {
                for (offer, slot) in o.iter().zip(slots) {
                    *slot = stores::fetch_first(&offer.images);
                }
            });
        }
    });
    out
}

impl Shell {
    /// Charge le catalogue unifié (« à la une ») ou les résultats d'une recherche.
    fn load_catalog(&self, query: Option<String>) {
        self.catalog_started.set(true);
        let gen = self.catalog_gen.get() + 1;
        self.catalog_gen.set(gen);
        self.ui.set_osel(0);
        self.ui.set_brow(0);
        self.ui.set_offers_title(match &query {
            Some(q) => format!("{} « {q} »", tr("Résultats pour")),
            None => tr("À la une"),
        }.into());
        if query.is_none() {
            if let Some((offers, paths)) = self.featured.borrow().as_ref() {
                self.apply_offers(offers, paths);
                return;
            }
        }
        self.offers.set_vec(vec![]);
        self.offer_data.borrow_mut().clear();
        self.ui.set_offers_state("loading".into());
        std::thread::spawn(move || {
            let offers = match &query {
                Some(q) => stores::search(q),
                None => stores::catalog(),
            };
            let featured = query.is_none();
            let listed = offers.clone();
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.offers_listed(gen, featured, listed)));
            // Les images arrivent ensuite, en parallèle, et remplissent les cartes au fur et à mesure.
            let jobs: Vec<(usize, Vec<String>)> = offers.iter().enumerate().map(|(i, o)| (i, o.images.clone())).collect();
            std::thread::scope(|sc| {
                for chunk in jobs.chunks(jobs.len().div_ceil(6).max(1)) {
                    sc.spawn(move || {
                        for (i, imgs) in chunk {
                            if let Some(p) = stores::fetch_first(imgs) {
                                let i = *i;
                                let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.offer_art(gen, featured, i, p)));
                            }
                        }
                    });
                }
            });
        });
    }

    fn offers_listed(&self, gen: u32, featured: bool, offers: Vec<stores::Offer>) {
        let paths = vec![None; offers.len()];
        if self.catalog_gen.get() == gen {
            self.apply_offers(&offers, &paths);
        }
        if featured {
            *self.featured.borrow_mut() = Some((offers, paths));
        }
    }

    fn offer_art(&self, gen: u32, featured: bool, i: usize, p: PathBuf) {
        if featured {
            if let Some((_, paths)) = self.featured.borrow_mut().as_mut() {
                if let Some(slot) = paths.get_mut(i) {
                    *slot = Some(p.clone());
                }
            }
        }
        if self.catalog_gen.get() == gen {
            if let (Some(mut row), Ok(img)) = (self.offers.row_data(i), slint::Image::load_from_path(&p)) {
                row.art = img;
                self.offers.set_row_data(i, row);
            }
        }
    }

    fn apply_offers(&self, offers: &[stores::Offer], paths: &[Option<PathBuf>]) {
        let rows: Vec<Offer> = offers
            .iter()
            .zip(paths)
            .map(|(o, p)| Offer {
                title: o.title.as_str().into(),
                price: if o.price == "Gratuit" { tr("Gratuit") } else { o.price.clone() }.into(),
                store: o.store.as_str().into(),
                art: p.as_ref().and_then(|p| slint::Image::load_from_path(p).ok()).unwrap_or_default(),
            })
            .collect();
        *self.offer_data.borrow_mut() = offers.to_vec();
        self.ui.set_offers_state(if rows.is_empty() { "empty" } else { "" }.into());
        self.offers.set_vec(rows);
    }

    fn search_open(&self) {
        self.kb_search.set(true);
        self.settings.kb_open("");
        self.kb_refresh();
        self.ui.set_kbx(0);
        self.ui.set_kby(0);
        self.ui.set_kb_title(tr("Rechercher un jeu").into());
        self.ui.set_zone(4);
    }

    /// Fiche d'un jeu de la boutique : description, captures et prix dans chaque boutique.
    fn show_offer(&self, i: usize) {
        let Some(o) = self.offer_data.borrow().get(i).cloned() else { return };
        let gen = self.det_gen.get() + 1;
        self.det_gen.set(gen);
        self.ui.set_det_title(o.title.as_str().into());
        self.ui.set_det_desc(o.desc.as_str().into());
        self.ui.set_det_meta(o.meta.as_str().into());
        self.ui.set_det_cover(self.offers.row_data(i).map(|r| r.art).unwrap_or_default());
        self.ui.set_det_hero(Default::default());
        self.det_shots.set_vec(vec![]);
        let rows: Vec<Deal> = o.deals.iter().map(|d| Deal { store: d.store.as_str().into(), price: if d.price == "Gratuit" { tr("Gratuit") } else { d.price.clone() }.into() }).collect();
        self.deals.set_vec(rows);
        *self.det_deals.borrow_mut() = o.deals.clone();
        *self.det_title.borrow_mut() = o.title.clone();
        self.ui.set_dsel(0);
        self.ui.set_det_loading(true);
        self.ui.set_zone(7);
        std::thread::spawn(move || {
            let d = stores::details(&o);
            let hero = stores::fetch_first(&o.heroes);
            let shots: Vec<PathBuf> = d.shots.iter().filter_map(|u| stores::fetch_first(std::slice::from_ref(u))).collect();
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.details_ready(gen, d, hero, shots)));
        });
    }

    fn details_ready(&self, gen: u32, d: stores::Details, hero: Option<PathBuf>, shots: Vec<PathBuf>) {
        if self.det_gen.get() != gen {
            return;
        }
        if !d.desc.is_empty() {
            self.ui.set_det_desc(d.desc.into());
        }
        if !d.meta.is_empty() {
            self.ui.set_det_meta(d.meta.into());
        }
        if let Some(h) = hero.and_then(|p| slint::Image::load_from_path(&p).ok()) {
            self.ui.set_det_hero(h);
        }
        self.det_shots.set_vec(shots.iter().filter_map(|p| slint::Image::load_from_path(p).ok()).collect::<Vec<_>>());
        self.ui.set_det_loading(false);
    }

    /// QR code d'achat chez la boutique choisie dans la fiche.
    fn deal_pick(&self, i: usize) {
        let Some(d) = self.det_deals.borrow().get(i).cloned() else { return };
        if let Some((w, h, px)) = stores::qr::rgb(&d.url, 8) {
            let buf = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(&px, w, h);
            self.ui.set_qr(slint::Image::from_rgb8(buf));
            self.ui.set_qr_title(format!("{} — {}", self.det_title.borrow(), d.store).into());
            self.ui.set_qr_url(d.url.into());
            self.ui.set_qr_hint(tr("Scannez avec votre téléphone pour acheter").into());
            self.ui.set_qr_action("".into());
            self.ui.set_qr_return(7);
            self.ui.set_zone(5);
        }
    }

    /// Télécharge en arrière-plan les visuels manquants des jeux des boutiques (installés et possédés).
    pub fn start_art(&self) {
        let key = i18n::pref("steamgriddb_key").or_else(|| std::env::var("HEARTH_SGDB_KEY").ok());
        let mut jobs: Vec<(String, String, stores::Art)> =
            self.owned.borrow().iter().map(|(o, id)| (id.clone(), o.title.clone(), stores::account::owned_art(o))).collect();
        let installed = self.games.borrow().iter().any(|g| !g.store.is_empty() && !g.install);
        std::thread::spawn(move || {
            if installed {
                jobs.extend(stores::installed().into_iter().map(|g| (g.id, g.title, g.art)));
            }
            let key = &key;
            std::thread::scope(|sc| {
                for chunk in jobs.chunks(jobs.len().div_ceil(6).max(1)) {
                    sc.spawn(move || {
                        for (id, title, art) in chunk {
                            if stores::ensure_art_for(id, title, art, key.as_deref()) {
                                let id = id.clone();
                                let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.reload_art(&id)));
                            }
                        }
                    });
                }
            });
        });
    }

    /// Un visuel vient d'arriver : on oublie le cache de ce jeu et on reconstruit (regroupé).
    pub fn reload_art(&self, id: &str) {
        self.art_cache.borrow_mut().remove(id);
        self.rebuild_soon();
    }

    pub fn rebuild_soon(&self) {
        if self.rebuild_pending.replace(true) {
            return;
        }
        slint::Timer::single_shot(Duration::from_millis(500), || {
            with_shell(|s| {
                s.rebuild_pending.set(false);
                s.rebuild();
            })
        });
    }
}

impl Shell {
    /// Écran de lancement : fenêtre superposée au jeu, qui reste le temps du chargement.
    fn start_splash(&self, i: usize, uri: bool) {
        let Some(t) = self.tile_for(i) else { return };
        self.splash.set_launch_name(t.name);
        self.splash.set_launch_cover(t.cover);
        // Un petit héros agrandi serait flou : on ne le garde que s'il est assez large.
        self.splash.set_launch_art(if t.hero.size().width >= 1400 { t.hero } else { Default::default() });
        self.splash.set_launch_hue(t.hue);
        self.splash.set_launch_initial(t.initial);
        self.splash.set_launch_t(0.0);
        self.splash.set_launch_spin(0.0);
        if let Some(h) = wm::find_window(SPLASH_TITLE) {
            let (x, y, w, hh) = wm::primary_monitor();
            wm::show_overlay(h, x, y, w, hh);
            self.splash.window().request_redraw();
        }
        self.splash_start.set(Some(std::time::Instant::now()));
        self.splash_uri.set(uri);
        self.splash_timer.start(slint::TimerMode::Repeated, Duration::from_millis(40), || with_shell(|s| s.splash_tick()));
    }

    pub fn hide_splash(&self) {
        if let Some(h) = wm::find_window(SPLASH_TITLE) {
            wm::hide(h);
        }
        self.splash_start.set(None);
        self.splash_timer.stop();
    }

    fn splash_tick(&self) {
        let Some(t0) = self.splash_start.get() else {
            self.splash_timer.stop();
            return;
        };
        let el = t0.elapsed().as_secs_f32();
        self.splash.set_launch_t((el / 0.9).min(1.0));
        self.splash.set_launch_spin((el * 540.0) % 360.0);
        let fg = wm::foreground();
        let shown = self.running.borrow().iter().any(|r| r.session.owns(fg));
        // Le jeu affiche tout de suite une fenêtre noire : on garde l'écran de lancement par-dessus quelques secondes.
        let done = (shown && el > 5.0) || el > 15.0 || (self.splash_uri.get() && el > 8.0);
        if done || wm::find_window(SPLASH_TITLE).is_none_or(|h| !wm::is_visible(h)) {
            self.hide_splash();
        }
    }
}

/// Clé de temps de jeu : identifiant du jeu, sinon son nom.
fn playtime_key(g: &Game) -> String {
    if g.id.is_empty() {
        format!("name-{}", g.name)
    } else {
        g.id.clone()
    }
}

impl Shell {
    fn art_of(&self, g: &Game) -> (slint::Image, slint::Image, slint::Image) {
        self.art_cache.borrow_mut().entry(g.id.clone()).or_insert_with(|| (cover_of(g), hero_of(g), logo_of(g))).clone()
    }

    fn make_tile(&self, gid: usize, g: &Game, running: bool, pt: &std::collections::HashMap<String, playtime::Entry>) -> Tile {
        let (cover, hero, logo) = self.art_of(g);
        Tile {
            gid: gid as i32,
            name: g.name.as_str().into(),
            initial: g.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default().into(),
            hue: hue_of(&g.name),
            cover,
            hero,
            logo,
            running,
            install: g.install,
            sub: if g.install { "".into() } else { playtime::label_in(pt, &playtime_key(g)).into() },
        }
    }

    fn tile_for(&self, gid: usize) -> Option<Tile> {
        let games = self.games.borrow();
        let g = games.get(gid)?;
        Some(self.make_tile(gid, g, false, &Default::default()))
    }

    /// Reconstruit les onglets Jeux (installés, triés par dernière partie) et Bibliothèque (à installer).
    pub fn rebuild(&self) {
        let cur = self.tiles.row_data(self.ui.get_sel().max(0) as usize).map(|t| t.gid);
        let cur_lib = self.library.row_data(self.ui.get_lsel().max(0) as usize).map(|t| t.gid);
        let pt = playtime::load();
        let running: Vec<usize> = self.running.borrow().iter().map(|r| r.tile).collect();
        let (mut a, mut b): (Vec<(u64, Tile)>, Vec<Tile>) = (vec![], vec![]);
        for (i, g) in self.games.borrow().iter().enumerate() {
            if g.hidden {
                continue;
            }
            let t = self.make_tile(i, g, running.contains(&i), &pt);
            if g.install {
                b.push(t);
            } else {
                a.push((playtime::last(&playtime_key(g), &pt), t));
            }
        }
        a.sort_by_key(|(last, _)| std::cmp::Reverse(*last));
        let a: Vec<Tile> = a.into_iter().map(|(_, t)| t).collect();
        let sel = cur.and_then(|g| a.iter().position(|t| t.gid == g)).unwrap_or((self.ui.get_sel().max(0) as usize).min(a.len().saturating_sub(1)));
        let lsel = cur_lib.and_then(|g| b.iter().position(|t| t.gid == g)).unwrap_or((self.ui.get_lsel().max(0) as usize).min(b.len().saturating_sub(1)));
        self.tiles.set_vec(a);
        self.library.set_vec(b);
        self.ui.set_sel(sel as i32);
        self.ui.set_lsel(lsel as i32);
    }

    /// Relit les jeux installés (après une installation, une désinstallation ou un changement de liste).
    pub fn reload_games(&self, installed: Vec<stores::Game>) {
        let old = self.games.borrow().clone();
        let mut new = load_games(installed, &self.overrides.borrow());
        // Les jeux en cours restent dans la liste même s'ils ont disparu des boutiques.
        for r in self.running.borrow().iter() {
            if let Some(g) = old.get(r.tile).filter(|g| !new.iter().any(|n| n.id == g.id)) {
                new.push(g.clone());
            }
        }
        self.append_owned(&mut new);
        for r in self.running.borrow_mut().iter_mut() {
            if let Some(p) = old.get(r.tile).and_then(|g| new.iter().position(|n| n.id == g.id)) {
                r.tile = p;
            }
        }
        *self.games.borrow_mut() = new;
        self.rebuild();
        self.refresh_store_status();
    }

    /// Toutes les 20 s : un jeu installé ou désinstallé par un client de boutique apparaît / disparaît seul.
    pub fn poll_installed(&self) {
        std::thread::spawn(|| {
            let inst = stores::installed();
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.installed_polled(inst)));
        });
    }

    fn installed_polled(&self, inst: Vec<stores::Game>) {
        let mut now: Vec<String> = inst.iter().map(|g| g.id.clone()).collect();
        now.sort();
        let mut cur: Vec<String> = self.games.borrow().iter().filter(|g| !g.store.is_empty() && !g.install).map(|g| g.id.clone()).collect();
        cur.sort();
        if now != cur {
            self.reload_games(inst);
            self.start_art();
        }
    }
}
