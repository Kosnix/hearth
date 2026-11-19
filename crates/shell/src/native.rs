//! Boîtes de dialogue Windows natives (sélection d'un fichier image : photo de profil, jeu local).

use std::path::PathBuf;

use windows::core::PCWSTR;
use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW};

/// Ouvre « Ouvrir » avec un filtre (`description\0*.ext;*.ext\0\0`) ; renvoie le fichier choisi.
pub fn open_file(title: &str, filter: &str) -> Option<PathBuf> {
    let mut file = [0u16; 1024];
    let wfilter: Vec<u16> = filter.encode_utf16().chain([0, 0]).collect();
    let wtitle: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let owner = wm::find_window("Hearth");
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner.unwrap_or_default(),
        lpstrFilter: PCWSTR(wfilter.as_ptr()),
        lpstrFile: windows::core::PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        lpstrTitle: PCWSTR(wtitle.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        return None;
    }
    let n = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Some(PathBuf::from(String::from_utf16_lossy(&file[..n])))
}

/// Image choisie, recadrée au carré et réduite à 256 px, enregistrée en PNG dans `dest`.
pub fn square_image(src: &std::path::Path, dest: &std::path::Path) -> bool {
    let Ok(img) = image::open(src) else { return false };
    let side = img.width().min(img.height());
    let (x, y) = ((img.width() - side) / 2, (img.height() - side) / 2);
    let sq = img.crop_imm(x, y, side, side).resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
    sq.save_with_format(dest, image::ImageFormat::Png).is_ok()
}
