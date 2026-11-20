//! Assistant de première exécution : étapes obligatoires et facultatives, reprise là où on s'était arrêté, impossible à quitter avant la fin.
//! Les étapes réutilisent les lignes des Réglages (langue, affichage, son, réseau, manettes, thème, comptes).

use std::rc::Rc;

use slint::{Model, SharedString, VecModel};

use crate::i18n::tr;
use crate::{settings, with_shell, SettingRow, Shell};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Welcome,
    /// Catégorie de Réglages.
    Cat(i32),
    Profile,
    System,
    Summary,
}

struct Step {
    kind: Kind,
    title: &'static str,
    desc: &'static str,
    skippable: bool,
}

const STEPS: [Step; 11] = [
    Step { kind: Kind::Welcome, title: "Bienvenue", desc: "Quelques réglages avant de commencer. Tout reste modifiable plus tard dans les Réglages.", skippable: false },
    Step { kind: Kind::Cat(4), title: "Connecter la manette", desc: "Branchez ou appairez votre manette : Hearth la détecte tout seul. Sans manette, continuez à la souris et au clavier. Astuce : cliquez les deux sticks (L3 + R3) pour piloter la souris.", skippable: true },
    Step { kind: Kind::Cat(5), title: "Langue", desc: "Choisissez la langue de l'interface.", skippable: false },
    Step { kind: Kind::Cat(1), title: "Affichage", desc: "Résolution et fréquence de votre écran. Un retour automatique vous protège d'un mauvais choix.", skippable: true },
    Step { kind: Kind::Cat(0), title: "Son", desc: "Sortie audio et volumes des effets et de la musique.", skippable: true },
    Step { kind: Kind::Cat(2), title: "Réseau", desc: "Connectez le Wi-Fi pour la boutique et les téléchargements (inutile en Ethernet).", skippable: true },
    Step { kind: Kind::Profile, title: "Votre profil", desc: "Votre nom et votre photo, affichés en haut de l'écran.", skippable: false },
    Step { kind: Kind::Cat(13), title: "Thème", desc: "Couleur d'accentuation et fond de l'interface.", skippable: true },
    Step { kind: Kind::Cat(12), title: "Comptes", desc: "Reliez vos boutiques pour retrouver vos jeux. Vous pourrez le faire plus tard.", skippable: true },
    Step { kind: Kind::System, title: "Composants", desc: "Une seule autorisation Windows : elle permet à Hearth d'installer ce dont les jeux ont besoin (manette virtuelle, bibliothèques Visual C++).", skippable: false },
    Step { kind: Kind::Summary, title: "Terminé", desc: "", skippable: false },
];

fn done_file() -> std::path::PathBuf {
    stores::base_dir().join("oobe.done")
}

fn step_file() -> std::path::PathBuf {
    stores::base_dir().join("oobe.step")
}

/// L'assistant a-t-il déjà été mené jusqu'au bout ? (`HEARTH_OOBE=1` force son affichage.)
pub fn needed() -> bool {
    std::env::var_os("HEARTH_OOBE").is_some() || !done_file().exists()
}

pub fn is_done() -> bool {
    done_file().exists()
}

fn row(label: &str, value: String, adjustable: bool) -> SettingRow {
    SettingRow { label: tr(label).into(), value: value.into(), adjustable }
}

fn hidden_cmd(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(program);
    c.creation_flags(0x0800_0000);
    c
}

/// État des composants : 0 à faire, 1 en cours, 2 en place, 3 échec. Ordre : autorisation, manette virtuelle, VC++ x64, VC++ x86.
fn detect_components() -> [u8; 4] {
    let ok = |b: bool| if b { 2 } else { 0 };
    let vc = |key: &str| hidden_cmd("reg").args(["query", key, "/v", "Installed"]).output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("0x1"));
    [
        ok(system::admin::enabled() && stores::base_dir().join("helper-v3.done").exists()),
        ok(hidden_cmd("sc").args(["query", "ViGEmBus"]).output().is_ok_and(|o| o.status.success())),
        ok(vc(r"HKLM\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64")),
        ok(vc(r"HKLM\SOFTWARE\WOW6432Node\Microsoft\VisualStudio\14.0\VC\Runtimes\x86")),
    ]
}

const COMPONENTS: [&str; 4] = ["Autorisation administrateur", "Manette virtuelle (ViGEmBus)", "Visual C++ (x64)", "Visual C++ (x86)"];

impl Shell {
    pub fn oobe_active(&self) -> bool {
        self.oobe_step.get() >= 0
    }

    /// Zone où l'on revient après un clavier, un QR code ou un sélecteur : l'assistant s'il est ouvert, sinon les Réglages.
    pub fn home_zone(&self) -> i32 {
        if self.oobe_active() { 13 } else { 3 }
    }

    /// Au démarrage : ouvre l'assistant s'il n'a jamais été terminé.
    pub fn oobe_start(&self) {
        if !needed() {
            return;
        }
        let saved = std::fs::read_to_string(step_file()).ok().and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(0);
        let titles: Vec<SharedString> = STEPS.iter().map(|s| tr(s.title).into()).collect();
        self.ui.set_oobe_titles(Rc::new(VecModel::from(titles)).into());
        self.ui.set_oobe_count(STEPS.len() as i32);
        self.ui.set_oobe_on(true);
        self.ui.set_oobe_rows(self.oobe_rows.clone().into());
        self.oobe_enter(saved.clamp(0, STEPS.len() as i32 - 1));
    }

    fn oobe_enter(&self, step: i32) {
        self.oobe_step.set(step);
        let _ = std::fs::write(step_file(), step.to_string());
        let s = &STEPS[step as usize];
        self.ui.set_oobe_step(step);
        self.ui.set_oobe_title(tr(if step == STEPS.len() as i32 - 1 { "Tout est prêt" } else if step == 0 { "Bienvenue dans Hearth" } else { s.title }).into());
        self.ui.set_oobe_desc(tr(s.desc).into());
        self.ui.set_oobe_sel(0);
        self.ui.set_oobe_pad(matches!(s.kind, Kind::Cat(4)));
        self.ui.set_zone(13);
        match s.kind {
            Kind::Cat(c) => {
                self.ui.set_cat(c);
                self.ui.set_srow(0);
                let hint = self.settings.open(c);
                self.ui.set_oobe_hint(hint.into());
                if settings::needs_fetch(c) {
                    self.spawn_fetch(c);
                }
            }
            Kind::System => {
                *self.oobe_sys.borrow_mut() = detect_components();
                self.ui.set_oobe_hint("".into());
            }
            Kind::Summary => {
                let p = self.profile_current();
                let theme = tr(crate::theme::PRESETS.get(crate::theme::preset() as usize).map(|p| p.0).unwrap_or("Personnalisé"));
                let res = system::display::current_mode().map(|m| format!("{} × {} @ {} Hz", m.w, m.h, m.hz)).unwrap_or_default();
                self.ui.set_oobe_desc(format!("{}\n{} : {}   ·   {} : {}   ·   {} : {}", tr("Hearth est configuré. Tout est modifiable dans les Réglages."), tr("Profil"), p.name, tr("Thème"), theme, tr("Écran"), res).into());
                self.ui.set_oobe_hint("".into());
            }
            _ => self.ui.set_oobe_hint("".into()),
        }
        self.oobe_sync();
    }

    /// Reconstruit les lignes de l'étape (réglages + « Continuer » / « Passer »).
    pub fn oobe_sync(&self) {
        if !self.oobe_active() {
            return;
        }
        let step = self.oobe_step.get() as usize;
        let s = &STEPS[step];
        let mut rows: Vec<SettingRow> = vec![];
        let mut content = 0usize;
        match s.kind {
            Kind::Welcome => rows.push(row("Commencer", String::new(), false)),
            Kind::Summary => rows.push(row("Commencer à jouer", String::new(), false)),
            Kind::Cat(c) => {
                self.ui.set_oobe_hint(self.settings.refresh(c).into());
                for i in 0..self.settings.rows.row_count() {
                    if let Some(r) = self.settings.rows.row_data(i) {
                        rows.push(r);
                    }
                }
                content = rows.len();
            }
            Kind::Profile => {
                let p = self.profile_current();
                rows.push(row("Nom du profil", p.name, false));
                rows.push(row("Photo de profil", format!("{} {}", tr("Avatar"), p.avatar + 1), false));
                content = 2;
            }
            Kind::System => {
                let st = *self.oobe_sys.borrow();
                for (i, name) in COMPONENTS.iter().enumerate() {
                    let v = match st[i] {
                        2 => tr("En place"),
                        1 => tr("Installation…"),
                        3 => tr("Échec"),
                        _ => tr("À installer"),
                    };
                    rows.push(row(name, v, false));
                }
                let busy = st.contains(&1);
                let all = st.iter().all(|&x| x == 2);
                if !all && !busy {
                    rows.push(row(if st.contains(&3) { "Réessayer" } else { "Installer maintenant" }, String::new(), false));
                }
                content = rows.len();
            }
        }
        match s.kind {
            Kind::Welcome | Kind::Summary => {}
            Kind::System => {
                let st = *self.oobe_sys.borrow();
                if st.iter().all(|&x| x == 2) || (!st.contains(&1) && st.contains(&3)) {
                    rows.push(row(if st.iter().all(|&x| x == 2) { "Continuer" } else { "Continuer malgré l'échec" }, String::new(), false));
                }
            }
            _ => {
                rows.push(row("Continuer", String::new(), false));
                if s.skippable {
                    rows.push(row("Passer cette étape", String::new(), false));
                }
            }
        }
        self.oobe_content.set(content);
        let sel = self.ui.get_oobe_sel().clamp(0, rows.len().saturating_sub(1) as i32);
        self.oobe_rows.set_vec(rows);
        self.ui.set_oobe_sel(sel);
    }

    pub fn oobe_click(&self, i: i32) {
        self.ui.set_oobe_sel(i);
        self.oobe_key(4);
    }

    pub fn oobe_key(&self, a: i32) {
        use system::sfx::{play, Sfx};
        let n = self.oobe_rows.row_count() as i32;
        let sel = self.ui.get_oobe_sel();
        let step = self.oobe_step.get() as usize;
        let kind = STEPS[step].kind;
        match a {
            0 | 1 => {
                let ns = (sel + if a == 0 { -1 } else { 1 }).clamp(0, (n - 1).max(0));
                if ns != sel {
                    self.ui.set_oobe_sel(ns);
                    play(Sfx::Move);
                }
            }
            2 | 3 => {
                if let Kind::Cat(c) = kind {
                    if (sel as usize) < self.oobe_content.get() {
                        self.setting(c, sel, if a == 2 { -1 } else { 1 });
                        self.oobe_sync();
                    }
                }
            }
            4 => self.oobe_activate(sel, kind),
            5 => {
                if step > 1 {
                    play(Sfx::Back);
                    self.oobe_enter(self.oobe_step.get() - 1);
                }
            }
            _ => {}
        }
    }

    fn oobe_activate(&self, sel: i32, kind: Kind) {
        use system::sfx::{play, Sfx};
        let content = self.oobe_content.get() as i32;
        let next = || {
            play(Sfx::Select);
            self.oobe_enter(self.oobe_step.get() + 1);
        };
        match kind {
            Kind::Welcome => next(),
            Kind::Summary => self.oobe_finish(),
            Kind::Cat(c) => {
                if sel < content {
                    self.setting(c, sel, 0);
                    self.oobe_sync();
                } else {
                    next();
                }
            }
            Kind::Profile => match sel {
                0 => {
                    self.kb_oobe_name.set(true);
                    self.settings.kb_open_text(&self.profile_current().name);
                    self.kb_refresh();
                    self.ui.set_kbx(0);
                    self.ui.set_kby(0);
                    self.ui.set_kb_title(tr("Nom du profil").into());
                    self.ui.set_zone(4);
                }
                1 => {
                    self.oobe_avatar.set(true);
                    let p = self.profile_current();
                    self.avatar_editor_open(p.avatar, self.profile_photo_current());
                }
                _ => {
                    if self.profile_current().name.trim().is_empty() {
                        play(Sfx::Error);
                    } else {
                        next();
                    }
                }
            },
            Kind::System => {
                let st = *self.oobe_sys.borrow();
                let all = st.iter().all(|&x| x == 2);
                if sel < content {
                    if !all && !st.contains(&1) {
                        self.oobe_install();
                    }
                } else {
                    next();
                }
            }
        }
    }

    /// Installe l'assistant administrateur (une validation Windows), puis les composants manquants, en tâche de fond.
    fn oobe_install(&self) {
        {
            let mut st = self.oobe_sys.borrow_mut();
            for v in st.iter_mut().filter(|v| **v != 2) {
                *v = 1;
            }
        }
        self.oobe_sync();
        let todo = *self.oobe_sys.borrow();
        std::thread::spawn(move || {
            let post = |i: usize, ok: bool| {
                let _ = slint::invoke_from_event_loop(move || {
                    with_shell(|s| {
                        s.oobe_sys.borrow_mut()[i] = if ok { 2 } else { 3 };
                        s.oobe_sync();
                    })
                });
            };
            let admin_ok = if todo[0] == 1 { system::admin::setup().is_ok() } else { true };
            if todo[0] == 1 {
                if admin_ok {
                    let _ = std::fs::write(stores::base_dir().join("helper-v3.done"), "1");
                }
                post(0, admin_ok);
            }
            for (i, id) in [(1usize, "ViGEm.ViGEmBus"), (2, "Microsoft.VCRedist.2015+.x64"), (3, "Microsoft.VCRedist.2015+.x86")] {
                if todo[i] == 1 {
                    post(i, admin_ok && system::install::winget(id).is_ok());
                }
            }
        });
    }

    fn oobe_finish(&self) {
        let _ = std::fs::write(done_file(), "1");
        let _ = std::fs::remove_file(step_file());
        self.oobe_step.set(-1);
        self.ui.set_oobe_on(false);
        self.ui.set_oobe_step(-1);
        self.ui.set_cat(0);
        self.ui.set_zone(0);
        system::sfx::play(system::sfx::Sfx::Launch);
        self.push_notif("Hearth", &tr("Bienvenue ! Votre console est prête."));
    }

    /// Nom saisi (étape Profil) : renomme le profil courant.
    pub fn oobe_profile_name(&self, name: String) {
        let name = name.trim().to_string();
        if !name.is_empty() {
            let mut s = crate::profiles::load();
            let cur = s.current.clone();
            if let Some(p) = s.profiles.iter_mut().find(|p| p.id == cur) {
                p.name = name;
            }
            crate::profiles::save(&s);
            self.profile_refresh_top();
        }
        self.ui.set_zone(13);
        self.oobe_sync();
    }

    /// Avatar choisi (étape Profil) : l'applique au profil courant.
    pub fn oobe_avatar_set(&self, a: i32) {
        let mut s = crate::profiles::load();
        let cur = s.current.clone();
        let photo = self.avatar_commit_photo(&cur);
        if let Some(p) = s.profiles.iter_mut().find(|p| p.id == cur) {
            p.avatar = a;
            p.photo = photo;
        }
        crate::profiles::save(&s);
        self.profile_refresh_top();
        self.ui.set_zone(13);
        self.oobe_sync();
    }

    /// Garde-fou : un flux annexe (connexion, clavier…) ne doit jamais laisser l'assistant.
    pub fn oobe_guard(&self) {
        if self.oobe_active() && matches!(self.ui.get_zone(), 0..=3) {
            self.ui.set_zone(13);
            self.oobe_sync();
        }
    }

    /// Relance l'assistant depuis les Réglages.
    pub fn oobe_restart(&self) {
        let _ = std::fs::remove_file(done_file());
        let _ = std::fs::write(step_file(), "0");
        self.oobe_start();
    }
}
