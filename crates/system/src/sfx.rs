//! Sons de l'interface et musique d'ambiance : tout est synthétisé au démarrage (aucun fichier audio),
//! puis mixé par un seul thread qui pilote la sortie waveOut (volumes séparés, fondus, pas de MCI).

use std::f32::consts::TAU;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::{
    waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutWrite, HWAVEOUT, WAVEFORMATEX, WAVEHDR, WAVE_FORMAT_PCM, WAVE_MAPPER,
};

const SR: usize = 44_100;
const WHDR_DONE: u32 = 1;
const FRAMES: usize = 512;

#[derive(Clone, Copy)]
pub enum Sfx {
    Move,
    Select,
    Back,
    Open,
    Close,
    Tab,
    Key,
    Launch,
    Notify,
    Error,
    Boot,
}


enum Cmd {
    Play(Sfx, Instant),
    Music(bool),
    Volumes(f32, f32),
}

static TX: OnceLock<Sender<Cmd>> = OnceLock::new();
static READY: AtomicBool = AtomicBool::new(false);

// ---------- Synthèse ----------

fn grow(out: &mut Vec<f32>, n: usize) {
    if out.len() < n {
        out.resize(n, 0.0);
    }
}

fn midi(n: f32) -> f32 {
    440.0 * 2f32.powf((n - 69.0) / 12.0)
}

/// Cloche douce : fondamentale + harmoniques qui s'éteignent plus vite, attaque de 4 ms.
fn bell(out: &mut Vec<f32>, at: f32, f: f32, dur: f32, amp: f32) {
    const P: [(f32, f32, f32); 4] = [(1.0, 1.0, 1.0), (2.0, 0.30, 1.7), (3.0, 0.14, 2.6), (4.13, 0.07, 4.0)];
    let s = (at * SR as f32) as usize;
    let n = (dur * SR as f32) as usize;
    grow(out, s + n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let att = (t / 0.004).min(1.0);
        let fin = 1.0 - (i as f32 / n as f32).powi(8);
        let v: f32 = P.iter().map(|&(r, a, d)| a * (TAU * f * r * t).sin() * (-d * 5.0 * t / dur).exp()).sum();
        out[s + i] += v * att * fin * amp;
    }
}

/// Note de piano douce : harmoniques légèrement inharmoniques, chacune avec sa propre décroissance.
fn piano(out: &mut Vec<f32>, at: f32, f: f32, dur: f32, amp: f32) {
    const A: [f32; 6] = [1.0, 0.5, 0.26, 0.14, 0.07, 0.035];
    let s = (at * SR as f32) as usize;
    let n = (dur * SR as f32) as usize;
    grow(out, s + n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let att = (t / 0.003).min(1.0);
        let fin = 1.0 - (i as f32 / n as f32).powi(6);
        let mut v = 0.0;
        for (k, a) in A.iter().enumerate() {
            let h = (k + 1) as f32;
            let fr = f * h * (1.0 + 0.0004 * h * h).sqrt();
            v += a * (TAU * fr * t).sin() * (-(0.9 + 0.9 * h) * t * 5.0 / dur).exp();
        }
        out[s + i] += v * att * fin * amp;
    }
}

/// Nappe : deux sinus légèrement désaccordés avec une octave discrète ; attaque et relâchement lents.
fn pad(out: &mut Vec<f32>, at: f32, f: f32, dur: f32, amp: f32, attack: f32, release: f32) {
    let s = (at.max(0.0) * SR as f32) as usize;
    let n = (dur * SR as f32) as usize;
    grow(out, s + n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let env = (t / attack).min(1.0).powi(2) * ((dur - t) / release).clamp(0.0, 1.0).powi(2);
        let v = (TAU * f * t).sin() + (TAU * (f + 0.35) * t).sin() + 0.25 * (TAU * 2.0 * f * t).sin();
        out[s + i] += v * env * amp * 0.5;
    }
}

/// Tic de bois : sinus à hauteur descendante, très court.
fn tick(out: &mut Vec<f32>, at: f32, f: f32, dur: f32, amp: f32) {
    let s = (at * SR as f32) as usize;
    let n = (dur * SR as f32) as usize;
    grow(out, s + n);
    let mut ph = 0.0f32;
    for i in 0..n {
        let t = i as f32 / n as f32;
        ph += TAU * f * (1.0 - 0.25 * t) / SR as f32;
        out[s + i] += ph.sin() * (1.0 - t).powi(3) * (t * n as f32 / 40.0).min(1.0) * amp;
    }
}

struct Comb {
    buf: Vec<f32>,
    i: usize,
    st: f32,
}

impl Comb {
    fn new(n: usize) -> Self {
        Comb { buf: vec![0.0; n], i: 0, st: 0.0 }
    }
    fn p(&mut self, x: f32, fb: f32, damp: f32) -> f32 {
        let y = self.buf[self.i];
        self.st = y * (1.0 - damp) + self.st * damp;
        self.buf[self.i] = x + self.st * fb;
        self.i = (self.i + 1) % self.buf.len();
        y
    }
}

struct Allpass {
    buf: Vec<f32>,
    i: usize,
}

impl Allpass {
    fn new(n: usize) -> Self {
        Allpass { buf: vec![0.0; n], i: 0 }
    }
    fn p(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        self.buf[self.i] = x + b * 0.5;
        self.i = (self.i + 1) % self.buf.len();
        b - x
    }
}

/// Réverbération stéréo (peignes + passe-tout) : mono en entrée, stéréo entrelacée en sortie, `tail` secondes de queue.
fn reverb(mono: &[f32], wet: f32, fb: f32, tail: f32) -> Vec<f32> {
    const D: [usize; 6] = [1116, 1188, 1277, 1356, 1422, 1491];
    let mut combs: [Vec<Comb>; 2] = [D.iter().map(|&d| Comb::new(d)).collect(), D.iter().map(|&d| Comb::new(d + 23)).collect()];
    let mut aps: [[Allpass; 2]; 2] = [[Allpass::new(556), Allpass::new(441)], [Allpass::new(579), Allpass::new(464)]];
    let n = mono.len() + (tail * SR as f32) as usize;
    let mut out = vec![0.0f32; n * 2];
    for i in 0..n {
        let x = mono.get(i).copied().unwrap_or(0.0);
        for ch in 0..2 {
            let mut r = 0.0;
            for c in combs[ch].iter_mut() {
                r += c.p(x * 0.12, fb, 0.35);
            }
            for a in aps[ch].iter_mut() {
                r = a.p(r);
            }
            out[i * 2 + ch] = x * (1.0 - wet * 0.5) + r * wet;
        }
    }
    out
}

fn normalize(s: &mut [f32], peak: f32) {
    let max = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if max > 1e-6 {
        let g = peak / max;
        s.iter_mut().for_each(|x| *x *= g);
    }
}

fn finish(mut mono: Vec<f32>, wet: f32, peak: f32) -> Vec<f32> {
    // Fin en fondu : jamais de clic de coupure.
    let n = mono.len();
    for i in 0..(n / 20).min(400) {
        mono[n - 1 - i] *= i as f32 / (n / 20).min(400) as f32;
    }
    let mut st = reverb(&mono, wet, 0.82, 0.45);
    normalize(&mut st, peak);
    st
}

/// Notes (mi-bémol évité) : tout est dans la tonalité de ré majeur, pour que les sons s'enchaînent harmonieusement.
fn build(s: Sfx) -> Vec<f32> {
    let mut o: Vec<f32> = vec![];
    match s {
        Sfx::Move => {
            tick(&mut o, 0.0, 1000.0, 0.05, 0.9);
            return finish(o, 0.10, 0.30);
        }
        Sfx::Select => {
            bell(&mut o, 0.0, midi(81.0), 0.5, 0.5);
            bell(&mut o, 0.07, midi(86.0), 0.55, 0.55);
        }
        Sfx::Back => {
            bell(&mut o, 0.0, midi(86.0), 0.32, 0.45);
            bell(&mut o, 0.06, midi(81.0), 0.4, 0.45);
        }
        Sfx::Open => {
            for n in [62.0, 69.0, 78.0] {
                pad(&mut o, 0.0, midi(n), 0.5, 0.35, 0.09, 0.25);
            }
            bell(&mut o, 0.1, midi(81.0), 0.5, 0.25);
        }
        Sfx::Close => {
            bell(&mut o, 0.0, midi(81.0), 0.3, 0.35);
            bell(&mut o, 0.07, midi(74.0), 0.4, 0.4);
        }
        Sfx::Tab => bell(&mut o, 0.0, midi(88.0), 0.28, 0.45),
        Sfx::Key => {
            tick(&mut o, 0.0, 1500.0, 0.03, 0.8);
            return finish(o, 0.05, 0.22);
        }
        Sfx::Launch => {
            for (i, n) in [74.0, 81.0, 86.0, 90.0, 93.0].iter().enumerate() {
                bell(&mut o, i as f32 * 0.09, midi(*n), 0.7, 0.4);
            }
            for n in [62.0, 69.0] {
                pad(&mut o, 0.0, midi(n), 0.8, 0.3, 0.2, 0.4);
            }
        }
        Sfx::Notify => {
            for (i, n) in [81.0, 86.0, 90.0].iter().enumerate() {
                bell(&mut o, i as f32 * 0.12, midi(*n), 0.75, 0.42);
            }
        }
        Sfx::Error => {
            bell(&mut o, 0.0, midi(54.0), 0.5, 0.55);
            bell(&mut o, 0.12, midi(50.0), 0.6, 0.55);
        }
        Sfx::Boot => {
            for n in [50.0, 57.0, 66.0, 69.0] {
                pad(&mut o, 0.0, midi(n), 1.8, 0.3, 0.5, 0.9);
            }
            for (i, n) in [81.0, 86.0, 90.0, 93.0].iter().enumerate() {
                bell(&mut o, 0.6 + i as f32 * 0.2, midi(*n), 0.9, 0.3);
            }
        }
    }
    finish(o, 0.22, 0.5)
}

fn lcg(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 8) as f32 / 16_777_216.0
}

/// Musique d'ambiance : piano doux sur quatre accords lents (si mineur, sol, ré, la sus), nappe très discrète dessous,
/// grande réverbération. Boucle de 96 s sans raccord : la queue de la fin est repliée sur le début.
fn build_music() -> Vec<f32> {
    const CHORD: f32 = 24.0;
    let chords: [[f32; 5]; 4] = [[47.0, 54.0, 57.0, 62.0, 73.0], [43.0, 50.0, 54.0, 59.0, 69.0], [38.0, 45.0, 49.0, 54.0, 64.0], [45.0, 52.0, 59.0, 66.0, 69.0]];
    let scale = [62.0f32, 64.0, 66.0, 69.0, 71.0, 74.0, 76.0, 78.0, 81.0];
    let total = CHORD * 4.0;
    let mut seed = 0x1234_5678u32;
    let mut o: Vec<f32> = vec![];
    for (c, notes) in chords.iter().enumerate() {
        let t0 = c as f32 * CHORD;
        for &n in &notes[..4] {
            // La nappe commence avant l'accord pour le recouvrir en douceur (repliée sur la fin pour le premier).
            pad(&mut o, (t0 - 3.0).max(0.0), midi(n), CHORD + 6.0 - (3.0 - t0).max(0.0), 0.035, 5.0, 6.0);
        }
        piano(&mut o, t0, midi(notes[0]), 9.0, 0.16);
        let mut t = t0 + 1.0;
        while t < t0 + CHORD - 1.0 {
            let r = lcg(&mut seed);
            if r > 0.18 {
                let pick = if r > 0.55 { notes[(lcg(&mut seed) * 5.0) as usize % 5] + 12.0 } else { scale[(lcg(&mut seed) * 9.0) as usize % 9] };
                piano(&mut o, t, midi(pick), 5.0, 0.10 + 0.10 * lcg(&mut seed));
            }
            t += 1.5 + 1.5 * lcg(&mut seed);
        }
    }
    let mut st = reverb(&o, 0.55, 0.9, 6.0);
    // Boucle sans raccord : la queue (au-delà de 96 s) revient au début.
    let len = (total * SR as f32) as usize * 2;
    if st.len() > len {
        let tail: Vec<f32> = st[len..].to_vec();
        st.truncate(len);
        for (i, v) in tail.iter().enumerate() {
            if i < len {
                st[i] += v;
            }
        }
    }
    normalize(&mut st, 0.2);
    st
}

// ---------- Mixeur ----------

struct Voice {
    buf: Arc<Vec<f32>>,
    pos: usize,
}

struct Out {
    hwo: HWAVEOUT,
    hdrs: Vec<(Box<WAVEHDR>, Box<[i16]>)>,
}

fn open_out() -> Option<Out> {
    let fmt = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_PCM as u16, nChannels: 2, nSamplesPerSec: SR as u32, nAvgBytesPerSec: SR as u32 * 4, nBlockAlign: 4, wBitsPerSample: 16, cbSize: 0 };
    let mut hwo = HWAVEOUT::default();
    let r = unsafe { waveOutOpen(Some(&mut hwo), WAVE_MAPPER, &fmt, None, None, windows::Win32::Media::Audio::CALLBACK_NULL) };
    if r != 0 {
        return None;
    }
    let mut hdrs = vec![];
    for _ in 0..4 {
        let mut data: Box<[i16]> = vec![0i16; FRAMES * 2].into_boxed_slice();
        let mut hdr = Box::new(WAVEHDR { lpData: windows::core::PSTR(data.as_mut_ptr() as *mut u8), dwBufferLength: (FRAMES * 4) as u32, ..Default::default() });
        unsafe {
            waveOutPrepareHeader(hwo, &mut *hdr, size_of::<WAVEHDR>() as u32);
            hdr.dwFlags |= WHDR_DONE; // marqué libre : rempli au premier tour
        }
        hdrs.push((hdr, data));
    }
    Some(Out { hwo, hdrs })
}

fn run(rx: Receiver<Cmd>) {
    let sfx: Vec<Arc<Vec<f32>>> = [Sfx::Move, Sfx::Select, Sfx::Back, Sfx::Open, Sfx::Close, Sfx::Tab, Sfx::Key, Sfx::Launch, Sfx::Notify, Sfx::Error, Sfx::Boot].iter().map(|&s| Arc::new(build(s))).collect();
    let (mtx, mrx) = channel::<Arc<Vec<f32>>>();
    std::thread::spawn(move || {
        let _ = mtx.send(Arc::new(build_music()));
    });
    let mut out = loop {
        match open_out() {
            Some(o) => break o,
            None => std::thread::sleep(Duration::from_secs(5)),
        }
    };
    READY.store(true, Ordering::SeqCst);
    let (mut sfx_vol, mut music_vol) = (0.8f32, 0.4f32);
    let mut voices: Vec<Voice> = vec![];
    let mut music: Option<Arc<Vec<f32>>> = None;
    let (mut mpos, mut mgain, mut want) = (0usize, 0.0f32, false);
    let mut mix = vec![0.0f32; FRAMES * 2];
    loop {
        let mut worked = false;
        for k in 0..out.hdrs.len() {
            if out.hdrs[k].0.dwFlags & WHDR_DONE == 0 {
                continue;
            }
            worked = true;
            while let Ok(c) = rx.try_recv() {
                match c {
                    // Sons en retard (interface figée un instant) abandonnés : jamais de rafale.
                    Cmd::Play(s, at) => {
                        if at.elapsed() < Duration::from_millis(150) && voices.len() < 12 {
                            voices.push(Voice { buf: sfx[s as usize].clone(), pos: 0 });
                        }
                    }
                    Cmd::Music(on) => want = on,
                    Cmd::Volumes(s, m) => (sfx_vol, music_vol) = (s, m),
                }
            }
            if music.is_none() {
                music = mrx.try_recv().ok();
            }
            mix.iter_mut().for_each(|x| *x = 0.0);
            for v in voices.iter_mut() {
                let n = (v.buf.len() - v.pos).min(FRAMES * 2);
                for i in 0..n {
                    mix[i] += v.buf[v.pos + i] * sfx_vol;
                }
                v.pos += n;
            }
            voices.retain(|v| v.pos < v.buf.len());
            if let Some(m) = &music {
                let target = if want { music_vol } else { 0.0 };
                for f in 0..FRAMES {
                    if mgain < target {
                        mgain = (mgain + 1.0 / (2.0 * SR as f32)).min(target);
                    } else if mgain > target {
                        mgain = (mgain - 1.0 / (0.6 * SR as f32)).max(target);
                    }
                    if mgain > 0.0 {
                        mix[f * 2] += m[mpos] * mgain;
                        mix[f * 2 + 1] += m[mpos + 1] * mgain;
                        mpos = (mpos + 2) % m.len();
                    }
                }
            }
            let (hdr, data) = &mut out.hdrs[k];
            for (d, s) in data.iter_mut().zip(mix.iter()) {
                *d = (s.clamp(-1.0, 1.0) * 32_000.0) as i16;
            }
            hdr.dwFlags &= !WHDR_DONE;
            unsafe { waveOutWrite(out.hwo, &mut **hdr, size_of::<WAVEHDR>() as u32) };
        }
        if !worked {
            std::thread::sleep(Duration::from_millis(3));
        }
    }
    #[allow(unreachable_code)]
    unsafe {
        waveOutClose(out.hwo);
    }
}

/// Démarre le thread audio (synthèse, ouverture de la sortie, mixage).
pub fn init() {
    TX.get_or_init(|| {
        let (tx, rx) = channel::<Cmd>();
        std::thread::spawn(move || run(rx));
        tx
    });
}

/// Joue un effet (asynchrone).
pub fn play(s: Sfx) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(Cmd::Play(s, Instant::now()));
    }
}

/// Musique d'ambiance : fondu entrant ou sortant. Faux tant que le moteur n'est pas prêt (l'appelant réessaie).
pub fn music(on: bool) -> bool {
    if !READY.load(Ordering::SeqCst) {
        return false;
    }
    TX.get().is_some_and(|tx| tx.send(Cmd::Music(on)).is_ok())
}

/// Volumes en pourcentage (effets, musique).
pub fn set_volumes(sfx: u32, music: u32) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(Cmd::Volumes(sfx.min(100) as f32 / 100.0, music.min(100) as f32 / 100.0));
    }
}
