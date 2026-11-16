# Hearth (nom de code)

Shell console de jeu pour Windows 10/11 : remplace `explorer.exe` et transforme un PC en console navigable à la manette.

- Langage : Rust (`windows-rs`), UI : Slint.
- Cibles : `x86_64-pc-windows-msvc` (testé), `aarch64-pc-windows-msvc` (compatible).
- Suivi d'avancement : [PHASES.md](PHASES.md).

## Prérequis (machine Windows)

- rustup (toolchain stable MSVC)
- Visual Studio 2022 Build Tools, charge de travail « Développement Desktop C++ » + SDK Windows
- Git

## Compiler

```
cargo build --release
```

Les instructions d'installation et de retour au bureau seront ajoutées avec la phase 1.

## Machine de test (dev)

- `tools/enable-ssh.cmd` : à double-cliquer une fois sur la machine de test (active OpenSSH, autorise la clé de dev).
- Le code est poussé dans un dépôt nu `C:\dev\hearth.git` (remote `optiplex`) ; copie de travail : `C:\dev\hearth`.
