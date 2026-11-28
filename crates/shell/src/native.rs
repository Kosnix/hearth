//! Boîtes de dialogue Windows natives (sélection d'un fichier image : photo de profil, jeu local).

use std::path::PathBuf;

use windows::core::PCWSTR;
use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW};

/// Ouvre « Ouvrir » avec un filtre (`description\0*.ext;*.ext\0\0`) ; renvoie le fichier choisi.
pub fn open_file(title: &str, filter: &str) -> Option<PathBuf> {
    let mut file = [0u16; 1024];
    let wfilter: Vec<u16> = filter.encode_utf16().chain([0, 0]).collect();
    let wtitle: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    // La fenêtre au premier plan si elle est à nous (Bureau ou console), sinon la console par défaut :
    // sinon, ouvert depuis le Bureau, le sélecteur se rattachait toujours à la console (masquée ou non
    // au premier plan), pouvant apparaître derrière la fenêtre réellement active.
    let owner = if wm::foreground_is_mine() { Some(wm::foreground()) } else { wm::find_window("Hearth") };
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

/// Question Oui / Non (boîte Windows) ; vrai si l'utilisateur répond Oui.
pub fn confirm(text: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO};
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("Hearth"), MB_YESNO | MB_ICONQUESTION) == IDYES }
}

/// Capture de l'écran principal enregistrée en PNG dans `dest` (GDI : un jeu en plein écran exclusif
/// peut donner une image noire). Bloquant.
pub fn screenshot(dest: &std::path::Path) -> bool {
    use windows::Win32::Graphics::Gdi::*;
    let (x, y, w, h) = wm::primary_monitor();
    if w <= 0 || h <= 0 {
        return false;
    }
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let ok = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY).is_ok();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(mem, bmp, 0, h as u32, Some(px.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if !ok || lines == 0 {
            return false;
        }
        // BGRA → RGBA, opaque.
        for p in px.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
            p[3] = 255;
        }
        let _ = std::fs::create_dir_all(dest.parent().unwrap_or(std::path::Path::new(".")));
        image::save_buffer(dest, &px, w as u32, h as u32, image::ColorType::Rgba8).is_ok()
    }
}
