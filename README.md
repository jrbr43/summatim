# Resume YouTube (Rust + LM Studio)

Workspace Rust avec **interface de bureau Tauri 2** (UI **Svelte** + Vite) et **CLI** partageant la même logique (`resume_youtube_core`).

L’application en ligne de commande permet de :
1. récupère les 10 dernières vidéos d’une chaîne YouTube (via `yt-dlp`),
2. extrait le transcript (sous-titres VTT/SRT via `yt-dlp`),
3. envoie le transcript à un modèle local via HTTP (LM Studio),
4. génère une **carte mentale** (Markdown).

## Prérequis

1. **Rust** (cargo)
2. **yt-dlp** accessible depuis le terminal (`yt-dlp --version` doit fonctionner).
3. **LM Studio** lancé sur une URL HTTP locale (par défaut, on vise une API “OpenAI-compatible”).
4. Pour l’UI de bureau : **Node.js** (npm) pour construire le front (`ui/`) et lancer `tauri dev` / `tauri build`.

## Structure du workspace

| Dossier / crate | Rôle |
|-----------------|------|
| `core/` (`resume_youtube_core`) | Logique : YouTube, transcripts, résumé, IA, `chaines.json`. |
| `cli/` (`resume_youtube_cli`, binaire `resume-youtube`) | Interface CLI. |
| `src-tauri/` (`resume_youtube_tauri`) | Backend Tauri (commandes IPC, événements jobs). |
| `ui/` | Front Svelte + Vite. |
| `chaines.json` | Liste des chaînes (lu/écrit par le core et l’UI ; chemin résolu : `cwd/chaines.json`, sinon parent, ex. racine du dépôt si le CWD est `src-tauri`). |

### Fichier de configuration `chaines.json`

Fichier JSON à la racine du dépôt (l’app résout le chemin même si le processus tourne depuis `src-tauri/`). Chaque entrée contient le nom affiché et l’URL de la chaîne. Depuis l’UI, seule l’URL est saisie : le **nom** est le handle après `@` dans l’URL.

```json
[
  { "nom": "Ma chaîne", "url_chaine": "https://www.youtube.com/@exemple" }
]
```

L’interface de bureau **charge ce fichier au démarrage**, restaure la dernière chaîne sélectionnée (mémorisée localement dans le navigateur embarqué), puis **récupère et affiche les dernières vidéos** sur le tableau de bord.
Depuis la colonne de gauche, une icône corbeille permet aussi de **supprimer une chaîne** de `chaines.json` (suppression locale de l’entrée, sans action Git).

La colonne de gauche liste les **chaînes enregistrées** ; un clic charge les dernières vidéos. Le bouton **Résumer** n’apparaît que si le résumé n’a pas déjà été enregistré localement pour cette vidéo. L’URL LM Studio est **fixée dans l’application** à **`http://192.168.1.71:1234`** (non affichée dans les paramètres).
Le modèle IA par défaut est **`google/gemma-4-26b-a4b`** (UI, Tauri et CLI si `--lm-modele` n’est pas fourni).

**Ordre des étapes :** le résumé télécharge d’abord le transcript (yt-dlp) ; **LM Studio n’est sollicité qu’ensuite**. Si cette première étape est longue ou bloquée, aucune requête n’apparaît encore côté LM — un bandeau « Traitement en cours » (non bloquant) et le bouton **Logs** le rappellent.

**Logs explicites dans l’application :**
- `[TRANSCRIPT]` : extraction/lecture/cache du transcript.
- `[MODELE]` : appel du modèle IA (LM Studio/OpenRouter) et réception de la réponse.
- `[SORTIE]` : enregistrement du résumé sur disque.

Les cartes mentales générées depuis l’UI sont **persistées sur disque** (avec cache transcript) dans une arborescence par chaîne et par vidéo :

- `donnees/chaines/<chaine>/videos/<id>/carte_mentale.md`
- `donnees/chaines/<chaine>/videos/<id>/transcript.txt` (cache pour éviter de relancer `yt-dlp`)

(Compat : lecture possible d’anciens fichiers `donnees/resumes/{id}_bullets.txt`.)

## Application de bureau (Tauri 2 + Svelte)

Depuis la racine du dépôt :

```bash
cd ui && npm install && cd ..
npm install
npm run dev
```

(Le `package.json` à la racine déclare `@tauri-apps/cli` ; `npm run dev` exécute `tauri dev`, qui lance Vite dans `ui/` puis l’app native.)

Build installable :

```bash
npm run build
```

L’UI couvre : gestion des chaînes (persistance dans `chaines.json`), chargement au démarrage et affichage des dernières vidéos, transcript / résumé par vidéo ou sur toute la chaîne, **paramètres** (bouton engrenage : nombre de vidéos à récupérer, actions par lot sur la chaîne, LM Studio, dossier de sortie), logs (bouton dédié) et barre de progression (événements `job-log` / `job-progression`). À l’ouverture d’un formulaire d’ajout (chaîne, vidéo hors chaîne, texte), la zone de saisie est préremplie automatiquement depuis le presse-papiers si possible. La **test-suite** reste disponible en ligne de commande uniquement.

En mode vidéos, la liste est à gauche et le détail (miniature ou **Afficher**) à droite avec des **onglets** : **Vidéo** (lecteur intégré) et **Carte mentale** (Markdown rendu).

### Rendu LaTeX dans la carte mentale

Dans les cartes mentales affichées par l’UI, les formules LaTeX sont rendues avec **KaTeX** quand des délimiteurs mathématiques sont détectés (`$...$`, `$$...$$`, `\(...\)`, `\[...\]`).

Les commandes LaTeX de flèche `\to`, `\rightarrow`, `\leftarrow`, `\uparrow` et `\downarrow` sont aussi converties en symboles visuels (`→`, `←`, `↑`, `↓`) hors blocs de code Markdown, pour éviter l’affichage littéral du texte simple.

## Commandes

### Résumer une chaîne

```bash
cargo run -p resume_youtube_cli -- resume-channel --channel-url "https://www.youtube.com/@CHANNEL" --nb-videos 10 --lm-base-url "http://localhost:1234" --lm-route "/v1/chat/completions" --lm-modele "nom-du-modele" --temperature 0.3
```

Si `--out` est fourni, les résumés sont enregistrés dans le dossier.

### Résumer une vidéo

```bash
cargo run -p resume_youtube_cli -- resume-video --video-url "https://www.youtube.com/watch?v=VIDEO_ID" --lm-base-url "http://localhost:1234" --lm-route "/v1/chat/completions" --lm-modele "nom-du-modele" --temperature 0.3
```

### Test “tout-en-un”

Le test exécute d’un coup : listing (si `--channel-url`), extraction du transcript, puis génération IA de la carte mentale.

Si `--lm-base-url` n’est pas fourni, le test ne fait que l’extraction transcript.

```bash
cargo run -p resume_youtube_cli -- test-suite --channel-url "https://www.youtube.com/@CHANNEL" --lm-base-url "http://localhost:1234" --lm-route "/v1/chat/completions" --lm-modele "nom-du-modele"
```

Alternative avec une vidéo unique :

```bash
cargo run -p resume_youtube_cli -- test-suite --video-url "https://www.youtube.com/watch?v=VIDEO_ID" --lm-base-url "http://localhost:1234"
```

### Extraire uniquement les transcripts (sans résumé)

```bash
cargo run -p resume_youtube_cli -- transcript-channel --channel-url "https://www.youtube.com/@CHANNEL" --nb-videos 10 --out "transcripts_out" --max-caracteres 25000
```

Pour une vidéo unique :

```bash
cargo run -p resume_youtube_cli -- transcript-video --video-url "https://www.youtube.com/watch?v=VIDEO_ID" --out "transcripts_out" --max-caracteres 25000
```

## Robustesse (anti-429)

Quand `yt-dlp` renvoie `HTTP 429 Too Many Requests` (souvent dû à `--sub-langs all`), le programme passe en mode plus parcimonieux :
- il liste d’abord les langues de sous-titres auto disponibles,
- il télécharge une langue à la fois (priorité `en`, puis `fr`, etc.),
- et il retry avec backoff si un `429` apparaît.

## Formats de sortie

Le programme force la réponse à respecter le format demandé :

Sortie : une **carte mentale** en Markdown, sous forme de liste hiérarchique (listes imbriquées).
  - `titre` (string)
  - `resume` (string)
  - `points_cles` (liste de strings)

## Notes

- L’extraction transcript dépend des sous-titres disponibles. En cas d’absence de VTT, le code tente un fallback SRT.
- L’appel LM Studio est configuré via `--lm-base-url` et `--lm-route`.

## Dépendances systèmes

**`yt-dlp`** est requis pour les résumés de **vidéos YouTube** (extraction des sous-titres). Il n’est **pas** fourni avec l’application : il faut l’installer une fois sur la machine.

- **Linux (recommandé, version à jour)** :  
  `python3 -m pip install --user -U yt-dlp`  
  Vérifie avec `python3 -m yt_dlp --version` (l’app utilise ce mode si le script `yt-dlp` n’est pas trouvé). L’application cherche aussi `~/.local/bin/yt-dlp` et le `PATH`. Variables utiles : **`YT_DLP_CHEMIN`** (script), **`PYTHON`** / **`PYTHON3`** (interpréteur pour `python -m yt_dlp`).
- **Linux (paquet)** : `sudo apt install yt-dlp` — souvent **plus ancien** que PyPI ; en cas d’erreur YouTube, préfère `pip` ou le [binaire officiel](https://github.com/yt-dlp/yt-dlp/releases).
- **macOS** : `brew install yt-dlp`.
- **Windows** : installe via [winget](https://github.com/yt-dlp/yt-dlp/wiki/Installation), [Scoop](https://scoop.sh/), `pip`, ou le binaire des *releases* GitHub ; l’exécutable doit être dans le **`PATH`**, ou indique son chemin avec **`YT_DLP_CHEMIN`**.

Détails (ordre de détection du binaire, AppImage, dépannage) : voir **`HELP.md`**.

