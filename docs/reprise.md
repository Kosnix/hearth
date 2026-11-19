# Reprise – état au 17/11/2026, 9 h

À lire après `CLAUDE.md` et `PHASES.md`.

## Où on en est
Phases 1 à 3 faites, phase 4 (détection pilotes) faite, phase 5 largement avancée côté code (boutiques, comptes, installation), phases 6 à 8 non commencées. Détails des fonctions dans le journal de `PHASES.md`.

## Carte du code
- `crates/shell/ui/app.slint` : tout l'UI (Theme, accueil PS5, boutique, Réglages = zone 3, clavier = 4, QR/connexion = 5, sélecteur = 6, notifications = 8, GuideMenu, Splash). Zones : 0 contenu, 1 barre du haut, 2 alimentation.
- `crates/shell/src/main.rs` (Shell : lancement, notifications, catalogue), `settings.rs` (Réglages), `addflow.rs`/`addgame.rs` (ajout de jeu), `accounts.rs` (comptes, bibliothèque possédée, installation), `playtime.rs`, `i18n.rs` (+ `lang/*/LC_MESSAGES/hearth-shell.po`, contextes `App`, `GameTile`, `GuideMenu`, `Splash`).
- `crates/stores` : `Store` (Steam/Epic/GOG), `account` (connexion, DPAPI, bibliothèque), `art` (visuels + cache), `http` (WinHTTP), `qr`. Sondes réseau : `cargo run -p stores --example probe` et `auth_probe`.
- `crates/system` : audio, affichage, Wi-Fi, Bluetooth, alimentation, réseau, installation winget, etc.

## Outillage de test
- `Deploy` (dans `tools/dev.ps1`) : compile, arrête tout, installe (recrée le raccourci « Mode console » du bureau avec icône), relance. **À lancer après chaque changement** (consigne de l'utilisateur : il teste via le raccourci). Le shell met ~20 s à apparaître après un nouveau binaire (analyse antivirus).
- `Press up|down|left|right|enter|back|g|m` : raccourcis Ctrl+Alt+Maj+F1..F8 ; `Shot`, `Fg`. `Dev` lance `target\release` avec F13..F20 si un ancien processus verrouille l'installation.
- Pièges : `sed` avale `\u` (utiliser l'outil Edit pour les `"\u{E...}"`) ; heredocs bash contenant des apostrophes échouent parfois (écrire les fichiers avec l'outil Write).

## Limites / à faire
- Wi-Fi : la liste des réseaux exige l'autorisation de localisation de Windows (non activée, réglage système).
- Comptes : Steam (QR), Epic (code d'autorisation après connexion sur téléphone) ; GOG = bibliothèque locale seulement. Les flux n'ont pas été validés avec un vrai compte (seuls le QR Steam et le catalogue ont été vérifiés en réseau).
- Installation des clients par winget : demande une confirmation d'administrateur (UAC) ; non lancée sur cette machine.
- Phase 4 : sources NVIDIA/AMD/Intel et installation de pilotes. Phase 6 (comptes Windows locaux) : accord de l'utilisateur nécessaire. Phases 7 et 8 : à faire.
- Volume système laissé à 20 % après les tests.

## Mise à jour du 17/11 (soir)
- Rendu Skia par défaut (`crates/shell/Cargo.toml` : `renderer-skia`). Repli logiciel : `HEARTH_BACKEND=winit-software`.
- Onglets : 0 Jeux (installés), 1 Bibliothèque (possédés à installer), 2 Boutique. Barre du haut : `top` 0-2 onglets, 3 « + », 4 notifications, 5 réglages, 6 alimentation. Zones ajoutées : 7 fiche boutique, 9 menu contextuel.
- `menus.rs` (options d'un jeu, menu « + »), `prefs.rs` (`overrides.json` : nom, caché), `Shell::rebuild()` reconstruit les deux listes de tuiles (le gid d'une tuile = indice dans `Shell::games`).
- Assistant administrateur : `crates/system/src/admin.rs` (côté shell) + `setup-admin` / `elevated` dans `crates/supervisor/src/main.rs`. **Non testé** : demande une confirmation UAC la première fois (Réglages > Comptes > Autorisation administrateur, ou au premier « installer le client »).
- Installer un jeu Steam : `steam://install/<id>` ouvre la fenêtre Steam, à valider à la souris (pas de mode silencieux).

## Mise à jour du 19/11 (refonte console)
- Nouveaux modules : `theme.rs` (jetons de couleur, `apply_theme` dans `main.rs`), `oobe.rs` (assistant), moteur audio maison dans `crates/system/src/sfx.rs` (mixeur waveOut, volumes `sfx_vol`/`music_vol`), `crates/input/src/mouse.rs` (mode souris L3+R3, clavier visuel `PadOverlay`), `xpad.rs` (DualSense → manette Xbox virtuelle ViGEm).
- Zones : 13 = assistant ; `home_zone()` renvoie 13 si l'assistant est ouvert (clavier, QR, sélecteur d'avatar y reviennent).
- Pièges : `Theme` est une global Slint par fenêtre (App, GuideMenu, Splash, PadOverlay) — `apply_theme` les met toutes à jour ; ne jamais utiliser `sed` pour les `\u{…}` ; `Press` injecte des touches dans la session réelle (ne pas l'utiliser pendant que l'utilisateur teste).
- Mesures de référence (Optiplex) : fenêtre en 0,75 s, 1re image ~1,25 s (4 fenêtres Skia), ~52 images/s max, repos ~3 % CPU.
