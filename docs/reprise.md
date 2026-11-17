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
