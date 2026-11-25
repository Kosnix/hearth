//! Thème personnalisable : préréglage d'accent (ou teinte libre) et ton du fond ; calcule les jetons de couleur de l'interface.

use std::sync::atomic::{AtomicU32, Ordering};

use crate::i18n;

/// (nom, teinte en degrés ; None = blanc).
pub const PRESETS: [(&str, Option<u32>); 8] = [
    ("Orange", Some(38)),
    ("Bleu", Some(215)),
    ("Rouge", Some(355)),
    ("Vert", Some(145)),
    ("Violet", Some(268)),
    ("Rose", Some(328)),
    ("Cyan", Some(186)),
    ("Blanc", None),
];
/// Indice du préréglage « Personnalisé » (teinte libre).
pub const CUSTOM: u32 = PRESETS.len() as u32;

/// (nom, fond, bandeau, tuile, guide, profond) — élévations façon iOS sombre (systemBackground /
/// secondarySystemBackground / tertiarySystemBackground), chaque préréglage appliquant une légère
/// teinte à cette même échelle plutôt qu'un fond coloré.
pub const TONES: [(&str, [u8; 3], [u8; 3], [u8; 3], [u8; 3], [u8; 3]); 5] = [
    ("Graphite", [0x00, 0x00, 0x00], [0x1c, 0x1c, 0x1e], [0x2c, 0x2c, 0x2e], [0x1c, 0x1c, 0x1e], [0x00, 0x00, 0x00]),
    ("Noir", [0x00, 0x00, 0x00], [0x14, 0x14, 0x14], [0x1f, 0x1f, 0x1f], [0x14, 0x14, 0x14], [0x00, 0x00, 0x00]),
    ("Bleu nuit", [0x00, 0x00, 0x05], [0x14, 0x1a, 0x28], [0x1e, 0x24, 0x34], [0x14, 0x1a, 0x28], [0x00, 0x01, 0x06]),
    ("Pourpre", [0x04, 0x00, 0x06], [0x1a, 0x14, 0x24], [0x26, 0x1e, 0x32], [0x1a, 0x14, 0x24], [0x04, 0x02, 0x08]),
    ("Forêt", [0x00, 0x04, 0x01], [0x12, 0x1c, 0x16], [0x1c, 0x28, 0x20], [0x12, 0x1c, 0x16], [0x02, 0x06, 0x03]),
];

static PRESET: AtomicU32 = AtomicU32::new(0);
static HUE: AtomicU32 = AtomicU32::new(38);
static TONE: AtomicU32 = AtomicU32::new(0);

pub struct Palette {
    pub accent: [u8; 3],
    pub on_accent: [u8; 3],
    pub bg: [u8; 3],
    pub band: [u8; 3],
    pub tile: [u8; 3],
    pub guide: [u8; 3],
    pub deep: [u8; 3],
}

pub fn preset() -> u32 {
    PRESET.load(Ordering::Relaxed)
}

pub fn hue() -> u32 {
    HUE.load(Ordering::Relaxed)
}

pub fn tone() -> u32 {
    TONE.load(Ordering::Relaxed)
}

fn save() {
    i18n::set_pref("theme", &format!("{}:{}:{}", preset(), hue(), tone()));
}

/// Lit le thème enregistré (« préréglage:teinte:fond »).
pub fn load() {
    if let Some(v) = i18n::pref("theme") {
        let p: Vec<u32> = v.split(':').filter_map(|x| x.parse().ok()).collect();
        if p.len() == 3 {
            PRESET.store(p[0].min(CUSTOM), Ordering::Relaxed);
            HUE.store(p[1] % 360, Ordering::Relaxed);
            TONE.store(p[2].min(TONES.len() as u32 - 1), Ordering::Relaxed);
        }
    }
}

pub fn set_preset(i: i32) {
    let n = CUSTOM as i32 + 1;
    let p = i.rem_euclid(n) as u32;
    PRESET.store(p, Ordering::Relaxed);
    if let Some((_, Some(h))) = PRESETS.get(p as usize) {
        HUE.store(*h, Ordering::Relaxed);
    }
    save();
}

/// Change la teinte (le préréglage passe en « Personnalisé »).
pub fn shift_hue(delta: i32) {
    HUE.store((hue() as i32 + delta).rem_euclid(360) as u32, Ordering::Relaxed);
    PRESET.store(CUSTOM, Ordering::Relaxed);
    save();
}

pub fn set_tone(i: i32) {
    TONE.store(i.rem_euclid(TONES.len() as i32) as u32, Ordering::Relaxed);
    save();
}

pub fn reset() {
    PRESET.store(0, Ordering::Relaxed);
    HUE.store(38, Ordering::Relaxed);
    TONE.store(0, Ordering::Relaxed);
    save();
}

fn hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8]
}

fn luminance(c: [u8; 3]) -> f32 {
    let f = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
}

/// Palette courante : l'accent vif, le texte posé dessus est noir ou blanc selon la luminance (contraste maximal).
pub fn palette() -> Palette {
    let white = PRESETS.get(preset() as usize).is_some_and(|p| p.1.is_none());
    let accent = if white { [0xf2, 0xf2, 0xf4] } else { hsv(hue() as f32, 0.95, 1.0) };
    let on_accent = if luminance(accent) > 0.30 { [0x11, 0x11, 0x11] } else { [0xff, 0xff, 0xff] };
    let t = &TONES[tone() as usize];
    Palette { accent, on_accent, bg: t.1, band: t.2, tile: t.3, guide: t.4, deep: t.5 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_choice() {
        assert_eq!(hsv(38.0, 0.95, 1.0)[0], 255);
        // L'orange est clair : texte noir ; le bleu est sombre : texte blanc.
        assert!(luminance(hsv(38.0, 0.95, 1.0)) > 0.30);
        assert!(luminance(hsv(215.0, 0.95, 1.0)) < 0.30);
    }
}
