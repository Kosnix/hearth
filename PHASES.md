# Suivi des phases

| Phase | Sujet | État |
|---|---|---|
| 0 | Chaîne d'outils sur l'Optiplex, dépôt | ✅ Terminé |
| 1 | Cœur du shell | ✅ Terminé |
| 2 | Encapsulation des fenêtres, menu guide | Validé (manette + scénario automatisé SuperTuxKart) ; reste anti-triche / plein écran exclusif réel |
| 3 | Réglages système | — |
| 4 | Pilotes | — |
| 5 | Bibliothèque et boutiques | — |
| 6 | Profils multi-utilisateurs | — |
| 7 | Consoles et PC portables | — |
| 8 | Mise à jour automatique | — |

## Journal

- 2026-11-16 : création du dépôt.
- 2026-11-16 : chaîne d'outils installée sur l'Optiplex (Win 11 Pro Éducation 26200, Intel UHD 630) : Git 2.56, VS 2022 Build Tools (C++, ARM64), Rust 1.100 stable MSVC, cibles x86_64 et aarch64 validées par compilation.
- 2026-11-16 : phase 1 implémentée (crates `input`, `shell`, `supervisor`). Mesuré sur l'Optiplex : démarrage ~100 ms jusqu'à la fenêtre, 31 Mo de mémoire privée, binaires sans warning. Validés : plein écran, relance après plantage, repli sur le bureau après 5 plantages, `hearth console`, install/uninstall, clés Game Bar. Reportés : Shell Launcher (Enterprise/IoT).
- 2026-11-16 : DualSense prise en charge (RawGameController, bouton PS = guide) ; le guide ramène le shell au premier plan. Validé à la manette sur l'Optiplex.
- 2026-11-16 : test en vraie session validé : Hearth remplace explorer à l'ouverture de session, « Bureau » ouvre le bureau complet (barre des tâches), « Mode console » ramène à la console. **Phase 1 terminée.**
- 2026-11-16 : phase 2 implémentée (crates `wm`, `system`). Validé sur l'Optiplex, Hearth installé comme shell : plein écran sans bordure automatique, menu guide (PS) par-dessus le jeu, volume, accueil + badge « En cours », reprise depuis la tuile, fermeture du jeu avec retour automatique à l'accueil. Non testés : Wi-Fi (l'Optiplex est relié en Wi-Fi), luminosité DDC/CI, jeux réels (plein écran exclusif, anti-triche).
- 2026-11-16 : phase 2 validée sur SuperTuxKart (2 tuiles) en test autonome ; correctif : fenêtre du guide masquée dès qu'elle existe (timer de réessai).
