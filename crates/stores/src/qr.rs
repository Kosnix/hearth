//! QR code d'une URL (achat sur téléphone) sous forme de pixels RGB.

use qrcode::{Color, QrCode};

/// Largeur, hauteur et pixels RGB (noir sur blanc, marge de 4 modules), `scale` pixels par module.
pub fn rgb(data: &str, scale: usize) -> Option<(u32, u32, Vec<u8>)> {
    let code = QrCode::new(data.as_bytes()).ok()?;
    let n = code.width();
    let side = (n + 8) * scale;
    let colors = code.to_colors();
    let mut px = vec![255u8; side * side * 3];
    for y in 0..n {
        for x in 0..n {
            if colors[y * n + x] == Color::Dark {
                for dy in 0..scale {
                    for dx in 0..scale {
                        let i = (((y + 4) * scale + dy) * side + (x + 4) * scale + dx) * 3;
                        px[i..i + 3].fill(0);
                    }
                }
            }
        }
    }
    Some((side as u32, side as u32, px))
}
