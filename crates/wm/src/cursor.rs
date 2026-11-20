//! Curseur de la souris masqué hors de Hearth (jeux, autres fenêtres) : curseurs système remplacés par un curseur transparent,
//! restaurés dès qu'on revient dans Hearth, que la souris bouge, ou à la fermeture.

use std::cell::Cell;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Identifiants des curseurs système (OCR_*) remplacés.
const IDS: [SYSTEM_CURSOR_ID; 13] = [OCR_NORMAL, OCR_IBEAM, OCR_WAIT, OCR_CROSS, OCR_UP, OCR_SIZENWSE, OCR_SIZENESW, OCR_SIZEWE, OCR_SIZENS, OCR_SIZEALL, OCR_NO, OCR_HAND, OCR_APPSTARTING];

/// Remplace tous les curseurs système par un curseur transparent.
fn blank() {
    let and = [0xFFu8; 128];
    let xor = [0u8; 128];
    for id in IDS {
        // `SetSystemCursor` détruit le curseur passé : un exemplaire par identifiant.
        if let Ok(c) = unsafe { CreateCursor(None, 0, 0, 32, 32, and.as_ptr().cast(), xor.as_ptr().cast()) } {
            let _ = unsafe { SetSystemCursor(c, id) };
        }
    }
}

/// Recharge les curseurs de l'utilisateur (registre).
pub fn restore() {
    let _ = unsafe { SystemParametersInfoW(SPI_SETCURSORS, 0, None, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
}

/// Suit la politique du curseur : visible dans Hearth ou quand la souris vient de bouger, masqué sinon.
pub struct Policy {
    hidden: Cell<bool>,
    last: Cell<(i32, i32)>,
    moved: Cell<Instant>,
}

impl Policy {
    pub fn new() -> Self {
        restore(); // un plantage précédent a pu laisser le curseur transparent
        Policy { hidden: Cell::new(false), last: Cell::new((0, 0)), moved: Cell::new(Instant::now()) }
    }

    /// `inside` : une fenêtre de Hearth est au premier plan.
    pub fn update(&self, inside: bool) {
        let mut p = POINT::default();
        let _ = unsafe { GetCursorPos(&mut p) };
        if self.last.get() != (p.x, p.y) {
            self.last.set((p.x, p.y));
            self.moved.set(Instant::now());
        }
        let want_hidden = !inside && self.moved.get().elapsed() > Duration::from_millis(1500);
        if want_hidden != self.hidden.get() {
            if want_hidden {
                blank();
            } else {
                restore();
            }
            self.hidden.set(want_hidden);
        }
    }

    pub fn show(&self) {
        if self.hidden.replace(false) {
            restore();
        }
    }
}

impl Drop for Policy {
    fn drop(&mut self) {
        restore();
    }
}
