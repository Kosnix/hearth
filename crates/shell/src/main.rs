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
mod imgload;
mod menus;
mod oobe;
mod profiles;
mod scmd;
mod playtime;
mod prefs;
mod settings;
mod theme;

slint::include_modules!();

/// Code de sortie lu par le superviseur : ouvrir le bureau Windows.
const EXIT_DESKTOP: i32 = 10;
const MAIN_TITLE: &str = "Hearth";
const GUIDE_TITLE: &str = "Hearth Guide";
const SPLASH_TITLE: &str = "Hearth Splash";
const PAD_TITLE: &str = "Hearth Pad";
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

/// Jeu lancé par le client d'une boutique (Steam…) : seule sa fenêtre est connue, il n'est pas encapsulé.
struct ExtGame {
    tile: usize,
    hwnd: wm::HWND,
    started: std::time::Instant,
    closing: bool,
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
    pad: PadOverlay,
    pad_acc: Cell<bool>,
    status_n: Cell<u32>,
    oobe_step: Cell<i32>,
    oobe_rows: Rc<VecModel<SettingRow>>,
    oobe_content: Cell<usize>,
    oobe_sys: RefCell<[u8; 4]>,
    kb_oobe_name: Cell<bool>,
    oobe_avatar: Cell<bool>,
    pad_up: Cell<bool>,
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
    anim_n: Cell<u32>,
    ext_game: RefCell<Option<ExtGame>>,
    ext_pending: Cell<Option<(usize, std::time::Instant)>>,
    last_tab: Cell<i32>,
    profile_rows: Rc<VecModel<ProfileRow>>,
    kb_profile: Cell<bool>,
    new_profile: RefCell<String>,
    lib_cover: RefCell<std::collections::HashMap<String, slint::Image>>,
    lib_last: Cell<(i32, usize)>,
    art_pending: RefCell<std::collections::HashSet<String>>,
    lib_pending: RefCell<std::collections::HashSet<String>>,
    music_on: Cell<bool>,
    fg_main: Cell<bool>,
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
    featured: RefCell<Option<(Vec<stores::Offer>, Vec<Option<imgload::Buf>>)>>,
    store_status: RefCell<Vec<(String, bool, usize)>>,
    catalog_started: Cell<bool>,
    catalog_gen: Cell<u32>,
    kb_search: Cell<bool>,
    kb_identify: Cell<bool>,
    kb_login: Cell<bool>,
    kb_sgdb: Cell<bool>,
    login: RefCell<Option<stores::account::Challenge>>,
    pick_mode: RefCell<addflow::PickMode>,
    pick_entries: RefCell<Vec<addgame::Entry>>,
    pick_offers: RefCell<Vec<stores::Offer>>,
    pick_gen: Cell<u32>,
    identify_exe: RefCell<Option<PathBuf>>,
    guess_title: RefCell<String>,
    notifs: Rc<VecModel<Notif>>,
    toast_gen: Cell<u32>,
    splash_timer: slint::Timer,
    splash_start: Cell<Option<std::time::Instant>>,
    splash_uri: Cell<bool>,
}

fn main() -> Result<(), slint::PlatformError> {
    // Le shell n'a pas de console : les paniques vont dans un fichier.
    std::panic::set_hook(Box::new(|info| {
        let _ = std::fs::write(games_file().with_file_name("panic.log"), info.to_string());
    }));
    mark("start");
    let _profile = profiles::activate();
    mark("profile");
    // set_fullscreen() avant run() est ignoré par winit : plein écran dès la création.
    std::env::set_var("SLINT_FULLSCREEN", "1");
    // Legendary (jeux Epic) lit sa configuration ici, y compris quand il est lancé par le gestionnaire de fenêtres.
    std::env::set_var("LEGENDARY_CONFIG_PATH", stores::epiccmd::config());
    // ponytail: femtovg cesse de se redessiner avec ce design (constaté sur l'Optiplex) ; rendu logiciel.
    std::env::set_var("SLINT_BACKEND", std::env::var("HEARTH_BACKEND").unwrap_or_else(|_| "winit-skia".into()));
    let overrides = prefs::load();
    let games = load_games(stores::installed(), &overrides);
    mark("games");
    let shell = Rc::new(Shell {
        ui: App::new()?,
        guide: GuideMenu::new()?,
        splash: Splash::new()?,
        pad: PadOverlay::new()?,
        pad_acc: Cell::new(false),
        status_n: Cell::new(0),
        oobe_step: Cell::new(-1),
        oobe_rows: Rc::new(VecModel::default()),
        oobe_content: Cell::new(0),
        oobe_sys: RefCell::new([0; 4]),
        kb_oobe_name: Cell::new(false),
        oobe_avatar: Cell::new(false),
        pad_up: Cell::new(false),
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
        anim_n: Cell::new(0),
        ext_game: RefCell::new(None),
        ext_pending: Cell::new(None),
        last_tab: Cell::new(0),
        profile_rows: Rc::new(VecModel::default()),
        kb_profile: Cell::new(false),
        new_profile: RefCell::new(String::new()),
        lib_cover: RefCell::new(Default::default()),
        lib_last: Cell::new((-1, 0)),
        art_pending: RefCell::new(Default::default()),
        lib_pending: RefCell::new(Default::default()),
        music_on: Cell::new(false),
        fg_main: Cell::new(true),
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
        kb_sgdb: Cell::new(false),
        login: RefCell::new(None),
        pick_mode: RefCell::new(addflow::PickMode::Root),
        pick_entries: RefCell::new(vec![]),
        pick_offers: RefCell::new(vec![]),
        pick_gen: Cell::new(0),
        identify_exe: RefCell::new(None),
        guess_title: RefCell::new(String::new()),
        notifs: Rc::new(VecModel::default()),
        toast_gen: Cell::new(0),
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
    // Bibliothèque possédée : affichée tout de suite depuis le cache disque (la mise à jour réseau suit).
    *shell.owned.borrow_mut() = stores::account::owned_cache_load();
    {
        let mut g = shell.games.borrow().clone();
        shell.append_owned(&mut g);
        *shell.games.borrow_mut() = g;
    }
    shell.rebuild();
    shell.guide.set_notes(shell.notes.clone().into());
    *shell.store_status.borrow_mut() = stores::all().iter().map(|s| (s.name().to_string(), s.detected(), shell.games.borrow().iter().filter(|g| g.store == s.name()).count())).collect();
    mark("shell_built");
    i18n::init();
    theme::load();
    shell.apply_theme();
    i18n::init_audio_prefs();
    system::sfx::init();
    i18n::init_volumes();
    shell.refresh_lang();
    shell.ui.set_rows(shell.settings.rows.clone().into());
    shell.ui.set_offers(shell.offers.clone().into());
    shell.ui.set_notifs(shell.notifs.clone().into());
    let s = shell.clone();
    shell.ui.on_notifs_opened(move || s.notifs_opened());
    let s = shell.clone();
    shell.ui.on_notif_action(move |i| s.notif_action(i));

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
    shell.ui.on_profile_open(move || s.profile_open());
    let s = shell.clone();
    shell.ui.on_profile_pick(move |i| s.profile_pick(i as usize));
    let s = shell.clone();
    shell.ui.on_profile_menu(move |i| s.profile_menu(i as usize));
    let s = shell.clone();
    shell.ui.on_avatar_pick(move |a| s.avatar_pick(a));
    shell.ui.set_profiles(shell.profile_rows.clone().into());
    shell.profile_refresh_top();
    let s = shell.clone();
    shell.ui.on_onboard_pick(move |i| s.onboard_pick(i as usize));
    let s = shell.clone();
    shell.ui.on_nav_key(move |a| s.nav_with_sound(a));
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
    tick.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || timed("tick", || s.tick()));

    // Curseur de souris masqué après 5 s sans mouvement, réaffiché au moindre mouvement.
    #[link(name = "user32")]
    extern "system" {
        fn GetCursorPos(p: *mut [i32; 2]) -> i32;
        fn ShowCursor(show: i32) -> i32;
    }
    let cursor = Cell::new(([0i32; 2], std::time::Instant::now(), false));
    let cursor_timer = slint::Timer::default();
    cursor_timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
        let mut p = [0i32; 2];
        if unsafe { GetCursorPos(&mut p) } == 0 {
            return;
        }
        let (last, since, hidden) = cursor.get();
        if p != last {
            if hidden {
                while unsafe { ShowCursor(1) } < 0 {}
            }
            cursor.set((p, std::time::Instant::now(), false));
        } else if !hidden && since.elapsed() >= Duration::from_secs(5) {
            while unsafe { ShowCursor(0) } >= 0 {}
            cursor.set((last, since, true));
        }
    });

    let s = shell.clone();
    let poll = slint::Timer::default();
    poll.start(slint::TimerMode::Repeated, Duration::from_secs(20), move || s.poll_installed());

    let s = shell.clone();
    let phase = Cell::new(0.0f32);
    let anim = slint::Timer::default();
    anim.start(slint::TimerMode::Repeated, Duration::from_millis(100), move || timed("animate", || s.animate(&phase)));

    // Le menu guide est créé une fois puis affiché/masqué en Win32, sans jamais prendre le focus
    // (un jeu plein écran qui perd le focus se réduit et clignote).
    shell.guide.show()?;
    shell.splash.show()?;
    shell.pad.show()?;
    let s = shell.clone();
    shell.pad.on_press(move |x, y| s.pad_press(x, y));
    // Les fenêtres natives n'existent pas forcément dès le premier tour : on réessaie.
    let setup = Rc::new(slint::Timer::default());
    let t = setup.clone();
    setup.start(slint::TimerMode::Repeated, Duration::from_millis(50), move || {
        if let (Some(h), Some(sp), Some(pd)) = (wm::find_window(GUIDE_TITLE), wm::find_window(SPLASH_TITLE), wm::find_window(PAD_TITLE)) {
            wm::make_overlay(pd);
            wm::hide(pd);
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
    // Pilote de manette Xbox virtuelle (ViGEmBus) : installé automatiquement s'il manque (un seul essai).
    // L'assistant administrateur (une seule validation Windows, au premier lancement) est réinstallé quand sa liste d'opérations change (version 2 : ViGEmBus) ; ensuite plus aucune invite.
    let marker = stores::base_dir().join("helper-v3.done");
    if !marker.exists() && oobe::is_done() {
        // Marqueur global (tous profils), posé avant la tentative : jamais plus d'une invite par version.
        let _ = std::fs::write(&marker, "1");
        std::thread::spawn(|| {
            use std::os::windows::process::CommandExt;
            if system::admin::setup().is_err() {
                return;
            }
            let present = std::process::Command::new("sc").args(["query", "ViGEmBus"]).creation_flags(0x0800_0000).output().is_ok_and(|o| o.status.success());
            if !present {
                let _ = system::install::winget("ViGEm.ViGEmBus");
            }
            // Bibliothèques Visual C++ à jour : les jeux n'ont plus à lancer leur propre installateur (fenêtre + UAC).
            for id in ["Microsoft.VCRedist.2015+.x64", "Microsoft.VCRedist.2015+.x86"] {
                let _ = system::install::winget(id);
            }
        });
    }
    input::spawn(|nav| {
        let _ = slint::invoke_from_event_loop(move || {
            SHELL.with(|s| {
                if let Some(s) = s.borrow().as_ref() {
                    s.on_nav(nav);
                }
            });
        });
    });

    // Détecteur de blocages (HEARTH_PERF=1) : un timer de 8 ms qui journalise tout retard > 25 ms du thread UI.
    let s = shell.clone();
    let last = Cell::new(std::time::Instant::now());
    let watchdog = slint::Timer::default();
    if std::env::var_os("HEARTH_PERF").is_some() {
        watchdog.start(slint::TimerMode::Repeated, Duration::from_millis(8), move || {
        let gap = last.replace(std::time::Instant::now()).elapsed().as_secs_f64() * 1000.0;
        if gap > 25.0 && std::env::var_os("HEARTH_PERF").is_some() {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(games_file().with_file_name("perf.log")) {
                let _ = writeln!(f, "STALL {gap:.0} ms (zone {} tab {})", s.ui.get_zone(), s.ui.get_tab());
            }
        }
    });
    }
    Shell::schedule_updates();
    let s = shell.clone();
    let upd = slint::Timer::default();
    upd.start(slint::TimerMode::Repeated, Duration::from_secs(6 * 3600), move || s.check_updates(false));
    // Petit son de démarrage une fois le moteur audio prêt.
    slint::Timer::single_shot(Duration::from_millis(1800), || {
        if i18n::sfx() {
            system::sfx::play(system::sfx::Sfx::Boot);
        }
    });
    // Test sans manette : HEARTH_PADDEMO=bar|kbd affiche le bandeau du mode souris ou le clavier visuel.
    if let Ok(v) = std::env::var("HEARTH_PADDEMO") {
        let s = shell.clone();
        slint::Timer::single_shot(Duration::from_secs(5), move || {
            input::set_mouse_active(true);
            if v == "kbd" {
                input::set_osk(true);
            }
            s.pad_show(v == "kbd");
        });
    }
    shell.start_art();
    shell.refresh_owned();
    // Diagnostic de latence (HEARTH_PERF) : délai entre l'ordre de navigation et l'image suivante réellement rendue.
    if std::env::var_os("HEARTH_PERF").is_some() {
        let _ = shell.ui.window().set_rendering_notifier(|state, _| {
            if let slint::RenderingState::AfterRendering = state {
                if let Some(t) = input::take_last_input() {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(games_file().with_file_name("perf.log")) {
                        let _ = writeln!(f, "LAT entree->image {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
                    }
                }
            }
        });
    }
    let s = shell.clone();
    shell.ui.on_oobe_key(move |a| s.oobe_key(a));
    let s = shell.clone();
    shell.ui.on_oobe_click(move |i| s.oobe_click(i));
    shell.oobe_start();
    mark("before_run");
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
        // Jeux Steam : client lancé et connecté, superposition Steam coupée (une fois), fenêtre Steam réduite.
        let steam_uri = g.uri.is_some() && g.store == "Steam";
        if (self.is_scmd_game(&g.exe) || steam_uri) && !self.steam_ready(i, &g) {
            return;
        }
        self.start_splash(i, g.uri.is_some());
        if let Some(uri) = &g.uri {
            // Lancement par le client de la boutique : le jeu n'est pas encapsulé, on suivra sa fenêtre (tick).
            system::open_uri(uri);
            self.ext_pending.set(Some((i, std::time::Instant::now())));
            if steam_uri {
                self.steam_keep(1800);
            }
            return;
        }
        let exe = Path::new(&g.exe);
        let compat = wm::compat::lookup(exe);
        compat.apply_ini();
        let args: Vec<String> = g.args.iter().chain(&compat.args).cloned().collect();
        // Pare-feu : règle d'autorisation posée une fois par jeu (assistant administrateur), pour qu'aucune fenêtre Windows n'apparaisse en jeu.
        {
            let path = g.exe.clone();
            std::thread::spawn(move || {
                if system::admin::enabled() {
                    let _ = system::admin::request(&format!("firewall {path}"));
                }
            });
        }
        match wm::Session::launch(exe, &args) {
            Ok(session) => {
                let hands_off = g.anticheat || compat.anticheat || !compat.borderless;
                self.running.borrow_mut().push(Running { tile: i, session, hands_off, closing: false, started: std::time::Instant::now() });
                if i18n::sfx() {
                    system::sfx::play(system::sfx::Sfx::Launch);
                }
                self.rebuild();
            }
            Err(_e) => {
                self.hide_splash();
                system::sfx::play(system::sfx::Sfx::Error);
            }
        }
    }

    /// Bandeau d'aide du mode souris (`kbd` faux) ou clavier visuel, en bas de l'écran, sans prendre le focus.
    fn pad_show(&self, kbd: bool) {
        self.pad.set_kbd(kbd);
        if kbd {
            self.pad_keys();
            self.pad.set_kbx(0);
            self.pad.set_kby(0);
        }
        if let Some(h) = wm::find_window(PAD_TITLE) {
            let (x, y, mw, mh) = wm::primary_monitor();
            let s = self.ui.window().scale_factor();
            let (w, hh) = if kbd { (1180.0 * s, 560.0 * s) } else { (1300.0 * s, 96.0 * s) };
            let (w, hh) = (w as i32, hh as i32);
            wm::show_overlay(h, x + (mw - w) / 2, y + mh - hh - if kbd { 40 } else { 140 }, w, hh);
            self.pad.window().request_redraw();
        }
    }

    fn pad_hide(&self) {
        input::set_osk(false);
        if let Some(h) = wm::find_window(PAD_TITLE) {
            wm::hide(h);
        }
    }

    /// Active ou quitte le mode souris (menu guide ; la manette le fait aussi avec L3 + R3).
    fn pad_toggle(&self) {
        let on = !input::mouse_active();
        input::set_mouse_active(on);
        self.pad_sync();
    }

    /// Affiche ou masque le bandeau selon l'état du mode souris.
    fn pad_sync(&self) {
        if input::mouse_active() {
            self.pad_show(false);
        } else {
            self.pad_hide();
        }
    }

    fn pad_keys(&self) {
        let (up, acc) = (self.pad_up.get(), self.pad_acc.get());
        let rows: [&str; 4] = if acc {
            ["éèêëàâäùûü", "ôöîïçœæñÿ'", "@.-_,;:!?/", "+*=%€$#&()"]
        } else {
            ["1234567890", "azertyuiop", "qsdfghjklm", "wxcvbn.-_@"]
        };
        let keys: Vec<SharedString> = rows.iter().flat_map(|r| r.chars()).map(|c| if up { c.to_uppercase().to_string() } else { c.to_string() }.into()).collect();
        self.pad.set_keys(Rc::new(VecModel::from(keys)).into());
        self.pad.set_acc(acc);
    }

    fn pad_press(&self, x: i32, y: i32) {
        if x < 0 {
            input::set_osk(false);
            self.pad_show(false);
            return;
        }
        if y < 4 {
            if let Some(k) = self.pad.get_keys().row_data((y * 10 + x) as usize) {
                input::send_text(k.as_str());
            }
            return;
        }
        match x {
            0 => self.pad_up.set(!self.pad_up.get()),
            1 => self.pad_acc.set(!self.pad_acc.get()),
            2 => input::send_text(" "),
            3 => input::backspace(),
            4 => input::enter(),
            _ => {
                input::set_osk(false);
                self.pad_show(false);
                return;
            }
        }
        self.pad_keys();
    }

    fn on_nav(&self, nav: Nav) {
        if let Some(t) = input::last_input() {
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            if ms > 8.0 {
                timed_ms("LAT file_attente", ms);
            }
        }
        // Mode souris : L3 + R3 (ou menu guide) l'active ; Triangle ouvre le clavier visuel qui écrit dans la fenêtre active.
        match nav {
            Nav::Mouse => {
                self.pad_sync();
                return;
            }
            Nav::Osk => {
                if input::mouse_active() {
                    input::set_osk(true);
                    self.pad_show(true);
                }
                return;
            }
            _ => {}
        }
        if input::osk_open() {
            self.pad.invoke_nav(nav as i32);
            return;
        }
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
                self.nav_with_sound(nav as i32);
            }
            return;
        }
        let fg = wm::foreground();
        let target = self.running.borrow().iter().find(|r| r.session.owns(fg)).map(|r| (r.tile, r.hands_off));
        // Jeu lancé par le client Steam : son menu s'ouvre par-dessus, quelle que soit la fenêtre devant (jeu, Steam, console).
        let ext_tile = self.ext_game.borrow().as_ref().map(|e| e.tile);
        if let (None, Some(tile)) = (target, ext_tile) {
            self.open_guide(tile);
            return;
        }
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
            settings::Outcome::Oobe => self.oobe_restart(),
            settings::Outcome::ThemeChanged => {
                self.apply_theme();
                self.open_cat_keep(13);
            }
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
            settings::Outcome::Sgdb => {
                self.kb_sgdb.set(true);
                self.settings.kb_open_text(&i18n::pref("steamgriddb_key").unwrap_or_default());
                self.kb_refresh();
                self.ui.set_kbx(0);
                self.ui.set_kby(0);
                self.ui.set_kb_title(tr("Clé API SteamGridDB").into());
                self.ui.set_zone(4);
                self.push_notif("SteamGridDB", &tr("Clé gratuite : steamgriddb.com, Préférences, API"));
            }
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
        // Ctrl+V : colle le texte du presse-papiers (une seule ligne).
        let pasted;
        let t = if t == "\u{16}" {
            pasted = clipboard_text().unwrap_or_default().replace(['\r', '\n'], " ").trim().to_string();
            pasted.as_str()
        } else {
            t
        };
        if t.is_empty() {
            return;
        }
        let k = self.settings.kb_char(t);
        self.kb_apply(k);
    }

    fn kb_apply(&self, k: settings::Kb) {
        self.ui.set_kb_acc(self.settings.kb_acc());
        match k {
            settings::Kb::Continue => self.kb_refresh(),
            settings::Kb::Cancel => {
                let back = if self.kb_profile.replace(false) {
                    11
                } else if self.kb_search.replace(false) {
                    0
                } else if self.kb_identify.replace(false) {
                    6
                } else if self.kb_login.replace(false) {
                    5
                } else if self.kb_sgdb.replace(false) {
                    self.home_zone()
                } else if self.kb_oobe_name.replace(false) {
                    13
                } else if self.rename_id.borrow_mut().take().is_some() {
                    0
                } else if self.scmd_stage.get() != 0 {
                    self.scmd_cancel();
                    0
                } else {
                    self.home_zone()
                };
                self.ui.set_zone(back);
            }
            settings::Kb::Done(ssid, text) => {
                let renaming = self.rename_id.borrow_mut().take();
                if self.kb_oobe_name.replace(false) {
                    self.oobe_profile_name(text);
                    return;
                }
                if self.kb_profile.replace(false) {
                    self.profile_named(text);
                } else if self.scmd_stage.get() != 0 {
                    self.scmd_text(text);
                } else if let Some(id) = renaming {
                    self.ui.set_zone(0);
                    self.rename_game(&id, text.trim());
                } else if self.kb_sgdb.replace(false) {
                    self.ui.set_zone(self.home_zone());
                    i18n::set_pref("steamgriddb_key", text.trim());
                    self.push_notif("SteamGridDB", &tr(if text.trim().is_empty() { "Clé supprimée" } else { "Clé enregistrée" }));
                    self.start_art();
                    if self.ui.get_cat() == 12 {
                        self.spawn_fetch(12);
                    }
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
                    self.ui.set_zone(self.home_zone());
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
        self.oobe_sync();
    }

    fn display_changed(&self) {
        if let Some(h) = wm::find_window(MAIN_TITLE) {
            wm::fit_to_monitor(h);
            wm::nudge(h);
        }
    }

    /// Barre d'état (réseau, volume, batterie de la manette) : lue hors du fil de l'interface, toutes les 10 s.
    pub fn refresh_status(&self) {
        let w = self.ui.as_weak();
        std::thread::spawn(move || {
            let net = system::net::current().map_or(-1, |l| if l.wifi { 1 } else { 0 });
            let vol = system::audio::volume().map_or(-1, |v| v as i32);
            let pad = system::pads::list().first().map_or(-1, |p| p.1.map_or(101, |b| b as i32));
            let _ = w.upgrade_in_event_loop(move |ui| {
                ui.set_st_net(net);
                ui.set_st_vol(vol);
                ui.set_st_pad(pad);
            });
        });
    }

    fn set_clock(&self) {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        self.ui.set_clock(format!("{:02}:{:02}", t.wHour, t.wMinute).into());
    }

    fn home(&self) {
        if let Some(h) = wm::find_window(MAIN_TITLE) {
            // Un jeu qui a changé la résolution a pu rétrécir notre fenêtre : on la recale.
            wm::fit_to_monitor(h);
            wm::nudge(h);
            wm::focus(h);
        }
        self.ui.invoke_nav(Nav::Guide as i32);
    }

    /// Applique le thème courant (couleurs) à toutes les fenêtres de Hearth.
    fn apply_theme(&self) {
        let p = theme::palette();
        let c = |v: [u8; 3]| slint::Color::from_rgb_u8(v[0], v[1], v[2]);
        macro_rules! set {
            ($w:expr) => {{
                let t = $w.global::<Theme>();
                t.set_accent(c(p.accent));
                t.set_on_accent(c(p.on_accent));
                t.set_bg(c(p.bg));
                t.set_bg_settings(c(p.bg));
                t.set_band(c(p.band));
                t.set_tile(c(p.tile));
                t.set_bg_guide(c(p.guide));
                t.set_deep(c(p.deep));
                t.set_panel(slint::Color::from_argb_u8(0xf2, p.band[0], p.band[1], p.band[2]));
            }};
        }
        set!(self.ui);
        set!(self.guide);
        set!(self.splash);
        set!(self.pad);
    }

    fn open_guide(&self, tile: usize) {
        input::set_capture(true);
        self.guide_tile.set(tile);
        let closing = self.running.borrow().iter().any(|r| r.tile == tile && r.closing) || self.ext_game.borrow().as_ref().is_some_and(|e| e.tile == tile && e.closing);
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
        let game = || {
            self.running.borrow().iter().find(|r| r.tile == tile).and_then(|r| r.session.main_window()).or_else(|| self.ext_game.borrow().as_ref().filter(|e| e.tile == tile).map(|e| e.hwnd))
        };
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
                {
                    let mut ext = self.ext_game.borrow_mut();
                    if let Some(e) = ext.as_mut().filter(|e| e.tile == tile) {
                        if e.closing {
                            wm::kill_owner(e.hwnd);
                        } else {
                            e.closing = true;
                            wm::close_window(e.hwnd);
                        }
                    }
                }
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
            (6, 0) => {
                self.hide_guide();
                self.pad_toggle();
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
        self.oobe_guard();
        let n = self.status_n.get().wrapping_add(1);
        self.status_n.set(n);
        if n % 10 == 1 {
            self.refresh_status();
        }
        self.set_clock();
        self.maybe_prompt_update();
        // Musique d'ambiance : seulement quand la console est devant (pas pendant un jeu).
        let want = i18n::music() && self.running.borrow().is_empty() && wm::find_window(MAIN_TITLE).is_some_and(|h| h == wm::foreground());
        if want != self.music_on.get() && system::sfx::music(want) {
            self.music_on.set(want);
        }
        if self.settings.tick() {
            self.display_changed();
            self.open_cat_keep(self.ui.get_cat());
        } else if self.settings.confirming() {
            self.open_cat_keep(self.ui.get_cat());
        }
        self.track_ext_game();
        let ended: Vec<(usize, u64)> = {
            let mut running = self.running.borrow_mut();
            let ended: Vec<(usize, u64)> = running.iter().filter(|r| !r.session.alive()).map(|r| (r.tile, r.started.elapsed().as_secs())).collect();
            running.retain(|r| r.session.alive());
            ended
        };
        for &(tile, secs) in &ended {
            let key = playtime_key(&self.games.borrow()[tile]);
            playtime::record(&key, secs);
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
        input::set_capture(false);
        if let Some(h) = wm::find_window(GUIDE_TITLE) {
            wm::hide(h);
        }
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
        if i18n::sfx() && self.fg_main.get() {
            system::sfx::play(system::sfx::Sfx::Notify);
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
    stores::profile_dir().join("games.json")
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
        static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
        if FIRST.swap(false, std::sync::atomic::Ordering::Relaxed) {
            mark("first_animate");
        }
        // Tick à 25 Hz pour un mouvement fluide ; les vérifications coûteuses (fenêtres, manette) une fois sur cinq.
        let n = self.anim_n.get().wrapping_add(1);
        self.anim_n.set(n);
        if n % 2 == 0 {
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
                self.pad.global::<Pad>().set_kind(pad);
            }
            self.fg_main.set(wm::find_window(MAIN_TITLE).is_some_and(|h| h == wm::foreground()));
            let tab = self.ui.get_tab();
            if tab != self.last_tab.replace(tab) && tab == 1 {
                self.ui.set_lsel(0);
                self.rebuild();
            }
        }
        {
            timed("sync_library_art", || self.sync_library_art());
        }
        if i18n::anim() && self.ui.get_zone() < 3 && self.fg_main.get() {
            phase.set((phase.get() + 0.2) % 3600.0);
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
                            if let Some(p) = stores::fetch_first(imgs).and_then(|p| imgload::decode(&p, 300, 450)) {
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
        let paths: Vec<Option<imgload::Buf>> = vec![None; offers.len()];
        if self.catalog_gen.get() == gen {
            self.apply_offers(&offers, &paths);
        }
        if featured {
            *self.featured.borrow_mut() = Some((offers, paths));
        }
    }

    fn offer_art(&self, gen: u32, featured: bool, i: usize, p: imgload::Buf) {
        if featured {
            if let Some((_, paths)) = self.featured.borrow_mut().as_mut() {
                if let Some(slot) = paths.get_mut(i) {
                    *slot = Some(p.clone());
                }
            }
        }
        if self.catalog_gen.get() == gen {
            if let Some(mut row) = self.offers.row_data(i) {
                let img = imgload::image(&p);
                row.art = img;
                self.offers.set_row_data(i, row);
            }
        }
    }

    fn apply_offers(&self, offers: &[stores::Offer], paths: &[Option<imgload::Buf>]) {
        let rows: Vec<Offer> = offers
            .iter()
            .zip(paths)
            .map(|(o, p)| Offer {
                title: o.title.as_str().into(),
                price: if o.price == "Gratuit" { tr("Gratuit") } else { o.price.clone() }.into(),
                store: o.store.as_str().into(),
                art: p.as_ref().map(imgload::image).unwrap_or_default(),
                owned: false,
                initial: o.title.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default().into(),
            })
            .collect();
        let own = self.owned_titles();
        let rows: Vec<Offer> = rows.into_iter().map(|mut r| { r.owned = own.contains(&stores::norm_title(&r.title)); r }).collect();
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
            let hero = stores::fetch_first(&o.heroes).and_then(|p| imgload::decode(&p, 1920, 1080));
            let shots: Vec<imgload::Buf> = d.shots.iter().filter_map(|u| stores::fetch_first(std::slice::from_ref(u))).filter_map(|p| imgload::decode(&p, 420, 240)).collect();
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.details_ready(gen, d, hero, shots)));
        });
    }

    fn details_ready(&self, gen: u32, d: stores::Details, hero: Option<imgload::Buf>, shots: Vec<imgload::Buf>) {
        if self.det_gen.get() != gen {
            return;
        }
        if !d.desc.is_empty() {
            self.ui.set_det_desc(d.desc.into());
        }
        if !d.meta.is_empty() {
            self.ui.set_det_meta(d.meta.into());
        }
        if let Some(h) = hero.as_ref().map(imgload::image) {
            self.ui.set_det_hero(h);
        }
        self.det_shots.set_vec(shots.iter().map(imgload::image).collect::<Vec<_>>());
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
        // Jeux sans aucun visuel (ajoutés à la main, etc.) : recherche par titre.
        let titleless: Vec<(String, String)> =
            self.games.borrow().iter().filter(|g| !g.id.starts_with("demo-") && stores::asset(&g.id, "cover").is_none()).map(|g| (g.id.clone(), g.name.clone())).collect();
        let installed = self.games.borrow().iter().any(|g| !g.store.is_empty() && !g.install);
        std::thread::spawn(move || {
            if installed {
                jobs.extend(stores::installed().into_iter().map(|g| (g.id, g.title, g.art)));
            }
            for (id, title) in titleless {
                if jobs.iter().any(|(j, _, a)| *j == id && !a.cover.is_empty()) {
                    continue;
                }
                if let Some(art) = stores::resolve_art(&title) {
                    jobs.retain(|(j, _, _)| *j != id);
                    jobs.push((id, title, art));
                }
            }
            let key = &key;
            std::thread::scope(|sc| {
                for chunk in jobs.chunks(jobs.len().div_ceil(3).max(1)) {
                    sc.spawn(move || {
                        imgload::background();
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

    /// Un visuel vient d'arriver : on met à jour uniquement la tuile concernée (pas de reconstruction de toute la liste).
    pub fn reload_art(&self, id: &str) {
        self.art_cache.borrow_mut().remove(id);
        self.lib_cover.borrow_mut().remove(id);
        self.lib_last.set((-1, 0));
        let games = self.games.borrow();
        let Some(g) = games.iter().find(|g| g.id == id) else { return };
        // Jeux installés : nouveau décodage en arrière-plan, l'ancien visuel reste affiché jusque-là.
        // Bibliothèque : lib_cover est vidé et sync_library_art recharge la tuile.
        if !g.install {
            self.art_request(g);
        }
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
    /// Visuels d'un jeu installé : depuis le cache ; sinon vides le temps du décodage en arrière-plan (art_request).
    fn art_of(&self, g: &Game) -> (slint::Image, slint::Image, slint::Image) {
        if let Some(a) = self.art_cache.borrow().get(&g.id) {
            return a.clone();
        }
        self.art_request(g);
        Default::default()
    }

    fn art_request(&self, g: &Game) {
        if !self.art_pending.borrow_mut().insert(g.id.clone()) {
            return;
        }
        let id = g.id.clone();
        let cover = if g.cover.is_empty() { stores::asset(&g.id, "cover") } else { Some(PathBuf::from(&g.cover)) };
        let (hero, logo) = (stores::asset(&g.id, "hero"), stores::asset(&g.id, "logo"));
        std::thread::spawn(move || {
            let (c, h, l) = imgload::limited(|| {
                let c = cover.and_then(|p| imgload::decode(&p, 600, 900));
                let h = hero.and_then(|p| imgload::decode(&p, 1920, 1080));
                let l = logo.and_then(|p| imgload::decode(&p, 800, 320));
                (c, h, l)
            });
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.art_ready(&id, c, h, l)));
        });
    }

    fn art_ready(&self, id: &str, c: Option<imgload::Buf>, h: Option<imgload::Buf>, l: Option<imgload::Buf>) {
        self.art_pending.borrow_mut().remove(id);
        let img = |b: &Option<imgload::Buf>| b.as_ref().map(imgload::image).unwrap_or_default();
        let art = (img(&c), img(&h), img(&l));
        self.art_cache.borrow_mut().insert(id.to_string(), art.clone());
        let Some(gid) = self.games.borrow().iter().position(|g| g.id == id) else { return };
        for r in 0..self.tiles.row_count() {
            if let Some(mut t) = self.tiles.row_data(r) {
                if t.gid == gid as i32 {
                    (t.cover, t.hero, t.logo) = art.clone();
                    self.tiles.set_row_data(r, t);
                }
            }
        }
    }

    fn make_tile(&self, gid: usize, g: &Game, running: bool, pt: &std::collections::HashMap<String, playtime::Entry>) -> Tile {
        // Bibliothèque (jeux à installer) : les jaquettes sont chargées à la demande (sync_library_art), jamais toutes d'un coup.
        let (cover, hero, logo) = if g.install {
            (self.lib_cover.borrow().get(&g.id).cloned().unwrap_or_default(), Default::default(), Default::default())
        } else {
            self.art_of(g)
        };
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
            store: if g.store.is_empty() { tr("Hearth") } else { g.store.clone() }.into(),
            played: playtime::played_in(pt, &playtime_key(g)).into(),
            lastp: playtime::last_in(pt, &playtime_key(g)).into(),
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
        let mut running: Vec<usize> = self.running.borrow().iter().map(|r| r.tile).collect();
        if let Some(e) = self.ext_game.borrow().as_ref() {
            running.push(e.tile);
        }
        let all_tab = false;
        let (mut a, mut b): (Vec<(u64, Tile)>, Vec<Tile>) = (vec![], vec![]);
        for (i, g) in self.games.borrow().iter().enumerate() {
            if g.hidden {
                continue;
            }
            let t = self.make_tile(i, g, running.contains(&i), &pt);
            // Onglet Tout : tous les jeux (installés ou non) ; sinon seulement ceux à installer.
            if g.install || all_tab {
                b.push(t.clone());
            }
            if !g.install {
                a.push((playtime::last(&playtime_key(g), &pt), t));
            }
        }
        a.sort_by_key(|(last, _)| std::cmp::Reverse(*last));
        let a: Vec<Tile> = a.into_iter().map(|(_, t)| t).collect();
        if all_tab {
            b.sort_by_key(|t| t.name.to_lowercase());
        }
        let sel = cur.and_then(|g| a.iter().position(|t| t.gid == g)).unwrap_or((self.ui.get_sel().max(0) as usize).min(a.len().saturating_sub(1)));
        let lsel = cur_lib.and_then(|g| b.iter().position(|t| t.gid == g)).unwrap_or((self.ui.get_lsel().max(0) as usize).min(b.len().saturating_sub(1)));
        self.tiles.set_vec(a);
        self.library.set_vec(b);
        self.lib_last.set((-1, 0));
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

impl Shell {
    fn ui_state(&self) -> [i32; 18] {
        let u = &self.ui;
        [
            u.get_zone(), u.get_tab(), u.get_top(), u.get_sel(), u.get_lsel(), u.get_osel(), u.get_psel(), u.get_msel(), u.get_nsel(),
            u.get_dsel(), u.get_dlsel(), u.get_srow(), u.get_cat(), u.get_brow(), u.get_kbx(), u.get_kby(), u.get_pw(), u.get_sfocus(),
        ]
    }

    /// Navigation (manette ou clavier) avec son : déplacement, validation, retour, ouverture d'un menu.
    fn nav_with_sound(&self, a: i32) {
        let before = self.ui_state();
        timed("invoke_nav", || self.ui.invoke_nav(a));
        if !i18n::sfx() || a == 6 {
            return;
        }
        let after = self.ui_state();
        use system::sfx::Sfx;
        let s = if after[0] != before[0] {
            if a == 5 {
                // Fermer un panneau ou un menu : son de fermeture ; simple retour en arrière : son de retour.
                if matches!(before[0], 2 | 3 | 7 | 8 | 9 | 10 | 4 | 5) { Some(Sfx::Close) } else { Some(Sfx::Back) }
            } else if matches!(after[0], 2 | 3 | 7 | 8 | 9 | 10) || a == 7 {
                Some(Sfx::Open)
            } else {
                Some(Sfx::Select)
            }
        } else if after[1] != before[1] {
            Some(Sfx::Tab)
        } else if after != before {
            match a {
                0..=3 => Some(Sfx::Move),
                4 if before[0] == 4 => Some(Sfx::Key),
                4 => Some(Sfx::Select),
                5 => Some(Sfx::Back),
                _ => None,
            }
        } else if a == 4 {
            Some(if before[0] == 4 { Sfx::Key } else { Sfx::Select })
        } else {
            None
        };
        if let Some(s) = s {
            system::sfx::play(s);
        }
    }
}

impl Shell {
    /// Jaquettes de la bibliothèque : décodées en arrière-plan (threads) pour les rangées proches de la sélection,
    /// libérées pour les autres (cf. app.slint : rangées lrow-2 .. lrow+3 affichées, une de marge de chaque côté ici).
    fn sync_library_art(&self) {
        let (lsel, n) = (self.ui.get_lsel(), self.library.row_count());
        if self.lib_last.get() == (lsel, n) {
            return;
        }
        self.lib_last.set((lsel, n));
        let cols = 10usize;
        let lrow = (lsel.max(0) as usize) / cols;
        let (lo, hi) = (lrow.saturating_sub(3) * cols, (lrow + 5) * cols);
        let mut jobs: Vec<(String, PathBuf)> = vec![];
        for r in 0..n {
            let Some(mut t) = self.library.row_data(r) else { continue };
            let inwin = r >= lo && r < hi;
            let loaded = t.cover.size().width > 0;
            let games = self.games.borrow();
            let Some(g) = games.get(t.gid as usize) else { continue };
            if inwin && !loaded {
                if let Some(img) = self.lib_cover.borrow().get(&g.id) {
                    t.cover = img.clone();
                    self.library.set_row_data(r, t);
                } else if !self.lib_pending.borrow().contains(&g.id) {
                    let path = if g.cover.is_empty() { stores::asset(&g.id, "cover") } else { Some(PathBuf::from(&g.cover)) };
                    if let Some(p) = path {
                        self.lib_pending.borrow_mut().insert(g.id.clone());
                        jobs.push((g.id.clone(), p));
                    }
                }
            } else if !inwin && loaded {
                self.lib_cover.borrow_mut().remove(&g.id);
                t.cover = Default::default();
                self.library.set_row_data(r, t);
            }
        }
        if jobs.is_empty() {
            return;
        }
        std::thread::spawn(move || {
            std::thread::scope(|sc| {
                for chunk in jobs.chunks(jobs.len().div_ceil(4).max(1)) {
                    sc.spawn(move || {
                        for (id, path) in chunk {
                            let buf = imgload::decode(path, 300, 450);
                            let id = id.clone();
                            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.lib_cover_ready(&id, buf)));
                        }
                    });
                }
            });
        });
    }

    fn lib_cover_ready(&self, id: &str, buf: Option<imgload::Buf>) {
        self.lib_pending.borrow_mut().remove(id);
        let Some(buf) = buf else { return };
        let img = imgload::image(&buf);
        self.lib_cover.borrow_mut().insert(id.to_string(), img);
        // Force la mise en place sur la tuile si elle est (toujours) dans la fenêtre.
        self.lib_last.set((-1, 0));
    }
}

/// Mesure de diagnostic : journalise dans perf.log tout traitement du thread UI qui dure plus de 8 ms.
fn timed<R>(what: &str, f: impl FnOnce() -> R) -> R {
    let t0 = std::time::Instant::now();
    let r = f();
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    if ms > 8.0 && std::env::var_os("HEARTH_PERF").is_some() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(games_file().with_file_name("perf.log")) {
            let _ = writeln!(f, "{what}: {ms:.1} ms");
        }
    }
    r
}

impl Shell {
    /// Jeux lancés par un client de boutique : retrouve leur fenêtre au premier plan après le lancement, et détecte leur fin.
    fn track_ext_game(&self) {
        if let Some((tile, t0)) = self.ext_pending.get() {
            if t0.elapsed() > Duration::from_secs(240) {
                self.ext_pending.set(None);
            } else if let Some(h) = wm::foreground_game(&["steam.exe", "steamwebhelper.exe", "explorer.exe", "hearth-shell.exe", "hearth.exe", "claude.exe", "cmd.exe"]) {
                *self.ext_game.borrow_mut() = Some(ExtGame { tile, hwnd: h, started: std::time::Instant::now(), closing: false });
                self.ext_pending.set(None);
                self.rebuild();
            }
        }
        let ended = self.ext_game.borrow().as_ref().filter(|e| !wm::is_alive(e.hwnd)).map(|e| (e.tile, e.started.elapsed().as_secs()));
        if let Some((tile, secs)) = ended {
            *self.ext_game.borrow_mut() = None;
            if let Some(g) = self.games.borrow().get(tile).cloned() {
                playtime::record(&playtime_key(&g), secs);
            }
            self.hide_guide();
            self.rebuild();
            self.home();
        }
    }
}

/// Texte du presse-papiers Windows (Unicode), s'il y en a.
fn clipboard_text() -> Option<String> {
    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(owner: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalLock(h: isize) -> *const u16;
        fn GlobalUnlock(h: isize) -> i32;
    }
    unsafe {
        if OpenClipboard(0) == 0 {
            return None;
        }
        let h = GetClipboardData(13); // CF_UNICODETEXT
        let mut out = None;
        if h != 0 {
            let p = GlobalLock(h);
            if !p.is_null() {
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
        out
    }
}

static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Repère de démarrage (HEARTH_PERF) : temps écoulé depuis le début de main.
fn mark(what: &str) {
    let t0 = *START.get_or_init(std::time::Instant::now);
    if std::env::var_os("HEARTH_PERF").is_some() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(games_file().with_file_name("perf.log")) {
            let _ = writeln!(f, "MARK {what} {:.0} ms", t0.elapsed().as_secs_f64() * 1000.0);
        }
    }
}

fn timed_ms(what: &str, ms: f64) {
    if std::env::var_os("HEARTH_PERF").is_some() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(games_file().with_file_name("perf.log")) {
            let _ = writeln!(f, "{what} {ms:.0} ms");
        }
    }
}
