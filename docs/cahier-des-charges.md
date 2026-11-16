# Cahier des charges (fourni par l'utilisateur, 2026-11-16)

## Contexte et objectif
Shell de console de jeu pour Windows 10/11, en Rust, qui remplace l'explorateur et transforme n'importe quel PC (salon, portable, console portable type ROG Ally / Legion Go) en console, à la manière de SteamOS, ergonomie inspirée de la Switch et de la PS5 (style propre, sans reprendre leurs éléments). D'abord le shell (installable sur un Windows existant, retour au bureau possible) ; l'ISO personnalisé viendra bien plus tard (ne pas y travailler).

## Contraintes techniques
- Rust + windows-rs ; UI Slint accélérée GPU, navigable à la manette (clavier/souris pour le debug).
- Légèreté absolue : démarrage quasi instantané, faible mémoire, aucun runtime lourd.
- Aucune injection de code dans les jeux (anti-triche).
- Code modulaire : un module/crate par grande fonction (shell, fenêtres, système, stores, profils, stockage, mises à jour).
- Code prêt à compiler, peu commenté mais clair. x86_64 en priorité (ARM64 : garder le code compatible, ne pas compiler pour l'instant).

## Architecture
1. Service système (SYSTEM) : sessions, profils, mises à jour, pilotes, opérations privilégiées.
2. Shell utilisateur (un par session) : plein écran, bibliothèque, boutique, réglages, menu guide.
3. Gestionnaire de fenêtres : encapsulation des jeux, overlay.
4. Connecteurs de boutiques derrière un trait Rust commun (bibliothèque, métadonnées, installation, désinstallation, lancement, mises à jour).

Lancé à la place d'explorer.exe (clé Winlogon Shell par utilisateur, ou Shell Launcher sur Enterprise/IoT), redémarre en cas de crash, option « Retour au bureau ».

## Phases
1. **Cœur du shell** : app Slint plein écran sans bordure au démarrage de session ; manette (Windows.Gaming.Input, repli XInput, bouton guide, Game Bar désactivée) ; accueil (rangée de tuiles, icônes système en bas, fond dynamique) ; superviseur ; installeur/désinstalleur réversible.
2. **Fenêtres et menu guide** : SetWinEventHook, identification des fenêtres des jeux lancés ; plein écran fenêtré sans bordure + premier plan ; menu guide overlay topmost (accueil, fermer le jeu, son, luminosité, Wi-Fi, notifications) ; plein écran exclusif : optimisations plein écran Windows, base de réglages par jeu (arguments, fichiers de config), DXVK/dgVoodoo optionnels, en dernier recours Alt+Tab propre ; liste d'exceptions anti-triche (aucune modification).
3. **Réglages système** (jamais dans les Paramètres Windows) : Wi-Fi, Bluetooth, appairage manettes (WinRT) ; son (sortie, volume), affichage (résolution, fréquence, HDR) ; alimentation ; Windows Update contrôlé (jamais de redémarrage forcé en partie) ; stockage : disques, espace, détail par jeu, disque par défaut, formatage/préparation, dossier de bibliothèque commun par disque pour tous les stores, déplacement de jeux, disques externes et réapparition de leurs jeux.
4. **Pilotes** : détection PnP (10DE NVIDIA, 1002 AMD, 8086 Intel) ; chipset/CPU/périphériques via l'API Windows Update, silencieux ; GPU : sources NVIDIA/AMD/Intel directes, pilote seul (type NVCleanstall), pnputil, version précédente conservée ; notification « mise à jour graphique » en un clic ; manettes : Xbox natif, DualSense et autres via Windows.Gaming.Input ; jamais dépendant d'une machine précise.
5. **Bibliothèque et boutiques unifiées** (Xbox/Game Pass hors périmètre), dans l'ordre : (1) GOG puis Epic complets (connexion, bibliothèque, installation, mises à jour, sauvegardes cloud ; s'inspirer de gogdl/Heroic et Legendary) ; (2) Steam : API Web, client caché en arrière-plan, steam://install / steam://uninstall, progression dans les manifestes ; (3) EA, Ubisoft, Rockstar via leurs launchers invisibles (API non officielles comme Playnite, uplay://install, pilotage du client EA, jeux installés localement pour Rockstar), installés en silence comme dépendances ; fenêtres imprévues interceptées et encapsulées.
   Communs : connexion par QR code, jetons chiffrés ; visuels des boutiques + SteamGridDB en secours ; boutique native sur les catalogues (API Steam, catalogue GOG, GraphQL Epic) : fiches, recherche, promos, comparaison des prix, aucune page web ; achat par QR code puis installation proposée dès l'apparition dans la bibliothèque ; file de téléchargements unique ; installation sur le disque par défaut en une pression (choix à la volée, disque le plus libre proposé) ; lancement en mode encapsulé.
6. **Profils multi-utilisateurs** : compte Windows local caché par profil (mot de passe aléatoire chiffré par le système) ; le service ouvre/rejoint la session via WTS sans écran de connexion ; un shell par session (comptes, sauvegardes, réglages propres) ; jeux partagés ; sessions inactives gardées (fermeture si RAM insuffisante) ; audio et manette suivent la session active ; « Autres profils » dans le menu guide (admin, PIN optionnel, avertissement si pas de sauvegarde récente).
7. **Portables et consoles portables** (si batterie ou modèle connu) : batterie, luminosité, TDP (RyzenAdj) ; manettes intégrées (référence : Handheld Companion) ; rotation des dalles portrait, clavier virtuel à la manette ; veille/reprise fiables.
8. **Mise à jour du shell** : GitHub Releases signées, vérification obligatoire ; téléchargement en arrière-plan, installation à côté ; bascule au prochain redémarrage du shell, jamais en partie ; retour arrière automatique si plantage au démarrage ; canaux stable et bêta.

## Problèmes connus
Anti-triche (aucune modification ni overlay sur les exceptions) ; API non officielles isolées avec dégradation propre ; mises à jour Windows qui réactivent des éléments masqués (réappliquer au démarrage) ; Shell Launcher, Keyboard Filter, Unbranded Boot, Custom Logon seulement sur Enterprise/Education/IoT : solutions de repli sur Famille/Pro.

## Méthode
Phase par phase ; passer à la suivante seulement après compilation et test sur l'Optiplex ; avant chaque phase, proposer un court plan technique (crates, modules, API) et attendre validation ; README + suivi des phases à jour, commits réguliers ; visuels fournis plus tard (interface sobre, facile à re-styler ; voir docs/design.md) ; nom de code neutre (Hearth).
