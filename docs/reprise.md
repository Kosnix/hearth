# Reprise – état au 16/11/2026, 20 h 30

Note de passage entre sessions Claude. À lire après `CLAUDE.md` et `PHASES.md`.

## Où on en est
- Phases 1 et 2 codées. Phase 1 validée. Phase 2 : menu guide validé à la manette sur une fenêtre de test (mintty) ; **reste à valider sur de vrais jeux**.
- Jeu de test installé : SuperTuxKart 1.5 dans `C:\Games\SuperTuxKart\...\build-x86_64\bin\supertuxkart.exe`.
  `%LOCALAPPDATA%\Hearth\games.json` contient 3 tuiles : SuperTuxKart (`--windowed`), SuperTuxKart plein écran natif (`--fullscreen`, change la résolution), mintty.
- Dernières corrections (commits `0a497dd`, `0dc14ea`) : overlay du guide sans focus, détection du changement de résolution, recalage de la console au retour, manette ignorée si la console n'est pas au premier plan, PS dans la console ouvre le menu du jeu en cours (seul moyen de fermer un jeu plein écran exclusif), raccourcis Ctrl+Alt+Maj pour simuler la manette, `tools/dev.ps1`.
- Vérifié automatiquement : SuperTuxKart `--windowed` lancé depuis la tuile passe en 2560×1440 sans bordure.

## Problèmes ouverts (par priorité)
1. **Raccourcis Ctrl+Alt+Maj non prouvés** : dans le premier scénario (`Press enter`), le jeu ne s'est pas lancé. À diagnostiquer en premier (RegisterHotKey sans fenêtre sur le thread manette + PeekMessage ; vérifier qu'ils sont bien enregistrés, ou passer par une fenêtre message-only). Sans eux, pas de test autonome.
2. L'utilisateur signale « toujours des problèmes à gérer les fenêtres » sans préciser : rejouer tout le scénario soi-même (lancement, PS → overlay visible sans clignotement, Accueil, reprise, Fermer → retour console) sur les deux tuiles SuperTuxKart, captures à l'appui, et corriger.
3. Un ancien superviseur `hearth.exe` lancé en administrateur (pendant les tests SSH) tourne encore et verrouille `%LOCALAPPDATA%\Hearth\app\hearth.exe` ; il disparaît à la prochaine déconnexion. `Deploy` ne remplace donc que `hearth-shell.exe` jusque-là.
4. Développement et console dans la même session : la console et la fenêtre Claude se disputent le premier plan. Penser à remettre Claude devant (`Ask`) à la fin de chaque test.

## Ensuite
Valider la phase 2 sur les deux tuiles SuperTuxKart, mettre `PHASES.md` à jour, puis proposer le plan de la phase 3 (réglages système) et attendre la validation.
