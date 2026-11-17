//! Décodage d'images hors du thread de l'interface : un JPEG de jaquette coûte 10 à 30 ms, assez pour saccader l'affichage.
//! Les images sont décodées et réduites à la taille d'affichage dans des threads, puis transformées en `slint::Image` (instantané).

use std::path::Path;

pub type Buf = slint::SharedPixelBuffer<slint::Rgba8Pixel>;

/// Décode `path` en l'inscrivant dans `max_w` × `max_h` (jamais agrandi).
pub fn decode(path: &Path, max_w: u32, max_h: u32) -> Option<Buf> {
    let img = image::open(path).ok()?;
    let img = if img.width() > max_w || img.height() > max_h { img.resize(max_w, max_h, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.to_rgba8();
    Some(Buf::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height()))
}

/// Image Slint à partir d'un tampon décodé (rapide : pas de décodage).
pub fn image(b: &Buf) -> slint::Image {
    slint::Image::from_rgba8(b.clone())
}
