//! Arbre de processus d'un jeu : le processus lancé (ou celui de sa fenêtre) et tous ses descendants,
//! y compris ceux devenus orphelins (un lanceur qui démarre le vrai jeu puis se ferme) ou sortis du
//! Job Object (relance élevée). Rafraîchi régulièrement (`refresh`), il garde la mémoire des processus
//! déjà vus : la filiation reste connue même quand un intermédiaire a disparu.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::*;

/// Processus jamais rattachés à un jeu même s'ils en descendent : navigateur ouvert depuis le jeu,
/// clients de boutique, bureau. Sans ça, le jeu ne « finirait » qu'à la fermeture du navigateur.
const NOT_GAME: &[&str] = &[
    "explorer.exe",
    "msedge.exe",
    "chrome.exe",
    "firefox.exe",
    "opera.exe",
    "brave.exe",
    "steam.exe",
    "steamwebhelper.exe",
    "epicgameslauncher.exe",
    "epicwebhelper.exe",
    "galaxyclient.exe",
    "hearth.exe",
    "hearth-shell.exe",
];

pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// Nom de l'exécutable, en minuscules.
    pub exe: String,
}

/// Instantané de tous les processus (Toolhelp).
pub fn snapshot() -> Vec<Proc> {
    let mut out = vec![];
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                out.push(Proc { pid: e.th32ProcessID, parent: e.th32ParentProcessID, exe: String::from_utf16_lossy(&e.szExeFile[..n]).to_lowercase() });
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

#[derive(Default)]
pub struct ProcTree {
    /// pid -> nom d'exécutable à sa découverte (vide tant qu'inconnu). Le nom protège de la
    /// réutilisation d'un pid par un processus sans rapport.
    known: RefCell<HashMap<u32, String>>,
}

impl ProcTree {
    pub fn new(root: u32) -> Self {
        let t = ProcTree::default();
        t.add(root);
        t
    }

    pub fn add(&self, pid: u32) {
        if pid != 0 && pid != std::process::id() {
            self.known.borrow_mut().entry(pid).or_default();
        }
    }

    /// Le processus `pid` (encore vivant sous le même nom) est-il connu ?
    fn matches(&self, p: &Proc) -> bool {
        self.known.borrow().get(&p.pid).is_some_and(|n| n.is_empty() || *n == p.exe)
    }

    /// Adopte les nouveaux descendants des processus connus et renvoie l'instantané utilisé.
    pub fn refresh(&self) -> Vec<Proc> {
        let snap = snapshot();
        let live: HashMap<u32, &str> = snap.iter().map(|p| (p.pid, p.exe.as_str())).collect();
        {
            let mut known = self.known.borrow_mut();
            for p in &snap {
                if let Some(n) = known.get_mut(&p.pid).filter(|n| n.is_empty()) {
                    n.clone_from(&p.exe);
                }
            }
        }
        // Fermeture transitive : un enfant d'un enfant adopté dans ce même passage l'est aussi.
        loop {
            let mut added = false;
            for p in &snap {
                if self.known.borrow().contains_key(&p.pid) || NOT_GAME.contains(&p.exe.as_str()) || p.pid == std::process::id() {
                    continue;
                }
                // Parent connu : vivant sous le nom enregistré, ou mort (l'enfant est alors un orphelin
                // de ce parent, pas d'un homonyme qui aurait récupéré son pid).
                let parent_ok = self.known.borrow().get(&p.parent).is_some_and(|n| match live.get(&p.parent) {
                    Some(exe) => n.is_empty() || n == exe,
                    None => true,
                });
                if parent_ok {
                    self.known.borrow_mut().insert(p.pid, p.exe.clone());
                    added = true;
                }
            }
            if !added {
                break;
            }
        }
        snap
    }

    /// Un processus connu tourne-t-il encore ?
    pub fn alive(&self, snap: &[Proc]) -> bool {
        snap.iter().any(|p| self.matches(p))
    }

    /// `pid` appartient-il à l'arbre ? Remonte aussi sa filiation actuelle (enfant tout juste créé,
    /// pas encore vu par `refresh`).
    pub fn contains(&self, pid: u32, snap: &[Proc]) -> bool {
        let parents: HashMap<u32, u32> = snap.iter().map(|p| (p.pid, p.parent)).collect();
        let mut cur = pid;
        for _ in 0..16 {
            if self.known.borrow().contains_key(&cur) {
                return true;
            }
            match parents.get(&cur) {
                Some(&parent) if parent != 0 && parent != cur => cur = parent,
                _ => return false,
            }
        }
        false
    }

    /// Termine de force tous les processus encore vivants de l'arbre.
    pub fn kill(&self) {
        use std::os::windows::process::CommandExt;
        for p in snapshot().iter().filter(|p| self.matches(p)) {
            let _ = std::process::Command::new("taskkill").args(["/pid", &p.pid.to_string(), "/f"]).creation_flags(0x0800_0000).status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Un « lanceur » (cmd) démarre le « jeu » (ping, ~4 s) puis se ferme aussitôt : le jeu, orphelin,
    /// doit rester rattaché à l'arbre tant qu'il tourne, puis l'arbre doit se savoir terminé.
    #[test]
    fn follows_orphaned_child() {
        let mut launcher = std::process::Command::new("cmd").args(["/c", "start", "", "/b", "ping", "-n", "5", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn().unwrap();
        let tree = ProcTree::new(launcher.id());
        std::thread::sleep(Duration::from_millis(1500));
        let snap = tree.refresh();
        assert!(!snap.iter().any(|p| p.pid == launcher.id() && p.exe == "cmd.exe"), "le lanceur aurait dû se fermer");
        assert!(tree.alive(&snap), "le jeu orphelin doit être suivi");
        let _ = launcher.wait();
        std::thread::sleep(Duration::from_secs(6));
        assert!(!tree.alive(&tree.refresh()), "le jeu terminé ne doit plus être vu vivant");
    }
}
