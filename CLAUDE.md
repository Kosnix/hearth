# Hearth – consignes pour Claude

Shell console de jeu pour Windows (Rust + windows-rs + Slint). Cahier des charges complet : `docs/cahier-des-charges.md`. Avancement : `PHASES.md`. Direction visuelle (à appliquer seulement quand demandé) : `docs/design.md`.

## Façon de travailler
- Répondre en français, de façon directe. Code léger, peu commenté mais clair ; pas d'abstraction spéculative.
- Avant chaque phase : proposer un court plan technique et **attendre la validation**. Passer à la phase suivante seulement après compilation + test sur la machine.
- Tenir `PHASES.md` (tableau + journal) et `README.md` à jour ; commits réguliers, poussés sur GitHub (`Kosnix/hearth`, privé).
- x86_64 uniquement pour l'instant (garder le code compatible ARM64).
- **Travailler en autonomie** : compiler, déployer et tester soi-même sur cette machine (`tools/dev.ps1` : `Deploy`, `Press`, `Fg`, `Shot`, puis lire la capture). L'utilisateur ne tape rien sur ce PC et ne veut pas du terminal : pour lui poser une question, mettre la fenêtre Claude au premier plan (`Ask`) et attendre sa réponse. Il teste à la manette (DualSense) seulement quand c'est indispensable.
- État courant et prochaines étapes : `docs/reprise.md`.

## Machine
Dell Optiplex 3060, Windows 11 Pro Éducation, Intel UHD 630, écran 2560×1440, compte `admin`. **Connectée en Wi-Fi** : ne pas couper le Wi-Fi.
Hearth est installé comme shell de ce compte (`hearth.exe install`) : pas d'explorer en mode console. Mettre à jour la copie installée : arrêter `hearth`/`hearth-shell`, attendre la libération des fichiers, puis `target\release\hearth.exe install`.

## Crates
`input` (manette), `shell` (UI Slint, bin `hearth-shell`), `supervisor` (bin `hearth`), `wm` (fenêtres des jeux, `compat.json`), `system` (son, luminosité, Wi-Fi).

## Pièges déjà rencontrés
- `windows` 0.62 : l'attente bloquante d'un async WinRT est `.join()` (pas `.get()`) ; ne pas l'appeler sur le thread UI (STA).
- Slint : `set_fullscreen()` avant `run()` est ignoré. Le vrai plein écran winit (`SLINT_FULLSCREEN`/`Fullscreen::Borderless`) redemande sa taille à l'OS de façon peu fiable (voir piège DPI ci-dessous) → fenêtre sans bordure dimensionnée nous-mêmes, recalée sur l'écran via `wm::fit_to_monitor` juste après création (même méthode que pour les jeux).
- DPI/mise à l'échelle Windows (ex. 150 %) : le manifeste doit déclarer un vrai `dpiAwareness PerMonitorV2` (pas `dpiAware=false`), sinon les API Windows mentent de façon incohérente entre elles (certaines renvoient la résolution physique, d'autres une résolution virtualisée par le facteur d'échelle) → interface démesurée/superposée. `SLINT_SCALE_FACTOR` est ensuite calculé au démarrage (`min(largeur/2560, hauteur/1440)` de l'écran physique) : l'interface est dessinée pour un canevas 2560×1440 et tient ainsi sur toute définition (1080p, 4K, Bureau à distance), jamais selon la mise à l'échelle Windows. **Piège critique** : `build.rs` doit avoir `cargo:rerun-if-changed=hearth.rc` et `hearth.manifest` explicitement — le premier `rerun-if-changed` émis dans le fichier désactive sinon la recompilation automatique sur changement pour tout le reste, et une modification du manifeste reste alors silencieusement non recompilée (vécu : plusieurs heures perdues avant de s'en apercevoir).
- La DualSense n'est pas un `Gamepad` WGI : elle est lue via `RawGameController` (Croix=1, Rond=2, Options=9, PS=12, chapeau 0). En Bluetooth, un jeu peut la passer en rapport étendu (0x31 ; 0x11 pour la DS4), qu'elle garde jusqu'à extinction : WGI renvoie alors un état figé sans erreur (manette muette en sortie de jeu). `input::hidpad` la lit alors directement en HID (sonde : `cargo run -p input --example hid_probe`).
- `SetForegroundWindow` depuis l'arrière-plan : maintenir Alt via `SendInput` (`wm::focus`).
- Les apps intégrées (Bloc-notes, Paint…) sont des apps empaquetées : leurs fenêtres n'appartiennent pas au job ; pour tester l'encapsulation, utiliser un vrai exe Win32 (ex. `C:\Program Files\Git\usr\bin\mintty.exe`).
- Ne jamais redémarrer `sshd` depuis une session SSH.
- La manette est lue même quand la console est en arrière-plan : toute action hors guide doit vérifier que la console est au premier plan (sinon navigation à l'aveugle, déjà arrivé jusqu'à « Veille »).
- Un jeu qui change la résolution (SDL/OpenGL `--fullscreen`) n'est pas signalé « D3D plein écran » : détecté via `EnumDisplaySettings` courant ≠ registre (`wm::exclusive_fullscreen`).
- L'overlay du guide ne doit jamais prendre le focus (WS_EX_NOACTIVATE, affichage par `SetWindowPos` + `SWP_NOACTIVATE`) : sinon le jeu plein écran se réduit et clignote.
- `games.json` peut contenir un BOM (PowerShell/Notepad) : il est retiré avant `serde_json`.
- Slint : avec le design actuel, le rendu femtovg (GPU) ne se redessine plus après la première image sur l'Optiplex ; le shell force `SLINT_BACKEND=winit-software`. Les `@tr` ont pour contexte le nom du composant (voir `docs/reprise.md`).
- Slint : un `animate` sur une propriété affectée depuis Rust (ou par une fonction appelée depuis Rust) n'a pas été joué (saut direct à la valeur finale) ; animations pilotées côté Rust par un `Timer` (ex. `Shell::zoom_to`). `HEARTH_ZOOM_MS` ralentit l'agrandissement de jaquette pour le vérifier en captures.
- Winit initialise COM en STA : ne rien appeler de COM multithread sur le thread UI avant la création de la fenêtre (`RPC_E_CHANGED_MODE`, panique → `panic.log`).
- Raccourcis de test = Ctrl+Alt+Maj+F1..F8 ; `Dev` lance le binaire de `targetelease` (voir `docs/reprise.md` : anciens processus administrateur).
- `tools/dev.ps1` : ne pas écrire `Deploy | Select -First n` (le pipeline interrompt le déploiement) ; `Press` ne doit pas être enchaîné deux fois sur « enter » sur une tuile déjà lancée.
