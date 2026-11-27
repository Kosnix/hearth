//! Packs audio DeckThemes (sons de l'interface et musiques) : catalogue complet, téléchargement à la demande, application à chaud.
//! Rien n'est embarqué dans Hearth : par défaut le pack « Switch » et la musique « Gleaming Void » sont téléchargés au premier démarrage.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::rc::Rc;

use slint::VecModel;

use crate::i18n::{self, tr};
use crate::{with_shell, PickItem, Shell};

const SWITCH: (&str, &str) = ("Switch", "7cd3f7a1-78a0-40a7-acae-83aef756194d");
const GLEAMING: (&str, &str) = ("Gleaming Void", "ce05a74e-aae2-49ea-8b22-e8df46c56e95");
const CATALOG: &str = "https://api.deckthemes.com/themes?perPage=100&filters=AUDIO&order=Most%20Downloaded&page=";
const BLOB: &str = "https://api.deckthemes.com/blobs/";

#[derive(Clone)]
pub struct Pack {
    pub name: String,
    pub author: String,
    pub blob: String,
    pub music: bool,
    pub downloads: i64,
}

fn dir_of(blob: &str) -> PathBuf {
    stores::base_dir().join("sounds").join(blob)
}

fn installed(blob: &str) -> bool {
    dir_of(blob).join(".ok").exists()
}

fn key(music: bool) -> &'static str {
    if music { "music_pack" } else { "sfx_pack" }
}

/// Pack courant : (identifiant du téléchargement, nom). Vide pour « aucune musique ».
pub fn current(music: bool) -> (String, String) {
    match i18n::pref(key(music)) {
        Some(v) => match v.split_once('|') {
            Some((b, n)) => (b.to_string(), n.to_string()),
            None => (String::new(), String::new()),
        },
        None => {
            let d = if music { GLEAMING } else { SWITCH };
            (d.1.to_string(), d.0.to_string())
        }
    }
}

// Le pack par défaut (démarrage) et un choix explicite de l'utilisateur peuvent se déclencher presque
// en même temps sur deux threads différents : sans verrou, les deux écriraient/décompresseraient le
// même `pack.zip` en parallèle (pack corrompu, `.ok` écrit avant que l'autre ait fini d'extraire).
static DL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Télécharge et décompresse un pack (une fois). Bloquant.
fn download(blob: &str) -> bool {
    let _guard = DL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if installed(blob) {
        return true;
    }
    let Some(bytes) = stores::download(&format!("{BLOB}{blob}")) else { return false };
    if bytes.len() < 1000 || !bytes.starts_with(b"PK") {
        return false;
    }
    let dir = dir_of(blob);
    let _ = std::fs::remove_dir_all(&dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let zip = dir.join("pack.zip");
    if std::fs::write(&zip, &bytes).is_err() {
        return false;
    }
    // `tar` (bsdtar, fourni avec Windows) sait lire les archives zip.
    let ok = std::process::Command::new("tar").arg("-xf").arg(&zip).arg("-C").arg(&dir).creation_flags(0x0800_0000).status().is_ok_and(|s| s.success());
    let _ = std::fs::remove_file(&zip);
    if ok {
        let _ = std::fs::write(dir.join(".ok"), "1");
    }
    ok
}

/// Envoie au moteur audio les packs courants (ceux qui sont téléchargés).
fn apply() {
    let sfx = Some(current(false).0).filter(|b| !b.is_empty() && installed(b)).map(|b| dir_of(&b));
    let music = Some(current(true).0).filter(|b| !b.is_empty() && installed(b)).and_then(|b| system::sfx::music_file(&dir_of(&b)));
    system::sfx::set_packs(sfx, music);
}

/// Au démarrage : télécharge les packs par défaut (ou courants) s'ils manquent, puis les applique.
pub fn start() {
    std::thread::spawn(|| {
        for music in [false, true] {
            let (blob, _) = current(music);
            if !blob.is_empty() {
                download(&blob);
            }
        }
        let _ = slint::invoke_from_event_loop(apply);
    });
}

/// Nom affiché dans les Réglages.
pub fn label(music: bool) -> String {
    let (blob, name) = current(music);
    if blob.is_empty() {
        tr("Aucune")
    } else if installed(&blob) {
        name
    } else {
        format!("{name} …")
    }
}

fn fetch_catalog() -> Vec<Pack> {
    let mut out = vec![];
    for page in 1..=10 {
        let Some(j) = stores::fetch_json(&format!("{CATALOG}{page}")) else { break };
        let items = j["items"].as_array().cloned().unwrap_or_default();
        if items.is_empty() {
            break;
        }
        for it in &items {
            let (Some(name), Some(blob)) = (it["displayName"].as_str(), it["download"]["id"].as_str()) else { continue };
            out.push(Pack {
                name: name.to_string(),
                author: it["author"]["username"].as_str().unwrap_or("").to_string(),
                blob: blob.to_string(),
                music: it["target"] == "Music",
                downloads: it["download"]["downloadCount"].as_i64().unwrap_or(0),
            });
        }
        if out.len() >= j["total"].as_u64().unwrap_or(0) as usize {
            break;
        }
    }
    out
}

impl Shell {
    /// Sélecteur de packs (sons ou musiques) : tout le catalogue DeckThemes, du plus téléchargé au moins téléchargé.
    pub fn sound_picker(&self, music: bool) {
        *self.pick_mode.borrow_mut() = crate::addflow::PickMode::Sounds(music);
        let title = tr(if music { "Packs de musique" } else { "Packs de sons" });
        if self.sound_catalog.borrow().is_empty() {
            self.sound_list.borrow_mut().clear();
            self.pick_set(title.clone(), tr("Chargement du catalogue…"), vec![PickItem { label: tr("Chargement du catalogue…").into(), sub: "".into(), glyph: "\u{E721}".into(), art: Default::default() }]);
            self.ui.set_zone(6);
            std::thread::spawn(move || {
                let list = fetch_catalog();
                let _ = slint::invoke_from_event_loop(move || {
                    with_shell(|s| {
                        *s.sound_catalog.borrow_mut() = list;
                        s.sound_list_show(music);
                    })
                });
            });
        } else {
            self.sound_list_show(music);
            self.ui.set_zone(6);
        }
    }

    fn sound_list_show(&self, music: bool) {
        let list: Vec<Pack> = self.sound_catalog.borrow().iter().filter(|p| p.music == music).cloned().collect();
        let title = tr(if music { "Packs de musique" } else { "Packs de sons" });
        if list.is_empty() {
            self.sound_list.borrow_mut().clear();
            self.pick_set(title, tr("Catalogue indisponible"), vec![PickItem { label: tr("Catalogue indisponible").into(), sub: "".into(), glyph: "\u{E783}".into(), art: Default::default() }]);
            return;
        }
        let cur = current(music).0;
        let mut items: Vec<PickItem> = vec![];
        let mut first = 0;
        if music {
            items.push(PickItem { label: tr("Aucune musique").into(), sub: "".into(), glyph: "\u{E74F}".into(), art: Default::default() });
        }
        for (i, p) in list.iter().enumerate() {
            if p.blob == cur {
                first = i + music as usize;
            }
            let state = if installed(&p.blob) { tr("Téléchargé") } else { tr("À télécharger") };
            items.push(PickItem { label: p.name.as_str().into(), sub: format!("{} · {} ↓ · {state}", p.author, p.downloads).into(), glyph: (if music { "\u{E8D6}" } else { "\u{E767}" }).into(), art: Default::default() });
        }
        *self.sound_list.borrow_mut() = list;
        self.pick_set(title, tr("Choisissez un pack : il est téléchargé puis appliqué"), items);
        self.ui.set_psel(first as i32);
    }

    /// Un pack est choisi : téléchargement si besoin, puis application.
    pub fn sound_pick(&self, i: usize, music: bool) {
        // Ligne d'attente (« Chargement du catalogue… », « Catalogue indisponible ») : rien à choisir —
        // elle prenait sinon la place de « Aucune musique » et coupait la musique.
        if self.sound_list.borrow().is_empty() {
            return;
        }
        let idx = if music {
            if i == 0 {
                i18n::set_pref(key(true), "|");
                apply();
                self.push_notif(&tr("Son"), &tr("Aucune musique"));
                self.ui.set_zone(self.home_zone());
                self.refresh_sound_rows();
                return;
            }
            i - 1
        } else {
            i
        };
        let Some(p) = self.sound_list.borrow().get(idx).cloned() else { return };
        self.push_notif(&tr("Son"), &format!("{} {}…", tr("Téléchargement du pack"), p.name));
        self.ui.set_zone(self.home_zone());
        std::thread::spawn(move || {
            let ok = download(&p.blob);
            let _ = slint::invoke_from_event_loop(move || {
                with_shell(|s| {
                    if ok {
                        i18n::set_pref(key(music), &format!("{}|{}", p.blob, p.name));
                        apply();
                        s.push_notif(&tr("Son"), &format!("{} : {}", tr("Pack appliqué"), p.name));
                        if !music {
                            system::sfx::play(system::sfx::Sfx::Select);
                        }
                    } else {
                        s.push_notif(&tr("Son"), &tr("Échec du téléchargement"));
                    }
                    s.refresh_sound_rows();
                })
            });
        });
    }

    fn refresh_sound_rows(&self) {
        if self.oobe_active() {
            self.oobe_sync();
        } else {
            self.open_cat_keep(self.ui.get_cat());
        }
    }

    fn pick_set(&self, title: String, hint: String, items: Vec<PickItem>) {
        self.ui.set_pick_title(title.into());
        self.ui.set_pick_hint(hint.into());
        self.ui.set_psel(0);
        self.ui.set_pick_items(Rc::new(VecModel::from(items)).into());
    }
}
