//! Manette -> actions de navigation. Windows.Gaming.Input + XInput (bouton guide, repli).

use std::sync::atomic::{AtomicU8, Ordering};
use std::thread;
use std::time::{Duration, Instant};
mod hidpad;
mod mouse;
mod xpad;

pub fn set_capture(on: bool) {
    xpad::set_capture(on);
}

pub use mouse::{set_chord, active as mouse_active, osk as osk_open, backspace, enter, send_text, set_active as set_mouse_active, set_osk};

use windows::core::{w, PCSTR};
use windows::Gaming::Input::{GameControllerSwitchPosition as Hat, Gamepad, GamepadButtons, RawGameController};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE, WM_HOTKEY};

/// L'ordre (valeurs entières) est partagé avec l'UI Slint (`nav(int)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Back,
    Guide,
    Menu,
    /// Hors manette de navigation : le mode souris a changé (L3 + R3) / le clavier visuel est demandé (Triangle en mode souris).
    Mouse,
    Osk,
    /// L1/LB, R1/RB : changement rapide de section (Jeux/Bibliothèque/Boutique) sans passer par la barre du haut.
    TabPrev,
    TabNext,
    /// L2/LT, R2/RT : volume moins / plus (géré par le shell, pas par l'UI).
    VolDown,
    VolUp,
    /// Triangle / Y : action secondaire de l'écran (ex. effacer la recherche de la Bibliothèque).
    Alt,
    /// Carré / X : effacer (clavier virtuel).
    Erase,
}

// Masques au format XInput (wButtons).
const UP: u16 = 0x0001;
const DOWN: u16 = 0x0002;
const LEFT: u16 = 0x0004;
const RIGHT: u16 = 0x0008;
const START: u16 = 0x0010;
const GUIDE: u16 = 0x0400;
const A: u16 = 0x1000;
const B: u16 = 0x2000;
// Mêmes valeurs que les bits LB/RB d'XInput (wButtons) : pas de conversion nécessaire pour la manette Xbox.
const L1: u16 = 0x0100;
const R1: u16 = 0x0200;
const Y: u16 = 0x8000;
const X: u16 = 0x4000;
// Gâchettes : pas de bit dans wButtons ; bits des clics de stick, que la navigation n'utilise pas
// (jamais relus tels quels depuis XInput, voir le masque de `XInput::read`).
const L2: u16 = 0x0040;
const R2: u16 = 0x0080;
const DIRS: u16 = UP | DOWN | LEFT | RIGHT;

const MAP: [(u16, Nav); 14] = [
    (UP, Nav::Up),
    (DOWN, Nav::Down),
    (LEFT, Nav::Left),
    (RIGHT, Nav::Right),
    (A, Nav::Accept),
    (B, Nav::Back),
    (GUIDE, Nav::Guide),
    (START, Nav::Menu),
    (L1, Nav::TabPrev),
    (R1, Nav::TabNext),
    (L2, Nav::VolDown),
    (R2, Nav::VolUp),
    (Y, Nav::Alt),
    (X, Nav::Erase),
];

/// L'événement en cours de traitement vient d'une direction maintenue (répétition), pas d'une pression :
/// lu par le rappel de `spawn`, sur le fil de la manette, au moment de l'appel.
static REPEATING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn repeating() -> bool {
    REPEATING.load(Ordering::Relaxed)
}

static FAST_REPEAT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Accélération de la répétition après 1,2 s de maintien (longues listes). À couper là où chaque pas
/// est grand (dock de jaquettes de l'accueil) : on y dépassait facilement le jeu visé.
pub fn set_fast_repeat(on: bool) {
    FAST_REPEAT.store(on, Ordering::Relaxed);
}

const POLL: Duration = Duration::from_millis(10);
const REPEAT_DELAY: Duration = Duration::from_millis(260);
const REPEAT_RATE: Duration = Duration::from_millis(80);
const REPEAT_FAST: Duration = Duration::from_millis(48);
const STICK: f64 = 0.5; // seuil de déviation du stick, 0..1

type GetState = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;

/// Lance le thread de lecture ; `on_nav` est appelé depuis ce thread.
pub fn spawn(on_nav: impl Fn(Nav) + Send + Sync + 'static) {
    let on_nav = std::sync::Arc::new(on_nav);
    hidpad::spawn();
    mouse::spawn(on_nav.clone());
    xpad::spawn();
    thread::spawn(move || {
        let on_nav = move |n: Nav| {
            stamp();
            (*on_nav)(n)
        };
        unsafe { RoInitialize(RO_INIT_MULTITHREADED).ok() };
        let get_state = xinput_get_state_ex().unwrap_or(xinput_get_state);
        let mut xinput = XInput { get_state, connected: [true; 4], tick: 0 };
        register_hotkeys();
        if sony_present() {
            LAST_PAD.store(1, Ordering::Relaxed);
        }
        let mut prev = 0u16;
        let mut next_repeat = Instant::now();
        let mut hold_start = Instant::now();
        let mut next_vol = Instant::now();
        loop {
            while let Some(nav) = poll_hotkey() {
                on_nav(nav);
            }
            // Avec une manette Sony, sa copie virtuelle Xbox (ViGEm) n'est pas relue : une seule source, pas de
            // double lecture. Seul son emplacement XInput est écarté — les vraies manettes Xbox branchées en
            // même temps restent utilisables (Windows.Gaming.Input, qui ne distingue pas la copie, est laissé de côté).
            let sony = read_sony();
            let other = if xpad::present() { xinput.read(xpad::virtual_slot()) } else { read_wgi() | xinput.read(None) };
            if sony != 0 {
                LAST_PAD.store(1, Ordering::Relaxed);
            } else if other != 0 {
                LAST_PAD.store(0, Ordering::Relaxed);
            }
            let mut cur = sony | other;
            // Mode souris (sans clavier visuel) : la manette pilote le curseur, seule la touche guide reste à Hearth.
            if mouse::active() && !mouse::osk() {
                cur &= GUIDE;
            }
            let pressed = cur & !prev;
            for (bit, nav) in MAP {
                if pressed & bit != 0 {
                    on_nav(nav);
                }
            }
            let now = Instant::now();
            // Un stick/croix qui reste bloqué dans une direction (lecture HID parasite, dérive) ne doit pas
            // faire défiler le menu indéfiniment : passé 6 s de maintien continu, la répétition s'arrête
            // jusqu'au relâchement (une vraie pression, elle, se relâche bien avant).
            let stuck = now.duration_since(hold_start) > Duration::from_secs(6);
            if pressed & DIRS != 0 {
                next_repeat = now + REPEAT_DELAY;
                hold_start = now;
            } else if cur & DIRS != 0 && now >= next_repeat && !stuck {
                REPEATING.store(true, Ordering::Relaxed);
                for (bit, nav) in &MAP[..4] {
                    if cur & bit != 0 {
                        on_nav(*nav);
                    }
                }
                REPEATING.store(false, Ordering::Relaxed);
                // Maintenu : la répétition accélère après 1,2 s (défilement rapide des longues listes).
                let fast = FAST_REPEAT.load(Ordering::Relaxed) && now.duration_since(hold_start) > Duration::from_millis(1200);
                next_repeat = now + if fast { REPEAT_FAST } else { REPEAT_RATE };
            }
            // L2/R2 maintenus : le volume continue de varier (un pas toutes les 150 ms).
            if pressed & (L2 | R2) != 0 {
                next_vol = now + Duration::from_millis(350);
            } else if cur & (L2 | R2) != 0 && now >= next_vol {
                REPEATING.store(true, Ordering::Relaxed);
                on_nav(if cur & R2 != 0 { Nav::VolUp } else { Nav::VolDown });
                REPEATING.store(false, Ordering::Relaxed);
                next_vol = now + Duration::from_millis(150);
            }
            prev = cur;
            thread::sleep(POLL);
        }
    });
}

/// Ctrl+Alt+Maj + F1..F8 (haut, bas, gauche, droite, valider, retour, guide, menu) = bouton de manette, quel que soit le focus : permet de piloter
/// et tester la console sans manette (scripts : SendKeys "^%+{UP}", "^%+g"…).
const HOTKEYS: [(VIRTUAL_KEY, Nav); 8] = [
    (VK_F1, Nav::Up),
    (VK_F2, Nav::Down),
    (VK_F3, Nav::Left),
    (VK_F4, Nav::Right),
    (VK_F5, Nav::Accept),
    (VK_F6, Nav::Back),
    (VK_F7, Nav::Guide),
    (VK_F8, Nav::Menu),
];

fn register_hotkeys() {
    // Test : HEARTH_DEV_TOPMOST décale les raccourcis sur F13..F20 (une autre instance peut garder F1..F8).
    let shift = if std::env::var_os("HEARTH_DEV_TOPMOST").is_some() { 12 } else { 0 };
    let mods = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
    for (i, (vk, _)) in HOTKEYS.iter().enumerate() {
        // Échec (raccourci déjà pris par une autre instance) : sans conséquence hors des tests.
        let _ = unsafe { RegisterHotKey(None, i as i32 + 1, mods, vk.0 as u32 + shift) };
    }
}

fn poll_hotkey() -> Option<Nav> {
    let mut msg = MSG::default();
    unsafe { PeekMessageW(&mut msg, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE) }
        .as_bool()
        .then(|| HOTKEYS.get(msg.wParam.0.wrapping_sub(1)).map(|(_, n)| *n))
        .flatten()
}

/// XInputGetStateEx (ordinal 100, non documenté) expose le bouton guide.
fn xinput_get_state_ex() -> Option<GetState> {
    unsafe {
        let dll = LoadLibraryW(w!("xinput1_4.dll")).ok()?;
        let f = GetProcAddress(dll, PCSTR(100 as *const u8))?;
        Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, GetState>(f))
    }
}

unsafe extern "system" fn xinput_get_state(i: u32, s: *mut XINPUT_STATE) -> u32 {
    XInputGetState(i, s)
}

fn read_wgi() -> u16 {
    let Ok(pads) = Gamepad::Gamepads() else { return 0 };
    let mut bits = 0;
    for pad in pads {
        let Ok(r) = pad.GetCurrentReading() else { continue };
        let b = r.Buttons;
        for (flag, bit) in [
            (GamepadButtons::DPadUp, UP),
            (GamepadButtons::DPadDown, DOWN),
            (GamepadButtons::DPadLeft, LEFT),
            (GamepadButtons::DPadRight, RIGHT),
            (GamepadButtons::A, A),
            (GamepadButtons::B, B),
            (GamepadButtons::Menu, START),
            (GamepadButtons::LeftShoulder, L1),
            (GamepadButtons::RightShoulder, R1),
            (GamepadButtons::Y, Y),
            (GamepadButtons::X, X),
        ] {
            if b.0 & flag.0 != 0 {
                bits |= bit;
            }
        }
        bits |= stick(r.LeftThumbstickX, r.LeftThumbstickY);
        if r.LeftTrigger > 0.5 {
            bits |= L2;
        }
        if r.RightTrigger > 0.5 {
            bits |= R2;
        }
    }
    bits
}

const SONY: u16 = 0x054C;

/// DualShock 4 / DualSense : boutons 1 Croix, 2 Rond, 9 Options, 12 PS, 4 L1, 5 R1 ; croix, stick gauche.
fn read_sony() -> u16 {
    let mut bits = 0;
    for s in sony_states() {
        for (i, bit) in [(1, A), (2, B), (9, START), (12, GUIDE), (4, L1), (5, R1), (3, Y), (0, X), (6, L2), (7, R2)] {
            if s.buttons[i] {
                bits |= bit;
            }
        }
        bits |= hat_bits(s.hat);
        bits |= stick((s.sticks[0] - 0.5) * 2.0, (0.5 - s.sticks[1]) * 2.0);
    }
    bits
}

/// Croix directionnelle 0..7 (haut, puis sens horaire ; 8 = repos) -> bits haut/bas/gauche/droite,
/// mêmes valeurs que la croix d'XInput (aussi utilisées pour la manette virtuelle).
pub(crate) fn hat_bits(h: u8) -> u16 {
    match h {
        0 => UP,
        1 => UP | RIGHT,
        2 => RIGHT,
        3 => DOWN | RIGHT,
        4 => DOWN,
        5 => DOWN | LEFT,
        6 => LEFT,
        7 => UP | LEFT,
        _ => 0,
    }
}

/// États de toutes les manettes Sony : lus directement en HID dès qu'une y est ouverte (fiable quel
/// que soit le mode de rapport où un jeu a laissé la manette, voir `hidpad`), sinon par
/// Windows.Gaming.Input.
pub(crate) fn sony_states() -> Vec<hidpad::SonyState> {
    let hid = hidpad::states();
    if !hid.is_empty() {
        return hid;
    }
    sony_pads().iter().filter_map(wgi_state).collect()
}

/// Lecture Windows.Gaming.Input d'une manette Sony, convertie au format de `hidpad`. Axes classés par
/// `mouse::split` (leur ordre n'est pas garanti : une gâchette au repos prise pour un stick dévié
/// figeait sinon la navigation dans une direction).
pub(crate) fn wgi_state(raw: &RawGameController) -> Option<hidpad::SonyState> {
    let (nb, ns, na) = (raw.ButtonCount().ok()?, raw.SwitchCount().ok()?, raw.AxisCount().ok()?);
    let mut b = vec![false; nb as usize];
    let mut s = vec![Hat::Center; ns as usize];
    let mut a = vec![0.5; na as usize];
    if raw.GetCurrentReading(&mut b, &mut s, &mut a).is_err() {
        sony_invalidate(raw);
        return None;
    }
    let mut buttons = [false; 14];
    for (i, v) in buttons.iter_mut().enumerate() {
        *v = b.get(i) == Some(&true);
    }
    let hat = match s.first().copied().unwrap_or(Hat::Center) {
        Hat::Up => 0,
        Hat::UpRight => 1,
        Hat::Right => 2,
        Hat::DownRight => 3,
        Hat::Down => 4,
        Hat::DownLeft => 5,
        Hat::Left => 6,
        Hat::UpLeft => 7,
        _ => 8,
    };
    let (sticks, triggers) = mouse::split(&a);
    Some(hidpad::SonyState { buttons, hat, sticks, triggers })
}

struct XInput {
    get_state: GetState,
    connected: [bool; 4],
    tick: u32,
}

impl XInput {
    /// `skip` : emplacement à ignorer (copie virtuelle d'une manette Sony).
    fn read(&mut self, skip: Option<u32>) -> u16 {
        // Interroger un emplacement vide est lent : on ne le refait qu'une fois par seconde.
        self.tick = self.tick.wrapping_add(1);
        let retry = self.tick.is_multiple_of(100);
        let mut bits = 0;
        for i in 0..4 {
            if skip == Some(i as u32) || (!self.connected[i] && !retry) {
                continue;
            }
            let mut st = XINPUT_STATE::default();
            self.connected[i] = unsafe { (self.get_state)(i as u32, &mut st) } == 0;
            if self.connected[i] {
                let g = st.Gamepad;
                bits |= g.wButtons.0 & (DIRS | START | GUIDE | A | B | L1 | R1 | Y | X);
                if g.bLeftTrigger > 128 {
                    bits |= L2;
                }
                if g.bRightTrigger > 128 {
                    bits |= R2;
                }
                bits |= stick(g.sThumbLX as f64 / 32767.0, g.sThumbLY as f64 / 32767.0);
            }
        }
        bits
    }
}

/// Direction du stick : seul l'axe dominant compte (un stick poussé un peu en biais déclenchait sinon
/// deux directions à la fois, ex. monter à la barre du haut et changer d'icône d'un seul geste).
fn stick(x: f64, y: f64) -> u16 {
    if x.abs().max(y.abs()) <= STICK {
        return 0;
    }
    if y.abs() >= x.abs() {
        if y > 0.0 { UP } else { DOWN }
    } else if x > 0.0 {
        RIGHT
    } else {
        LEFT
    }
}

/// Manette Sony choisie comme « principale » (Réglages > Manettes) : celle transmise aux jeux quand
/// plusieurs sont branchées. `None` : la première trouvée, peu importe laquelle.
static PRIMARY_PAD: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn set_primary_pad(id: Option<String>) {
    *PRIMARY_PAD.lock().unwrap_or_else(|e| e.into_inner()) = id;
}

pub(crate) fn primary_pad() -> Option<String> {
    PRIMARY_PAD.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Dernière manette utilisée (pour afficher les bons pictogrammes) : 0 Xbox/générique, 1 PlayStation.
static LAST_PAD: AtomicU8 = AtomicU8::new(0);

/// Une manette Sony est-elle branchée ? (le mode souris ne lit qu'elles)
pub fn sony_connected() -> bool {
    !hidpad::states().is_empty() || !sony_pads().is_empty()
}

pub fn last_pad() -> u8 {
    LAST_PAD.load(Ordering::Relaxed)
}

fn sony_present() -> bool {
    RawGameController::RawGameControllers().is_ok_and(|l| l.into_iter().any(|r| r.HardwareVendorId() == Ok(SONY)))
}

static LAST_INPUT: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// Instant du dernier ordre de navigation (mesure de latence : voir HEARTH_PERF dans le shell).
pub fn last_input() -> Option<Instant> {
    *LAST_INPUT.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn take_last_input() -> Option<Instant> {
    LAST_INPUT.lock().unwrap_or_else(|e| e.into_inner()).take()
}

fn stamp() {
    *LAST_INPUT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

/// Manettes Sony : liste tenue à jour en direct par les événements WinRT Ajout/Retrait (pas de
/// ré-énumération périodique) — une ré-énumération ponctuelle tombant pile pendant une micro-coupure
/// Bluetooth (manette faible en charge) pouvait renvoyer une liste vide ou un objet resté lié à une
/// session HID périmée (lecture figée, cf. `stuck` plus haut).
static SONY_LIST: std::sync::Mutex<Vec<RawGameController>> = std::sync::Mutex::new(Vec::new());
static SONY_WATCH: std::sync::Once = std::sync::Once::new();

fn refresh_sony_list() {
    let fresh = RawGameController::RawGameControllers()
        .map(|l| l.into_iter().filter(|r| r.HardwareVendorId() == Ok(SONY)).collect())
        .unwrap_or_default();
    *SONY_LIST.lock().unwrap_or_else(|e| e.into_inner()) = fresh;
}

fn sony_watch_start() {
    SONY_WATCH.call_once(|| {
        refresh_sony_list();
        let added = windows::Foundation::EventHandler::new(|_, r: windows::core::Ref<'_, RawGameController>| {
            if let Some(r) = r.as_ref() {
                if r.HardwareVendorId() == Ok(SONY) {
                    let mut l = SONY_LIST.lock().unwrap_or_else(|e| e.into_inner());
                    if !l.contains(r) {
                        l.push(r.clone());
                    }
                }
            }
            Ok(())
        });
        let removed = windows::Foundation::EventHandler::new(|_, r: windows::core::Ref<'_, RawGameController>| {
            if let Some(r) = r.as_ref() {
                SONY_LIST.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| c != r);
            }
            Ok(())
        });
        let _ = RawGameController::RawGameControllerAdded(&added);
        let _ = RawGameController::RawGameControllerRemoved(&removed);
    });
}

pub(crate) fn sony_pads() -> Vec<RawGameController> {
    sony_watch_start();
    SONY_LIST.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Force une relecture complète des manettes (nouvel appel WinRT, pas la liste mémorisée) : à utiliser
/// en sortant d'un jeu, au cas où celui-ci aurait changé le mode de rapport HID d'une manette Sony
/// (Steam Input, DirectInput…) sans déclencher d'événement Ajout/Retrait.
pub fn resync_pads() {
    sony_watch_start();
    refresh_sony_list();
}

/// Une lecture a échoué sur cet objet précis (session HID périmée) : on le retire, un vrai événement
/// Ajout/Retrait ou `resync_pads()` le restaurera proprement.
pub(crate) fn sony_invalidate(raw: &RawGameController) {
    SONY_LIST.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| c != raw);
}

/// Décodage d'un rapport HID Sony (sonde de diagnostic `examples/hid_probe.rs`).
pub fn probe_parse(product: u16, report: &[u8]) -> Option<String> {
    hidpad::parse(product, report).map(|s| format!("{s:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_thresholds() {
        assert_eq!(stick(0.0, 0.0), 0);
        assert_eq!(stick(0.9, 0.0), RIGHT);
        assert_eq!(stick(-0.9, 0.6), LEFT);
        assert_eq!(stick(0.55, -0.8), DOWN);
        assert_eq!(stick(0.3, -0.3), 0);
    }
}
