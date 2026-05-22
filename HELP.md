# HELP (détails techniques)

## Prérequis : installer `yt-dlp`

L’application **ne télécharge pas** `yt-dlp` : sans ce programme, les boutons qui passent par YouTube (liste, transcript, **Résumer** vidéo) échouent.

**Linux (recommandé)** — version à jour via PyPI utilisateur :

```bash
python3 -m pip install --user -U yt-dlp
python3 -m yt_dlp --version
# ou, si le script est sur le PATH :
yt-dlp --version
```

Le binaire se trouve en général sous **`~/.local/bin/yt-dlp`**. L’application cherche ce chemin **même si** le lanceur bureau n’a pas `~/.local/bin` dans le `PATH`. Si le script `yt-dlp` est absent ou cassé mais le module est installé, le backend utilise automatiquement **`python3 -m yt_dlp`** (en testant `PYTHON` / `PYTHON3` puis `python3` / `python` trouvés sur le `PATH`). Tu peux forcer le script : **`YT_DLP_CHEMIN=/chemin/vers/yt-dlp`**, ou forcer l’interpréteur pour le mode module : **`PYTHON=/usr/bin/python3`** (ou **`PYTHON3`**).

**Alternative** : [binaire unique](https://github.com/yt-dlp/yt-dlp/releases) (télécharger, `chmod +x yt-dlp`, placer dans un dossier listé dans le `PATH` ou dans `bin/` à côté du projet / de l’exécutable).

**Ubuntu / Debian** : `sudo apt install yt-dlp` fonctionne, mais le paquet est souvent **en retard** sur YouTube ; en cas de 403 / extraction KO, passe à `pip install -U` ou au binaire officiel.

**macOS** : `brew install yt-dlp`.

**Windows** : winget, Scoop, `pip install yt-dlp`, ou binaire des *releases* ; le `PATH` ou **`YT_DLP_CHEMIN`**.

## Interface — liste « Vidéos »

- **Persistance** : la carte mentale est enregistrée dans `donnees/chaines/<chaine>/videos/<id>/carte_mentale.md` (compat : lecture possible d’anciens `donnees/resumes/{id}_bullets.txt`). Pour une **vidéo hors chaîne**, `<chaine>` est le dossier **`direct`** (`donnees/chaines/direct/videos/<id>/`).
- **Cache transcript** : `donnees/chaines/<chaine>/videos/<id>/transcript.txt` est écrit au premier résumé/extraction et relu ensuite pour éviter de relancer `yt-dlp` à chaque fois.
- **Suppression** : le bouton **« Supprimer »** supprime les fichiers locaux associés à une vidéo (dossier `donnees/chaines/<chaine>/videos/<id>/` : `transcript.txt` + `carte_mentale.md`). La vidéo peut réapparaître dans la liste après rafraîchissement YouTube.
- **Suppression de chaîne** : dans la colonne **Chaînes enregistrées**, l’icône corbeille retire l’entrée de `chaines.json` via `supprimer_chaine` (IPC). Si la chaîne supprimée était active, l’UI bascule automatiquement sur une autre chaîne (ou « Vidéos seules » si activé).
- **Navigation** : liste des vidéos à gauche, panneau de détail à droite. **Onglets** **Vidéo** | **Carte mentale** (Markdown rendu si un fichier existe).
- **Vidéos hors chaîne** : sous la liste des chaînes, **Vidéos seules** et **+ Ajouter une vidéo (hors chaîne)** ; à l’ouverture du formulaire, le **presse-papiers** est proposé dans le champ URL (comme pour l’ajout de texte). La liste est mise en cache localement (clé `__video_directe__`). Les actions **Transcripts (chaîne)** / **Résumés (chaîne)** et **Vidéos plus récentes** concernent uniquement une **chaîne** YouTube, pas ce mode.
- **Textes à résumer** : **+ Ajouter un texte** puis **Enregistrer le texte** enregistre les métadonnées dans `localStorage` (clé `resume_youtube_textes_libres_v1`) et **lance tout de suite** l’appel IA (`resumer_texte_libre`, fournisseur selon les paramètres). La carte mentale et le texte source envoyé au modèle sont **écrits sur disque** sous `donnees/textes_libres/<id>/` : `texte_source.txt` et `carte_mentale.md` (comme pour les vidéos sous `donnees/chaines/.../videos/...`). **Au démarrage** de l’app Tauri, une synchronisation (`synchroniser_textes_libres_vers_disque`) recopie aussi tout le contenu déjà présent dans le stockage local vers ces dossiers (migration des anciennes entrées uniquement navigateur). Sans lancer l’app : exporter la valeur JSON de la clé dans les DevTools, enregistrer un fichier puis `npm run sync-textes-libres -- chemin/vers/export.json`. À l’**ouverture** du formulaire d’ajout (vidéo hors chaîne ou texte), le contenu du **presse-papiers** est recopié automatiquement dans la zone de saisie. Sous **Tauri**, la lecture passe par le plugin `clipboard-manager` (API native, sans dialogue du type « localhost souhaite voir le presse-papiers »). En ouvrant seulement le front dans un navigateur sans Tauri, le repli utilise l’API web, qui peut encore demander une permission. Le panneau affiche en **deux colonnes** le **texte source** et la **carte mentale**, tous deux rendus en **Markdown GFM** à l’écran (comme pour la carte mentale des vidéos ; le fichier `texte_source.txt` sur disque reste du texte brut). Un bouton **Régénérer la carte mentale** permet de refaire un passage LM sur le même texte. La **suppression** d’un texte retire aussi le dossier `donnees/textes_libres/<id>/`. Au démarrage, si la carte n’est pas dans le stockage local mais qu’un fichier existe, elle est **rechargée depuis le disque**.
- **Panneaux d’ajout (UX)** : à l’ouverture des formulaires **+ Ajouter une chaîne**, **+ Ajouter une vidéo (hors chaîne)** et **+ Ajouter un texte**, le contenu du **presse-papiers** est proposé automatiquement dans la zone de saisie. Le formulaire vidéo s’ouvre **sous son bouton** avec une animation de **descente**. Les formulaires chaîne et texte s’ouvrent **au-dessus du bouton** avec une animation de **remontée** (depuis le bouton vers le haut), pour rester visibles même en bas de la barre latérale.
- **En-tête** : le titre affiche le **nom de la chaîne** sélectionnée (ou « Vidéos seules (hors chaîne) ») ; à côté, **Vidéos plus récentes** relance la récupération (fusion avec la liste déjà en cache), **sauf** en mode vidéos seules. Le **nombre de vidéos** à interroger sur YouTube se règle dans **Paramètres** (icône engrenage), section « Liste des vidéos » (également dossier de sortie, transcripts / résumés par chaîne).
- **Anti-collision de rafraîchissement** : si vous lancez **Vidéos plus récentes** sur une chaîne puis sélectionnez une autre chaîne avant la fin, la réponse tardive de la première requête est **ignorée**. Ainsi, les vidéos d’une chaîne ne peuvent plus écraser la liste d’une autre.
- **Paramètres (IA)** : icône **engrenage** à gauche du titre « Resume YouTube » dans la barre latérale ; le bouton **Logs** est à droite sur la même ligne (plus d’affichage du chemin vers `chaines.json` dans l’UI). Section **Fournisseur IA** : onglets **LM Studio** (champ **adresse du serveur** — URL de base HTTP, valeur par défaut `http://192.168.1.71:1234`, mémorisée dans le stockage local ; choix du modèle et liste via `GET …/v1/models`, puis `GET …/api/v1/models` si la route OpenAI répond **404** ou renvoie une liste **sans identifiant** ; bouton **Rafraîchir** après changement d’URL), **OpenRouter** (champ **clé API** — à saisir par l’utilisateur, rien n’est prérempli dans le code ; la valeur est mémorisée dans le stockage local ; liste des modèles **gratuits** filtrés sur `id` contenant `:free` via l’API publique `GET https://openrouter.ai/api/v1/models`) et **Local** (GGUF MARTHA sous `donnees/modeles_locaux/`, inférence llama.cpp dans le processus Tauri). Le fournisseur actif (onglet sélectionné) et les champs sont mémorisés dans `localStorage`. Les résumés utilisent le même schéma `lm` (camelCase) : `fournisseur` (`lm_studio`, `openrouter` ou `local`), `baseUrl` (LM Studio, depuis les paramètres), `cleLmStudio` (non utilisé côté app), `cleOpenrouter`, `modele`, `temperature`, et `nCtx` pour le mode **Local**.
- **Données sur disque (AppImage / paquet release)** : le dossier `donnees/` (modèle local, `chaines/`, textes libres, etc.) est créé sous le répertoire données de l’application Tauri (`app_data_dir`, ex. sous Linux `~/.local/share/com.resume.youtube.desktop/donnees/`), pas à côté de l’exécutable — sinon les écritures peuvent échouer avec « permission non accordée ». En `tauri dev` (debug), le comportement reste le `donnees/` du dépôt à la racine du workspace ; `chaines.json` reste à la racine du projet en dev, et sous `donnees/chaines.json` en release. Au premier accès en **release**, si ce fichier est vide ou absent alors qu’un `chaines.json` existe encore à la racine des données app, à côté du binaire (remontée des dossiers) ou remonté depuis le répertoire courant, il est **copié** vers `donnees/chaines.json` (migration).
- **Local (robustesse)** : les inférences sur le modèle GGUF sont **sérialisées** (mutex global côté Tauri) : ne pas lancer plusieurs résumés « Local » en parallèle sur la même instance — sinon llama.cpp peut planter (SIGSEGV). Le chargement et la génération s’exécutent dans **le même** `spawn_blocking` pour éviter que `LlamaModel` soit touché depuis deux threads du pool. Le `n_ctx` demandé par l’UI est **plafonné** au maximum annoncé par le fichier GGUF (`n_ctx_train`) pour limiter les OOM et les comportements indéfinis. Côté llama.cpp, chaque `decode` doit respecter **`n_tokens ≤ n_batch`** : le code fixe donc `n_batch` au minimum à la taille du prompt tokenisé (plancher 2048, plafond `n_ctx`). Téléchargement : IPC `telecharger_modele_local`, progression via l’évènement `modele-local-progression` (`recus`, `total`).
- **Modèle par défaut** : si aucun modèle n’est fourni par l’utilisateur, l’application utilise `google/gemma-4-26b-a4b` (UI/Tauri et CLI via `--lm-modele` optionnel).
- **IPC** : `lire_tous_resumes_enregistres` charge la carte mentale pour une `videoId`.
- **Schémas ASCII / dessins Unicode** : les blocs entre **triple backticks** (```) sont affichés comme des **cartes** (fond dégradé, bordure accent, en-tête « Schéma », police **JetBrains Mono**, défilement horizontal stylé). L’alignement est préservé (`white-space: pre`). Les schémas alignés avec des espaces **en dehors** d’un bloc de code peuvent être déformés.
- **Traitement en cours** : le bandeau « Traitement en cours » peut s’afficher pendant un résumé LM ou une opération globale (liste, chaîne). **Afficher** reste utilisable pour les autres vidéos (lecture disque). **Résumer** n’est désactivé que pour la vidéo dont le résumé est en cours (les opérations globales désactivent **Résumer** sur toutes les lignes).

## Développement (`tauri dev`)

Le serveur Vite écoute sur le port **5173** (`ui/vite.config.ts`, `devUrl` dans `src-tauri/tauri.conf.json`). Si une ancienne instance de Vite occupe encore ce port (`Error: Port 5173 is already in use`), le script `npm run dev` dans `ui/` exécute **`kill-port 5173`** avant de lancer Vite pour libérer le port automatiquement.

## Pipeline

### Application Tauri — bouton « Résumer »

Le flux est **toujours** : (1) extraction du transcript YouTube via **yt-dlp** (sous-titres), puis (2) **seulement après** une requête HTTP vers le **fournisseur IA** choisi (LM Studio local, compatible OpenAI, ou **OpenRouter**). Tant que l’étape 1 n’est pas terminée, l’IA ne reçoit **aucune** instruction : c’est normal. Un bandeau non bloquant en haut de la zone principale (« Traitement en cours ») et le panneau **Logs** (bouton en haut à droite de la barre latérale) indiquent où en est le traitement ; vous pouvez continuer à utiliser l’application.

Les cartes mentales générées côté Tauri sont **enregistrées automatiquement** sous `donnees/chaines/...` (voir ci-dessus) :

- pour une vidéo **sans chaîne** (dossier `direct`) : `donnees/chaines/direct/videos/<id>/carte_mentale.md` ;
- pour une **chaîne** : `donnees/chaines/<chaine>/videos/<id>/carte_mentale.md`.

Chaque succès de génération ajoute un log `Résumé enregistré: <chemin>` dans le panneau **Logs**.

Pour un **texte collé** (section **Textes à résumer**), le flux est : **aucune** étape YouTube / `yt-dlp` ; dès **Enregistrer le texte**, le contenu est envoyé tel quel au **fournisseur IA** (troncature éventuelle à 25 000 caractères, log côté backend). La carte mentale renvoyée est enregistrée sous `donnees/textes_libres/<id>/carte_mentale.md` avec le texte effectivement utilisé dans `texte_source.txt` ; une copie de la carte reste aussi dans `localStorage` pour l’affichage hors ligne. L’affichage utilise **Markdown GFM** (titres, listes, gras, tableaux, etc.) avec `marked` + styles dédiés dans l’UI. Souvent le modèle ajoute **d’abord** un paragraphe d’intro, **puis** un bloc ` ```markdown ` … ` ``` ` : l’UI déballe **tous** ces blocs `markdown` / `md` où qu’ils soient (pas seulement en tout début de texte), avec la même logique de **profondeur** pour les blocs internes (` ```mermaid `, etc.). Si toute la carte est uniquement enveloppée dans ` ```markdown ` sans intro, ce cas est aussi géré. Les blocs **Mermaid** (` ```mermaid `) sont rendus en schémas SVG via la bibliothèque **mermaid** (mindmap, flux, etc.), comme pour les cartes des vidéos. Quand une formule LaTeX est détectée (`$...$`, `$$...$$`, `\(...\)`, `\[...\]`), le rendu passe par **KaTeX** pour l’affichage mathématique.

### Détails techniques

1. `lister_dernieres_videos` :
   - exécute `yt-dlp` en mode JSON (`--dump-json` puis fallback `-J --flat-playlist`)
   - prend les `nb_videos` premières entrées et produit une liste d’objets `VideoInfo`.
2. `extraire_transcript` :
   - crée un dossier temporaire
   - exécute `yt-dlp` pour écrire des sous-titres VTT (`--sub-format vtt`)
   - si aucun VTT n’est trouvé, fallback SRT (`--sub-format srt`)
   - parse VTT/SRT vers un texte condensé (suppression timestamps et tags).
3. IA (LM Studio ou OpenRouter) :
   - **LM Studio** : `POST` vers `{baseUrl}/v1/chat/completions` (`baseUrl` configurable dans l’onglet LM Studio ; défaut `http://192.168.1.71:1234`).
   - **OpenRouter** : `POST` vers `https://openrouter.ai/api/v1/chat/completions` avec en-tête `Authorization: Bearer <clé>` (et en-têtes `Referer` / `X-Title` recommandés par OpenRouter).
   - requête « OpenAI-compatible » avec `messages: [{role, content}, ...]`
4. Validation :
   - contenu non vide (Markdown).

## Qualité Markdown (carte mentale)

La carte mentale est demandée en **Markdown structuré** (titres `#`/`##`, liste hiérarchique, mots-clés en **gras**, synthèse finale). Les schémas ASCII dans des blocs ``` sont **optionnels** et doivent rester courts (pas de ligne unique interminable). Si la sortie est trop “plate” ou trop “schéma seul”, ajuster le prompt dans `core/src/resume.rs`.

## Options CLI (résumé)

### `resume-channel`

Arguments importants :
- `--channel-url` : URL de la chaîne YouTube
- `--nb-videos` : nombre de vidéos (défaut 10)
- (format unique) : carte mentale (Markdown)
- `--lm-base-url` : ex `http://localhost:1234`
- `--lm-route` : ex `/v1/chat/completions`
- `--lm-modele` : optionnel
- `--out` : dossier de sortie (optionnel)

### `resume-video`

- `--video-url`
- `--format`
- `--lm-*` (voir ci-dessus)
- `--out` (optionnel)

### `test-suite`

- `--channel-url` ou `--video-url` (un seul des deux)
- `--lm-base-url` optionnel :
  - absent: le test valide uniquement l’extraction transcript
  - présent: le test valide aussi la carte mentale IA

## Extraction Transcript Seulement

### `transcript-channel`
- `--channel-url` : URL de la chaîne YouTube
- `--nb-videos` : nombre de vidéos (défaut 10)
- `--out` : dossier de sortie (optionnel)
- `--max-caracteres` : limite de taille du transcript

### `transcript-video`
- `--video-url` : URL de la vidéo YouTube
- `--out` : dossier de sortie (optionnel)
- `--max-caracteres` : limite de taille du transcript

## Robustesse anti-429 (yt-dlp)

Le programme évite `--sub-langs all` en première intention. Il :
1. récupère les langues de captions auto disponibles (`yt-dlp --no-playlist --list-subs`),
2. télécharge une langue à la fois (priorité `en` puis `fr`, etc.),
3. et retry avec backoff si un `HTTP 429` est détecté.

## Dépannage

### `yt-dlp` introuvable

Le programme échoue avec une erreur explicite. Assure-toi que :
- `yt-dlp` est installé
- `yt-dlp --version` fonctionne dans le même terminal.

Ordre de résolution du binaire côté backend :
- variable d’environnement **`YT_DLP_CHEMIN`** (ou **`YTDLP_CHEMIN`**) : chemin absolu vers l’exécutable à utiliser (recommandé sous AppImage / lanceur bureau si le `PATH` est minimal) ;
- **`$HOME/.local/bin/yt-dlp`** (installations `pip install --user`, souvent absentes du `PATH` des `.desktop`) ;
- **`bin/yt-dlp`** (ou `bin/yt-dlp.exe`) depuis le répertoire courant ou un ancêtre,
- puis à côté du binaire applicatif (et `bin/` voisin),
- puis dans **`PATH`**.

Cela permet d’utiliser une version récente de `yt-dlp` sans dépendre de la version `apt`.

### Liste de vidéos vide sur Linux (`yt-dlp` = 0 entrée)

Le backend accepte désormais plusieurs formes de sortie JSON de `yt-dlp` (lignes JSON ou objet JSON unique avec `entries`) pour rester compatible entre Windows et Linux.  
Si `yt-dlp` renvoie des entrées sans URL complète, l’application reconstruit automatiquement une URL YouTube valide à partir de l’identifiant vidéo.

### Erreur Python `No module named 'encodings'` (AppImage)

Sur certains postes, l’environnement AppImage peut polluer l’exécution de `yt-dlp` avec
`PYTHONHOME` / `PYTHONPATH` pointant vers un runtime Python incomplet.

Le backend neutralise désormais ces variables avant chaque appel à `yt-dlp` pour éviter
cette erreur.

### Extraction transcript qui tourne en boucle

Chaque sous-commande `yt-dlp` côté backend Linux est maintenant exécutée avec un délai
maximal (timeout). En cas de blocage réseau/processus, la tâche est interrompue et une
erreur explicite est renvoyée au lieu d’afficher indéfiniment « Extraction du transcript en cours… ».

### HTTP 429 sur les sous-titres

YouTube limite le débit si `yt-dlp` enchaîne trop de téléchargements. L’app tente d’abord un **lot de langues en VTT** (une commande), puis le VTT **langue par langue**, puis JSON3/SRT sur un sous-ensemble — avec des **pauses uniquement en cas de 429** sur une réponse. Variable optionnelle **`YT_DLP_SLEEP_SOUS_TITRES_SEC`** : `0` (défaut, pas d’option `--sleep-subtitles`) ou `1`–`30` secondes entre sous-titres côté yt-dlp si tu vois encore des 429. Pendant l’extraction, les logs **« Extraction en cours… (Ns) »** apparaissent au plus **environ toutes les 20 s** avec le temps écoulé (plus de spam toutes les 2 s).

### Avertissements `Precondition check failed` / HTTP 400 (API iOS / Android)

YouTube rejette parfois les requêtes des clients internes **iOS** / **Android** utilisés par `yt-dlp` : tu vois alors des `WARNING` et `HTTP Error 400`, **sans** que la commande échoue pour autant — vérifie la fin de la sortie : si **`[info] Available … subtitles`** apparaît avec des langues, `list-subs` a réussi. Mets quand même **`yt-dlp` à jour** (`python3 -m pip install -U yt-dlp`) pour limiter ces messages.

Si tu veux forcer un client d’extracteur (au prix parfois d’autres erreurs selon les vidéos), tu peux définir avant de lancer l’app :

`YT_DLP_EXTRACTOR_ARGS_YOUTUBE=youtube:player_client=web`

Pour revenir au comportement sans ce préfixe : **`YT_DLP_NO_EXTRACTOR_ARGS=1`**.

### Transcript disponible sur YouTube mais extraction KO

Si YouTube affiche des captions mais que l’extraction échoue (`Did not get any data blocks`,
`429`, etc.), la cause la plus fréquente est une version trop ancienne de `yt-dlp`.

Solution recommandée :
1. placer une version récente dans `bin/yt-dlp`,
2. rendre le fichier exécutable (`chmod +x bin/yt-dlp`),
3. relancer l’application.

Le backend utilisera automatiquement ce binaire local en priorité.

### LM Studio HTTP 400 « Failed to load model … Operation canceled »

Ce 400 peut arriver pendant le chargement initial d’un gros modèle. Le backend tente désormais :
1. un appel de **chargement explicite** (`POST /api/v1/models/load` avec le modèle sélectionné),
2. puis une **seconde tentative automatique** sur le chat (courte pause puis retry).

Si LM Studio n’a pas le modèle demandé dans sa liste locale, l’erreur persiste tant que le modèle n’est pas téléchargé/importé dans LM Studio.

### « Impossible de trouver un transcript » / aucun VTT/SRT valide

Ce message apparaît lorsque **aucun fichier de sous-titres exploitable** n’a été obtenu après les tentatives par langue. Causes fréquentes :
- la vidéo n’a **pas** de sous-titres (y compris automatiques) ;
- l’URL pointe vers une **playlist** ou un contenu ambigu (l’app force `--no-playlist` pour viser une seule vidéo) ;
- **yt-dlp** est absent, trop ancien, ou bloqué par YouTube (403, détection bot).

En cas d’échec de `yt-dlp`, le message affiché peut inclure un **extrait de la sortie d’erreur** de `yt-dlp` pour faciliter le diagnostic. Vérifie une mise à jour (`pip install -U yt-dlp` ou binaire officiel), définis **`YT_DLP_CHEMIN`** si le lanceur ne voit pas `yt-dlp`, et teste la même URL dans un terminal avec `yt-dlp --list-subs "<url>"`.

### Résumés JSON invalides

Si le serveur IA ne respecte pas strictement le format attendu, le parseur échoue. Côté **CLI**, utilise une route OpenAI-compatible (`--lm-route`) ; côté **app**, LM Studio est fixé sur `/v1/chat/completions` et OpenRouter sur l’API officielle — ajuste le modèle si besoin.

## Fichier `chaines.json`

Liste des chaînes au format JSON (tableau d’objets `nom` + `url_chaine`). Lu au démarrage de l’UI et lors des commandes qui gèrent les chaînes. Sous Tauri, le fichier canonique est celui de `chemin_fichier_chaines` ; si la liste y est vide alors qu’un autre `chaines.json` est trouvé (ancien emplacement à la racine `app_data`, à côté de l’exécutable en remontant l’arborescence, ou depuis le CWD), le backend **migre** ce contenu vers le chemin canonique. En **aperçu navigateur** (`npm run dev` sans Tauri), Vite sert le `chaines.json` du dépôt à l’URL `/chaines.json` pour afficher la liste (sans IPC).

Depuis l’interface, l’ajout d’une chaîne ne demande que l’URL : le champ `nom` enregistré est le **handle YouTube après `@`** (décodage pourcent si besoin).

## Interface Tauri (IPC)

Le backend `src-tauri` expose des commandes `invoke` (noms Rust en `snake_case`, arguments sérialisés en **camelCase** côté front) :

| Commande | Rôle |
|----------|------|
| `lister_chaines` | Lit `chaines.json` (chemin résolu comme ci-dessus). |
| `ajouter_chaine` | Ajoute une entrée à partir de `{ urlChaine }` ; le `nom` est dérivé du handle après `@`. |
| `supprimer_chaine` | Supprime une entrée de `chaines.json` à partir de `{ urlChaine }` (suppression locale). |
| `chemin_fichier_chaines` | Affiche le chemin résolu utilisé. |
| `lister_videos_chaine` | Liste les vidéos (`url_chaine`, `nb_videos`). |
| `infos_video_depuis_url` | Métadonnées d’une vidéo (`videoUrl`) via `yt-dlp` (`--dump-json` / `-J`, `--no-playlist`). |
| `extraire_transcript_video` | Transcript d’une URL vidéo. |
| `extraire_transcripts_chaine` | Transcripts pour N vidéos ; émet `job-log` et `job-progression`. |
| `lister_modeles_lm_studio` | Liste des modèles LM Studio : `GET /v1/models` (OpenAI, `data[].id` ou `data[].model`) ; si **404** ou si la réponse **200** ne contient aucun identifiant, repli sur `GET /api/v1/models` (REST, `models[].key`) ; `baseUrl` depuis le payload `lm` (défaut UI : `http://192.168.1.71:1234`). |
| `lister_modeles_openrouter_gratuits` | Liste des modèles OpenRouter dont l’`id` contient `:free`. |
| `resumer_video_direct` | Carte mentale pour une URL vidéo (`format` accepté : `carte_mentale` ; compat : `bullets`) ; `lm` inclut `fournisseur`, `cleOpenrouter`, etc. |
| `resumer_texte_libre` | Carte mentale à partir d’un **texte** (`texte`, `format`, `lm`) — pas d’extraction YouTube. |
| `resumer_chaine` | Résumé pour les vidéos d’une chaîne (même schéma `lm`). |
| `supprimer_video` | Supprime localement le cache d’une vidéo (`videoId`, `channelUrl` optionnel). |
| `lancer_test_suite` | Équivalent logique du `test-suite` CLI. |
| `demander_annulation` / `reinitialiser_annulation` | Annulation best-effort entre vidéos. |
| `selectionner_dossier_sortie` | Dialogue natif (dossier de sortie). |
| `lire_texte_presse_papiers` | Texte du presse-papiers (système via `arboard`), pour préremplir les formulaires d’ajout. |

Événements émis vers l’UI : `job-log` (texte), `job-progression` (JSON : `courant`, `total`, `titre`, `id`).

## Dépannage : erreur **-102** sur `http://localhost:5173/`

Sous Chromium / WebView2, **-102** correspond en général à une **connexion refusée** : rien n’écoute sur ce port (Vite arrêté), ou bien le navigateur résout **`localhost` en IPv6 (`::1`)** alors que Vite n’écoutait que sur **IPv4** — d’où l’intérêt de `server.host: true` dans `ui/vite.config.ts` (écoute sur toutes les interfaces).

- Lance l’app avec **`npm run dev`** à la **racine du dépôt** (démarre Vite puis Tauri), ou seulement le front : **`npm run dev:ui`** (alias de `npm run dev --prefix ui`).
- N’ouvre l’URL du dev server dans le navigateur **que si** Vite tourne ; l’URL Tauri est **`http://localhost:5173`** (`build.devUrl` dans `src-tauri/tauri.conf.json`).

## Publication sur GitHub (dépôt **summatim**)

Le dépôt Git local est prêt (branche `main`). Les gros artefacts (`donnees/modeles_locaux/`, `bin/**`, binaires et archives sous `dist/`) et les **données utilisateur** (`chaines.json`, `donnees/chaines/`, `donnees/textes_libres/`) sont ignorés par `.gitignore` : elles ne doivent pas être poussées sur GitHub.

1. **Ne jamais** coller le mot de passe du compte GitHub dans un terminal ou un script : utilisez un [jeton d’accès personnel (PAT)](https://github.com/settings/tokens) ou la **clé SSH** déjà générée sur cette machine (`~/.ssh/id_ed25519_summatim.pub`). Si un mot de passe a été divulgué ailleurs, **changez-le** sur GitHub.
2. Ajoutez la **clé publique** sur GitHub : [SSH and GPG keys](https://github.com/settings/keys) → *New SSH key* (contenu de `~/.ssh/id_ed25519_summatim.pub`). Le fichier `~/.ssh/config` est configuré pour utiliser cette clé avec `github.com`.
3. Authentifiez **GitHub CLI** une fois : `gh auth login -h github.com` (SSH ou HTTPS selon votre choix ; scopes conseillés : `repo`, `read:org`, `gist`).
4. Depuis la racine du projet : `./scripts/publier_summatim_github.sh` — crée le dépôt public **`jrbr43/summatim`**, ajoute `origin` et exécute `git push`. URL cible : `https://github.com/jrbr43/summatim`.

