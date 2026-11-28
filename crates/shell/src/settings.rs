//! Écran Réglages : état et actions (la mise en page est dans app.slint).

use std::cell::RefCell;
use std::rc::Rc;

use slint::{Model, VecModel};
use system::audio::{self, Device};
use system::bluetooth::Dev;
use system::display::{self, Mode};
use system::wifi::Net;

use crate::i18n::{self, tr};
use crate::theme;
use crate::SettingRow;

const CATS: [&str; 14] =
    ["Son", "Affichage", "Wi-Fi", "Bluetooth", "Manettes", "Langue", "Mises à jour", "Stockage", "Pilotes", "Alimentation", "Notifications", "À propos", "Comptes", "Thème"];

pub fn cats() -> Vec<String> {
    CATS.iter().map(|c| tr(c)).collect()
}

const CONFIRM_SECS: u32 = 15;
const SLEEP_CHOICES: [u32; 5] = [0, 15, 30, 60, 120];
const SCREEN_CHOICES: [u32; 6] = [0, 5, 10, 15, 30, 60];

/// Données lentes (WinRT, commandes), lues hors du thread UI.
#[derive(Default)]
pub struct Fetched {
    wifi_on: Option<bool>,
    nets: Vec<Net>,
    link: Option<system::net::Link>,
    bt_on: Option<bool>,
    bt_paired: Vec<Dev>,
    pads: Vec<system::pads::Pad>,
    wifi_err: String,
    drives: Vec<system::storage::Drive>,
    update: String,
    update_drv: String,
    gpus: Vec<system::drivers::Gpu>,
    plans: Vec<system::power::Plan>,
    sleep_min: Option<u32>,
    screen_min: Option<u32>,
    about: Vec<(&'static str, String)>,
    accounts: Vec<(String, bool, Option<String>)>,
}

/// Ordre choisi par l'utilisateur (identifiants de manettes, le premier est la « principale »,
/// transmise aux jeux). Les manettes non classées suivent, dans l'ordre où Windows les signale.
fn pad_order() -> Vec<String> {
    i18n::pref("pad_order").map(|s| s.split(',').map(String::from).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
}

fn set_pad_order(ids: &[String]) {
    i18n::set_pref("pad_order", &ids.join(","));
    input::set_primary_pad(ids.first().cloned());
}

pub fn ordered_pads(raw: Vec<system::pads::Pad>) -> Vec<system::pads::Pad> {
    // La manette Sony branchée a sa propre copie virtuelle Xbox 360 (ViGEmBus, pour les jeux) : Windows la
    // signale comme une seconde manette, à tort (nom générique, « 100 % » de batterie fictifs). On la cache.
    let mut rest: Vec<system::pads::Pad> = if raw.iter().any(|p| p.vendor == system::pads::SONY_VENDOR) {
        raw.into_iter().filter(|p| p.vendor != system::pads::XBOX_VENDOR).collect()
    } else {
        raw
    };
    // Windows ne connaît pas toujours le nom d'une manette Sony (« Contrôleur de jeu HID » générique).
    for p in rest.iter_mut().filter(|p| p.vendor == system::pads::SONY_VENDOR) {
        if !p.name.to_lowercase().contains("dualsense") && !p.name.to_lowercase().contains("dualshock") {
            p.name = tr("Manette PlayStation");
        }
    }
    let order = pad_order();
    let mut out: Vec<system::pads::Pad> = vec![];
    for id in &order {
        if let Some(i) = rest.iter().position(|p| &p.id == id) {
            out.push(rest.remove(i));
        }
    }
    out.extend(rest);
    // La manette en tête (choisie ou, à défaut, la première détectée) est celle transmise aux jeux.
    input::set_primary_pad(out.first().map(|p| p.id.clone()));
    out
}

pub fn fetch(cat: i32) -> Fetched {
    match cat {
        2 => {
            let nets = system::wifi::scan();
            Fetched { wifi_on: system::radio::wifi_on(), nets, link: system::net::current(), wifi_err: system::wifi::last_error(), ..Default::default() }
        }
        3 => Fetched { bt_on: system::radio_ext::bluetooth_on(), bt_paired: system::bluetooth::devices(true), ..Default::default() },
        4 => Fetched { pads: ordered_pads(system::pads::list()), ..Default::default() },
        7 => Fetched { drives: system::storage::drives(), ..Default::default() },
        8 => Fetched { gpus: system::drivers::gpus(), ..Default::default() },
        9 => Fetched { plans: system::power::plans(), sleep_min: system::power::timeout(true), screen_min: system::power::timeout(false), ..Default::default() },
        11 => Fetched { about: system::about::info(), ..Default::default() },
        12 => {
            let mut accounts: Vec<(String, bool, Option<String>)> = stores::all().iter().map(|s| (s.name().to_string(), s.detected(), stores::account::session(s.name()).map(|a| a.name))).collect();
            accounts.push(("@admin".into(), system::admin::enabled(), None));
            accounts.push(("@sgdb".into(), i18n::pref("steamgriddb_key").is_some_and(|k| !k.trim().is_empty()), None));
            Fetched { accounts, ..Default::default() }
        }
        _ => Fetched::default(),
    }
}

pub fn needs_fetch(cat: i32) -> bool {
    (2..=4).contains(&cat) || matches!(cat, 7 | 8 | 9 | 11 | 12)
}

/// Ce que le shell doit faire après une action.
pub enum Outcome {
    None,
    DisplayChanged,
    Toggle { wifi: bool, on: bool },
    Password(String),
    Connect(String),
    LangChanged,
    ThemeChanged,
    Oobe,
    Desktop,
    Import,
    SoundPicker(bool),
    CheckUpdates(bool),
    BtScan,
    BtPair(String, String),
    BtUnpair(String, String),
    /// « Ajouter une manette via Bluetooth » (catégorie Manettes) : bascule sur la catégorie Bluetooth.
    OpenBluetooth,
    InstallClient(String),
    Admin,
    Sgdb,
    Login(String),
    Logout(String),
}

pub enum Kb {
    Continue,
    Cancel,
    Done(String, String),
}

#[derive(Default)]
struct State {
    devices: Vec<Device>,
    modes: Vec<Mode>,
    res: Vec<(u32, u32)>,
    ri: usize,
    hzs: Vec<u32>,
    hi: usize,
    /// Mode précédent et secondes restantes avant retour automatique.
    revert: Option<(Mode, u32)>,
    loading: bool,
    scanning: bool,
    data: Fetched,
    bt_found: Vec<Dev>,
    kb_text: String,
    kb_shift: bool,
    kb_acc: bool,
    kb_ssid: String,
    message: String,
}

pub struct Settings {
    pub rows: Rc<VecModel<SettingRow>>,
    state: RefCell<State>,
}

fn row(label: &str, value: String, adjustable: bool) -> SettingRow {
    SettingRow { label: tr(label).into(), value: value.into(), adjustable }
}

fn step(i: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        0
    } else {
        (i as i32 + delta).rem_euclid(len as i32) as usize
    }
}

fn onoff(v: Option<bool>) -> String {
    match v {
        Some(true) => tr("Activé"),
        Some(false) => tr("Coupé"),
        None => tr("Indisponible"),
    }
}

fn minutes(m: Option<u32>) -> String {
    match m {
        Some(0) => tr("Jamais"),
        Some(m) if m >= 60 => format!("{} {}", m / 60, tr("h")),
        Some(m) => format!("{m} {}", tr("min")),
        None => "—".into(),
    }
}

/// Choix suivant/précédent. Valeur réglée ailleurs (Windows) absente de la liste : on part du choix
/// juste au-dessus ou juste en dessous d'elle, pas du début de la liste.
fn cycle(choices: &[u32], cur: Option<u32>, delta: i32) -> u32 {
    let Some(c) = cur else { return choices[0] };
    if let Some(i) = choices.iter().position(|&x| x == c) {
        return choices[step(i, choices.len(), delta.signum())];
    }
    // « 0 » (jamais) est rangé en tête mais vaut l'infini : comparaison sur les seules durées.
    let timed: Vec<u32> = choices.iter().copied().filter(|&x| x > 0).collect();
    let pick = if c == 0 || delta > 0 { timed.iter().copied().find(|&x| x > c) } else { timed.iter().rev().copied().find(|&x| x < c) };
    pick.unwrap_or(if delta > 0 { 0 } else { timed[0] })
}

impl State {
    fn load_display(&mut self) {
        self.modes = display::modes();
        self.res = vec![];
        for m in &self.modes {
            if !self.res.contains(&(m.w, m.h)) {
                self.res.push((m.w, m.h));
            }
        }
        let cur = display::current_mode().unwrap_or(Mode { w: 0, h: 0, hz: 0 });
        self.ri = self.res.iter().position(|&r| r == (cur.w, cur.h)).unwrap_or(0);
        self.load_hzs(cur.hz);
    }

    fn load_hzs(&mut self, prefer: u32) {
        let (w, h) = self.res.get(self.ri).copied().unwrap_or((0, 0));
        self.hzs = self.modes.iter().filter(|m| (m.w, m.h) == (w, h)).map(|m| m.hz).collect();
        self.hi = self.hzs.iter().position(|&z| z == prefer).unwrap_or(0);
    }

    fn chosen(&self) -> Option<Mode> {
        let (w, h) = *self.res.get(self.ri)?;
        Some(Mode { w, h, hz: *self.hzs.get(self.hi)? })
    }
}

impl Settings {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { rows: Rc::new(VecModel::default()), state: RefCell::new(State::default()) })
    }

    /// Prépare la catégorie (données rapides rechargées, lentes marquées « en cours »).
    pub fn open(&self, cat: i32) -> String {
        {
            let mut s = self.state.borrow_mut();
            s.message.clear();
            match cat {
                0 => s.devices = audio::devices(),
                1 if s.revert.is_none() => s.load_display(),
                3 => {
                    s.bt_found.clear();
                    s.loading = true;
                }
                c if needs_fetch(c) => s.loading = true,
                _ => {}
            }
        }
        self.refresh(cat)
    }

    pub fn apply(&self, f: Fetched) {
        let mut s = self.state.borrow_mut();
        s.data = f;
        s.loading = false;
    }

    pub fn set_message(&self, m: String) {
        self.state.borrow_mut().message = m;
    }

    pub fn start_loading(&self) {
        self.state.borrow_mut().loading = true;
    }

    /// Résultat d'une recherche d'appareils Bluetooth à proximité.
    pub fn set_bt_found(&self, found: Vec<Dev>) {
        let mut s = self.state.borrow_mut();
        s.scanning = false;
        s.bt_found = found;
    }

    /// Un appareil trouvé pendant la recherche, ajouté dès sa découverte (pas à la fin de la recherche).
    pub fn bt_found_push(&self, d: Dev) {
        let mut s = self.state.borrow_mut();
        if !s.bt_found.iter().any(|o| o.id == d.id) {
            s.bt_found.push(d);
        }
    }

    /// Fin de la recherche (délai écoulé) : arrête l'indication « Recherche en cours… ».
    pub fn bt_scan_done(&self) {
        self.state.borrow_mut().scanning = false;
    }

    /// Reconstruit les lignes ; renvoie le texte d'aide.
    pub fn refresh(&self, cat: i32) -> String {
        let s = self.state.borrow();
        let mut hint = s.message.clone();
        let rows = match cat {
            0 => vec![
                row("Sortie audio", s.devices.iter().find(|d| d.default).map_or("—".into(), |d| d.name.clone()), s.devices.len() > 1),
                row("Volume", audio::volume().map_or("—".into(), |v| format!("{v} %")), audio::volume().is_some()),
                row("Volume des effets", format!("{} %", i18n::sfx_vol()), true),
                row("Volume de la musique", format!("{} %", i18n::music_vol()), true),
                row("Musique d'ambiance", onoff(Some(i18n::music())), false),
                row("Effets sonores", onoff(Some(i18n::sfx())), false),
                row("Pack de sons", crate::sounds::label(false), false),
                row("Pack de musique", crate::sounds::label(true), false),
            ],
            1 => match &s.revert {
                Some((_, secs)) => {
                    hint = format!("{} {secs} s", tr("Retour automatique à l'ancien mode dans"));
                    vec![row("Garder ce réglage", format!("{secs} s"), false), row("Annuler", String::new(), false)]
                }
                None => {
                    if let Some(c) = display::current_mode().filter(|_| hint.is_empty()) {
                        hint = format!("{} {} × {} @ {} Hz", tr("Mode actuel :"), c.w, c.h, c.hz);
                    }
                    vec![
                        row("Résolution", s.res.get(s.ri).map_or("—".into(), |r| format!("{} × {}", r.0, r.1)), s.res.len() > 1),
                        row("Fréquence", s.hzs.get(s.hi).map_or("—".into(), |z| format!("{z} Hz")), s.hzs.len() > 1),
                        row("Appliquer", String::new(), false),
                        row("Animations", onoff(Some(i18n::anim())), false),
                    ]
                }
            },
            2 if s.loading => vec![row("Recherche des réseaux…", String::new(), false)],
            2 => {
                if let Some(l) = &s.data.link {
                    let kind = if l.wifi { "Wi-Fi" } else { "Ethernet" };
                    hint = format!("{} : {kind} — {} {}", tr("Connecté"), l.name, l.ip);
                } else if !s.data.wifi_err.is_empty() && hint.is_empty() {
                    hint = if s.data.nets.is_empty() { tr("Windows exige l'autorisation de localisation pour lister les réseaux Wi-Fi.") } else { s.data.wifi_err.clone() };
                }
                let mut v = vec![row("Wi-Fi", onoff(s.data.wifi_on), false)];
                v.extend(s.data.nets.iter().map(|n| {
                    let bars = "●".repeat(n.bars.min(4) as usize) + &"○".repeat(4 - n.bars.min(4) as usize);
                    let tag = if n.connected {
                        tr("Connecté") + "   "
                    } else if n.secured {
                        tr("Sécurisé") + "   "
                    } else {
                        String::new()
                    };
                    row(&n.ssid, format!("{tag}{bars}"), false)
                }));
                v
            }
            3 if s.loading => vec![row("Lecture…", String::new(), false)],
            3 => {
                if hint.is_empty() {
                    hint = tr("Valider sur un appareil appairé pour l'oublier ; sur un appareil trouvé pour l'appairer.");
                }
                let mut v = vec![row("Bluetooth", onoff(s.data.bt_on), false)];
                v.extend(s.data.bt_paired.iter().map(|d| row(&d.name, if d.connected { tr("Connecté") } else { tr("Appairé") }, false)));
                v.push(row("Rechercher des appareils", if s.scanning { tr("Recherche en cours…") } else { String::new() }, false));
                v.extend(s.bt_found.iter().map(|d| row(&d.name, tr("À proximité"), false)));
                v
            }
            4 if s.loading => vec![row("Lecture…", String::new(), false)],
            4 => {
                if s.data.pads.len() > 1 {
                    hint = tr("‹ › pour changer l'ordre : la première est la manette transmise aux jeux.");
                }
                let mut v: Vec<SettingRow> = if s.data.pads.is_empty() {
                    vec![row("Aucune manette détectée", String::new(), false)]
                } else {
                    s.data.pads.iter().map(|p| row(&p.name, p.battery.map_or(tr("Connectée"), |b| format!("{} · {b} %", tr("Connectée"))), s.data.pads.len() > 1)).collect()
                };
                v.push(row("Ajouter une manette via Bluetooth", String::new(), false));
                v
            }
            13 => {
                hint = tr("Le thème s'applique tout de suite, à tout Hearth.");
                vec![
                    row("Préréglage", theme_name(theme::preset()), true),
                    row("Teinte de l'accent", format!("{}°", theme::hue()), true),
                    row("Fond", tr(theme::TONES[theme::tone() as usize].0), true),
                    row("Réinitialiser le thème", String::new(), false),
                ]
            }
            5 => vec![row("Langue", i18n::LANGS[i18n::lang()].1.into(), true), row("Disposition du clavier", crate::keyboard::layout().name.into(), true)],
            6 => vec![row("Windows Update", if s.loading { tr("Recherche en cours…") } else if s.data.update.is_empty() { tr("Rechercher des mises à jour") } else { s.data.update.clone() }, false)],
            7 if s.loading => vec![row("Lecture…", String::new(), false)],
            7 => s.data.drives.iter().map(|d| row(&d.name, format!("{} {} {}{}", gb(d.free), tr("libres sur"), gb(d.total), if d.removable { " (USB)" } else { "" }), false)).collect(),
            8 if s.loading => vec![row("Détection du matériel…", String::new(), false)],
            8 => {
                let mut v: Vec<SettingRow> = s.data.gpus.iter().map(|g| row(&g.name, format!("{} {} {}", g.vendor, tr("pilote"), g.version), false)).collect();
                v.push(row("Pilotes Windows Update", if s.data.update_drv.is_empty() { tr("Rechercher des mises à jour") } else { s.data.update_drv.clone() }, false));
                v
            }
            9 if s.loading => vec![row("Lecture…", String::new(), false)],
            9 => vec![
                row("Mode d'alimentation", s.data.plans.iter().find(|p| p.active).map_or("—".into(), |p| p.name.clone()), s.data.plans.len() > 1),
                row("Mise en veille après", minutes(s.data.sleep_min), true),
                row("Extinction de l'écran après", minutes(s.data.screen_min), true),
            ],
            10 => vec![
                row("Afficher les bannières", onoff(Some(i18n::toasts())), false),
            ],
            12 if s.loading => vec![row("Lecture…", String::new(), false)],
            12 => {
                if hint.is_empty() {
                    hint = tr("Connexion par QR code. Le client de la boutique s'installe tout seul quand il faut installer un jeu.");
                }
                account_rows(&s.data.accounts).into_iter().map(|(l, v, _)| row(&l, v, false)).collect()
            }
            11 if s.loading => vec![row("Lecture…", String::new(), false)],
            11 => {
                let mut v: Vec<SettingRow> = s.data.about.iter().map(|(k, val)| row(k, val.clone(), false)).collect();
                v.push(row("Hearth", env!("HEARTH_VERSION").into(), false));
                v.push(row("Assistant de démarrage", tr("Revoir"), false));
                v.push(row("Ouvrir la version bureau", String::new(), false));
                v.push(row("Importer mes jeux", String::new(), false));
                v
            }
            _ => vec![],
        };
        self.rows.set_vec(rows);
        hint
    }

    /// Valide (delta 0) ou ajuste une ligne.
    pub fn act(&self, cat: i32, row: i32, delta: i32) -> Outcome {
        let mut out = Outcome::None;
        let mut s = self.state.borrow_mut();
        match (cat, row, delta) {
            (0, 0, d) if d != 0 && s.devices.len() > 1 => {
                let i = s.devices.iter().position(|x| x.default).unwrap_or(0);
                audio::set_default(&s.devices[step(i, s.devices.len(), d)].id);
                s.devices = audio::devices();
            }
            (0, 1, d) if d != 0 => {
                if let Some(v) = audio::volume() {
                    audio::set_volume((v as i32 + d * 5).clamp(0, 100) as u32);
                }
            }
            (0, 2, d) if d != 0 => i18n::set_sfx_vol((i18n::sfx_vol() as i32 + d * 10).clamp(0, 100) as u32),
            (0, 3, d) if d != 0 => i18n::set_music_vol((i18n::music_vol() as i32 + d * 10).clamp(0, 100) as u32),
            (0, 4, 0) => i18n::set_music(!i18n::music()),
            (4, r, d) if d != 0 && s.data.pads.len() > 1 && (r as usize) < s.data.pads.len() => {
                let r = r as usize;
                let n = step(r, s.data.pads.len(), d);
                s.data.pads.swap(r, n);
                let ids: Vec<String> = s.data.pads.iter().map(|p| p.id.clone()).collect();
                set_pad_order(&ids);
            }
            (4, r, 0) if r as usize == s.data.pads.len().max(1) => out = Outcome::OpenBluetooth,
            (0, 5, 0) => i18n::set_sfx(!i18n::sfx()),
            (0, 6, 0) => out = Outcome::SoundPicker(false),
            (0, 7, 0) => out = Outcome::SoundPicker(true),
            (13, 0, d) if d != 0 => {
                theme::set_preset(theme::preset() as i32 + d);
                out = Outcome::ThemeChanged;
            }
            (13, 1, d) if d != 0 => {
                theme::shift_hue(d * 10);
                out = Outcome::ThemeChanged;
            }
            (13, 2, d) if d != 0 => {
                theme::set_tone(theme::tone() as i32 + d);
                out = Outcome::ThemeChanged;
            }
            (11, r, 0) if r as usize == s.data.about.len() + 1 => out = Outcome::Oobe,
            (11, r, 0) if r as usize == s.data.about.len() + 2 => out = Outcome::Desktop,
            (11, r, 0) if r as usize == s.data.about.len() + 3 => out = Outcome::Import,
            (13, 3, 0) => {
                theme::reset();
                out = Outcome::ThemeChanged;
            }
            (1, r, d) if s.revert.is_none() => match (r, d) {
                (0, d) if d != 0 => {
                    s.ri = step(s.ri, s.res.len(), d);
                    let prefer = s.hzs.get(s.hi).copied().unwrap_or(0);
                    s.load_hzs(prefer);
                }
                (1, d) if d != 0 => s.hi = step(s.hi, s.hzs.len(), d),
                (2, 0) => {
                    if let (Some(prev), Some(new)) = (display::current_mode(), s.chosen()) {
                        if prev == new {
                            s.message = tr("Déjà le mode actuel");
                        } else if display::set_mode(new) {
                            s.revert = Some((prev, CONFIRM_SECS));
                            out = Outcome::DisplayChanged;
                        }
                    } else {
                        // Windows ne propose aucun mode (Bureau à distance, pilote générique…).
                        s.message = tr("Aucun autre mode disponible");
                    }
                }
                (3, 0) => i18n::set_anim(!i18n::anim()),
                _ => {}
            },
            (1, 0, 0) => s.revert = None, // garder
            (1, 1, 0) => {
                if let Some((prev, _)) = s.revert.take() {
                    display::set_mode(prev);
                    s.load_display();
                    out = Outcome::DisplayChanged;
                }
            }
            (2, 0, 0) if !s.loading => {
                if let Some(on) = s.data.wifi_on {
                    out = Outcome::Toggle { wifi: true, on: !on };
                }
            }
            (2, r, 0) if r >= 1 && !s.loading => {
                if let Some(n) = s.data.nets.get(r as usize - 1) {
                    if !n.connected {
                        out = if n.secured { Outcome::Password(n.ssid.clone()) } else { Outcome::Connect(n.ssid.clone()) };
                    }
                }
            }
            (3, 0, 0) if !s.loading => {
                if let Some(on) = s.data.bt_on {
                    out = Outcome::Toggle { wifi: false, on: !on };
                }
            }
            (3, r, 0) if r >= 1 && !s.loading => {
                let r = r as usize - 1;
                let paired = s.data.bt_paired.len();
                if r < paired {
                    let d = &s.data.bt_paired[r];
                    out = Outcome::BtUnpair(d.id.clone(), d.name.clone());
                } else if r == paired {
                    if !s.scanning {
                        s.scanning = true;
                        out = Outcome::BtScan;
                    }
                } else if let Some(d) = s.bt_found.get(r - paired - 1) {
                    // Appareil trouvé pendant la recherche : elle est arrêtée d'abord (voir BtPair).
                    out = Outcome::BtPair(d.id.clone(), d.name.clone());
                }
            }
            (5, 0, d) if d != 0 => {
                i18n::set(step(i18n::lang(), i18n::LANGS.len(), d), true);
                crate::keyboard::follow_language();
                out = Outcome::LangChanged;
            }
            (5, 1, d) if d != 0 => {
                crate::keyboard::shift(d);
                out = Outcome::LangChanged;
            }
            (6, 0, 0) if !s.loading => {
                s.loading = true;
                out = Outcome::CheckUpdates(false);
            }
            (8, r, 0) if !s.loading && r as usize == s.data.gpus.len() => {
                s.loading = true;
                out = Outcome::CheckUpdates(true);
            }
            // `powercfg` (2 à 3 processus) hors du fil de l'interface : la valeur choisie s'affiche tout de
            // suite, celle réellement retenue par Windows la remplace dès qu'elle est relue.
            (9, 0, d) if d != 0 && s.data.plans.len() > 1 => {
                let i = s.data.plans.iter().position(|p| p.active).unwrap_or(0);
                let n = step(i, s.data.plans.len(), d);
                let guid = s.data.plans[n].guid.clone();
                for (k, p) in s.data.plans.iter_mut().enumerate() {
                    p.active = k == n;
                }
                power_bg(move || system::power::set_plan(&guid));
            }
            (9, 1, d) if d != 0 => {
                let m = cycle(&SLEEP_CHOICES, s.data.sleep_min, d);
                s.data.sleep_min = Some(m);
                power_bg(move || system::power::set_timeout(true, m));
            }
            (9, 2, d) if d != 0 => {
                let m = cycle(&SCREEN_CHOICES, s.data.screen_min, d);
                s.data.screen_min = Some(m);
                power_bg(move || system::power::set_timeout(false, m));
            }
            (10, 0, 0) => i18n::set_toasts(!i18n::toasts()),
            (12, r, 0) if !s.loading => {
                out = match account_rows(&s.data.accounts).into_iter().nth(r as usize).map(|(_, _, a)| a) {
                    Some(Act::Install(n)) => Outcome::InstallClient(n),
                    Some(Act::Admin) => Outcome::Admin,
                    Some(Act::Sgdb) => Outcome::Sgdb,
                    Some(Act::Login(n)) => Outcome::Login(n),
                    Some(Act::Logout(n)) => Outcome::Logout(n),
                    _ => Outcome::None,
                };
            }
            _ => {}
        }
        out
    }

    /// Chaque seconde : compte à rebours de confirmation. Vrai si l'affichage a changé.
    pub fn tick(&self) -> bool {
        let mut s = self.state.borrow_mut();
        let Some((prev, secs)) = s.revert else { return false };
        if secs > 1 {
            s.revert = Some((prev, secs - 1));
            return false;
        }
        s.revert = None;
        display::set_mode(prev);
        s.load_display();
        true
    }

    pub fn confirming(&self) -> bool {
        self.state.borrow().revert.is_some()
    }

    pub fn row_count(&self) -> usize {
        self.rows.row_count()
    }

    // --- Clavier virtuel ---

    pub fn kb_open(&self, ssid: &str) {
        let mut s = self.state.borrow_mut();
        s.kb_text.clear();
        s.kb_shift = false;
        s.kb_acc = false;
        s.kb_ssid = ssid.into();
    }

    /// Ouvre le clavier avec un texte de départ (renommer un jeu).
    pub fn kb_open_text(&self, text: &str) {
        self.kb_open("");
        self.state.borrow_mut().kb_text = text.to_string();
    }

    pub fn kb_text(&self) -> String {
        self.state.borrow().kb_text.clone()
    }

    /// Réseau Wi-Fi dont on saisit le mot de passe (vide pour toute autre saisie).
    pub fn kb_ssid(&self) -> String {
        self.state.borrow().kb_ssid.clone()
    }

    pub fn kb_acc(&self) -> bool {
        self.state.borrow().kb_acc
    }

    pub fn kb_keys(&self) -> Vec<String> {
        let (up, acc) = {
            let s = self.state.borrow();
            (s.kb_shift, s.kb_acc)
        };
        let l = crate::keyboard::layout();
        let rows: [&str; 4] = if acc { l.alt } else { l.main };
        rows.iter().flat_map(|r| r.chars()).map(|c| if up { c.to_uppercase().to_string() } else { c.to_string() }).collect()
    }

    pub fn kb_press(&self, x: i32, y: i32) -> Kb {
        if x < 0 {
            return Kb::Cancel;
        }
        let keys = self.kb_keys();
        let mut s = self.state.borrow_mut();
        if y < 4 {
            if let Some(k) = keys.get((y * 10 + x) as usize) {
                s.kb_text.push_str(k);
            }
            return Kb::Continue;
        }
        match x {
            0 => s.kb_shift = !s.kb_shift,
            1 => s.kb_acc = !s.kb_acc,
            2 => s.kb_text.push(' '),
            3 => {
                s.kb_text.pop();
            }
            4 => return Kb::Done(s.kb_ssid.clone(), s.kb_text.clone()),
            _ => return Kb::Cancel,
        }
        Kb::Continue
    }

    /// Clavier physique : un caractère, "\b" pour effacer, "\n" pour valider.
    pub fn kb_char(&self, t: &str) -> Kb {
        let mut s = self.state.borrow_mut();
        match t {
            "\n" | "\r" => return Kb::Done(s.kb_ssid.clone(), s.kb_text.clone()),
            "\u{8}" => {
                s.kb_text.pop();
            }
            _ => {
                // Les touches de fonction arrivent en caractères privés (U+F700…) : ignorées.
                for c in t.chars().filter(|c| !c.is_control() && !('\u{f700}'..='\u{f8ff}').contains(c)) {
                    s.kb_text.push(c);
                }
            }
        }
        Kb::Continue
    }

    /// Résultat d'une recherche Windows Update (appelé sur le thread UI).
    pub fn set_update(&self, drivers: bool, r: Result<i32, String>) {
        let mut s = self.state.borrow_mut();
        s.loading = false;
        let text = match r {
            Ok(0) => tr("À jour"),
            Ok(n) => format!("{n} {}", tr("disponible(s)")),
            Err(e) => e,
        };
        if drivers {
            s.data.update_drv = text;
        } else {
            s.data.update = text;
        }
    }
}

/// Applique un réglage d'alimentation en arrière-plan puis relit les délais réellement en place.
fn power_bg(apply: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        apply();
        let (sleep, screen) = (system::power::timeout(true), system::power::timeout(false));
        let _ = slint::invoke_from_event_loop(move || {
            crate::with_shell(|sh| {
                {
                    let mut s = sh.settings.state.borrow_mut();
                    s.data.sleep_min = sleep.or(s.data.sleep_min);
                    s.data.screen_min = screen.or(s.data.screen_min);
                }
                if sh.ui.get_cat() == 9 {
                    sh.open_cat_keep(9);
                }
            })
        });
    });
}

fn gb(bytes: u64) -> String {
    format!("{} {}", bytes / 1_000_000_000, tr("Go"))
}

/// Action associée à une ligne de la catégorie « Comptes ».
enum Act {
    Install(String),
    Admin,
    Sgdb,
    Login(String),
    Logout(String),
    None,
}

/// Lignes (libellé, valeur, action) : un compte par boutique, et l'installation du client s'il manque.
fn account_rows(accounts: &[(String, bool, Option<String>)]) -> Vec<(String, String, Act)> {
    let mut v = vec![];
    for (name, detected, session) in accounts {
        if name == "@sgdb" {
            v.push((tr("Visuels SteamGridDB (clé API)"), tr(if *detected { "Configurée" } else { "Non configurée" }), Act::Sgdb));
            continue;
        }
        if name == "@admin" {
            let value = if *detected { tr("Activée") } else { tr("Activer (une seule fois)") };
            v.push((tr("Autorisation administrateur"), value, if *detected { Act::None } else { Act::Admin }));
            continue;
        }
        if name == "GOG" {
            let value = if *detected { tr("Bibliothèque locale") } else { tr("Client non installé") };
            v.push((name.clone(), value, Act::None));
        } else if let Some(user) = session {
            v.push((name.clone(), format!("{} · {user}", tr("Connecté")), Act::Logout(name.clone())));
        } else {
            v.push((name.clone(), tr("Se connecter"), Act::Login(name.clone())));
        }
        if !*detected {
            v.push((format!("{} {name}", tr("Installer le client")), tr("Automatique (winget)"), Act::Install(name.clone())));
        }
    }
    v
}

fn theme_name(i: u32) -> String {
    match theme::PRESETS.get(i as usize) {
        Some((name, _)) => tr(name),
        None => tr("Personnalisé"),
    }
}
