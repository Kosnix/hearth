# Suivi des phases

| Phase | Sujet | État |
|---|---|---|
| 0 | Chaîne d'outils sur l'Optiplex, dépôt | ✅ Terminé |
| 1 | Cœur du shell | ✅ Terminé |
| 2 | Encapsulation des fenêtres, menu guide | Validé (manette + scénario automatisé SuperTuxKart) ; reste anti-triche / plein écran exclusif réel |
| 3 | Réglages système | ✅ Son, affichage, Wi-Fi (liste limitée par la localisation Windows), Bluetooth, manettes, langue, mises à jour (recherche), stockage (lecture) ; reste : installation des mises à jour, déplacement de jeux, formatage |
| 4 | Pilotes | Détection GPU + recherche Windows Update faites ; sources NVIDIA/AMD/Intel et installation à faire |
| 5 | Bibliothèque et boutiques | Squelette : onglet Boutique, crate `stores`, visuels de jeux (`assets<id>`) |
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
- 2026-11-16 : design de référence appliqué (dégradé bleu→indigo, tuiles paysage, rangée de navigation, en-tête profil/heure). Phase 3a (écran Réglages navigable, catégories Son et Affichage) et 3b (sortie audio, volume, résolution/fréquence avec confirmation 15 s et retour automatique) implémentées ; volume et changement de résolution testés sur l'Optiplex, bascule de sortie audio non testée (une seule sortie). Rendu Slint passé en logiciel : femtovg ne se redessinait plus avec ce design.
- 2026-11-16 : accueil refait façon PS5 en gardant le thème bleu (barre du haut avec onglets Jeux/Boutique et raccourcis système, héros du jeu sélectionné, rangée de tuiles carrées, fond animé à ~6 images/s, coupé en jeu et désactivable dans Affichage), pictogrammes selon la dernière manette utilisée (PlayStation ×/○/PS ou Xbox A/B), langues fr/en (Slint `@tr` + `lang/`, langue de Windows par défaut, réglage « Langue »), menu en jeu refait (icônes, jauges, notifications), phases 3c à 3e et 4 (détection) en lecture/recherche, crate `stores` (trait `Store`, connecteurs factices, visuels de jeux).
- 2026-11-17 : raccourci « Mode console » réparé (icône Hearth générée à l'installation, description) ; Hearth réinstallé après redémarrage de session (anciens processus administrateur disparus : `Deploy` refonctionne, raccourcis de test inchangés F1..F8). Rendu : tuiles et héros colorés par jeu avec initiale quand il n'y a pas de visuel, cartes boutique sans chevauchement, icônes dans les catégories de Réglages, titre long réduit.
