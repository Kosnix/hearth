//! Écran Réglages : état et actions (la mise en page est dans app.slint).

use std::cell::RefCell;
use std::rc::Rc;

use slint::{Model, VecModel};
use system::audio::{self, Device};
use system::display::{self, Mode};
use system::wifi::Net;

use crate::i18n::{self, tr};
use crate::SettingRow;

pub fn cats() -> Vec<String> {
    ["Son", "Affichage", "Wi-Fi", "Bluetooth", "Manettes", "Langue", "Mises à jour", "Stockage", "Pilotes"].iter().map(|c| tr(c)).collect()
}
const CONFIRM_SECS: u32 = 15;

/// Données lentes (WinRT), lues hors du thread UI.
#[derive(Default)]
pub struct Fetched {
    wifi_on: Option<bool>,
    nets: Vec<Net>,
    bt_on: Option<bool>,
    pads: Vec<String>,
    wifi_err: String,
    drives: Vec<system::storage::Drive>,
    update: String,
    update_drv: String,
    gpus: Vec<system::drivers::Gpu>,
}

pub fn fetch(cat: i32) -> Fetched {
    match cat {
        2 => {
            let nets = system::wifi::scan();
            Fetched { wifi_on: system::radio::wifi_on(), nets, wifi_err: system::wifi::last_error(), ..Default::default() }
        }
        3 => Fetched { bt_on: system::radio_ext::bluetooth_on(), ..Default::default() },
        4 => Fetched { pads: system::pads::names(), ..Default::default() },
        7 => Fetched { drives: system::storage::drives(), ..Default::default() },
        8 => Fetched { gpus: system::drivers::gpus(), ..Default::default() },
        _ => Fetched::default(),
    }
}

pub fn needs_fetch(cat: i32) -> bool {
    (2..=4).contains(&cat) || cat == 7 || cat == 8
}

/// Ce que le shell doit faire après une action.
pub enum Outcome {
    None,
    DisplayChanged,
    Toggle { wifi: bool, on: bool },
    Password(String),
    Connect(String),
    LangChanged,
    CheckUpdates(bool),
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
    data: Fetched,
    kb_text: String,
    kb_shift: bool,
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

    /// Reconstruit les lignes ; renvoie le texte d'aide.
    pub fn refresh(&self, cat: i32) -> String {
        let s = self.state.borrow();
        let mut hint = s.message.clone();
        let rows = match cat {
            0 => vec![
                row("Sortie audio", s.devices.iter().find(|d| d.default).map_or("—".into(), |d| d.name.clone()), s.devices.len() > 1),
                row("Volume", audio::volume().map_or("—".into(), |v| format!("{v} %")), audio::volume().is_some()),
            ],
            1 => match &s.revert {
                Some((_, secs)) => {
                    hint = format!("{} {secs} s", tr("Retour automatique à l'ancien mode dans"));
                    vec![row("Garder ce réglage", format!("{secs} s"), false), row("Annuler", String::new(), false)]
                }
                None => {
                    if let Some(c) = display::current_mode() {
                        hint = format!("{} {} × {} @ {} Hz", tr("Mode actuel :"), c.w, c.h, c.hz);
                    }
                    vec![
                        row("Résolution", s.res.get(s.ri).map_or("—".into(), |r| format!("{} × {}", r.0, r.1)), s.res.len() > 1),
                        row("Fréquence", s.hzs.get(s.hi).map_or("—".into(), |z| format!("{z} Hz")), s.hzs.len() > 1),
                        row("Appliquer", String::new(), false),
                        row("Fond animé", onoff(Some(i18n::anim())), false),
                    ]
                }
            },
            2 if s.loading => vec![row("Recherche des réseaux…", String::new(), false)],
            2 => {
                if !s.data.wifi_err.is_empty() && hint.is_empty() {
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
            3 => vec![row("Bluetooth", onoff(s.data.bt_on), false)],
            4 if s.loading => vec![row("Lecture…", String::new(), false)],
            4 if s.data.pads.is_empty() => vec![row("Aucune manette détectée", String::new(), false)],
            4 => s.data.pads.iter().map(|p| row(p, tr("Connectée"), false)).collect(),
            5 => vec![row("Langue", i18n::LANGS[i18n::lang()].1.into(), true)],
            6 => vec![row("Windows Update", if s.loading { tr("Recherche en cours…") } else if s.data.update.is_empty() { tr("Rechercher des mises à jour") } else { s.data.update.clone() }, false)],
            7 if s.loading => vec![row("Lecture…", String::new(), false)],
            8 if s.loading => vec![row("Détection du matériel…", String::new(), false)],
            8 => {
                let mut v: Vec<SettingRow> = s.data.gpus.iter().map(|g| row(&g.name, format!("{} {} {}", g.vendor, tr("pilote"), g.version), false)).collect();
                v.push(row("Pilotes Windows Update", if s.data.update_drv.is_empty() { tr("Rechercher des mises à jour") } else { s.data.update_drv.clone() }, false));
                v
            }
            7 => s.data.drives.iter().map(|d| row(&d.name, format!("{} {} {} {}", gb(d.free), tr("libres sur"), gb(d.total), if d.removable { "(USB)" } else { "" }), false)).collect(),
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
            (1, r, d) if s.revert.is_none() => match (r, d) {
                (0, d) if d != 0 => {
                    s.ri = step(s.ri, s.res.len(), d);
                    let prefer = s.hzs.get(s.hi).copied().unwrap_or(0);
                    s.load_hzs(prefer);
                }
                (1, d) if d != 0 => s.hi = step(s.hi, s.hzs.len(), d),
                (2, 0) => {
                    if let (Some(prev), Some(new)) = (display::current_mode(), s.chosen()) {
                        if prev != new && display::set_mode(new) {
                            s.revert = Some((prev, CONFIRM_SECS));
                            out = Outcome::DisplayChanged;
                        }
                    }
                }
                _ => {}
            },
            (1, 3, 0) if s.revert.is_none() => i18n::set_anim(!i18n::anim()),
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
            (5, 0, d) if d != 0 => {
                i18n::set(step(i18n::lang(), i18n::LANGS.len(), d), true);
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
            (3, 0, 0) if !s.loading => {
                if let Some(on) = s.data.bt_on {
                    out = Outcome::Toggle { wifi: false, on: !on };
                }
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
        s.kb_ssid = ssid.into();
    }

    pub fn kb_text(&self) -> String {
        self.state.borrow().kb_text.clone()
    }

    pub fn kb_keys(&self) -> Vec<String> {
        let up = self.state.borrow().kb_shift;
        ["1234567890", "azertyuiop", "qsdfghjklm", "wxcvbn.-_@"]
            .iter()
            .flat_map(|r| r.chars())
            .map(|c| if up { c.to_uppercase().to_string() } else { c.to_string() })
            .collect()
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
            1 => s.kb_text.push(' '),
            2 => {
                s.kb_text.pop();
            }
            3 => return Kb::Done(s.kb_ssid.clone(), s.kb_text.clone()),
            _ => return Kb::Cancel,
        }
        Kb::Continue
    }
}

fn gb(bytes: u64) -> String {
    format!("{} {}", bytes / 1_000_000_000, tr("Go"))
}

impl Settings {
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
