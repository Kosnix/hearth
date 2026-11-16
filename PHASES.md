# Suivi des phases

| Phase | Sujet | État |
|---|---|---|
| 0 | Chaîne d'outils sur l'Optiplex, dépôt | ✅ Terminé |
| 1 | Cœur du shell | Implémenté ; reste à tester à la manette et en vraie session |
| 2 | Encapsulation des fenêtres, menu guide | — |
| 3 | Réglages système | — |
| 4 | Pilotes | — |
| 5 | Bibliothèque et boutiques | — |
| 6 | Profils multi-utilisateurs | — |
| 7 | Consoles et PC portables | — |
| 8 | Mise à jour automatique | — |

## Journal

- 2026-11-16 : création du dépôt.
- 2026-11-16 : chaîne d'outils installée sur l'Optiplex (Win 11 Pro Éducation 26200, Intel UHD 630) : Git 2.56, VS 2022 Build Tools (C++, ARM64), Rust 1.100 stable MSVC, cibles x86_64 et aarch64 validées par compilation.
- 2026-11-16 : phase 1 implémentée (crates `input`, `shell`, `supervisor`). Mesuré sur l'Optiplex : démarrage ~100 ms jusqu'à la fenêtre, 31 Mo de mémoire privée, binaires sans warning. Validés : plein écran, relance après plantage, repli sur le bureau après 5 plantages, `hearth console`, install/uninstall, clés Game Bar. Reportés : premier plan forcé du shell au bouton guide (phase 2), bouton PS DualSense (phase 4), Shell Launcher (Enterprise/IoT).
