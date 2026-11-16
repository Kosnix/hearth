//! Écran Réglages : état et actions (la mise en page est dans app.slint).

use std::cell::RefCell;
use std::rc::Rc;

use slint::{Model, VecModel};
use system::audio::{self, Device};
use system::display::{self, Mode};

use crate::SettingRow;

pub const CATS: [&str; 2] = ["Son", "Affichage"];
const CONFIRM_SECS: u32 = 15;

#[derive(Default)]
pub struct State {
    devices: Vec<Device>,
    modes: Vec<Mode>,
    res: Vec<(u32, u32)>,
    ri: usize,
    hzs: Vec<u32>,
    hi: usize,
    /// Mode précédent et secondes restantes avant retour automatique.
    revert: Option<(Mode, u32)>,
}

pub struct Settings {
    pub rows: Rc<VecModel<SettingRow>>,
    state: RefCell<State>,
}

fn row(label: &str, value: String, adjustable: bool) -> SettingRow {
    SettingRow { label: label.into(), value: value.into(), adjustable }
}

fn step(i: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        0
    } else {
        (i as i32 + delta).rem_euclid(len as i32) as usize
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

    /// Recharge les données de la catégorie (à l'ouverture) puis les lignes.
    pub fn open(&self, cat: i32) -> String {
        {
            let mut s = self.state.borrow_mut();
            match cat {
                0 => s.devices = audio::devices(),
                1 if s.revert.is_none() => s.load_display(),
                _ => {}
            }
        }
        self.refresh(cat)
    }

    /// Reconstruit les lignes ; renvoie le texte d'aide.
    pub fn refresh(&self, cat: i32) -> String {
        let s = self.state.borrow();
        let mut hint = String::new();
        let rows = match cat {
            0 => vec![
                row("Sortie audio", s.devices.iter().find(|d| d.default).map_or("—".into(), |d| d.name.clone()), s.devices.len() > 1),
                row("Volume", audio::volume().map_or("—".into(), |v| format!("{v} %")), audio::volume().is_some()),
            ],
            1 => match &s.revert {
                Some((_, secs)) => {
                    hint = format!("Retour automatique à l'ancien mode dans {secs} s");
                    vec![row("Garder ce réglage", format!("{secs} s"), false), row("Annuler", String::new(), false)]
                }
                None => {
                    if let Some(c) = display::current_mode() {
                        hint = format!("Mode actuel : {} × {} à {} Hz", c.w, c.h, c.hz);
                    }
                    vec![
                        row("Résolution", s.res.get(s.ri).map_or("—".into(), |r| format!("{} × {}", r.0, r.1)), s.res.len() > 1),
                        row("Fréquence", s.hzs.get(s.hi).map_or("—".into(), |z| format!("{z} Hz")), s.hzs.len() > 1),
                        row("Appliquer", String::new(), false),
                    ]
                }
            },
            _ => vec![],
        };
        self.rows.set_vec(rows);
        hint
    }

    /// Valide (delta 0) ou ajuste une ligne. Renvoie vrai si l'affichage a changé de mode.
    pub fn act(&self, cat: i32, row: i32, delta: i32) -> bool {
        let mut changed = false;
        {
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
                                changed = true;
                            }
                        }
                    }
                    _ => {}
                },
                (1, 0, 0) => s.revert = None, // garder
                (1, 1, 0) => {
                    if let Some((prev, _)) = s.revert.take() {
                        display::set_mode(prev);
                        s.load_display();
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        changed
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
}
