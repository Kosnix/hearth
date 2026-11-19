//! Compile l'interface Slint avec ses traductions (générées depuis `lang/strings-*.tsv`) et embarque l'icône de l'exécutable.
//! Colonnes des fichiers TSV : français (source) | en | zh | hi | es | ar | bn | pt | ru | ur.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const CODES: [&str; 9] = ["en", "zh", "hi", "es", "ar", "bn", "pt", "ru", "ur"];

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let lang_dir = out.join("lang");
    let tsv = merged_tsv();
    fs::write(out.join("strings.tsv"), &tsv).unwrap();
    gen_po(&lang_dir, &tsv);
    let config = slint_build::CompilerConfiguration::new().with_bundled_translations(&lang_dir);
    slint_build::compile_with_config("ui/app.slint", config).unwrap();
    println!("cargo:rerun-if-changed=lang");
    println!("cargo:rerun-if-changed=ui/app.slint");
    // Icône de l'exécutable (logo Hearth).
    embed_resource::compile("hearth.rc", embed_resource::NONE).manifest_optional().unwrap();
}

/// Concatène `lang/strings-*.tsv` (ordre alphabétique).
fn merged_tsv() -> String {
    let mut files: Vec<PathBuf> = fs::read_dir("lang").unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "tsv")).collect();
    files.sort();
    files.iter().map(|p| fs::read_to_string(p).unwrap()).collect::<Vec<_>>().join("\n")
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// Un .po par langue : contexte = composant Slint qui contient le `@tr`.
fn gen_po(dir: &Path, tsv: &str) {
    let mut table: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for line in tsv.lines().filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.trim_end_matches('\r').split('|').collect();
        if cols.len() == 10 {
            table.insert(cols[0], cols[1..].to_vec());
        }
    }
    let src = fs::read_to_string("ui/app.slint").unwrap();
    let mut pairs: BTreeSet<(String, String)> = BTreeSet::new();
    let mut ctx = String::new();
    for line in src.lines() {
        let t = line.trim_start();
        let t = t.strip_prefix("export ").unwrap_or(t);
        if let Some(rest) = t.strip_prefix("component ") {
            ctx = rest.split_whitespace().next().unwrap_or("").to_string();
        }
        let mut rest = line;
        while let Some(i) = rest.find("@tr(\"") {
            rest = &rest[i + 5..];
            if let Some(j) = rest.find('"') {
                pairs.insert((ctx.clone(), rest[..j].to_string()));
                rest = &rest[j..];
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
    // « fr » : langue source, fichier sans traduction (permet de revenir au français).
    for code in std::iter::once("fr").chain(CODES.iter().copied()) {
        let d = dir.join(code).join("LC_MESSAGES");
        fs::create_dir_all(&d).unwrap();
        let mut po = String::from("msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n");
        let _ = writeln!(po, "\"Language: {code}\\n\"\n");
        if let Some(col) = CODES.iter().position(|c| *c == code) {
            for (c, id) in &pairs {
                if let Some(tr) = table.get(id.as_str()).and_then(|v| v.get(col)) {
                    if !tr.is_empty() && tr != id {
                        let _ = writeln!(po, "msgctxt \"{}\"\nmsgid \"{}\"\nmsgstr \"{}\"\n", esc(c), esc(id), esc(tr));
                    }
                }
            }
        }
        fs::write(d.join("hearth-shell.po"), po).unwrap();
    }
}
