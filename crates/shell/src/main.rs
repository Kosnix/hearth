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
mod desktop;
mod accounts;
mod addflow;
mod i18n;
mod imgload;
mod keyboard;
mod launchers;
mod menus;
mod native;
mod oobe;
mod profiles;
mod scmd;
mod sounds;
mod selfupdate;
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
const OFFERS_PAGE: usize = 24; // Boutique : nombre de cartes révélées par défaut / par clic sur « Voir plus ».
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
    /// Arguments de lancement ajoutés par l'utilisateur (ou appliqués d'office pour passer le launcher).
    #[serde(skip)]
    user_args: String,
    /// Autre exécutable à lancer (option de lancement Steam sans launcher), chemin complet.
    #[serde(skip)]
    exe_override: Option<String>,
}

/// Jeu lancé par le client d'une boutique (Steam…) : seule sa fenêtre est connue, il n'est pas encapsulé.
struct ExtGame {
    tile: usize,
    hwnd: wm::HWND,
    started: std::time::Instant,
    closing: bool,
    /// Processus de la fenêtre repérée et ses descendants : un lanceur de jeu (première fenêtre vue)
    /// qui démarre le vrai jeu puis se ferme ne doit pas faire croire la partie terminée.
    tree: wm::ProcTree,
    /// Depuis quand la fenêtre suivie a disparu sans qu'aucune autre de l'arbre ne l'ait remplacée.
    lost: Option<std::time::Instant>,
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
    /// Dernier état réseau relevé (i32::MIN : aucun encore) et alerte batterie déjà donnée (`status_alerts`).
    last_net: Cell<i32>,
    pad_low_warned: Cell<bool>,
    net_pending: Cell<bool>,
    desktop: RefCell<Option<DesktopWindow>>,
    desk_games: Rc<VecModel<DeskGame>>,
    desk_rows: Rc<VecModel<SettingRow>>,
    desk_stores: Rc<VecModel<DeskStore>>,
    desk_filter: Cell<i32>,
    desk_query: RefCell<String>,
    photo_fresh: Cell<bool>,
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
    /// Jeu dont on saisit les arguments de lancement au clavier virtuel.
    kb_args: RefCell<Option<String>>,
    /// Profil existant dont on modifie le nom (clavier) ou l'avatar (éditeur).
    profile_edit: RefCell<Option<String>>,
    avatar_for: RefCell<Option<String>>,
    scmd_stage: Cell<u8>,
    steam_wait: Cell<bool>,
    /// Un lanceur de boutique (Epic, GOG Galaxy) démarre avant un jeu (voir `launchers`).
    launcher_wait: Cell<bool>,
    /// Jaquette agrandie en plein écran au lancement : 0 rien, 1 en train de grandir (l'écran de
    /// lancement suit), 2 en attente derrière le jeu — elle se rétracte au retour sur Hearth.
    zoom: Cell<u8>,
    zoom_timer: slint::Timer,
    /// Animation de `zoom-t` en cours : (début, valeur de départ, valeur d'arrivée).
    zoom_anim: Cell<Option<(std::time::Instant, f32, f32)>>,
    zoom_anim_timer: slint::Timer,
    /// Dernière action manette/clavier et niveau du voile d'inactivité de l'accueil (voir `idle_tick`).
    last_activity: Cell<std::time::Instant>,
    dim: Cell<f32>,
    /// Dernier son de « limite » (bord d'une liste), pour ne pas le répéter en rafale en maintenant.
    last_bump: Cell<Option<std::time::Instant>>,
    vol_osd_gen: Cell<u32>,
    /// Filtre de la Bibliothèque : 0 tous, 1 à installer, 2 cachés.
    lib_filter: Cell<u8>,
    anim_n: Cell<u32>,
    ext_game: RefCell<Option<ExtGame>>,
    ext_pending: Cell<Option<(usize, std::time::Instant)>>,
    last_tab: Cell<i32>,
    /// Dernier ordre reçu au clavier physique (`NavKeys`) : pour savoir si c'est le clavier ou la manette
    /// qui a servi en dernier (affichage des indications à l'écran, voir `Pad.kind` == 2).
    kb_last: Cell<Option<std::time::Instant>>,
    /// Appareil Bluetooth (identifiant, nom) à oublier, en attente de confirmation (`Act::ConfirmBtUnpair`).
    bt_unpair_id: RefCell<Option<(String, String)>>,
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
    hearth_update: RefCell<Option<(String, String)>>,
    hearth_prompted: Cell<bool>,
    /// Dernière vérification de mise à jour de Hearth : `None` jamais faite, `Some(false)` GitHub injoignable.
    hearth_checked: Cell<Option<bool>>,
    /// Octets reçus / total, pendant le téléchargement de la mise à jour de Hearth (affiché dans
    /// Téléchargements comme les jeux) ; `None` hors téléchargement.
    hearth_dl: RefCell<Option<std::sync::Arc<(std::sync::atomic::AtomicU64, std::sync::atomic::AtomicU64)>>>,
    hearth_dl_timer: slint::Timer,
    sound_catalog: RefCell<Vec<sounds::Pack>>,
    sound_list: RefCell<Vec<sounds::Pack>>,
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
    /// Toutes les cartes déjà construites (art compris), même celles pas encore révélées (« Voir plus »)
    /// ou cachées par le filtre de boutique courant. Index stable : celui du tableau renvoyé par la requête.
    offers_full: RefCell<Vec<(stores::Offer, Offer)>>,
    /// Index dans `offers_full` sans aucune jaquette trouvée nulle part : masqués (voir `offer_no_art`).
    offers_no_art: RefCell<std::collections::HashSet<usize>>,
    /// Requête (`None` = catalogue « à la une ») et nombre d'entrées demandées par boutique à la
    /// dernière/prochaine requête réseau — « Voir plus » l'augmente et relit (voir `show_more_offers`).
    catalog_query: RefCell<Option<String>>,
    catalog_limit: Cell<usize>,
    /// Boutiques distinctes présentes dans `offers_full` (pour les puces de filtre) et filtre actif (`None` = Tous).
    offer_store_names: RefCell<Vec<String>>,
    offer_filter: RefCell<Option<String>>,
    /// 0 pertinence (ordre renvoyé par les boutiques), 1 nom A→Z, 2 prix croissant, 3 prix décroissant.
    offer_sort: Cell<i32>,
    /// Filtre « Gratuits uniquement » de la Boutique.
    offer_free: Cell<bool>,
    featured: RefCell<Option<Featured>>,
    store_status: RefCell<Vec<(String, bool, usize)>>,
    catalog_started: Cell<bool>,
    catalog_gen: Cell<u32>,
    kb_search: Cell<bool>,
    kb_lib_search: Cell<bool>,
    lib_query: RefCell<String>,
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
    /// Titre de la bannière affichée et nombre de notifications regroupées dessous.
    toast_key: RefCell<String>,
    toast_n: Cell<u32>,
    splash_timer: slint::Timer,
    splash_start: Cell<Option<std::time::Instant>>,
    splash_uri: Cell<bool>,
    splash_tile: Cell<Option<usize>>,
    /// Tant que le guide est ouvert : le reclame régulièrement au premier plan (un jeu peut reprendre
    /// la main sur l'ordre d'affichage après l'avoir ouvert — redimensionnement, changement de mode
    /// plein écran, alt-tab — sans qu'aucun événement ne nous prévienne).
    guide_watch: slint::Timer,
    /// Démarrage : les jaquettes de l'écran d'accueil sont décodées tout de suite (bloquant) pour éviter qu'elles apparaissent une par une.
    boot: Cell<bool>,
}

fn main() -> Result<(), slint::PlatformError> {
    // Le shell n'a pas de console : les paniques vont dans un fichier.
    std::panic::set_hook(Box::new(|info| {
        let _ = std::fs::write(games_file().with_file_name("panic.log"), info.to_string());
    }));
    mark("start");
    let _profile = profiles::activate();
    // `hearth-shell --desktop` : version bureau seule (fenêtre normale). Si la console tourne déjà, on lui demande d'ouvrir la fenêtre.
    let desktop_only = std::env::args().any(|a| a == "--desktop");
    if desktop_only && desktop::console_running(MAIN_TITLE) {
        let _ = std::fs::write(desktop::request_file(), "1");
        return Ok(());
    }
    mark("profile");
    // Legendary (jeux Epic) lit sa configuration ici, y compris quand il est lancé par le gestionnaire de fenêtres.
    std::env::set_var("LEGENDARY_CONFIG_PATH", stores::epiccmd::config());
    // Console plein écran à résolution fixe, quel que soit le réglage de mise à l'échelle de Windows
    // (voir hearth.manifest : le processus doit être authentiquement adapté au DPI pour que les
    // coordonnées physiques lues partout ailleurs — wm::primary_monitor, GetWindowRect… — restent
    // fiables). L'interface est dessinée pour un canevas de 2560×1440 pixels logiques : l'échelle est
    // celle qui l'y fait tenir entièrement sur l'écran réel (0,75 en 1080p, 1 en 1440p, 1,5 en 4K,
    // ~0,65 dans une fenêtre de Bureau à distance), le sens qui a de la marge s'agrandissant (ancrages
    // sur parent.width/height). Valeur fixée nous-mêmes, jamais celle de la mise à l'échelle Windows.
    let (_, _, mw, mh) = wm::primary_monitor();
    let scale = (mw as f32 / 2560.0).min(mh as f32 / 1440.0).clamp(0.3, 4.0);
    std::env::set_var("SLINT_SCALE_FACTOR", format!("{scale:.4}"));
    // ponytail: femtovg cesse de se redessiner avec ce design (constaté sur l'Optiplex) ; rendu logiciel.
    // Glisser-déposer désactivé : winit initialise sinon OLE à la création de chaque fenêtre, ce qui échoue (RPC_E_CHANGED_MODE)
    // pour les fenêtres créées après coup (version bureau) une fois du COM multithread initialisé sur ce thread.
    let backend = std::env::var("HEARTH_BACKEND").unwrap_or_else(|_| "winit-skia".into());
    let renderer = backend.strip_prefix("winit-").unwrap_or("skia").to_string();
    // Le vrai plein écran de winit (`Fullscreen::Borderless`, déclenché par SLINT_FULLSCREEN) redemande
    // sa taille à l'OS lui-même, avec son propre risque d'incohérence DPI ; on préfère dimensionner et
    // positionner nous-mêmes une fenêtre sans bordure à la résolution physique réelle du moniteur.
    let (_, _, real_w, real_h) = wm::primary_monitor();
    let (real_w, real_h) = (real_w as u32, real_h as u32);
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name(renderer)
        .with_winit_window_attributes_hook(move |attrs| {
            use slint::winit_030::winit::dpi::{PhysicalPosition, PhysicalSize};
            use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
            let attrs = attrs.with_drag_and_drop(false);
            if desktop_only {
                attrs
            } else {
                attrs.with_decorations(false).with_inner_size(PhysicalSize::new(real_w, real_h)).with_position(PhysicalPosition::new(0, 0))
            }
        })
        .select()?;
    let ui = App::new()?;
    let guide = GuideMenu::new()?;
    let splash = Splash::new()?;
    let pad = PadOverlay::new()?;
    mark("ui_created");
    let overrides = prefs::load();
    let games = load_games(stores::installed(), &overrides);
    mark("games");
    let shell = Rc::new(Shell {
        ui,
        guide,
        splash,
        pad,
        pad_acc: Cell::new(false),
        status_n: Cell::new(0),
        last_net: Cell::new(i32::MIN),
        pad_low_warned: Cell::new(false),
        net_pending: Cell::new(false),
        desktop: RefCell::new(None),
        desk_games: Rc::new(VecModel::default()),
        desk_rows: Rc::new(VecModel::default()),
        desk_stores: Rc::new(VecModel::default()),
        desk_filter: Cell::new(0),
        desk_query: RefCell::new(String::new()),
        photo_fresh: Cell::new(false),
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
        kb_args: RefCell::new(None),
        profile_edit: RefCell::new(None),
        avatar_for: RefCell::new(None),
        scmd_stage: Cell::new(0),
        steam_wait: Cell::new(false),
        launcher_wait: Cell::new(false),
        zoom: Cell::new(0),
        zoom_timer: slint::Timer::default(),
        zoom_anim: Cell::new(None),
        zoom_anim_timer: slint::Timer::default(),
        last_activity: Cell::new(std::time::Instant::now()),
        dim: Cell::new(0.0),
        last_bump: Cell::new(None),
        vol_osd_gen: Cell::new(0),
        lib_filter: Cell::new(0),
        anim_n: Cell::new(0),
        ext_game: RefCell::new(None),
        ext_pending: Cell::new(None),
        last_tab: Cell::new(0),
        kb_last: Cell::new(None),
        bt_unpair_id: RefCell::new(None),
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
        hearth_update: RefCell::new(None),
        hearth_prompted: Cell::new(false),
        hearth_checked: Cell::new(None),
        hearth_dl: RefCell::new(None),
        hearth_dl_timer: slint::Timer::default(),
        sound_catalog: RefCell::new(vec![]),
        sound_list: RefCell::new(vec![]),
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
        offers_full: RefCell::new(vec![]),
        offers_no_art: RefCell::new(Default::default()),
        catalog_query: RefCell::new(None),
        catalog_limit: Cell::new(OFFERS_PAGE),
        offer_store_names: RefCell::new(vec![]),
        offer_filter: RefCell::new(None),
        offer_sort: Cell::new(0),
        offer_free: Cell::new(false),
        featured: RefCell::new(Default::default()),
        store_status: RefCell::new(vec![]),
        catalog_started: Cell::new(false),
        catalog_gen: Cell::new(0),
        kb_search: Cell::new(false),
        kb_lib_search: Cell::new(false),
        lib_query: RefCell::new(String::new()),
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
        toast_key: RefCell::new(String::new()),
        toast_n: Cell::new(0),
        splash_timer: slint::Timer::default(),
        splash_start: Cell::new(None),
        splash_uri: Cell::new(false),
        splash_tile: Cell::new(None),
        guide_watch: slint::Timer::default(),
        boot: Cell::new(true),
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
    shell.boot.set(false);
    shell.guide.set_notes(shell.notes.clone().into());
    *shell.store_status.borrow_mut() = stores::all().iter().map(|s| (s.name().to_string(), s.detected(), shell.games.borrow().iter().filter(|g| g.store == s.name()).count())).collect();
    mark("shell_built");
    i18n::init();
    theme::load();
    keyboard::load();
    // Manette principale (Réglages > Manettes), transmise aux jeux quand plusieurs sont branchées.
    settings::ordered_pads(system::pads::list());
    shell.apply_theme();
    i18n::init_audio_prefs();
    if !desktop_only {
        system::sfx::init();
        sounds::start();
    }
    i18n::init_volumes();
    shell.refresh_lang();
    shell.apply_font();
    shell.ui.set_rows(shell.settings.rows.clone().into());
    if let Some(c) = i18n::pref("settings_cat").and_then(|c| c.parse::<i32>().ok()).filter(|c| (0..settings::cats().len() as i32).contains(c)) {
        shell.ui.set_cat(c);
    }
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
    shell.ui.on_nav_key(move |a| {
        s.kb_last.set(Some(std::time::Instant::now()));
        s.nav_with_sound(a);
    });
    let s = shell.clone();
    shell.ui.on_add_menu(move || s.add_menu());
    let s = shell.clone();
    shell.ui.on_menu_pick(move |i| s.menu_pick(i as usize));
    let s = shell.clone();
    shell.ui.on_deal_pick(move |i| s.deal_pick(i as usize));
    let s = shell.clone();
    shell.ui.on_show_more_offers(move || s.show_more_offers());
    let s = shell.clone();
    shell.ui.on_open_offer_filters(move || s.open_offer_filters());
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
    let s = shell.clone();
    shell.ui.on_power(move |a| s.power_ask(a));
    shell.ui.on_desktop(|| {
        wm::cursor::restore();
        std::process::exit(EXIT_DESKTOP)
    });
    // Alt+F4 / fermeture de la console : bureau Windows plutôt qu'un écran noir pendant la relance.
    shell.ui.window().on_close_requested(|| {
        if wm::ctrl_down() {
            return slint::CloseRequestResponse::KeepWindowShown;
        }
        wm::cursor::restore();
        std::process::exit(EXIT_DESKTOP)
    });
    let s = shell.clone();
    shell.ui.on_search_open(move || s.search_open());
    let s = shell.clone();
    shell.ui.on_lib_search_open(move || s.lib_search_open());
    let s = shell.clone();
    shell.ui.on_lib_search_clear(move || s.lib_search_clear());
    let s = shell.clone();
    shell.ui.on_lib_filters(move || s.open_lib_filters());
    shell.ui.set_lib_filter_label(tr("Filtres").into());
    let s = shell.clone();
    shell.ui.on_store_search_clear(move || s.store_search_clear());
    let s = shell.clone();
    shell.ui.on_offer_step(move |d| s.offer_step(d));
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
    let anim = slint::Timer::default();
    anim.start(slint::TimerMode::Repeated, Duration::from_millis(100), move || timed("animate", || s.animate()));

    // Souris masquée en dehors de Hearth (jeux, autres fenêtres) ; visible dès qu'elle bouge.
    let cursor_timer = slint::Timer::default();
    if !desktop_only {
        let policy = wm::cursor::Policy::new();
        cursor_timer.start(slint::TimerMode::Repeated, Duration::from_millis(200), move || policy.update(wm::foreground_is_mine()));
    }

    // Le menu guide est créé une fois puis affiché/masqué en Win32, sans jamais prendre le focus
    // (un jeu plein écran qui perd le focus se réduit et clignote). Une fenêtre fraîchement créée par
    // winit n'est trouvable par titre (FindWindow) qu'une fois la boucle d'événements démarrée (testé :
    // avant `ui.run()`, l'attendre bloque plusieurs secondes pour rien) ; on tente quand même une fois,
    // gratuitement, au cas où, puis le minuteur ci-dessous rattrape dès que la boucle tourne.
    if !desktop_only {
    shell.guide.show()?;
    if let Some(h) = wm::find_window(GUIDE_TITLE) { wm::make_overlay(h); wm::hide(h); }
    shell.splash.show()?;
    if let Some(h) = wm::find_window(SPLASH_TITLE) { wm::make_overlay(h); wm::hide(h); }
    shell.pad.show()?;
    if let Some(h) = wm::find_window(PAD_TITLE) { wm::make_overlay(h); wm::hide(h); }
    }
    let s = shell.clone();
    shell.pad.on_press(move |x, y| s.pad_press(x, y));
    // Les fenêtres natives n'existent pas forcément dès le premier tour : on réessaie, au rythme le plus
    // serré possible (sinon ces fenêtres plein écran restent visibles/activables un instant et volent le
    // focus l'une après l'autre : l'ouverture de Hearth « saute » d'un écran à l'autre).
    let setup = Rc::new(slint::Timer::default());
    let t = setup.clone();
    if !desktop_only {
    setup.start(slint::TimerMode::Repeated, Duration::from_millis(4), move || {
        if let (Some(h), Some(sp), Some(pd)) = (wm::find_window(GUIDE_TITLE), wm::find_window(SPLASH_TITLE), wm::find_window(PAD_TITLE)) {
            wm::make_overlay(pd);
            wm::hide(pd);
            wm::make_overlay(h);
            wm::hide(h);
            wm::make_overlay(sp);
            wm::hide(sp);
            if let Some(m) = wm::find_window(MAIN_TITLE) {
                // La taille/position demandée à winit à la création (voir plus haut) n'est pas fiable
                // telle quelle : on recale nous-mêmes sur l'écran, comme pour un jeu.
                wm::fit_to_monitor(m);
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
    }

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
    if !desktop_only {
    input::spawn(|nav| {
        // Lu ici, sur le fil de la manette, au moment de l'événement (plus tard il aurait changé).
        let repeat = input::repeating();
        let _ = slint::invoke_from_event_loop(move || {
            SHELL.with(|s| {
                if let Some(s) = s.borrow().as_ref() {
                    s.ui.set_repeating(repeat);
                    s.guide.set_repeating(repeat);
                    s.on_nav(nav);
                    s.ui.set_repeating(false);
                    s.guide.set_repeating(false);
                }
            });
        });
    });
    }

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
    Shell::schedule_hearth_updates();
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
    std::thread::spawn(stores::purge_offers);
    std::thread::spawn(cleanup_obsolete);
    cleanup_obsolete_prefs();
    shell.start_art();
    shell.auto_launch_options();
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
    shell.ui.on_avatar_photo({
        let s = shell.clone();
        move || s.avatar_photo()
    });
    shell.ui.on_dl_retry({
        let s = shell.clone();
        move |i| s.dl_retry(i as usize)
    });
    shell.ui.on_dl_clear_done({
        let s = shell.clone();
        move || s.dl_clear_done()
    });
    shell.ui.on_dl_cancel({
        let s = shell.clone();
        move |i| s.dl_cancel_ask(i as usize)
    });
    shell.ui.on_oobe_key(move |a| s.oobe_key(a));
    let s = shell.clone();
    shell.ui.on_oobe_click(move |i| s.oobe_click(i));
    if !desktop_only {
        shell.oobe_start();
    }
    mark("before_run");
    if desktop_only {
        shell.desktop_open();
        return slint::run_event_loop();
    }
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
            // Un appui (ou un clic) ne doit pas lancer à lui seul un téléchargement de plusieurs Go.
            self.menu_show(
                format!("{} {} ?", tr("Installer"), g.name),
                vec![(tr("Installer"), menus::Act::Install(g.id.clone())), (tr("Annuler"), menus::Act::Cancel)],
            );
            return;
        }
        // Un jeu de boutique (pas encapsulé dans un job, suivi par sa fenêtre) est déjà en cours de
        // lancement ou de suivi pour une AUTRE tuile : refuser plutôt qu'écraser `ext_pending`, sinon
        // la fenêtre qui finit par apparaître en premier se voit attribuer le nom/l'index de l'autre
        // jeu (guide qui ferme le mauvais jeu, temps de jeu mal crédité, double lancement possible).
        if g.uri.is_some() && (self.ext_pending.get().is_some_and(|(t, _)| t != i) || self.ext_game.borrow().as_ref().is_some_and(|e| e.tile != i)) {
            self.push_notif(&g.name, &tr("Attendez la fin du lancement du jeu précédent"));
            return;
        }
        // Animation et écran de lancement tout de suite, dès l'appui : la préparation éventuelle du
        // lanceur (ci-dessous) se fait derrière, sans que rien n'en paraisse. Pas une seconde fois quand
        // `launch` est rappelé une fois le lanceur prêt.
        let resuming = self.splash_tile.get() == Some(i) && (self.splash_start.get().is_some() || self.zoom.get() == 1);
        if !resuming {
            self.begin_splash(i, g.uri.is_some());
        }
        // Jeux Steam : client lancé et connecté, superposition Steam coupée (une fois), fenêtre Steam réduite.
        let steam_uri = g.uri.is_some() && g.store == "Steam";
        if (self.is_scmd_game(&g.exe) || steam_uri) && !self.steam_ready(&g) {
            return;
        }
        // Epic, GOG Galaxy : lanceur démarré réduit d'abord s'il ne tourne pas (le jeu repart ensuite seul).
        if !self.launcher_ready(&g) {
            return;
        }
        if let Some(uri) = &g.uri {
            // Lancement par le client de la boutique : le jeu n'est pas encapsulé, on suivra sa fenêtre (tick).
            // Arguments de l'utilisateur : `steam://run/<id>//<arguments>/`, le moyen prévu par Steam.
            let uri = match (uri.strip_prefix("steam://rungameid/"), g.user_args.trim()) {
                (Some(id), a) if !a.is_empty() => format!("steam://run/{id}//{}/", a.replace(' ', "%20")),
                _ => uri.clone(),
            };
            system::open_uri(&uri);
            self.ext_pending.set(Some((i, std::time::Instant::now())));
            if steam_uri {
                self.steam_keep(1800);
            }
            return;
        }
        // Exécutable remplacé (option de lancement Steam sans launcher), sinon celui du jeu.
        let exe_path = g.exe_override.clone().filter(|p| Path::new(p).exists()).unwrap_or_else(|| g.exe.clone());
        let exe = Path::new(&exe_path);
        let compat = wm::compat::lookup(exe);
        compat.apply_ini();
        // Arguments : ceux du jeu (boutique, games.json), de la base de compatibilité, puis ceux de l'utilisateur.
        let mut args: Vec<String> = g.args.iter().chain(&compat.args).cloned().collect();
        args.extend(split_args(&g.user_args));
        // Script (.bat/.cmd lancés tels quels par Windows via cmd ; .ps1 par PowerShell), dossier du script
        // comme dossier du jeu.
        let script_dir = exe.parent().map(Path::to_path_buf);
        let ext = exe.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let (exe, args): (PathBuf, Vec<String>) = if ext == "ps1" {
            let mut a: Vec<String> = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"].iter().map(|s| s.to_string()).collect();
            a.push(exe_path.clone());
            a.extend(args);
            (PathBuf::from("powershell.exe"), a)
        } else {
            (exe.to_path_buf(), args)
        };
        let exe = exe.as_path();
        // Pare-feu : règle d'autorisation posée une fois par jeu (assistant administrateur), pour qu'aucune fenêtre Windows n'apparaisse en jeu.
        {
            let path = g.exe.clone();
            // Jeux déjà autorisés retenus dans `firewall.txt` : plus de tâche administrateur à chaque lancement.
            std::thread::spawn(move || {
                let list = stores::base_dir().join("firewall.txt");
                let known = std::fs::read_to_string(&list).unwrap_or_default();
                if known.lines().any(|l| l.eq_ignore_ascii_case(&path)) || !system::admin::enabled() {
                    return;
                }
                if system::admin::request(&format!("firewall {path}")).is_ok() {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&list) {
                        let _ = writeln!(f, "{path}");
                    }
                }
            });
        }
        match wm::Session::launch(exe, &args, script_dir.as_deref()) {
            Ok(session) => {
                let hands_off = g.anticheat || compat.anticheat || !compat.borderless;
                self.running.borrow_mut().push(Running { tile: i, session, hands_off, closing: false, started: std::time::Instant::now() });
                if i18n::sfx() {
                    system::sfx::play(system::sfx::Sfx::Launch);
                }
                self.rebuild();
            }
            Err(e) => {
                self.hide_splash();
                if self.zoom.get() != 0 {
                    self.zoom_out();
                }
                system::sfx::play(system::sfx::Sfx::Error);
                // Dire pourquoi (exécutable déplacé ou supprimé, droits…) plutôt qu'un simple son.
                let why = if Path::new(&exe_path).exists() { e.message() } else { tr("fichier introuvable") };
                self.push_notif(&g.name, &format!("{} ({why})", tr("Impossible de lancer le jeu")));
            }
        }
    }

    /// Veille / redémarrer / éteindre (panneau Alimentation) : confirmation seulement si un
    /// téléchargement ou un jeu est en cours (il serait interrompu).
    fn power_ask(&self, a: i32) {
        let dl = self.dl.borrow().iter().any(|d| matches!(d.state, scmd::DlState::Active | scmd::DlState::Queued)) || self.hearth_dl.borrow().is_some();
        let game = !self.running.borrow().is_empty() || self.ext_game.borrow().is_some();
        if !dl && !game {
            power_do(a);
            return;
        }
        let what = tr(if dl { "Un téléchargement est en cours" } else { "Un jeu est en cours" });
        let label = tr(match a {
            0 => "Mettre en veille quand même",
            1 => "Redémarrer quand même",
            3 => "Redémarrer Hearth quand même",
            _ => "Éteindre quand même",
        });
        self.menu_show(format!("{what}."), vec![(label, menus::Act::Power(a)), (tr("Annuler"), menus::Act::Cancel)]);
    }

    /// Lanceur ou Steam prêt (préparation en arrière-plan) : relance le jeu, retrouvé par son identifiant
    /// (la liste a pu être relue entre-temps), sauf si le lancement a été annulé ou abandonné depuis.
    pub(crate) fn resume_launch(&self, id: &str) {
        let Some(i) = self.games.borrow().iter().position(|g| g.id == id) else { return };
        if self.splash_tile.get() == Some(i) && (self.splash_start.get().is_some() || self.zoom.get() == 1) {
            self.launch(i);
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
        if on && !input::sony_connected() {
            return;
        }
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
        let l = keyboard::layout();
        let rows: [&str; 4] = if acc { l.alt } else { l.main };
        self.pad.set_kb_alt(l.alt_label.into());
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
        // Jaquette en train de s'agrandir (avant l'écran de lancement) : Retour annule déjà le lancement.
        if self.zoom.get() == 1 && self.splash_tile.get().is_some() {
            if nav == Nav::Back {
                self.cancel_launch();
            }
            return;
        }
        if wm::find_window(SPLASH_TITLE).is_some_and(wm::is_visible) {
            if nav == Nav::Back {
                self.cancel_launch();
            }
            return;
        }
        if wm::find_window(GUIDE_TITLE).is_some_and(wm::is_visible) {
            if nav == Nav::Guide {
                self.guide_action(-1, 0);
            } else if matches!(nav, Nav::VolDown | Nav::VolUp) {
                // L2/R2 dans le guide : la jauge de volume, comme sur l'accueil.
                self.guide_action(3, if nav == Nav::VolUp { 1 } else { -1 });
            } else {
                self.guide.invoke_nav(nav as i32);
            }
            return;
        }
        if nav != Nav::Guide {
            // La manette est lue même en arrière-plan : n'agir que si une fenêtre à nous est au premier
            // plan, sinon on navigue à l'aveugle (jusqu'à « Veille »…). Vérifier que c'est *exactement*
            // la fenêtre principale (par titre) a déjà fait rater le retour de focus manette après un
            // jeu : juste après `home()`, une autre fenêtre à nous peut brièvement être rapportée comme
            // premier plan par Windows alors que le clavier, lui, répond déjà (focus clavier et fenêtre
            // au premier plan ne sont pas rigoureusement la même chose).
            if wm::foreground_is_mine() {
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
        // Dernière catégorie ouverte retrouvée au prochain démarrage.
        if !self.oobe_active() {
            i18n::set_pref("settings_cat", &cat.to_string());
        }
        let hint = self.settings.open(cat);
        self.ui.set_shint(hint.into());
        if cat == 6 {
            self.settings.rows.push(self.hearth_update_row());
        }
        if settings::needs_fetch(cat) {
            self.spawn_fetch(cat);
        }
    }

    /// Ligne « Hearth » ajoutée à côté de Windows Update (cat 6) : les deux mises à jour au même endroit.
    fn hearth_update_row(&self) -> SettingRow {
        let value = match self.hearth_update.borrow().as_ref() {
            Some((ver, _)) => format!("{} {ver}", tr("Mise à jour disponible :")),
            None => match self.hearth_checked.get() {
                Some(true) => format!("{} · {}", env!("HEARTH_VERSION"), tr("à jour")),
                Some(false) => format!("{} · {}", env!("HEARTH_VERSION"), tr("vérification impossible")),
                None => format!("{} · {}", env!("HEARTH_VERSION"), tr("non vérifié")),
            },
        };
        SettingRow { label: tr("Hearth").into(), value: value.into(), adjustable: false }
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
        // Ligne « Hearth » ajoutée après coup (hearth_update_row) : pas connue de Settings::act.
        if cat == 6 && row == 1 && delta == 0 {
            self.check_hearth_update(true);
            self.open_cat_keep(cat);
            return;
        }
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
                // Réseau déjà enregistré dans Windows : connexion directe, le mot de passe n'est demandé
                // qu'en cas d'échec.
                self.settings.set_message(format!("{} {ssid}…", tr("Connexion à")));
                std::thread::spawn(move || {
                    let ok = system::wifi::has_profile(&ssid) && system::wifi::connect(&ssid, None);
                    let _ = slint::invoke_from_event_loop(move || {
                        with_shell(|s| {
                            if ok {
                                s.settings.set_message(format!("{} {ssid}", tr("Connecté à")));
                                s.settings.start_loading();
                                s.spawn_fetch(2);
                            } else {
                                s.settings.set_message(String::new());
                                s.wifi_password(&ssid);
                            }
                            s.open_cat_keep(2);
                        })
                    });
                });
            }
            settings::Outcome::Connect(ssid) => self.connect(ssid, None),
            settings::Outcome::LangChanged => {
                self.refresh_lang();
                self.rebuild();
            }
            settings::Outcome::Oobe => self.oobe_restart(),
            settings::Outcome::Desktop => self.desktop_open(),
            settings::Outcome::OpenBluetooth => {
                self.ui.set_cat(3);
                self.ui.set_srow(0);
                self.open_cat(3);
            }
            settings::Outcome::Import => self.import_games(),
            settings::Outcome::SoundPicker(music) => self.sound_picker(music),
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
                    // Chaque appareil est ajouté à la liste dès sa découverte (pas à la fin des 15 s).
                    system::bluetooth::scan(15, |d| {
                        let _ = slint::invoke_from_event_loop(move || {
                            with_shell(|s| {
                                s.settings.bt_found_push(d);
                                s.open_cat_keep(3);
                            })
                        });
                    });
                    let _ = slint::invoke_from_event_loop(move || {
                        with_shell(|s| {
                            s.settings.bt_scan_done();
                            s.settings.set_message(String::new());
                            s.open_cat_keep(3);
                        })
                    });
                });
            }
            settings::Outcome::BtPair(id, name) => {
                self.push_notif("Bluetooth", &format!("{} {name}…", tr("Appairage de")));
                system::bluetooth::scan_stop();
                std::thread::spawn(move || {
                    // Laisse la recherche se terminer (pas de 100 ms) avant d'appairer.
                    std::thread::sleep(Duration::from_millis(250));
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
                *self.bt_unpair_id.borrow_mut() = Some((id, name.clone()));
                self.menu_show(format!("{} {name} ?", tr("Oublier")), vec![(tr("Confirmer"), menus::Act::ConfirmBtUnpair), (tr("Annuler"), menus::Act::Cancel)]);
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
        // Mots de passe masqués : SteamCMD et Wi-Fi.
        let secret = self.scmd_stage.get() == 2 || !self.settings.kb_ssid().is_empty();
        let shown = if secret { "•".repeat(text.chars().count()) } else { text };
        self.ui.set_kb_text(shown.into());
        self.ui.set_kb_alt(keyboard::layout().alt_label.into());
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
                let back = if self.kb_profile.replace(false) || self.profile_edit.borrow_mut().take().is_some() {
                    11
                } else if self.kb_search.replace(false) | self.kb_lib_search.replace(false) {
                    0
                } else if self.kb_identify.replace(false) {
                    6
                } else if self.kb_login.replace(false) {
                    5
                } else if self.kb_sgdb.replace(false) {
                    self.home_zone()
                } else if self.kb_oobe_name.replace(false) {
                    13
                } else if self.rename_id.borrow_mut().take().is_some() || self.kb_args.borrow_mut().take().is_some() {
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
                let profile_edit = self.profile_edit.borrow_mut().take();
                if let Some(id) = profile_edit {
                    self.profile_renamed(&id, &text);
                } else if self.kb_profile.replace(false) {
                    self.profile_named(text);
                } else if self.scmd_stage.get() != 0 {
                    self.scmd_text(text);
                } else if let Some(id) = renaming {
                    self.ui.set_zone(0);
                    self.rename_game(&id, text.trim());
                } else if let Some(id) = self.kb_args.borrow_mut().take() {
                    self.ui.set_zone(0);
                    self.set_launch(&id, text, None);
                    self.push_notif(&tr("Arguments de lancement"), &tr("Enregistrés"));
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
                } else if self.kb_lib_search.replace(false) {
                    self.ui.set_zone(0);
                    *self.lib_query.borrow_mut() = text.trim().to_string();
                    self.ui.set_lib_query(text.into());
                    self.rebuild();
                    // Recherche validée : le focus passe directement sur le premier résultat.
                    if self.library.row_count() > 0 {
                        self.ui.set_lsel(0);
                        self.ui.set_lbrow(1);
                    }
                } else if !ssid.is_empty() && text.chars().count() < 8 {
                    // Clé WPA trop courte : refusée par le réseau après 10 s d'attente ; on le dit tout de
                    // suite et le clavier reste ouvert pour corriger.
                    self.push_notif("Wi-Fi", &tr("Mot de passe trop court (8 caractères minimum)"));
                } else {
                    self.ui.set_zone(self.home_zone());
                    self.connect(ssid, Some(text));
                }
            }
        }
    }

    /// Saisie du mot de passe d'un réseau Wi-Fi (masqué, voir `kb_refresh`).
    fn wifi_password(&self, ssid: &str) {
        self.settings.kb_open(ssid);
        self.kb_refresh();
        self.ui.set_kbx(0);
        self.ui.set_kby(0);
        self.ui.set_kb_title(format!("{} {ssid}", tr("Mot de passe :")).into());
        self.ui.set_zone(4);
    }

    /// Clavier virtuel quitté sans passer par Annuler (bouton guide / Origine) : la saisie en cours est
    /// annulée proprement, sinon son drapeau resterait levé et la saisie suivante partirait au mauvais endroit.
    fn kb_abort(&self) {
        if self.ui.get_zone() == 4 {
            self.kb_apply(settings::Kb::Cancel);
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
        if cat == 6 {
            self.settings.rows.push(self.hearth_update_row());
        }
        self.desktop_sync_rows_with(&hint);
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
            let link = system::net::current();
            let net = link.as_ref().map_or(-1, |l| if l.wifi { 1 } else { 0 });
            // Signal Wi-Fi : Windows donne 0 à 5 barres, l'icône en a 4.
            let bars = link.as_ref().and_then(|l| l.bars).map_or(4, |b| ((b as i32 * 4 + 4) / 5).clamp(1, 4));
            // Batterie du PC (portable) : -1 sans batterie.
            let (pc_bat, pc_charging) = {
                let mut st = windows::Win32::System::Power::SYSTEM_POWER_STATUS::default();
                let ok = unsafe { windows::Win32::System::Power::GetSystemPowerStatus(&mut st) }.is_ok();
                if ok && st.BatteryFlag & 128 == 0 && st.BatteryLifePercent <= 100 { (st.BatteryLifePercent as i32, st.ACLineStatus == 1) } else { (-1, false) }
            };
            let vol = system::audio::volume().map_or(-1, |v| if system::audio::muted() { 0 } else { v as i32 });
            // Manette « principale » (Réglages > Manettes) en premier, pour la pastille de la barre d'état.
            let first = settings::ordered_pads(system::pads::list()).into_iter().next();
            let pad = first.as_ref().map_or(-1, |p| p.battery.map_or(101, |b| b as i32));
            let charging = first.is_some_and(|p| p.charging);
            let _ = w.upgrade_in_event_loop(move |ui| {
                ui.set_st_net(net);
                ui.set_st_bars(bars);
                ui.set_st_pc(pc_bat);
                ui.set_st_pc_charging(pc_charging);
                ui.set_st_vol(vol);
                ui.set_st_pad(pad);
                ui.set_st_pad_charging(charging);
                with_shell(|s| s.status_alerts(net, pad, charging));
            });
        });
    }

    /// Notifications d'état : connexion perdue / rétablie, batterie de la manette faible (une fois par
    /// décharge, réarmée en charge ou au-dessus de 25 %). Rien au premier relevé (état de départ).
    fn status_alerts(&self, net: i32, pad: i32, charging: bool) {
        // Réseau : le changement doit tenir sur deux relevés de suite (10 s) avant d'être signalé — une
        // lecture ratée isolée de l'état de connexion ne doit pas produire « perdue » puis « rétablie ».
        let off = (net < 0) as i32;
        let confirmed = self.last_net.get();
        if confirmed == i32::MIN {
            self.last_net.set(off);
        } else if off == confirmed {
            self.net_pending.set(false);
        } else if !self.net_pending.replace(true) {
            // Premier relevé différent : on attend le suivant.
        } else {
            self.net_pending.set(false);
            self.last_net.set(off);
            if net < 0 {
                self.push_notif(&tr("Réseau"), &tr("Connexion perdue"));
            } else {
                self.push_notif(&tr("Réseau"), &tr("Connexion rétablie"));
            }
        }
        let low = (0..=15).contains(&pad) && !charging;
        if low && !self.pad_low_warned.get() {
            self.pad_low_warned.set(true);
            self.push_notif(&tr("Manette"), &format!("{} ({pad} %)", tr("Batterie faible")));
        } else if charging || pad > 25 {
            self.pad_low_warned.set(false);
        }
    }

    fn set_clock(&self) {
        // Format de l'heure de Windows (24 h, ou 12 h avec AM/PM selon la région), sans les secondes.
        use windows::Win32::Globalization::{GetTimeFormatEx, TIME_NOSECONDS};
        let mut buf = [0u16; 32];
        let n = unsafe { GetTimeFormatEx(windows::core::PCWSTR::null(), TIME_NOSECONDS, None, windows::core::PCWSTR::null(), Some(&mut buf)) } as usize;
        let c: slint::SharedString = if n > 1 {
            String::from_utf16_lossy(&buf[..n - 1]).into()
        } else {
            let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
            format!("{:02}:{:02}", t.wHour, t.wMinute).into()
        };
        self.ui.set_clock(c.clone());
        self.guide.set_clock(c);
    }

    /// Oublie un appareil Bluetooth (après confirmation, voir `Act::ConfirmBtUnpair`).
    fn bt_unpair_do(&self, id: String, name: String) {
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

    fn home(&self) {
        // Retour au menu : la manette doit naviguer (un mode souris resté actif la rendait muette,
        // ou L3+R3 encore réservé au jeu qu'on vient de quitter empêchait de rebasculer). Une manette Sony
        // relue de force, au cas où le jeu aurait changé son mode de rapport HID (Steam Input…) sans que
        // Windows.Gaming.Input ne s'en rende compte tout seul.
        self.kb_abort();
        input::set_chord(true);
        input::resync_pads();
        if input::mouse_active() {
            input::set_mouse_active(false);
            self.pad_sync();
        }
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
        self.desktop_theme_changed();
        // Bandeau lumineux de la DualSense assorti à l'accent du thème (HID brut, au mieux : lecture
        // bloquante de courte durée, donc hors du thread UI).
        let [r, g, b] = p.accent;
        std::thread::spawn(move || system::dualsense::set_lightbar(r, g, b));
    }

    fn open_guide(&self, tile: usize) {
        // `tile` peut avoir été invalidé par un rechargement de la liste pendant la partie (voir
        // `reload_games`) : on renonce plutôt que de paniquer ou d'afficher un autre jeu.
        let Some(name) = self.games.borrow().get(tile).map(|g| g.name.clone()) else { return };
        self.guide_tile.set(tile);
        let closing = self.running.borrow().iter().any(|r| r.tile == tile && r.closing) || self.ext_game.borrow().as_ref().is_some_and(|e| e.tile == tile && e.closing);
        self.guide.set_game(name.as_str().into());
        // Boutique et temps de jeu total (hors partie en cours, comptée à la fin).
        let info = self.games.borrow().get(tile).map(|g| {
            let pt = playtime::load();
            let store = if g.store.is_empty() { tr("Local") } else { g.store.clone() };
            format!("{store}  ·  {}", playtime::played_in(&pt, &playtime_key(g)))
        });
        self.guide.set_info(info.unwrap_or_default().into());
        // Notifications du guide avec leur heure relative (« il y a 5 min »), recalculée à l'ouverture.
        let now = unix_now();
        let notes: Vec<SharedString> = self.notifs.iter().take(MAX_NOTES).map(|n| format!("{}  ·  {}", n.text, notif_age(now.saturating_sub(n.stamp as u64))).into()).collect();
        self.notes.set_vec(notes);
        self.guide_session(tile);
        self.guide.set_art(self.tile_for(tile).map(|t| t.cover).unwrap_or_default());
        self.guide.set_closing(closing);
        self.guide.set_pad(self.ui.get_st_pad());
        self.guide.set_mouse_ok(input::sony_connected());
        self.guide.set_sel(0);
        self.guide.set_volume(system::audio::volume().map_or(-1, |v| if system::audio::muted() { 0 } else { v as i32 }));
        if let Some(h) = wm::find_window(GUIDE_TITLE) {
            // La capture n'est posée qu'une fois l'overlay effectivement retrouvé : sinon elle resterait
            // bloquée à `true` sans aucun guide visible pour la relâcher (Retour), la manette virtuelle
            // (ViGEm) arrêtant de transmettre au jeu pour de bon.
            input::set_capture(true);
            self.reassert_guide_overlay(h);
            self.guide.window().request_redraw();
            // Reclame le premier plan tant que le guide reste ouvert : un jeu peut reprendre l'ordre
            // d'affichage après coup (redimensionnement, bascule plein écran, alt-tab) sans nous
            // prévenir ; rien ne remettait alors l'overlay devant avant la prochaine ouverture.
            self.guide_watch.start(slint::TimerMode::Repeated, Duration::from_millis(200), || with_shell(|s| s.guide_watch_tick()));
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

    /// Durée de la partie affichée dans le guide (recalculée chaque seconde tant qu'il est ouvert).
    fn guide_session(&self, tile: usize) {
        let started = self.running.borrow().iter().find(|r| r.tile == tile).map(|r| r.started).or_else(|| self.ext_game.borrow().as_ref().filter(|e| e.tile == tile).map(|e| e.started));
        self.guide.set_session(started.map(|t| session_label(t.elapsed().as_secs())).unwrap_or_default().into());
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
                            e.tree.kill();
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
            (3, d) if d != 0 && self.guide.get_volume() >= 0 => {
                let v = (self.guide.get_volume() + d * 5).clamp(0, 100);
                system::audio::set_volume(v as u32);
                if d > 0 && system::audio::muted() {
                    system::audio::set_mute(false);
                }
                self.guide.set_volume(system::audio::volume().map_or(-1, |v| if system::audio::muted() { 0 } else { v as i32 }));
            }
            (4, d) if d != 0 && self.guide.get_brightness() >= 0 => {
                let b = (self.guide.get_brightness() + d * 10).clamp(0, 100);
                self.guide.set_brightness(b);
                // Un seul envoi DDC/CI à la fois, avec la dernière valeur demandée : des appuis rapides
                // lançaient des commandes concurrentes à l'écran, et la valeur finale était aléatoire.
                use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
                static WANT: AtomicI32 = AtomicI32::new(-1);
                static BUSY: AtomicBool = AtomicBool::new(false);
                WANT.store(b, Ordering::SeqCst);
                if !BUSY.swap(true, Ordering::SeqCst) {
                    std::thread::spawn(|| {
                        loop {
                            let v = WANT.swap(-1, Ordering::SeqCst);
                            if v < 0 {
                                BUSY.store(false, Ordering::SeqCst);
                                // Valeur arrivée entre-temps : on reprend la main si personne d'autre ne l'a.
                                if WANT.load(Ordering::SeqCst) >= 0 && !BUSY.swap(true, Ordering::SeqCst) {
                                    continue;
                                }
                                break;
                            }
                            system::display::set_brightness(v as u32);
                        }
                    });
                }
            }
            (6, 0) => {
                // Le guide est masqué avant la capture (le temps que l'écran se redessine), puis on revient au jeu.
                self.hide_guide();
                let name = self.games.borrow().get(tile).map(|g| g.name.clone()).unwrap_or_else(|| "Hearth".into());
                if let Some(h) = game() {
                    wm::focus(h);
                }
                slint::Timer::single_shot(Duration::from_millis(400), move || {
                    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
                    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' { c } else { '_' }).collect();
                    let file = std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default())
                        .join("Pictures")
                        .join("Hearth")
                        .join(format!("{} {:04}-{:02}-{:02} {:02}.{:02}.{:02}.png", safe.trim(), t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond));
                    std::thread::spawn(move || {
                        let ok = native::screenshot(&file);
                        let _ = slint::invoke_from_event_loop(move || {
                            with_shell(|s| s.push_notif(&tr("Capture d'écran"), &if ok { file.display().to_string() } else { tr("Échec de la capture") }))
                        });
                    });
                });
            }
            (7, 0) => {
                self.hide_guide();
                self.pad_toggle();
            }
            // A seulement : gauche/droite en passant sur la ligne coupait le Wi-Fi par mégarde.
            (5, 0) if self.guide.get_wifi() >= 0 => {
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
        self.reclaim_focus();
        if desktop::request_file().exists() {
            let _ = std::fs::remove_file(desktop::request_file());
            self.desktop_open();
        }
        let n = self.status_n.get().wrapping_add(1);
        self.status_n.set(n);
        if n % 5 == 1 {
            self.refresh_status();
        }
        self.set_clock();
        if wm::find_window(GUIDE_TITLE).is_some_and(wm::is_visible) {
            self.guide_session(self.guide_tile.get());
        }
        if self.ui.get_zone() == 8 {
            self.notif_times();
        }
        self.maybe_prompt_update();
        self.maybe_prompt_hearth_update();
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
        // En jeu, L3 + R3 appartient au jeu : pas de mode souris (sinon la manette ne navigue plus au retour au menu).
        let in_game = !self.running.borrow().is_empty() || self.ext_game.borrow().is_some() || self.ext_pending.get().is_some();
        input::set_chord(!in_game);
        if in_game && input::mouse_active() && wm::find_window(MAIN_TITLE).is_none_or(|h| h != wm::foreground()) {
            input::set_mouse_active(false);
            self.pad_sync();
        }
        let ended: Vec<(usize, u64)> = {
            let mut running = self.running.borrow_mut();
            let ended: Vec<(usize, u64)> = running.iter().filter(|r| !r.session.alive()).map(|r| (r.tile, r.started.elapsed().as_secs())).collect();
            running.retain(|r| r.session.alive());
            ended
        };
        for &(tile, secs) in &ended {
            if let Some(g) = self.games.borrow().get(tile) {
                playtime::record(&playtime_key(g), secs);
            }
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

    /// Filet de sécurité : une fenêtre qui ne nous appartient pas passe au premier plan puis disparaît
    /// (notification Windows, boîte de dialogue…) sans que Hearth ne redevienne jamais le premier plan
    /// tout seul — la manette, lue même en arrière-plan, n'agit alors plus (voir `foreground_is_mine`
    /// dans `nav`). Ne reprend la main que si **rien de réel** (fenêtre visible, titrée, assez grande —
    /// `foreground_game`, même logique que pour détecter un jeu) n'est au premier plan : ne concurrence
    /// donc jamais une vraie application que l'utilisateur garde volontairement à côté de Hearth (par ex.
    /// pour lui écrire), contrairement à une temporisation aveugle. Un jeu suivi (job ou lancé par une
    /// boutique) ou un lancement en cours (`ext_pending`) ne sont jamais concernés.
    fn reclaim_focus(&self) {
        if wm::foreground_is_mine() {
            return;
        }
        let fg = wm::foreground();
        let tracked = self.running.borrow().iter().any(|r| r.session.owns(fg))
            || self.ext_game.borrow().as_ref().is_some_and(|e| e.hwnd == fg || e.tree.contains(wm::window_pid(fg), &wm::proctree::snapshot()))
            || self.ext_pending.get().is_some();
        // explorer.exe (bureau) : ce que Windows remet au premier plan par défaut quand un jeu plein
        // écran se ferme, pas une vraie appli ouverte par l'utilisateur — sinon la manette (et le
        // clavier) resterait muette indéfiniment après la fermeture d'un jeu.
        if tracked || wm::foreground_game(&["explorer.exe"]).is_some() {
            return;
        }
        if let Some(m) = wm::find_window(MAIN_TITLE) {
            wm::focus(m);
        }
    }

    fn hide_guide(&self) {
        input::set_capture(false);
        self.guide_watch.stop();
        if let Some(h) = wm::find_window(GUIDE_TITLE) {
            wm::hide(h);
        }
    }

    fn reassert_guide_overlay(&self, h: wm::HWND) {
        let (x, y, _, height) = wm::primary_monitor();
        let width = (GUIDE_WIDTH * self.ui.window().scale_factor()) as i32;
        wm::show_overlay(h, x, y, width, height);
    }

    /// Appelé toutes les 200 ms tant que le guide est ouvert : le remet au premier plan si un jeu a
    /// repris la main sur l'ordre d'affichage entre-temps. S'arrête de lui-même si la fenêtre a
    /// disparu ou si le guide n'est plus censé être visible (fermé par un autre chemin que `hide_guide`).
    fn guide_watch_tick(&self) {
        let Some(h) = wm::find_window(GUIDE_TITLE) else {
            self.guide_watch.stop();
            return;
        };
        if !wm::is_visible(h) {
            self.guide_watch.stop();
            return;
        }
        self.reassert_guide_overlay(h);
    }


    /// Ajoute une notification au centre, au menu en jeu, et l'affiche en toast quelques secondes.
    fn push_notif(&self, title: &str, text: &str) {
        self.push_notif_ex(title, text, false);
    }

    /// `pinned` : ne se supprime pas (seule ou via « tout effacer ») — voir `notif_action`.
    pub(crate) fn push_notif_ex(&self, title: &str, text: &str, pinned: bool) {
        let stamp = unix_now() as i32;
        self.notifs.insert(0, Notif { title: title.into(), text: text.into(), time: notif_age(0).into(), unread: true, pinned, stamp });
        // Centre ouvert : la sélection suit la notification qu'elle désignait (la nouvelle arrive au-dessus).
        if self.ui.get_zone() == 8 && self.notifs.row_count() > 1 {
            self.ui.set_nsel((self.ui.get_nsel() + 1).min(29));
        }
        // Au-delà de 30 : la plus ancienne non épinglée part (la mise à jour de Hearth en attente reste).
        if self.notifs.row_count() > 30 {
            if let Some(i) = (0..self.notifs.row_count()).rev().find(|&i| self.notifs.row_data(i).is_some_and(|n| !n.pinned)) {
                self.notifs.remove(i);
            }
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
        // Bannières de même titre à la suite (ex. plusieurs téléchargements) : une seule, avec un compteur.
        let n = if self.ui.get_toast_on() && *self.toast_key.borrow() == title { self.toast_n.get() + 1 } else { 1 };
        *self.toast_key.borrow_mut() = title.to_string();
        self.toast_n.set(n);
        let title = if n > 1 { format!("{title} ({n})") } else { title.to_string() };
        let title = title.as_str();
        self.ui.set_toast_title(title.into());
        self.ui.set_toast_text(text.into());
        self.ui.set_toast_on(true);
        let gen = self.toast_gen.get() + 1;
        self.toast_gen.set(gen);
        // Bannière courte pour les notifications ordinaires (elles restent dans le centre de
        // notifications) ; plus longue pour celles qui attendent une action (mise à jour de Hearth).
        let secs = if pinned { 6 } else { 3 };
        slint::Timer::single_shot(Duration::from_secs(secs), move || {
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

    /// Heures relatives des notifications (« il y a 5 min »), recalculées à l'ouverture du centre et
    /// chaque seconde tant qu'il reste ouvert.
    fn notif_times(&self) {
        let now = unix_now();
        for i in 0..self.notifs.row_count() {
            if let Some(mut n) = self.notifs.row_data(i) {
                let age = notif_age(now.saturating_sub(n.stamp as u64));
                if n.time.as_str() != age {
                    n.time = age.into();
                    self.notifs.set_row_data(i, n);
                }
            }
        }
    }

    fn notifs_opened(&self) {
        self.notif_times();
        for i in 0..self.notifs.row_count() {
            if let Some(mut n) = self.notifs.row_data(i) {
                n.unread = false;
                self.notifs.set_row_data(i, n);
            }
        }
        self.update_unread();
        self.ui.set_toast_on(false);
    }

    /// Geste « effacer » sur une notification. Pour celle de mise à jour Hearth (épinglée, tant que la
    /// mise à jour n'est pas installée) : installe au lieu de supprimer — « tout effacer » l'épargne.
    fn notif_action(&self, i: i32) {
        if i < 0 {
            let keep: Vec<Notif> = self.notifs.iter().filter(|n| n.pinned).collect();
            self.notifs.set_vec(keep);
            // La liste du guide en jeu aussi.
            self.notes.set_vec(Vec::<SharedString>::new());
        } else if (i as usize) < self.notifs.row_count() {
            if self.notifs.row_data(i as usize).is_some_and(|n| n.pinned) {
                self.hearth_update_install();
                self.update_unread();
                return;
            }
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
            g.user_args = o.args.clone().unwrap_or_default();
            g.exe_override = o.exe.clone();
        }
    }
    games
}

/// Prix d'une boutique (« 17,99 € », « Gratuit ») dans la langue de l'interface : mot traduit,
/// séparateur décimal de la langue (le montant et la devise ne changent pas).
fn price_text(p: &str) -> String {
    if p == "Gratuit" {
        tr("Gratuit")
    } else {
        p.replace(',', i18n::decimal_sep())
    }
}

/// Arguments saisis : séparés aux espaces, sauf entre guillemets (`-path "C:\Mes jeux"`).
fn split_args(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (vec![], String::new(), false);
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Fichiers qu'on peut ajouter comme jeu : exécutables et scripts (.bat, .cmd, .ps1).
pub(crate) fn launchable(p: &Path) -> bool {
    p.extension().is_some_and(|x| ["exe", "bat", "cmd", "ps1"].iter().any(|e| x.eq_ignore_ascii_case(e)))
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
    /// Police de l'interface selon la langue (le chinois, l'hindi et le bengali exigent une police qui contient leurs caractères).
    pub fn apply_font(&self) {
        let f = match i18n::code() {
            "zh" => "Microsoft YaHei UI",
            "hi" | "bn" => "Nirmala UI",
            "ar" | "ur" => "Segoe UI",
            _ => "",
        };
        self.ui.set_ui_font(f.into());
        self.guide.set_ui_font(f.into());
        self.splash.set_ui_font(f.into());
        self.pad.set_ui_font(f.into());
    }

    fn refresh_lang(&self) {
        self.apply_font();
        let cats: Vec<SharedString> = settings::cats().into_iter().map(Into::into).collect();
        self.ui.set_cats(Rc::new(VecModel::from(cats.clone())).into());
        // Version bureau : mêmes catégories, dans la nouvelle langue.
        if let Some(w) = self.desktop.borrow().as_ref() {
            w.set_cats(Rc::new(VecModel::from(cats)).into());
        }
        // Pas avant la création de la fenêtre : COM multithread sur ce thread ferait échouer winit.
        if self.ui.get_zone() == 3 {
            self.open_cat_keep(self.ui.get_cat());
        }
        // Textes posés depuis Rust (les @tr de Slint suivent seuls) : libellé du filtre de la
        // Bibliothèque, titre de la Boutique, mention « Local » des tuiles, étapes de l'assistant.
        let f = self.lib_filter.get();
        self.ui.set_lib_filter_label(if f == 0 { tr("Filtres") } else { tr(LIB_FILTERS[f as usize]) }.into());
        let title = match self.catalog_query.borrow().as_ref() {
            Some(q) => tr("Résultats pour « {q} »").replace("{q}", q),
            None => tr("À la une"),
        };
        self.ui.set_offers_title(title.into());
        if !self.boot.get() {
            self.refresh_onboard();
            // Textes des boutiques dans la nouvelle langue (descriptions, « Gratuit ») : « À la une »
            // est relu ; une recherche en cours garde ses cartes jusqu'à la prochaine.
            *self.featured.borrow_mut() = None;
            if self.catalog_started.get() && self.catalog_query.borrow().is_none() {
                self.load_catalog(None);
            }
        }
        self.oobe_relang();
    }

    /// Tâches périodiques de l'interface (pictogrammes manette/clavier, jaquettes, onglets, mise en veille).
    fn animate(&self) {
        static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
        if FIRST.swap(false, std::sync::atomic::Ordering::Relaxed) {
            mark("first_animate");
        }
        // Tick à 10 Hz ; les vérifications coûteuses (fenêtres, manette) une fois sur deux.
        let n = self.anim_n.get().wrapping_add(1);
        self.anim_n.set(n);
        if n.is_multiple_of(2) {
            if self.ui.get_tab() == 2 && !self.catalog_started.get() {
                self.load_catalog(None);
            }
            let pad = match std::env::var("HEARTH_PAD").as_deref() {
                Ok("ps") => 1,
                Ok("xbox") => 0,
                // Clavier (NavKeys) plus récent que le dernier ordre manette : les indications à l'écran
                // affichent les touches plutôt qu'un bouton de manette (kind 2, voir `PadGlyph`).
                _ => match (self.kb_last.get(), input::last_input()) {
                    (Some(k), pad_t) if pad_t.is_none_or(|p| k > p) => 2,
                    _ => input::last_pad() as i32,
                },
            };
            if self.ui.global::<Pad>().get_kind() != pad {
                self.ui.global::<Pad>().set_kind(pad);
                self.guide.global::<Pad>().set_kind(pad);
                self.pad.global::<Pad>().set_kind(pad);
            }
            self.fg_main.set(wm::find_window(MAIN_TITLE).is_some_and(|h| h == wm::foreground()));
            // Jaquette restée en plein écran derrière le jeu : dès que Hearth repasse au premier plan sans
            // écran de lancement (jeu fermé, lancement annulé ou échoué, retour à l'accueil), elle se rétracte.
            if self.zoom.get() == 2 && self.fg_main.get() && self.splash_start.get().is_none() {
                self.zoom_out();
            }
            let tab = self.ui.get_tab();
            if tab != self.last_tab.replace(tab) && tab == 1 {
                self.ui.set_lsel(0);
                self.rebuild();
            }
            // Dock de l'accueil : pas d'accélération de la répétition (chaque pas y est une grande jaquette).
            input::set_fast_repeat(!(tab == 0 && self.ui.get_zone() == 0));
            self.idle_tick();
        }
        timed("sync_library_art", || self.sync_library_art());
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

/// Catalogue « à la une » gardé en mémoire (rouvrir la Boutique ne refait pas la requête) : visuels
/// déjà reçus et offres sans aucun visuel, pour ne jamais les réafficher en tuile vide.
struct Featured {
    offers: Vec<stores::Offer>,
    art: Vec<Option<imgload::Buf>>,
    no_art: std::collections::HashSet<usize>,
}

impl Shell {
    /// Charge le catalogue unifié (« à la une ») ou les résultats d'une recherche, depuis le début
    /// (filtre remis à « Tous », limite remise à une page).
    fn load_catalog(&self, query: Option<String>) {
        self.catalog_started.set(true);
        let gen = self.catalog_gen.get() + 1;
        self.catalog_gen.set(gen);
        self.ui.set_osel(0);
        self.ui.set_brow(0);
        self.ui.set_offers_title(match &query {
            Some(q) => tr("Résultats pour « {q} »").replace("{q}", q),
            None => tr("À la une"),
        }.into());
        *self.catalog_query.borrow_mut() = query.clone();
        self.catalog_limit.set(OFFERS_PAGE);
        // Filtre et tri choisis gardés (y compris en revenant à « À la une »).
        if query.is_none() {
            if let Some(f) = self.featured.borrow().as_ref() {
                self.apply_offers(&f.offers, &f.art);
                *self.offers_no_art.borrow_mut() = f.no_art.clone();
                self.reveal_offers();
                return;
            }
        }
        self.fetch_catalog(gen, query, OFFERS_PAGE, false);
    }

    /// « Voir plus » : relit le catalogue/la recherche courante, en demandant 24 jeux de plus par
    /// boutique qu'au tour précédent — une vraie requête réseau (GOG pagine réellement ; Steam/Epic
    /// ont une liste déjà bornée côté serveur et cesseront de grandir une fois celle-ci épuisée).
    fn show_more_offers(&self) {
        let gen = self.catalog_gen.get() + 1;
        self.catalog_gen.set(gen);
        let limit = self.catalog_limit.get() + OFFERS_PAGE;
        self.catalog_limit.set(limit);
        let query = self.catalog_query.borrow().clone();
        self.fetch_catalog(gen, query, limit, true);
    }

    /// Requête réseau proprement dite (chargement initial ou « Voir plus ») : liste puis visuels.
    /// `keep` (« Voir plus ») : les cartes déjà affichées restent en place pendant le chargement.
    fn fetch_catalog(&self, gen: u32, query: Option<String>, limit: usize, keep: bool) {
        if !keep {
            self.offers.set_vec(vec![]);
            self.offer_data.borrow_mut().clear();
            self.ui.set_offers_state("loading".into());
        }
        std::thread::spawn(move || {
            let offers = match &query {
                Some(q) => stores::search(q, limit),
                None => stores::catalog(limit),
            };
            let featured = query.is_none();
            let listed = offers.clone();
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.offers_listed(gen, featured, listed)));
            // Visuel manquant à la boutique d'origine (jeu trop récent/confidentiel) : repli sur
            // SteamGridDB, comme déjà fait pour la bibliothèque possédée (si une clé est enregistrée).
            let sgdb_key = i18n::pref("steamgriddb_key").filter(|k| !k.trim().is_empty());
            // Les images arrivent ensuite, en parallèle, et remplissent les cartes au fur et à mesure.
            let jobs: Vec<(usize, String, Vec<String>)> = offers.iter().enumerate().map(|(i, o)| (i, o.title.clone(), o.images.clone())).collect();
            std::thread::scope(|sc| {
                for chunk in jobs.chunks(jobs.len().div_ceil(6).max(1)) {
                    let sgdb_key = sgdb_key.clone();
                    sc.spawn(move || {
                        for (i, title, imgs) in chunk {
                            let found = stores::fetch_first(imgs).or_else(|| {
                                let key = sgdb_key.as_deref()?;
                                stores::search_sgdb(title, key).first().and_then(|o| stores::fetch_first(&o.images))
                            });
                            let i = *i;
                            match found.and_then(|p| imgload::decode(&p, 300, 450)) {
                                Some(p) => {
                                    let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.offer_art(gen, featured, i, p)));
                                }
                                // Aucune jaquette nulle part (ni la boutique d'origine, ni SteamGridDB) :
                                // presque toujours un jeu confidentiel/retiré, pas un vrai titre à montrer.
                                None => {
                                    let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.offer_no_art(gen, featured, i)));
                                }
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
            // Jaquettes déjà reçues (« Voir plus » relit toute la liste) : reprises tout de suite, les
            // cartes ne disparaissent pas le temps de leur nouveau téléchargement.
            let prev: std::collections::HashMap<String, slint::Image> =
                self.offers_full.borrow().iter().filter(|(_, u)| u.art.size().width > 0).map(|(d, u)| (d.title.clone(), u.art.clone())).collect();
            self.apply_offers(&offers, &paths);
            if !prev.is_empty() {
                for (d, u) in self.offers_full.borrow_mut().iter_mut() {
                    if let Some(img) = prev.get(&d.title) {
                        u.art = img.clone();
                    }
                }
                self.reveal_offers();
            }
        }
        if featured {
            *self.featured.borrow_mut() = Some(Featured { offers, art: paths, no_art: Default::default() });
        }
    }

    /// La carte `i` du lot `gen` est-elle celle affichée en ce moment ? Vrai aussi pour un lot « à la une »
    /// plus ancien rouvert depuis le cache (même liste, même ordre) : sinon une jaquette arrivée après un
    /// aller-retour par une recherche serait perdue et l'offre resterait cachée.
    fn offer_slot_live(&self, gen: u32, featured: bool, i: usize) -> bool {
        if self.catalog_gen.get() == gen {
            return true;
        }
        if !featured || self.catalog_query.borrow().is_some() {
            return false;
        }
        let title = self.featured.borrow().as_ref().and_then(|f| f.offers.get(i).map(|o| o.title.clone()));
        title.is_some() && self.offers_full.borrow().get(i).map(|(o, _)| o.title.clone()) == title
    }

    fn offer_art(&self, gen: u32, featured: bool, i: usize, p: imgload::Buf) {
        if featured {
            if let Some(slot) = self.featured.borrow_mut().as_mut().and_then(|f| f.art.get_mut(i)) {
                *slot = Some(p.clone());
            }
        }
        if !self.offer_slot_live(gen, featured, i) {
            return;
        }
        let img = imgload::image(&p);
        if let Some((_, row)) = self.offers_full.borrow_mut().get_mut(i) {
            row.art = img;
        }
        self.reveal_offers();
    }

    /// Aucune jaquette trouvée nulle part pour cette offre (voir `fetch_catalog`) : on la masque plutôt
    /// que de montrer une tuile vide pour un jeu presque toujours confidentiel/retiré.
    fn offer_no_art(&self, gen: u32, featured: bool, i: usize) {
        if featured {
            if let Some(f) = self.featured.borrow_mut().as_mut() {
                f.no_art.insert(i);
            }
        }
        if !self.offer_slot_live(gen, featured, i) {
            return;
        }
        self.offers_no_art.borrow_mut().insert(i);
        self.reveal_offers();
    }

    /// Reconstruit `offers` (le modèle exposé à l'UI) et `offer_data` à partir de `offers_full` :
    /// filtre de boutique (« Tous » ne retire rien), offres sans jaquette masquées (`offers_no_art`),
    /// puis tri courant (voir `offer_sort`).
    fn reveal_offers(&self) {
        let full = self.offers_full.borrow();
        let filter = self.offer_filter.borrow();
        let no_art = self.offers_no_art.borrow();
        // Une offre n'apparaît qu'une fois sa jaquette reçue : jamais de tuile vide (initiale seule),
        // ni pendant le chargement ni pour un jeu sans aucun visuel.
        let has_art = |u: &Offer| u.art.size().width > 0;
        let free_only = self.offer_free.get();
        let keep = |d: &stores::Offer, u: &Offer| filter.as_deref().is_none_or(|f| d.store == f) && has_art(u) && (!free_only || d.cents == Some(0));
        let mut kept: Vec<&(stores::Offer, Offer)> = full.iter().filter(|(d, u)| keep(d, u)).collect();
        if !full.is_empty() {
            let pending = full.iter().enumerate().any(|(i, (_, u))| !has_art(u) && !no_art.contains(&i));
            self.ui.set_offers_state(if !kept.is_empty() { "" } else if pending { "loading" } else { "empty" }.into());
        }
        match self.offer_sort.get() {
            1 => kept.sort_by_key(|(_, a)| a.title.to_lowercase()),
            2 => kept.sort_by_key(|(d, _)| d.cents.unwrap_or(i64::MAX)),
            3 => kept.sort_by_key(|(d, _)| std::cmp::Reverse(d.cents.unwrap_or(i64::MIN))),
            _ => {}
        }
        self.offers.set_vec(kept.iter().map(|(_, u)| u.clone()).collect::<Vec<_>>());
        *self.offer_data.borrow_mut() = kept.iter().map(|(d, _)| d.clone()).collect();
    }

    /// Bouton « Filtres » de la Boutique : un seul menu (boutique puis tri), qui se réaffiche après
    /// chaque choix pour ajuster l'autre sans le rouvrir — fermé seulement par « Fermer ».
    fn open_offer_filters(&self) {
        let current = self.offer_filter.borrow().clone();
        let mark = |on: bool, label: String| if on { format!("✓ {label}") } else { label };
        let mut v: Vec<(String, menus::Act)> = vec![(mark(current.is_none(), tr("Toutes les boutiques")), menus::Act::OfferStore(None))];
        for name in self.offer_store_names.borrow().iter() {
            v.push((mark(current.as_deref() == Some(name.as_str()), name.clone()), menus::Act::OfferStore(Some(name.clone()))));
        }
        let sort = self.offer_sort.get();
        v.push((mark(self.offer_free.get(), tr("Gratuits uniquement")), menus::Act::OfferFree));
        for (i, label) in [tr("Pertinence"), tr("Nom (A→Z)"), tr("Prix croissant"), tr("Prix décroissant")].into_iter().enumerate() {
            v.push((mark(sort == i as i32, label), menus::Act::OfferSort(i as i32)));
        }
        v.push((tr("Fermer"), menus::Act::Cancel));
        self.menu_show(tr("Filtres"), v);
    }

    /// Bouton « Filtres » de la Bibliothèque : tous les jeux, seulement ceux à installer, ou les cachés.
    fn open_lib_filters(&self) {
        let cur = self.lib_filter.get();
        // Nombre de jeux par filtre (mêmes critères que `rebuild`, recherche non comprise).
        let n = {
            let games = self.games.borrow();
            let lib: Vec<&Game> = games.iter().filter(|g| (g.install || g.hidden) && self.dl_pct_of(g) < 0).collect();
            [lib.len(), lib.iter().filter(|g| g.install).count(), lib.iter().filter(|g| g.hidden).count(), lib.iter().filter(|g| g.store == "Steam").count(), lib.iter().filter(|g| g.store == "Epic Games").count()]
        };
        let mut v: Vec<(String, menus::Act)> = LIB_FILTERS
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let label = format!("{} ({})", tr(l), n[i]);
                (if cur == i as u8 { format!("✓ {label}") } else { label }, menus::Act::LibFilter(i as u8))
            })
            .collect();
        v.push((tr("Fermer"), menus::Act::Cancel));
        self.menu_show(tr("Filtres"), v);
    }

    pub(crate) fn set_lib_filter(&self, f: u8) {
        self.lib_filter.set(f);
        let label = if f == 0 { tr("Filtres") } else { tr(LIB_FILTERS[f as usize]) };
        self.ui.set_lib_filter_label(label.into());
        self.ui.set_lib_filter(f as i32);
        self.ui.set_lsel(0);
        self.rebuild();
        self.open_lib_filters();
    }

    /// Triangle / Y dans la Bibliothèque : efface la recherche.
    fn lib_search_clear(&self) {
        if self.lib_query.borrow().is_empty() {
            return;
        }
        self.lib_query.borrow_mut().clear();
        self.ui.set_lib_query("".into());
        self.ui.set_lsel(0);
        self.rebuild();
    }

    /// Triangle / Y dans la Boutique : efface la recherche et revient à la sélection « À la une ».
    fn store_search_clear(&self) {
        self.ui.set_search_query("".into());
        self.ui.set_osel(0);
        self.load_catalog(None);
    }

    /// L1/R1 dans la fiche d'un jeu de la boutique : offre voisine dans la grille.
    fn offer_step(&self, d: i32) {
        let i = self.ui.get_osel() + d;
        if i < 0 || i as usize >= self.offer_data.borrow().len() {
            return;
        }
        self.ui.set_osel(i);
        self.show_offer(i as usize);
    }

    fn set_offer_store_filter(&self, f: Option<String>) {
        *self.offer_filter.borrow_mut() = f;
        self.reveal_offers();
        self.open_offer_filters();
    }

    fn set_offer_sort(&self, s: i32) {
        self.offer_sort.set(s);
        self.reveal_offers();
        self.open_offer_filters();
    }

    fn apply_offers(&self, offers: &[stores::Offer], paths: &[Option<imgload::Buf>]) {
        let rows: Vec<Offer> = offers
            .iter()
            .zip(paths)
            .map(|(o, p)| Offer {
                title: o.title.as_str().into(),
                price: price_text(&o.price).into(),
                store: o.store.as_str().into(),
                art: p.as_ref().map(imgload::image).unwrap_or_default(),
                owned: false,
                initial: o.title.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default().into(),
            })
            .collect();
        let own = self.owned_titles();
        let rows: Vec<Offer> = rows.into_iter().map(|mut r| { r.owned = own.contains(&stores::norm_title(&r.title)); r }).collect();
        let state = if !rows.is_empty() {
            ""
        } else if !stores::any_connected() {
            "disconnected"
        } else {
            "empty"
        };
        self.ui.set_offers_state(state.into());
        // « Voir plus » reste toujours proposé : Steam/Epic ont une liste déjà bornée côté serveur, donc
        // au-delà d'un certain point un clic ne ramène plus rien de neuf, mais rien n'empêche de réessayer.
        self.ui.set_offers_more(!rows.is_empty());
        self.offers_no_art.borrow_mut().clear();
        *self.offers_full.borrow_mut() = offers.iter().cloned().zip(rows).collect();
        let mut names: Vec<String> = offers.iter().map(|o| o.store.clone()).collect();
        names.sort();
        names.dedup();
        *self.offer_store_names.borrow_mut() = names;
        self.reveal_offers();
    }

    fn search_open(&self) {
        self.kb_search.set(true);
        // Recherche en cours reprise (comme dans la Bibliothèque), modifiable.
        let cur = self.catalog_query.borrow().clone().unwrap_or_default();
        self.settings.kb_open_text(&cur);
        self.kb_refresh();
        self.ui.set_kbx(0);
        self.ui.set_kby(0);
        self.ui.set_kb_title(tr("Rechercher un jeu").into());
        self.ui.set_zone(4);
    }

    fn lib_search_open(&self) {
        self.kb_lib_search.set(true);
        self.settings.kb_open_text(&self.lib_query.borrow());
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
        self.ui.set_det_owned(self.offers.row_data(i).is_some_and(|r| r.owned));
        self.ui.set_det_desc(o.desc.as_str().into());
        self.ui.set_det_meta(o.meta.as_str().into());
        self.ui.set_det_cover(self.offers.row_data(i).map(|r| r.art).unwrap_or_default());
        self.ui.set_det_hero(Default::default());
        self.det_shots.set_vec(vec![]);
        self.ui.set_det_shots_full(Default::default());
        let rows: Vec<Deal> = o.deals.iter().map(|d| Deal { store: d.store.as_str().into(), price: price_text(&d.price).into() }).collect();
        self.deals.set_vec(rows);
        *self.det_deals.borrow_mut() = o.deals.clone();
        *self.det_title.borrow_mut() = o.title.clone();
        self.ui.set_dsel(0);
        self.ui.set_det_loading(true);
        self.ui.set_zone(7);
        std::thread::spawn(move || {
            let d = stores::details(&o);
            let hero = stores::fetch_first(&o.heroes).and_then(|p| imgload::decode(&p, 2560, 1440));
            let shots: Vec<imgload::Buf> = d.shots.iter().filter_map(|u| stores::fetch_first(std::slice::from_ref(u))).filter_map(|p| imgload::decode(&p, 420, 240)).collect();
            let full_urls = d.shots_full.clone();
            let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.details_ready(gen, d, hero, shots)));
            // Captures en pleine taille pour la visionneuse (les vignettes, agrandies, étaient floues) :
            // ensuite, sans retarder l'affichage de la fiche.
            let full: Vec<imgload::Buf> = full_urls.iter().filter_map(|u| stores::fetch_first(std::slice::from_ref(u))).filter_map(|p| imgload::decode(&p, 1920, 1080)).collect();
            if !full.is_empty() {
                let _ = slint::invoke_from_event_loop(move || {
                    with_shell(|s| {
                        if s.det_gen.get() == gen {
                            s.ui.set_det_shots_full(Rc::new(VecModel::from(full.iter().map(imgload::image).collect::<Vec<_>>())).into());
                        }
                    })
                });
            }
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
        let again = titleless.clone();
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
            // Deuxième passe : jeux dont les visuels de la boutique n'ont pas pu être téléchargés (format non géré, lien mort…).
            // Recherche par titre sur Steam, puis SteamGridDB ; un marqueur évite de réessayer pendant 24 h.
            for (id, title) in &again {
                let marker = stores::asset_dir(id).join(".noart");
                let fresh = std::fs::metadata(&marker).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e < Duration::from_secs(24 * 3600));
                if fresh || stores::asset(id, "cover").is_some() {
                    continue;
                }
                let art = stores::resolve_art(title).unwrap_or_default();
                let got = stores::ensure_art_for(id, title, &art, key.as_deref());
                if got || stores::asset(id, "cover").is_some() {
                    let id = id.clone();
                    let _ = slint::invoke_from_event_loop(move || with_shell(|s| s.reload_art(&id)));
                } else {
                    let _ = std::fs::create_dir_all(stores::asset_dir(id));
                    let _ = std::fs::write(&marker, "1");
                }
            }
        });
    }

    /// Un visuel vient d'arriver : on met à jour uniquement la tuile concernée (pas de reconstruction de toute la liste).
    pub fn reload_art(&self, id: &str) {
        self.desktop_refresh_games();
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

/// Durée de l'agrandissement (et de la rétractation) de la jaquette ; `HEARTH_ZOOM_MS` la change (tests au ralenti).
fn zoom_ms() -> u64 {
    std::env::var("HEARTH_ZOOM_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(420)
}

impl Shell {
    /// Lancement depuis le dock de Jeux : la jaquette grandit jusqu'au plein écran, puis l'écran de
    /// lancement prend le relais sur la même image. Sinon (Bibliothèque, animations coupées…), l'écran
    /// de lancement directement.
    fn begin_splash(&self, i: usize, uri: bool) {
        let from_dock = i18n::anim()
            && self.ui.get_tab() == 0
            && self.ui.get_zone() == 0
            && self.tiles.row_data(self.ui.get_sel().max(0) as usize).is_some_and(|t| t.gid as usize == i);
        let Some(t) = from_dock.then(|| self.tile_for(i)).flatten() else {
            self.start_splash(i, uri);
            return;
        };
        self.ui.set_zoom_cover(t.cover);
        self.ui.set_zoom_hero(t.hero);
        self.zoom_to(1.0);
        self.zoom.set(1);
        self.splash_tile.set(Some(i));
        self.zoom_timer.start(slint::TimerMode::SingleShot, Duration::from_millis(zoom_ms() + 30), move || {
            with_shell(|s| {
                if s.zoom.get() == 1 {
                    s.zoom.set(2);
                    s.start_splash(i, uri);
                }
            })
        });
    }

    /// Animation inverse : l'image plein écran se rétracte vers la jaquette du dock.
    fn zoom_out(&self) {
        self.zoom_timer.stop();
        self.zoom.set(0);
        self.zoom_to(0.0);
    }

    /// Fait glisser `zoom-t` de sa valeur actuelle vers `to` en `zoom_ms()`, avec accélération puis
    /// décélération (cubique), une image toutes les ~8 ms.
    fn zoom_to(&self, to: f32) {
        self.zoom_anim.set(Some((std::time::Instant::now(), self.ui.get_zoom_t(), to)));
        self.zoom_anim_timer.start(slint::TimerMode::Repeated, Duration::from_millis(8), || {
            with_shell(|s| {
                let Some((t0, from, to)) = s.zoom_anim.get() else {
                    s.zoom_anim_timer.stop();
                    return;
                };
                let p = (t0.elapsed().as_secs_f32() * 1000.0 / zoom_ms() as f32).min(1.0);
                let e = if p < 0.5 { 4.0 * p * p * p } else { 1.0 - (-2.0 * p + 2.0).powi(3) / 2.0 };
                s.ui.set_zoom_t(from + (to - from) * e);
                if p >= 1.0 {
                    s.zoom_anim.set(None);
                    s.zoom_anim_timer.stop();
                }
            })
        });
    }

    /// Écran de lancement : fenêtre superposée au jeu, qui reste le temps du chargement.
    fn start_splash(&self, i: usize, uri: bool) {
        let Some(t) = self.tile_for(i) else { return };
        // Après l'agrandissement de la jaquette : le premier plan de l'écran de lancement est la même image
        // plein écran que la dernière de l'animation (héros s'il existe, sinon jaquette), qui s'efface ensuite.
        let zoomed = self.zoom.get() == 2;
        self.splash.set_launch_zoom(if !zoomed { Default::default() } else if t.hero.size().width > 0 { t.hero.clone() } else { t.cover.clone() });
        self.splash.set_launch_cover(t.cover);
        // Ordre : le logo du jeu si on l'a ; sinon un héros assez grand pour ne pas être flou une fois agrandi ;
        // sinon rien (la roue de chargement, seule, au centre, avec le nom).
        let screen_w = wm::primary_monitor().2 as u32;
        self.splash.set_launch_logo(t.logo);
        self.splash.set_launch_art(if t.hero.size().width >= screen_w { t.hero } else { Default::default() });
        self.splash.set_launch_status(format!("{} {}", tr("Démarrage de"), t.name).into());
        self.splash.set_launch_hue(t.hue);
        self.splash.set_launch_t(0.0);
        self.splash.set_launch_spin(0.0);
        if let Some(h) = wm::find_window(SPLASH_TITLE) {
            let (x, y, w, hh) = wm::primary_monitor();
            wm::show_overlay(h, x, y, w, hh);
            self.splash.window().request_redraw();
        }
        self.splash_start.set(Some(std::time::Instant::now()));
        self.splash_uri.set(uri);
        self.splash_tile.set(Some(i));
        self.splash_timer.start(slint::TimerMode::Repeated, Duration::from_millis(40), || with_shell(|s| s.splash_tick()));
    }

    pub fn hide_splash(&self) {
        // Jaquette encore en train de grandir : l'écran de lancement ne doit plus s'afficher après coup ;
        // elle se rétractera au prochain passage de Hearth au premier plan (voir `animate`).
        if self.zoom.get() == 1 {
            self.zoom_timer.stop();
            self.zoom.set(2);
        }
        if let Some(h) = wm::find_window(SPLASH_TITLE) {
            wm::hide(h);
        }
        self.splash_start.set(None);
        self.splash_timer.stop();
    }

    /// Annule un lancement en cours (écran de chargement) : O (PS5) / B (Xbox). Tue le job du jeu
    /// lancé directement ; pour un lancement par une boutique (processus hors de notre contrôle),
    /// au mieux : ferme sa fenêtre si elle est déjà repérée, sinon on ne fait que cesser de la suivre.
    fn cancel_launch(&self) {
        let Some(tile) = self.splash_tile.get() else { return };
        self.hide_splash();
        if let Some(pos) = self.running.borrow().iter().position(|r| r.tile == tile) {
            self.running.borrow_mut().remove(pos).session.kill();
        }
        if self.ext_pending.get().is_some_and(|(t, _)| t == tile) {
            self.ext_pending.set(None);
        }
        let mut ext = self.ext_game.borrow_mut();
        if ext.as_ref().is_some_and(|e| e.tile == tile) {
            if let Some(e) = ext.take() {
                wm::kill_owner(e.hwnd);
                e.tree.kill();
            }
        }
        drop(ext);
        self.rebuild();
    }

    fn splash_tick(&self) {
        let Some(t0) = self.splash_start.get() else {
            self.splash_timer.stop();
            return;
        };
        let el = t0.elapsed().as_secs_f32();
        self.splash.set_launch_t((el / 0.9).min(1.0));
        self.splash.set_launch_spin((el * 540.0) % 360.0);
        let tile = self.splash_tile.get();
        // « Montré » : le jeu a sa propre fenêtre au premier plan (lancement direct), ou sa fenêtre externe a été repérée
        // (lancement par Steam/Epic : voir track_ext_game). Tant que ce n'est pas le cas, le chargement peut continuer
        // sans limite fixe (certains jeux mettent largement plus de 15 s) — un plafond très large sert juste de filet.
        let fg = wm::foreground();
        let shown_exe = self.running.borrow().iter().any(|r| Some(r.tile) == tile && r.session.owns(fg));
        let shown_ext = self.ext_game.borrow().as_ref().is_some_and(|e| Some(e.tile) == tile);
        // Lancement direct dont le processus s'est arrêté avant même d'apparaître (plantage, antitriche refusé…) : inutile d'attendre.
        // (Pas pendant que le lanceur de la boutique démarre : le jeu n'est simplement pas encore parti.)
        let preparing = self.steam_wait.get() || self.launcher_wait.get();
        let gave_up_exe = !preparing && !self.splash_uri.get() && tile.is_some_and(|t| !self.running.borrow().iter().any(|r| r.tile == t));
        let done = ((shown_exe || shown_ext) && el > 5.0) || gave_up_exe || el > 90.0;
        if done || wm::find_window(SPLASH_TITLE).is_none_or(|h| !wm::is_visible(h)) {
            self.hide_splash();
            // Le jeu s'est arrêté avant d'ouvrir sa fenêtre : on le dit, plutôt qu'un retour muet à l'accueil.
            if gave_up_exe && !shown_exe {
                if let Some(name) = tile.and_then(|t| self.games.borrow().get(t).map(|g| g.name.clone())) {
                    self.push_notif(&name, &tr("Le jeu s'est fermé au démarrage"));
                }
            }
        }
    }
}

const LIB_FILTERS: [&str; 5] = ["Tous", "À installer", "Cachés", "Steam", "Epic Games"];

/// Durée de la partie en cours pour le guide : « 12 min », « 1 h 05 ».
fn session_label(secs: u64) -> String {
    match secs / 60 {
        m if m < 60 => format!("{} {}", m.max(1), tr("min")),
        m => format!("{} {} {:02}", m / 60, tr("h"), m % 60),
    }
}

/// Clé de temps de jeu : identifiant du jeu, sinon son nom.
pub(crate) fn playtime_key(g: &Game) -> String {
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

    /// Jaquette décodée tout de suite (bloquant) : au démarrage, évite que les tuiles de l'écran d'accueil
    /// apparaissent une par une. Pas mise en cache : la vignette complète (avec héros/logo) arrive ensuite via `art_request`.
    fn cover_sync(&self, g: &Game) -> slint::Image {
        let path = if g.cover.is_empty() { stores::asset(&g.id, "cover") } else { Some(PathBuf::from(&g.cover)) };
        // 320×480 : la plus grande tuile affichée ne dépasse pas ~234×370 px — une jaquette bien plus grande que
        // sa taille d'affichage force le moteur de rendu à la réduire à chaque image, ce qui crénelait (aliasing).
        path.and_then(|p| imgload::decode(&p, 320, 480)).map(|b| imgload::image(&b)).unwrap_or_default()
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
                let c = cover.and_then(|p| imgload::decode(&p, 320, 480));
                // 2560×1440 : la largeur de l'écran de l'accueil (où le héros est affiché le plus grand).
                let h = hero.and_then(|p| imgload::decode(&p, 2560, 1440));
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
        } else if self.boot.get() {
            let cover = self.cover_sync(g);
            self.art_request(g);
            (cover, Default::default(), Default::default())
        } else {
            self.art_of(g)
        };
        Tile {
            gid: gid as i32,
            name: g.name.as_str().into(),
            initial: g.name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default().into(),
            hue: hue_of(&g.name),
            cover,
            hero,
            logo,
            running,
            install: g.install,
            hidden: g.hidden,
            sub: if g.install { "".into() } else { playtime::label_in(pt, &playtime_key(g)).into() },
            store: if g.store.is_empty() { tr("Local") } else { g.store.clone() }.into(),
            played: playtime::played_in(pt, &playtime_key(g)).into(),
            lastp: playtime::last_in(pt, &playtime_key(g)).into(),
            dl_pct: self.dl_pct_of(g),
        }
    }

    /// Progression (0..100) du téléchargement SteamCMD en cours de ce jeu, -1 sinon.
    fn dl_pct_of(&self, g: &Game) -> i32 {
        let Some(appid) = scmd::appid_of(&g.id) else { return -1 };
        self.dl
            .borrow()
            .iter()
            .find(|d| d.appid == appid)
            .and_then(|d| match d.state {
                scmd::DlState::Active => Some(d.pct as i32),
                scmd::DlState::Queued => Some(0),
                _ => None,
            })
            .unwrap_or(-1)
    }

    fn tile_for(&self, gid: usize) -> Option<Tile> {
        let games = self.games.borrow();
        let g = games.get(gid)?;
        Some(self.make_tile(gid, g, false, &Default::default()))
    }

    /// Reconstruit les onglets Jeux (installés, triés par dernière partie) et Bibliothèque (à installer).
    pub fn rebuild(&self) {
        let old_sel = self.ui.get_sel().max(0) as usize;
        // Au-delà du dernier jeu : le raccourci Bibliothèque (tuile de droite), pas un jeu réel.
        // (Liste encore vide = tout premier remplissage au démarrage : sélection sur le premier jeu.)
        let sel_on_library_shortcut = self.tiles.row_count() > 0 && old_sel >= self.tiles.row_count();
        let cur = self.tiles.row_data(old_sel).map(|t| t.gid);
        let cur_lib = self.library.row_data(self.ui.get_lsel().max(0) as usize).map(|t| t.gid);
        let pt = playtime::load();
        let mut running: Vec<usize> = self.running.borrow().iter().map(|r| r.tile).collect();
        if let Some(e) = self.ext_game.borrow().as_ref() {
            running.push(e.tile);
        }
        let all_tab = false;
        let query = self.lib_query.borrow().to_lowercase();
        let (mut a, mut b): (Vec<(u64, Tile)>, Vec<Tile>) = (vec![], vec![]);
        let mut lib_total = 0;
        for (i, g) in self.games.borrow().iter().enumerate() {
            let t = self.make_tile(i, g, running.contains(&i), &pt);
            let downloading = t.dl_pct >= 0;
            // Onglet Tout : tous les jeux (installés ou non) ; sinon seulement ceux à installer.
            // Un jeu caché n'est retiré que de Jeux : il reste dans Bibliothèque, seul endroit
            // d'où le réafficher (sinon aucun des deux onglets ne le montre plus). Un jeu en cours de
            // téléchargement (SteamCMD) passe dans Jeux avec son anneau de progression, pas Bibliothèque.
            let filter_ok = match self.lib_filter.get() {
                1 => g.install,
                2 => g.hidden,
                3 => g.store == "Steam",
                4 => g.store == "Epic Games",
                _ => true,
            };
            if (g.install || g.hidden) && !downloading {
                lib_total += 1;
            }
            if (g.install || g.hidden || all_tab) && filter_ok && !downloading && (query.is_empty() || t.name.to_lowercase().contains(&query) || g.store.to_lowercase().contains(&query)) {
                b.push(t.clone());
            }
            // Un jeu caché dont on relance le téléchargement (mise à jour…) doit quand même montrer
            // son anneau de progression, sinon il disparaît des deux onglets jusqu'à la fin.
            if (!g.install || downloading) && (!g.hidden || downloading) {
                a.push((playtime::last(&playtime_key(g), &pt), t));
            }
        }
        a.sort_by_key(|(last, _)| std::cmp::Reverse(*last));
        let a: Vec<Tile> = a.into_iter().map(|(_, t)| t).collect();
        // Toujours alphabétique : sans ça, l'ordre dépend de la boutique qui a signalé le jeu en premier.
        b.sort_by_key(|t| t.name.to_lowercase());
        let sel = cur.and_then(|g| a.iter().position(|t| t.gid == g)).unwrap_or_else(|| {
            if sel_on_library_shortcut { a.len() } else { old_sel.min(a.len().saturating_sub(1)) }
        });
        let lsel = cur_lib.and_then(|g| b.iter().position(|t| t.gid == g)).unwrap_or((self.ui.get_lsel().max(0) as usize).min(b.len().saturating_sub(1)));
        self.tiles.set_vec(a);
        self.library.set_vec(b);
        self.ui.set_lib_total(lib_total);
        self.desktop_refresh_games();
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
        self.replace_games(new);
        self.prune_updates();
        self.rebuild();
        self.refresh_store_status();
    }

    /// Remplace la liste des jeux en recalant tous les repères par position (jeux en cours, jeu de
    /// boutique suivi ou en attente, écran de lancement, guide) sur le même jeu dans la nouvelle liste.
    fn replace_games(&self, new: Vec<Game>) {
        let old = self.games.borrow().clone();
        let remap = |t: usize| old.get(t).and_then(|g| new.iter().position(|n| n.id == g.id));
        for r in self.running.borrow_mut().iter_mut() {
            if let Some(p) = remap(r.tile) {
                r.tile = p;
            }
        }
        if let Some(e) = self.ext_game.borrow_mut().as_mut() {
            if let Some(p) = remap(e.tile) {
                e.tile = p;
            }
        }
        if let Some((t, t0)) = self.ext_pending.get() {
            self.ext_pending.set(remap(t).map(|p| (p, t0)));
        }
        if let Some(t) = self.splash_tile.get() {
            self.splash_tile.set(remap(t));
        }
        if let Some(p) = remap(self.guide_tile.get()) {
            self.guide_tile.set(p);
        }
        *self.games.borrow_mut() = new;
    }

    /// Premier vrai jeu ajouté : les jeux de démonstration (affichés seulement quand la liste est vide)
    /// disparaissent tout de suite, sauf celui qui tournerait encore.
    pub(crate) fn drop_demos(&self) {
        let running: Vec<usize> = self.running.borrow().iter().map(|r| r.tile).collect();
        let games = self.games.borrow().clone();
        if !games.iter().any(|g| g.id.starts_with("demo-")) {
            return;
        }
        let kept: Vec<Game> = games.into_iter().enumerate().filter(|(i, g)| !g.id.starts_with("demo-") || running.contains(i)).map(|(_, g)| g).collect();
        self.replace_games(kept);
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
            self.auto_launch_options();
        }
    }
}

impl Shell {
    fn ui_state(&self) -> [i32; 21] {
        let u = &self.ui;
        [
            u.get_zone(), u.get_tab(), u.get_top(), u.get_sel(), u.get_lsel(), u.get_osel(), u.get_psel(), u.get_msel(), u.get_nsel(),
            u.get_dsel(), u.get_dlsel(), u.get_srow(), u.get_cat(), u.get_brow(), u.get_kbx(), u.get_kby(), u.get_pw(), u.get_sfocus(),
            u.get_lbrow(), u.get_lib_col(), u.get_obsel(),
        ]
    }

    /// L2/R2 (ou Page préc./suiv.) sur l'accueil : volume par pas de 5 %, pastille éphémère.
    fn volume_step(&self, delta: i32) {
        let Some(v) = system::audio::volume() else { return };
        let v = (v as i32 + delta).clamp(0, 100);
        system::audio::set_volume(v as u32);
        // Son coupé dans Windows : monter le volume le rétablit ; sinon la jauge affiche « Muet ».
        let mut muted = system::audio::muted();
        if muted && delta > 0 {
            system::audio::set_mute(false);
            muted = false;
        }
        let v = if muted { 0 } else { system::audio::volume().map_or(v, |x| x as i32) };
        self.ui.set_st_vol(v);
        self.ui.set_vol_osd(v);
        if i18n::sfx() {
            system::sfx::play(system::sfx::Sfx::Move);
        }
        let gen = self.vol_osd_gen.get() + 1;
        self.vol_osd_gen.set(gen);
        slint::Timer::single_shot(Duration::from_millis(1500), move || {
            with_shell(|s| {
                if s.vol_osd_gen.get() == gen {
                    s.ui.set_vol_osd(-1);
                }
            })
        });
    }

    /// Accueil sans action depuis 5 min (console devant, aucun jeu, aucun panneau) : l'écran s'assombrit
    /// doucement (~1,5 s) ; `nav_with_sound` le rallume au premier bouton, un mouvement de souris aussi.
    /// Appelé toutes les 200 ms (`animate`, une fois sur deux).
    fn idle_tick(&self) {
        thread_local! {
            static CURSOR: Cell<(i32, i32)> = const { Cell::new((0, 0)) };
        }
        let mut p = windows::Win32::Foundation::POINT::default();
        if unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) }.is_ok() && CURSOR.with(|c| c.replace((p.x, p.y))) != (p.x, p.y) {
            self.last_activity.set(std::time::Instant::now());
        }
        let dim_min = i18n::dim_min();
        let idle = dim_min > 0 && self.last_activity.get().elapsed() > Duration::from_secs(dim_min as u64 * 60)
            && self.fg_main.get()
            && self.ui.get_zone() <= 1
            && self.running.borrow().is_empty()
            && self.ext_game.borrow().is_none()
            && self.splash_start.get().is_none();
        let d = self.dim.get();
        let next = if idle { (d + 0.1).min(0.7) } else { 0.0 };
        if next != d {
            self.dim.set(next);
            self.ui.set_dim(next);
        }
    }

    /// Navigation (manette ou clavier) avec son : déplacement, validation, retour, ouverture d'un menu.
    fn nav_with_sound(&self, a: i32) {
        self.last_activity.set(std::time::Instant::now());
        // Écran assombri : le premier bouton ne fait que le rallumer (pas d'action à l'aveugle).
        if self.dim.get() > 0.0 {
            self.dim.set(0.0);
            self.ui.set_dim(0.0);
            return;
        }
        if a == Nav::VolDown as i32 || a == Nav::VolUp as i32 {
            self.volume_step(if a == Nav::VolUp as i32 { 5 } else { -5 });
            return;
        }
        if a == Nav::Guide as i32 {
            self.kb_abort();
        }
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
        } else if (0..=3).contains(&a) && before[0] <= 1 && self.last_bump.get().is_none_or(|t| t.elapsed() > Duration::from_millis(400)) {
            // Direction sans effet sur l'accueil ou la barre du haut : bord de la liste atteint. Au plus
            // un son toutes les 400 ms (un stick maintenu contre le bord répète la direction en continu).
            self.last_bump.set(Some(std::time::Instant::now()));
            Some(Sfx::Error)
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
            } else if let Some(h) = wm::foreground_game(&["steam.exe", "steamwebhelper.exe", "explorer.exe", "hearth-shell.exe", "hearth.exe", "cmd.exe"]) {
                let tree = wm::ProcTree::new(wm::window_pid(h));
                *self.ext_game.borrow_mut() = Some(ExtGame { tile, hwnd: h, started: std::time::Instant::now(), closing: false, tree, lost: None });
                self.ext_pending.set(None);
                self.rebuild();
            }
        }
        // Fenêtre suivie disparue : se raccrocher à une autre fenêtre de l'arbre (le vrai jeu lancé par
        // un lanceur, ou une nouvelle fenêtre après changement de mode) ; tant qu'un de ses processus
        // tourne encore, patienter (le jeu peut mettre du temps à ouvrir sa fenêtre) ; sinon, fin.
        let ended = {
            let mut ext = self.ext_game.borrow_mut();
            match ext.as_mut() {
                None => None,
                Some(e) => {
                    let snap = e.tree.refresh();
                    if wm::is_alive(e.hwnd) {
                        e.lost = None;
                        None
                    } else if let Some(h) = wm::main_windows().into_iter().find(|&h| e.tree.contains(wm::window_pid(h), &snap)) {
                        e.hwnd = h;
                        e.lost = None;
                        None
                    } else {
                        let since = *e.lost.get_or_insert_with(std::time::Instant::now);
                        (!e.tree.alive(&snap) || since.elapsed() > Duration::from_secs(90)).then(|| (e.tile, e.started.elapsed().as_secs()))
                    }
                }
            }
        };
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

/// Restes d'anciennes versions dans `%LOCALAPPDATA%\Hearth`, que plus aucun code ne lit ni n'écrit
/// (journaux de diagnostic, anciens sons générés, marqueur d'un assistant administrateur périmé), et
/// clés de `settings.json` abandonnées. Liste fermée : rien d'autre n'est touché.
fn cleanup_obsolete() {
    let base = stores::base_dir();
    let mut files: Vec<String> = ["guide.txt", "nav.log", "tick.log", "zone.log", "hotkey-got.log", "scale-debug.log", "helper-v2.done", "lang.log"].iter().map(|s| s.to_string()).collect();
    files.extend((0..8).map(|i| format!("hotkey-{i}.log")));
    for f in files {
        let _ = std::fs::remove_file(base.join(f));
    }
    let _ = std::fs::remove_dir_all(base.join("sfx"));
    let _ = std::fs::remove_file(base.join("steamcmd").join("hearth-login.log"));
}

/// Clés abandonnées de `settings.json` (sur le fil de l'interface, comme toutes ses écritures).
fn cleanup_obsolete_prefs() {
    let path = stores::base_dir().join("settings.json");
    if let Some(mut v) = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(s.trim_start_matches('\u{feff}')).ok()) {
        if let Some(m) = v.as_object_mut() {
            let n = m.len();
            for k in ["helper_ver", "vigem_tried", "steam_overlay_off", "steam_input_off"] {
                m.remove(k);
            }
            if m.len() != n {
                prefs::write_atomic(&path, &v.to_string());
            }
        }
    }
}

pub(crate) fn power_do(a: i32) {
    match a {
        0 => unsafe {
            let _ = windows::Win32::System::Power::SetSuspendState(false, false, false);
        },
        1 => drop(Command::new("shutdown").args(["/r", "/t", "0"]).spawn()),
        2 => drop(Command::new("shutdown").args(["/s", "/t", "0"]).spawn()),
        // Le superviseur relance la console (même code que le changement de profil).
        3 => {
            wm::cursor::restore();
            std::process::exit(profiles::EXIT_RESTART)
        }
        _ => {}
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Âge d'une notification : « à l'instant », « il y a 5 min », « il y a 2 h », « il y a 3 j ».
fn notif_age(secs: u64) -> String {
    match secs {
        s if s < 60 => tr("à l'instant"),
        s if s < 3600 => tr("il y a {n} min").replace("{n}", &(s / 60).to_string()),
        s if s < 86_400 => tr("il y a {n} h").replace("{n}", &(s / 3600).to_string()),
        s => tr("il y a {n} j").replace("{n}", &(s / 86_400).to_string()),
    }
}
