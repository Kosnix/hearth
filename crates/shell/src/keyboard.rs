//! Dispositions du clavier visuel : 4 rangées de 10 touches (chiffres, puis trois rangées de lettres) et une page « Alt » (accents, symboles, lettres en plus).

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::i18n;

pub struct Layout {
    pub name: &'static str,
    pub main: [&'static str; 4],
    pub alt: [&'static str; 4],
    /// Libellé de la touche qui ouvre la page Alt.
    pub alt_label: &'static str,
}

pub const LAYOUTS: [Layout; 10] = [
    Layout { name: "AZERTY", main: ["1234567890", "azertyuiop", "qsdfghjklm", "wxcvbn.-_@"], alt: ["éèêëàâäùûü", "ôöîïçœæñÿ'", "@.-_,;:!?/", "+*=%€$#&()"], alt_label: "é è à" },
    Layout { name: "QWERTY", main: ["1234567890", "qwertyuiop", "asdfghjkl'", "zxcvbnm.-_"], alt: ["@.-_,;:!?/", "+*=%€$#&()", "éèàùçñüöä¿", "'\"~^[]{}<>"], alt_label: "@ # é" },
    Layout { name: "QWERTZ", main: ["1234567890", "qwertzuiop", "asdfghjklö", "yxcvbnmäü."], alt: ["ß@.-_,;:!?", "+*=%€$#&()", "éèàùçñ'\"~^", "[]{}<>/|`¿"], alt_label: "ß @ é" },
    Layout { name: "Español", main: ["1234567890", "qwertyuiop", "asdfghjklñ", "zxcvbnm.-_"], alt: ["áéíóúüç@.-", "¿¡_,;:!?/+", "=%€$#&()'\"", "^[]{}<>/|`~"], alt_label: "á é ñ" },
    Layout { name: "Português", main: ["1234567890", "qwertyuiop", "asdfghjklç", "zxcvbnm.-_"], alt: ["áâãàéêíóôõ", "úüñ@.-_,;:", "?/+*=%€$#&", ")'\"~^[]{}<"], alt_label: "ã é ç" },
    Layout { name: "Русский", main: ["1234567890", "йцукенгшщз", "фывапролдж", "ячсмитьбюэ"], alt: ["хъёіїґ@.-_", ",;:!?/+*=%", "$#&()'\"~^[", "{}<>/|`№«»"], alt_label: "х ъ ё" },
    Layout { name: "العربية", main: ["1234567890", "ضصثقفغعهخح", "شسيبلاتنمك", "ظطذدزروةىء"], alt: ["جأإآؤئ،؛؟.", "@-_,;:!?/+", "=%$#&()'\"~", "[]{}<>/|`«"], alt_label: "ج أ ؤ" },
    Layout { name: "हिन्दी", main: ["1234567890", "कखगघचछजझटठ", "डढणतथदधनपफ", "बभमयरलवशषस"], alt: ["हअआइईउऊएऐओ", "औऋािीुूेैो", "ौंँः़्।ॐ॰ॅ", "@.-_,;:!?/"], alt_label: "अ आ ा" },
    Layout { name: "বাংলা", main: ["1234567890", "কখগঘচছজঝটঠ", "ডঢণতথদধনপফ", "বভমযরলশষসহ"], alt: ["অআইঈউঊএঐওঔ", "ািীুূেৈোৌৃ", "ংঃঁ্ৎ।@.-_", ".-_,;:!?/+"], alt_label: "অ আ া" },
    Layout { name: "اردو", main: ["1234567890", "ابپتٹثجچحخ", "دڈذرڑزژسشص", "ضطظعغفقکگل"], alt: ["مںوہھءیےۓآ", "أؤئۂة@.-_،", "؛؟۔!:;/+*=", "%$#&()'\"~^"], alt_label: "م ں و" },
];

static CHOICE: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Disposition par défaut d'une langue.
fn default_for(lang: &str) -> usize {
    match lang {
        "fr" => 0,
        "es" => 3,
        "pt" => 4,
        "ru" => 5,
        "ar" => 6,
        "hi" => 7,
        "bn" => 8,
        "ur" => 9,
        _ => 1,
    }
}

/// Disposition choisie, sinon celle de la langue.
pub fn current() -> usize {
    let c = CHOICE.load(Ordering::Relaxed);
    if c < LAYOUTS.len() { c } else { default_for(i18n::code()) }
}

pub fn load() {
    if let Some(n) = i18n::pref("kb_layout").and_then(|s| s.parse::<usize>().ok()) {
        if n < LAYOUTS.len() {
            CHOICE.store(n, Ordering::Relaxed);
        }
    }
}

pub fn shift(delta: i32) {
    let n = (current() as i32 + delta).rem_euclid(LAYOUTS.len() as i32) as usize;
    CHOICE.store(n, Ordering::Relaxed);
    i18n::set_pref("kb_layout", &n.to_string());
}

/// La langue change : sans choix explicite, la disposition suit la langue.
pub fn follow_language() {
    if i18n::pref("kb_layout").is_none() {
        CHOICE.store(usize::MAX, Ordering::Relaxed);
    }
}

pub fn layout() -> &'static Layout {
    &LAYOUTS[current()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_has_ten_keys() {
        for l in LAYOUTS.iter() {
            for r in l.main.iter().chain(l.alt.iter()) {
                assert_eq!(r.chars().count(), 10, "{} : {r}", l.name);
            }
        }
    }
}
