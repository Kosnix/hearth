# Reprise – état au 16/11/2026, 20 h 50

Note de passage entre sessions Claude. À lire après `CLAUDE.md` et `PHASES.md`.

## Où on en est
- Phases 1 et 2 codées et validées. Phase 2 validée sur SuperTuxKart (tuiles `--windowed` et `--fullscreen`) en test autonome : lancement, PS → overlay sans perte de focus, Reprendre, Accueil, PS sur la console, Fermer → retour console.
- Jeu de test : SuperTuxKart 1.5 dans `C:\Games\SuperTuxKart\...\build-x86_64\bin\supertuxkart.exe` ; `%LOCALAPPDATA%\Hearth\games.json` a 3 tuiles (SuperTuxKart, SuperTuxKart plein écran natif, mintty).
- Raccourcis Ctrl+Alt+Maj (`Press` dans `tools/dev.ps1`) : ils fonctionnaient ; la vraie cause des échecs était la fenêtre du guide non masquée au démarrage (elle captait la navigation). Corrigé dans `shell/src/main.rs` (timer de réessai).
- Piège outillage : ne pas écrire `Deploy | Select -First n` (le pipeline interrompt le déploiement). Ne pas enchaîner deux `Press enter` sur une tuile en cours (ça refocalise le jeu).

## Problèmes ouverts
1. Un ancien `hearth.exe` administrateur verrouille `app\hearth.exe` (disparaît à la déconnexion) ; `Deploy` ne remplace que `hearth-shell.exe`.
2. Le volume lu dans le menu guide affichait 0 % lors d'un test (100 % avant) : à vérifier.
3. Bande d'environ 22 px en haut où la console ne couvre pas la fenêtre derrière (vue sur captures).
4. Pas encore testé : jeu avec anti-triche, vrai plein écran exclusif.

## Ensuite
Proposer le plan de la phase 3 (réglages système) et attendre la validation de l'utilisateur.
