# Reprise – état au 16/11/2026, 21 h 45

À lire après `CLAUDE.md` et `PHASES.md`.

## Où on en est
- Phases 1 et 2 validées. Design de référence appliqué (`docs/design.md`).
- Phase 3a/3b faites : écran Réglages (Son : sortie audio, volume ; Affichage : résolution, fréquence, confirmation 15 s avec retour automatique). Code : `crates/shell/src/settings.rs`, `crates/system/src/lib.rs` (audio, display), `app.slint` (zone 3).
- Rendu logiciel Slint forcé (femtovg ne se redessinait plus).

## À faire ensuite
1. Phase 3c : Wi-Fi (liste, connexion avec clavier virtuel manette), Bluetooth, manettes. Ne jamais couper le Wi-Fi de la machine.
2. 3d : alimentation, Windows Update contrôlé. 3e : stockage.
3. Valider avec l'utilisateur le rendu visuel (l'image de référence d'origine n'est connue que par les notes de `docs/design.md`).

## Problèmes ouverts
- Ancien `hearth.exe` administrateur verrouillant `app\hearth.exe` (disparaît à la déconnexion).
- Volume système à 0 % observé (réglé par les tests) : à remettre à un niveau normal si besoin.
- Bascule de sortie audio non testée (une seule sortie). Anti-triche et plein écran exclusif réel non testés.
- Le menu guide (GuideMenu) n'est pas encore restylé en détail (seulement les couleurs du Theme).
