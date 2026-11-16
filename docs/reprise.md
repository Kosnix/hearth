# Reprise – état au 17/11/2026, 0 h 15

À lire après `CLAUDE.md` et `PHASES.md`.

## Où on en est
- Phases 1 à 3 faites ; phase 4 (pilotes) : détection GPU + recherche Windows Update ; phase 5 : squelette (onglet Boutique, crate `stores`, visuels `%LOCALAPPDATA%\Hearth\assets\<id>\{cover,hero,logo}.png` ou champ `cover`/`id` de `games.json`).
- UI : `crates/shell/ui/app.slint` (Theme, accueil façon PS5, Réglages zone 3, clavier virtuel zone 4, GuideMenu). Logique des réglages : `crates/shell/src/settings.rs`. Langues : `crates/shell/src/i18n.rs` + `crates/shell/lang/*/LC_MESSAGES/hearth-shell.po` (les `@tr` Slint ont pour contexte le nom du composant : `App`, `GameTile`, `GuideMenu` ; ajouter chaque nouveau texte aux trois contextes).
- Rendu Slint logiciel forcé ; animation du fond ~22 % d'un cœur sur l'Optiplex.

## Outillage de test
- Raccourcis de test : Ctrl+Alt+Maj+F1..F8 (`Press up/down/left/right/enter/back/g/m`).


## Limites connues
- Wi-Fi : Windows 11 exige l'autorisation de localisation pour toute lecture Wi-Fi (WinRT et `netsh`) ; sans elle la liste est vide et un message l'explique. Activer les services de localisation = réglage système à faire avec l'accord de l'utilisateur (ou à l'installation).
- Non testés : bascule de sortie audio (une seule sortie), connexion Wi-Fi, Bluetooth (bascule), installation de mises à jour, anti-triche, plein écran exclusif réel. Volume système laissé à 20 %.
- Le menu guide n'a pas été revu sur un vrai jeu avec la nouvelle fenêtre (testé avec la console au premier plan en mode dev).

## À faire ensuite
- Phase 4 : sources NVIDIA/AMD/Intel, installation silencieuse (pnputil), version précédente conservée.
- Phase 5 : connecteurs Steam/Epic/GOG (QR code), SteamGridDB pour les visuels, file de téléchargements.
- Phase 6 (profils : crée des comptes Windows locaux) : demander l'accord de l'utilisateur avant.
- Phases 7 (portables) et 8 (mise à jour du shell).
