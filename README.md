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

Binaires produits dans `target\release\` :

| Binaire | Rôle |
|---|---|
| `hearth.exe` | Superviseur, déclaré comme shell Windows. Lance `hearth-shell.exe` et le relance s'il plante (5 plantages en 60 s : repli sur le bureau). |
| `hearth-shell.exe` | Interface console plein écran (Slint). |

## Installer / désinstaller (utilisateur courant)

```
hearth.exe install     # copie dans %LOCALAPPDATA%\Hearth\app et remplace explorer pour cet utilisateur
hearth.exe uninstall   # rétablit explorer
```

Effet à la prochaine ouverture de session. Fonctionne sur Famille/Pro (clé `HKCU\...\Winlogon\Shell`).
Secours si l'interface ne s'affiche pas : Ctrl+Alt+Suppr → Gestionnaire des tâches → Exécuter `explorer`.

## Utilisation

- Manette Xbox : croix/stick = naviguer, A = valider, B = retour, Guide = accueil. DualSense/DualShock 4 : ✕ = valider, ○ = retour, PS = accueil. Clavier : flèches, Entrée, Échap, Début.
- **Bureau** (icône du bas) : ouvre le bureau Windows. Le raccourci « Mode console » du bureau (`hearth.exe console`) ramène à la console.
- Jeux (provisoire, avant les boutiques) : `%LOCALAPPDATA%\Hearth\games.json`

```json
[{ "name": "Mon jeu", "exe": "C:\\Jeux\\jeu.exe", "args": [], "cover": "C:\\Jeux\\cover.png" }]
```

- Journal du superviseur : `%LOCALAPPDATA%\Hearth\supervisor.log`.
- Style : toutes les couleurs et tailles sont dans `Theme` (`crates/shell/ui/app.slint`).

## Machine de test (dev)

- `tools/enable-ssh.cmd` : à double-cliquer une fois sur la machine de test (active OpenSSH, autorise la clé de dev).
- Le code est poussé dans un dépôt nu `C:\dev\hearth.git` (remote `optiplex`) ; copie de travail : `C:\dev\hearth`.
