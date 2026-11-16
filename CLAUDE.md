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
- Slint : `set_fullscreen()` avant `run()` est ignoré → `SLINT_FULLSCREEN=1` au démarrage.
- La DualSense n'est pas un `Gamepad` WGI : elle est lue via `RawGameController` (Croix=1, Rond=2, Options=9, PS=12, chapeau 0).
- `SetForegroundWindow` depuis l'arrière-plan : maintenir Alt via `SendInput` (`wm::focus`).
- Les apps intégrées (Bloc-notes, Paint…) sont des apps empaquetées : leurs fenêtres n'appartiennent pas au job ; pour tester l'encapsulation, utiliser un vrai exe Win32 (ex. `C:\Program Files\Git\usr\bin\mintty.exe`).
- Ne jamais redémarrer `sshd` depuis une session SSH.
- La manette est lue même quand la console est en arrière-plan : toute action hors guide doit vérifier que la console est au premier plan (sinon navigation à l'aveugle, déjà arrivé jusqu'à « Veille »).
- Un jeu qui change la résolution (SDL/OpenGL `--fullscreen`) n'est pas signalé « D3D plein écran » : détecté via `EnumDisplaySettings` courant ≠ registre (`wm::exclusive_fullscreen`).
- L'overlay du guide ne doit jamais prendre le focus (WS_EX_NOACTIVATE, affichage par `SetWindowPos` + `SWP_NOACTIVATE`) : sinon le jeu plein écran se réduit et clignote.
- `games.json` peut contenir un BOM (PowerShell/Notepad) : il est retiré avant `serde_json`.
