//! Sons de l'interface et musique d'ambiance : tout est synthétisé au premier lancement (dossier `Hearth\sfx`),
//! puis joué par MCI (plusieurs sons peuvent se superposer à la musique).

use std::path::PathBuf;
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Media::Multimedia::mciSendStringW;

#[derive(Clone, Copy)]
pub enum Sfx {
    Move,
    Select,
    Back,
    Open,
    Notify,
}

const NAMES: [&str; 5] = ["move", "select", "back", "open", "notify"];
const VERSION: &str = "v3";

fn dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth").join("sfx")
}

fn mci(cmd: &str) {
    let w: Vec<u16> = cmd.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let _ = mciSendStringW(PCWSTR(w.as_ptr()), None, None);
    }
}

// ---------- Synthèse ----------

fn tone(rate: u32, out: &mut Vec<f32>, at_ms: u32, f0: f32, f1: f32, ms: u32, vol: f32, attack_ms: f32, decay: f32) {
    let start = (rate * at_ms / 1000) as usize;
    let n = (rate * ms / 1000) as usize;
    if out.len() < start + n {
        out.resize(start + n, 0.0);
    }
    let mut ph = 0.0f32;
    for i in 0..n {
        let t = i as f32 / n as f32;
        let f = f0 + (f1 - f0) * t;
        ph += 2.0 * std::f32::consts::PI * f / rate as f32;
        let att = (i as f32 / (rate as f32 * attack_ms / 1000.0)).min(1.0);
        let env = att * (1.0 - t).powf(decay);
        // Timbre chaud : fondamentale + douce octave.
        out[start + i] += (ph.sin() + 0.25 * (2.0 * ph).sin()) * env * vol;
    }
}

/// Filtre passe-bas simple : retire les aigus (sons moins agressifs).
fn lowpass(s: &mut [f32], k: f32) {
    let mut y = 0.0;
    for x in s.iter_mut() {
        y += k * (*x - y);
        *x = y;
    }
}

fn wav(rate: u32, samples: &[f32]) -> Vec<u8> {
    let data: Vec<u8> = samples.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 30_000.0) as i16).to_le_bytes()).collect();
    let mut w = Vec::with_capacity(44 + data.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    w
}

fn build(i: usize) -> Vec<u8> {
    const R: u32 = 44_100;
    let mut o = vec![];
    match i {
        // Léger « tic » mat.
        0 => tone(R, &mut o, 0, 520.0, 440.0, 55, 0.12, 3.0, 3.0),
        // Deux notes douces montantes (sol → ré).
        1 => {
            tone(R, &mut o, 0, 392.0, 392.0, 140, 0.15, 5.0, 2.2);
            tone(R, &mut o, 70, 587.0, 587.0, 220, 0.15, 5.0, 2.2);
        }
        // Une note descendante.
        2 => tone(R, &mut o, 0, 440.0, 330.0, 170, 0.14, 5.0, 2.0),
        // Montée lente douce.
        3 => {
            tone(R, &mut o, 0, 262.0, 330.0, 320, 0.10, 90.0, 1.4);
            tone(R, &mut o, 0, 392.0, 494.0, 320, 0.07, 90.0, 1.4);
        }
        // Petit arpège de type marimba.
        _ => {
            tone(R, &mut o, 0, 659.0, 659.0, 420, 0.12, 3.0, 3.0);
            tone(R, &mut o, 110, 784.0, 784.0, 420, 0.12, 3.0, 3.0);
            tone(R, &mut o, 220, 988.0, 988.0, 560, 0.12, 3.0, 3.0);
        }
    }
    lowpass(&mut o, 0.45);
    wav(R, &o)
}

/// Boucle d'ambiance de 64 s : nappes lentes (accords la mineur 9, fa maj 7, do maj 7, sol 6), sans pulsation.
fn build_music() -> Vec<u8> {
    const R: u32 = 22_050;
    let chords: [[f32; 4]; 4] = [[110.0, 164.8, 246.9, 329.6], [87.3, 130.8, 220.0, 329.6], [130.8, 196.0, 246.9, 329.6], [98.0, 146.8, 246.9, 293.7]];
    let seg = R as usize * 16;
    let mut o = vec![0.0f32; seg * 4];
    for (c, notes) in chords.iter().enumerate() {
        for (k, f) in notes.iter().enumerate() {
            for detune in [-0.6f32, 0.0, 0.7] {
                let freq = f + detune;
                let mut ph = (k * 7 + c) as f32;
                for i in 0..seg {
                    let t = i as f32 / seg as f32;
                    // Chaque accord naît et s'éteint à zéro (boucle sans raccord audible), avec chevauchement doux.
                    let env = (std::f32::consts::PI * t).sin().powf(1.5);
                    let trem = 1.0 + 0.12 * (i as f32 / R as f32 * 0.4 + k as f32).sin();
                    ph += 2.0 * std::f32::consts::PI * freq / R as f32;
                    o[c * seg + i] += ph.sin() * env * trem * 0.018;
                }
            }
        }
    }
    lowpass(&mut o, 0.25);
    wav(R, &o)
}

// ---------- Lecture ----------
// Les périphériques MCI sont fermés quand le thread qui les a ouverts se termine : un seul thread audio permanent
// ouvre et pilote tout (les autres threads lui envoient des commandes).

enum Cmd {
    Play(usize, std::time::Instant),
    Music(bool),
}

static TX: OnceLock<std::sync::mpsc::Sender<Cmd>> = OnceLock::new();
static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Démarre le thread audio : génère les fichiers au besoin, ouvre les sons, puis attend les commandes.
pub fn init() {
    TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
        std::thread::spawn(move || {
            let d = dir();
            if std::fs::create_dir_all(&d).is_err() {
                return;
            }
            for (i, n) in NAMES.iter().enumerate() {
                let p = d.join(format!("{n}-{VERSION}.wav"));
                if !p.exists() {
                    let _ = std::fs::write(&p, build(i));
                }
                mci(&format!("open \"{}\" type waveaudio alias hs_{n}", p.display()));
            }
            let m = d.join(format!("music-{VERSION}.wav"));
            if !m.exists() {
                let _ = std::fs::write(&m, build_music());
            }
            mci(&format!("open \"{}\" type waveaudio alias hs_music", m.display()));
            READY.store(true, std::sync::atomic::Ordering::SeqCst);
            while let Ok(c) = rx.recv() {
                match c {
                    Cmd::Play(i, at) => {
                        // Sons en retard (interface bloquée un instant) abandonnés : jamais de rafale de sons à la suite.
                        if at.elapsed() < std::time::Duration::from_millis(150) {
                            mci(&format!("play hs_{} from 0", NAMES[i]));
                        }
                    }
                    Cmd::Music(on) => {
                        if on && !STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
                            mci("play hs_music repeat");
                        } else {
                            mci(if on { "resume hs_music" } else { "pause hs_music" });
                        }
                    }
                }
            }
        });
        tx
    });
}

/// Joue un effet (asynchrone ; se superpose à la musique).
pub fn play(s: Sfx) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(Cmd::Play(s as usize, std::time::Instant::now()));
    }
}

/// Musique d'ambiance : lecture en boucle (la première fois), puis pause / reprise. Faux tant que le thread audio n'est pas prêt.
pub fn music(on: bool) -> bool {
    if !READY.load(std::sync::atomic::Ordering::SeqCst) {
        return false;
    }
    TX.get().is_some_and(|tx| tx.send(Cmd::Music(on)).is_ok())
}
