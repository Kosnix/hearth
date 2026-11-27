//! Décodage d'images hors du thread de l'interface : un JPEG de jaquette coûte 10 à 30 ms, assez pour saccader l'affichage.
//! Les images sont décodées et réduites à la taille d'affichage dans des threads, puis transformées en `slint::Image` (instantané).

use std::path::Path;

pub type Buf = slint::SharedPixelBuffer<slint::Rgba8Pixel>;

/// Décode `path` en l'inscrivant dans `max_w` × `max_h` (jamais agrandi).
/// Fichier refusé avant décodage s'il est démesuré (plus de 16 384 px de côté) : le profil release
/// arrête le programme à la moindre panique, un décodeur qui en déclencherait une fermerait Hearth
/// (relancé ensuite par le superviseur) — rien ne peut la rattraper ici.
pub fn decode(path: &Path, max_w: u32, max_h: u32) -> Option<Buf> {
    let (w, h) = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_dimensions().ok()?;
    if w == 0 || h == 0 || w > 16_384 || h > 16_384 {
        return None;
    }
    let img = image::open(path).ok()?;
    let img = if img.width() > max_w || img.height() > max_h { img.resize(max_w, max_h, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.to_rgba8();
    Some(Buf::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height()))
}

/// Image Slint à partir d'un tampon décodé (rapide : pas de décodage).
pub fn image(b: &Buf) -> slint::Image {
    slint::Image::from_rgba8(b.clone())
}

/// Priorité basse pour le thread courant : le téléchargement et le décodage de fond ne doivent jamais retarder l'interface.
pub fn background() {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThread() -> isize;
        fn SetThreadPriority(h: isize, p: i32) -> i32;
    }
    unsafe {
        SetThreadPriority(GetCurrentThread(), -2); // THREAD_PRIORITY_LOWEST
    }
}

static SLOTS: (std::sync::Mutex<u32>, std::sync::Condvar) = (std::sync::Mutex::new(0), std::sync::Condvar::new());

/// Rend le jeton à la fin de `f()`. (Utile seulement en profil debug : en release, `panic = "abort"`
/// arrête tout le programme à la moindre panique, voir `decode`.)
struct Slot;
impl Drop for Slot {
    fn drop(&mut self) {
        *SLOTS.0.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        SLOTS.1.notify_one();
    }
}

/// Au plus 2 décodages de fond en même temps (le rafraîchissement de tous les visuels lançait un thread par jeu).
pub fn limited<T>(f: impl FnOnce() -> T) -> T {
    {
        let mut n = SLOTS.0.lock().unwrap_or_else(|e| e.into_inner());
        while *n >= 2 {
            n = SLOTS.1.wait(n).unwrap_or_else(|e| e.into_inner());
        }
        *n += 1;
    }
    let _slot = Slot;
    background();
    f()
}
