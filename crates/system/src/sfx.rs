//! Sons de l'interface et musique d'ambiance : packs audio DeckThemes (WAV/MP3/OGG…), décodés puis mixés par un seul thread
//! qui pilote la sortie waveOut (volumes séparés, fondus, pas de MCI). Aucun son n'est fourni avec Hearth : les packs sont téléchargés.

use std::fs::File;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
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

const ALL: [Sfx; 11] = [Sfx::Move, Sfx::Select, Sfx::Back, Sfx::Open, Sfx::Close, Sfx::Tab, Sfx::Key, Sfx::Launch, Sfx::Notify, Sfx::Error, Sfx::Boot];

/// Fichiers d'un pack (noms des sons de l'interface du Steam Deck), par ordre de préférence.
fn files(s: Sfx) -> &'static [&'static str] {
    match s {
        Sfx::Move => &["deck_ui_navigation", "deck_ui_tile_scroll"],
        Sfx::Select => &["deck_ui_default_activation"],
        Sfx::Back => &["deck_ui_hide_modal", "deck_ui_out_of_game_detail"],
        Sfx::Open => &["deck_ui_into_game_detail", "deck_ui_show_modal"],
        Sfx::Close => &["deck_ui_side_menu_fly_out", "deck_ui_out_of_game_detail", "deck_ui_hide_modal"],
        Sfx::Tab => &["deck_ui_tab_transition_01", "deck_ui_navigation"],
        Sfx::Key => &["deck_ui_typing", "deck_ui_navigation"],
        Sfx::Launch => &["deck_ui_launch_game"],
        Sfx::Notify => &["deck_ui_message_toast", "deck_ui_toast"],
        Sfx::Error => &["confirmation_negative", "deck_ui_misc_10", "deck_ui_misc_08"],
        Sfx::Boot => &["deck_ui_achievement_toast", "deck_ui_launch_game"],
    }
}

enum Cmd {
    Play(Sfx, Instant),
    Music(bool),
    Volumes(f32, f32),
    /// Sons décodés d'un pack (dans l'ordre de `ALL`).
    Pack(Vec<Option<Arc<Vec<f32>>>>),
    /// Musique décodée (stéréo 44,1 kHz, 16 bits) ; `None` : aucune.
    Tune(Option<Arc<Vec<i16>>>),
}

static TX: OnceLock<Sender<Cmd>> = OnceLock::new();
static READY: AtomicBool = AtomicBool::new(false);

// ---------- Décodage ----------

/// Décode un fichier audio en PCM flottant stéréo 44,1 kHz (entrelacé).
fn decode(path: &Path) -> Option<Vec<f32>> {
    let file = File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(e) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(e);
    }
    let probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).ok()?;
    let mut format = probed.format;
    let track = format.default_track()?;
    let id = track.id;
    let rate = track.codec_params.sample_rate? as f64;
    let mut dec = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).ok()?;
    let mut raw: Vec<f32> = vec![];
    let mut channels = 2usize;
    loop {
        let Ok(pkt) = format.next_packet() else { break };
        if pkt.track_id() != id {
            continue;
        }
        match dec.decode(&pkt) {
            Ok(buf) => {
                let spec = *buf.spec();
                channels = spec.channels.count().max(1);
                let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, spec);
                sb.copy_interleaved_ref(buf);
                raw.extend_from_slice(sb.samples());
            }
            Err(SymError::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }
    if raw.is_empty() {
        return None;
    }
    // Stéréo.
    let frames = raw.len() / channels;
    let at = |i: usize| -> (f32, f32) {
        let f = &raw[i * channels..(i + 1) * channels];
        if channels == 1 { (f[0], f[0]) } else { (f[0], f[1]) }
    };
    // Rééchantillonnage linéaire vers 44,1 kHz.
    let out_frames = ((frames as f64) * SR as f64 / rate) as usize;
    let mut out = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let p = i as f64 * rate / SR as f64;
        let (a, frac) = (p as usize, (p - p.floor()) as f32);
        let x = at(a.min(frames - 1));
        let y = at((a + 1).min(frames - 1));
        out.push(x.0 + (y.0 - x.0) * frac);
        out.push(x.1 + (y.1 - x.1) * frac);
    }
    Some(out)
}

/// Premier fichier audio de ce nom (toute extension) dans `dir`, en descendant d'un niveau (les packs ont un dossier interne).
fn find(dir: &Path, stem: &str) -> Option<PathBuf> {
    let mut dirs = vec![dir.to_path_buf()];
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        if e.path().is_dir() {
            dirs.push(e.path());
        }
    }
    for d in dirs {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            let p = e.path();
            let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            if name == stem && ["wav", "mp3", "ogg", "flac", "m4a", "webm"].contains(&ext.as_str()) {
                return Some(p);
            }
        }
    }
    None
}

fn load_sfx(dir: &Path) -> Vec<Option<Arc<Vec<f32>>>> {
    ALL.iter()
        .map(|&s| {
            files(s).iter().find_map(|stem| {
                let mut b = decode(&find(dir, stem)?)?;
                // Fichier vide (certains packs laissent des silences) : on passe au suivant.
                let peak = b.iter().fold(0.0f32, |m, x| m.max(x.abs()));
                if b.len() < 200 || peak < 0.001 {
                    return None;
                }
                // Niveau homogène d'un pack à l'autre, sans amplifier les sons faibles de plus de 2×.
                let g = (0.7 / peak).min(2.0);
                b.iter_mut().for_each(|x| *x *= g);
                Some(Arc::new(b))
            })
        })
        .collect()
}

fn load_music(file: &Path) -> Option<Arc<Vec<i16>>> {
    let b = decode(file)?;
    let peak = b.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(0.001);
    let g = 0.9 / peak;
    Some(Arc::new(b.iter().map(|x| (x * g * 32_000.0) as i16).collect()))
}

/// Charge un pack de sons (dossier) et une musique (fichier) : décodage en tâche de fond, remplacement à chaud.
pub fn set_packs(sfx_dir: Option<PathBuf>, music: Option<PathBuf>) {
    let Some(tx) = TX.get().cloned() else { return };
    std::thread::spawn(move || {
        let pack = sfx_dir.map(|d| load_sfx(&d)).unwrap_or_else(|| ALL.iter().map(|_| None).collect());
        let _ = tx.send(Cmd::Pack(pack));
        let tune = music.and_then(|m| load_music(&m));
        let _ = tx.send(Cmd::Tune(tune));
    });
}

/// Fichier de musique d'un pack de musique (`menu_music.*`).
pub fn music_file(dir: &Path) -> Option<PathBuf> {
    find(dir, "menu_music")
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
    let mut sfx: Vec<Option<Arc<Vec<f32>>>> = ALL.iter().map(|_| None).collect();
    let mut out = loop {
        match open_out() {
            Some(o) => break o,
            None => std::thread::sleep(Duration::from_secs(5)),
        }
    };
    READY.store(true, Ordering::SeqCst);
    let (mut sfx_vol, mut music_vol) = (0.8f32, 0.4f32);
    let mut voices: Vec<Voice> = vec![];
    let mut music: Option<Arc<Vec<i16>>> = None;
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
                        if let Some(buf) = &sfx[s as usize] {
                            if at.elapsed() < Duration::from_millis(150) && voices.len() < 12 {
                                voices.push(Voice { buf: buf.clone(), pos: 0 });
                            }
                        }
                    }
                    Cmd::Music(on) => want = on,
                    Cmd::Volumes(s, m) => (sfx_vol, music_vol) = (s, m),
                    Cmd::Pack(p) => {
                        sfx = p;
                        voices.clear();
                    }
                    Cmd::Tune(t) => {
                        music = t;
                        mpos = 0;
                    }
                }
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
                        mix[f * 2] += m[mpos] as f32 / 32_768.0 * mgain;
                        mix[f * 2 + 1] += m[mpos + 1] as f32 / 32_768.0 * mgain;
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

/// Démarre le thread audio (ouverture de la sortie, mixage).
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Décode les packs téléchargés par Hearth, s'ils sont là (sinon le test ne vérifie rien).
    #[test]
    fn decodes_downloaded_packs() {
        let base = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Hearth").join("sounds");
        let Ok(dirs) = std::fs::read_dir(&base) else { return };
        for d in dirs.flatten() {
            if let Some(m) = music_file(&d.path()) {
                let t = load_music(&m).expect("musique décodée");
                assert!(t.len() > SR * 2 * 10, "musique trop courte : {}", t.len());
                println!("musique {} : {} s", m.display(), t.len() / 2 / SR);
            } else {
                let p = load_sfx(&d.path());
                let n = p.iter().flatten().count();
                println!("pack {} : {n}/11 sons", d.path().display());
                assert!(n >= 8, "pack incomplet : {n}");
            }
        }
    }
}
