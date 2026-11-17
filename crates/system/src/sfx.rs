//! Sons de l'interface : petits sons doux synthétisés au premier usage, joués en mémoire (aucun fichier).

use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

#[derive(Clone, Copy)]
pub enum Sfx {
    /// Déplacement de la sélection.
    Move,
    /// Validation.
    Select,
    /// Retour / fermeture.
    Back,
    /// Ouverture d'un menu.
    Open,
    /// Notification.
    Notify,
}

const RATE: u32 = 44_100;

/// Note : (fréquence de départ, fréquence de fin) en Hz, durée en ms, volume 0..1.
fn tone(out: &mut Vec<f32>, f0: f32, f1: f32, ms: u32, vol: f32) {
    let n = (RATE * ms / 1000) as usize;
    let start = out.len();
    out.resize(start + n, 0.0);
    let mut phase = 0.0f32;
    for i in 0..n {
        let t = i as f32 / n as f32;
        let f = f0 + (f1 - f0) * t;
        phase += 2.0 * std::f32::consts::PI * f / RATE as f32;
        // Attaque courte (évite le clic) puis décroissance douce.
        let env = (t * 40.0).min(1.0) * (1.0 - t).powf(1.6);
        out[start + i] += phase.sin() * env * vol;
    }
}

fn silence(out: &mut Vec<f32>, ms: u32) {
    out.resize(out.len() + (RATE * ms / 1000) as usize, 0.0);
}

fn wav(samples: &[f32]) -> Vec<u8> {
    let data: Vec<u8> = samples.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 30_000.0) as i16).to_le_bytes()).collect();
    let mut w = Vec::with_capacity(44 + data.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    w
}

fn build(s: Sfx) -> Vec<u8> {
    let mut o = vec![];
    match s {
        Sfx::Move => tone(&mut o, 1500.0, 1200.0, 45, 0.16),
        Sfx::Select => {
            tone(&mut o, 700.0, 700.0, 55, 0.2);
            tone(&mut o, 1050.0, 1050.0, 110, 0.2);
        }
        Sfx::Back => tone(&mut o, 900.0, 500.0, 110, 0.18),
        Sfx::Open => {
            tone(&mut o, 600.0, 600.0, 40, 0.16);
            tone(&mut o, 800.0, 800.0, 40, 0.16);
            tone(&mut o, 1100.0, 1100.0, 90, 0.16);
        }
        Sfx::Notify => {
            tone(&mut o, 1320.0, 1320.0, 450, 0.18);
            silence(&mut o, 0);
            let mut hi = vec![];
            tone(&mut hi, 1980.0, 1980.0, 450, 0.08);
            for (a, b) in o.iter_mut().zip(hi) {
                *a += b;
            }
        }
    }
    wav(&o)
}

fn bank() -> &'static [Vec<u8>; 5] {
    static BANK: OnceLock<[Vec<u8>; 5]> = OnceLock::new();
    BANK.get_or_init(|| [build(Sfx::Move), build(Sfx::Select), build(Sfx::Back), build(Sfx::Open), build(Sfx::Notify)])
}

/// Joue un son (asynchrone, remplace le précédent).
pub fn play(s: Sfx) {
    let data = &bank()[s as usize];
    unsafe {
        let _ = PlaySoundW(PCWSTR(data.as_ptr().cast()), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}
