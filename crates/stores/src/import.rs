//! Import des jeux déjà présents sur le PC : raccourcis « jeu non-Steam » ajoutés dans Steam (`shortcuts.vdf`).
//! Les jeux Steam, Epic et GOG installés sont déjà détectés par les boutiques.

use std::path::PathBuf;

/// Jeu trouvé hors boutique.
#[derive(Clone, Debug)]
pub struct Found {
    pub title: String,
    pub exe: PathBuf,
    pub args: Vec<String>,
}

fn cstr(data: &[u8], from: usize) -> Option<(String, usize)> {
    let end = from + data.get(from..)?.iter().position(|&b| b == 0)?;
    Some((String::from_utf8_lossy(&data[from..end]).into_owned(), end + 1))
}

/// Valeur texte (type 0x01) d'une clé de VDF binaire, entre `from` et `to`.
fn value(data: &[u8], lower: &[u8], key: &str, from: usize, to: usize) -> Option<String> {
    let pat = [&[1u8][..], key.as_bytes(), &[0u8]].concat();
    let at = lower[from..to].windows(pat.len()).position(|w| w == pat.as_slice())? + from + pat.len();
    cstr(data, at).map(|(s, _)| s)
}

/// Jeux d'un `shortcuts.vdf` (format binaire : entrées `AppName`, `Exe`, `LaunchOptions`…).
fn parse(data: &[u8]) -> Vec<Found> {
    let lower: Vec<u8> = data.to_ascii_lowercase();
    let pat = b"\x01appname\0";
    let starts: Vec<usize> = lower.windows(pat.len()).enumerate().filter(|(_, w)| *w == pat).map(|(i, _)| i).collect();
    let mut out = vec![];
    for (n, &s) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(data.len());
        let Some(title) = value(data, &lower, "appname", s, end) else { continue };
        let Some(exe) = value(data, &lower, "exe", s, end) else { continue };
        let exe = PathBuf::from(exe.trim().trim_matches('"'));
        if title.trim().is_empty() || !exe.is_file() {
            continue;
        }
        let args = value(data, &lower, "launchoptions", s, end).map(|a| a.split_whitespace().map(String::from).collect()).unwrap_or_default();
        out.push(Found { title: title.trim().to_string(), exe, args });
    }
    out
}

/// Jeux non-Steam ajoutés dans Steam, pour tous les comptes Steam de ce PC.
pub fn steam_shortcuts() -> Vec<Found> {
    let Some(root) = crate::steam::root_dir() else { return vec![] };
    let mut out: Vec<Found> = vec![];
    for user in std::fs::read_dir(root.join("userdata")).into_iter().flatten().flatten() {
        let Ok(data) = std::fs::read(user.path().join("config").join("shortcuts.vdf")) else { continue };
        for f in parse(&data) {
            if !out.iter().any(|o| o.exe == f.exe) {
                out.push(f);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_binary_shortcuts() {
        let exe = std::env::current_exe().unwrap();
        let mut d = b"\0shortcuts\0\0" .to_vec();
        d.extend(b"0\0\x02appid\0\x01\0\0\0\x01AppName\0Mon Jeu\0\x01Exe\0\"");
        d.extend(exe.to_string_lossy().as_bytes());
        d.extend(b"\"\0\x01LaunchOptions\0-a -b\0\x08\x08");
        let f = parse(&d);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].title, "Mon Jeu");
        assert_eq!(f[0].args, ["-a", "-b"]);
    }
}
