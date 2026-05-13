<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fly } from "svelte/transition";
  import DOMPurify from "dompurify";
  import renderMathInElement from "katex/contrib/auto-render";
  import "katex/dist/katex.min.css";
  import mermaid from "mermaid";
  import { marked } from "marked";
  import type { Tokens } from "marked";

  /** Échappe le texte inséré dans du HTML (blocs Mermaid, etc.). */
  function echapperPourInsertionHtml(texte: string): string {
    return texte
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  marked.use({
    gfm: true,
    breaks: true,
  });

  marked.use({
    renderer: {
      code(token: Tokens.Code) {
        const langue = (token.lang ?? "").trim().toLowerCase();
        if (langue === "mermaid") {
          const brut = token.text.replace(/\n$/, "") + "\n";
          const corps = token.escaped ? brut : echapperPourInsertionHtml(brut);
          return `<div class="mermaid diagramme-markdown">${corps}</div>\n`;
        }
        return false as unknown as string;
      },
    },
  });

  type EntreeChaine = { nom: string; url_chaine: string };
  type VideoInfo = {
    id: string;
    titre: string;
    url: string;
    datePublication?: string | null;
  };
  /** Texte collé par l’utilisateur pour résumer sans passer par une URL. */
  type TexteLibre = {
    id: string;
    titre: string;
    contenu: string;
    /** Carte mentale (Markdown) : copie locale + fichier `donnees/textes_libres/<id>/carte_mentale.md`. */
    resumeCarteMentale?: string;
  };
  type Progression = { courant: number; total: number; titre: string; id: string };

  let vue: "videos" | "parametres" = "videos";

  let chaines: EntreeChaine[] = [];
  let urlNouvelleChaine = "";
  /** Formulaire d’ajout repliable dans la barre latérale */
  let panneauAjoutOuvert = false;

  /** Clé de cache local (vidéos sans chaîne) — alignée sur le dossier `donnees/chaines/direct/`. */
  const CLE_URL_VIDEO_DIRECTE = "__video_directe__";
  let panneauAjoutVideoSeuleOuvert = false;
  let urlVideoSeule = "";

  let urlChaineActive = "";
  let nbVideos = 10;
  let videos: VideoInfo[] = [];

  /** Mode liste : vidéos ajoutées une par une, stockées sous `direct` côté disque. */
  $: estModeVideoDirecte = urlChaineActive === CLE_URL_VIDEO_DIRECTE;

  $: nomChaineCourante =
    urlChaineActive === CLE_URL_VIDEO_DIRECTE
      ? "Vidéos seules (hors chaîne)"
      : chaines.find((c) => c.url_chaine === urlChaineActive)?.nom ?? "";

  /** `null` = dossier `direct` sur disque (pas d’URL de chaîne). */
  function urlChainePourIpc(): string | null {
    const u = urlChaineActive.trim();
    if (!u || u === CLE_URL_VIDEO_DIRECTE) return null;
    return u;
  }

  /** Panneau principal : liste YouTube ou texte libre à résumer. */
  let modeContenuPrincipal: "youtube" | "texte_libre" = "youtube";
  let listeTextesLibres: TexteLibre[] = [];
  let idTexteLibreEnCours: string | null = null;
  let panneauAjoutTexteLibreOuvert = false;
  let saisieTexteLibre = "";
  const transitionDescenteDepuisBouton = { y: -14, duration: 220 };
  const transitionMonteeDepuisBouton = { y: 14, duration: 220 };

  /** Vidéo sélectionnée (affichée dans le panneau de droite). */
  let videoSelectionneeId: string | null = null;
  /** Onglet actif pour la vidéo sélectionnée (panneau de droite). */
  type OngletVideo = "video" | "carte_mentale";
  let ongletPourVideoSelectionnee: OngletVideo = "video";
  /**
   * Carte mentale chargée depuis le disque, par **contexte de chaîne + id vidéo**
   * (aligné sur `donnees/chaines/<contexte>/videos/<id>/`). Sans cela, la même `videoId`
   * sous « Vidéos seules » vs une chaîne YouTube partageait une entrée de cache incorrecte.
   */
  let cacheCarteMentaleParVideo: Record<string, string> = {};

  /** Même logique que le backend : dossier `direct` si pas d’URL de chaîne. */
  function cleContexteCachePourCarte(): string {
    const u = urlChaineActive.trim();
    if (!u || u === CLE_URL_VIDEO_DIRECTE) return "direct";
    return u;
  }

  function cleCacheCartePourVideo(idVideo: string): string {
    return `${cleContexteCachePourCarte()}::${idVideo}`;
  }
  // Réservé (ancienne lecture autoplay) : conservé si on veut réintroduire un mode lecture.
  let videoEnLectureId: string | null = null;

  function supprimerRaisonnementPourAffichage(texte: string): string {
    let t = texte ?? "";
    // Blocs XML-style souvent utilisés par des modèles de raisonnement.
    t = t.replace(/<think>[\s\S]*?<\/think>/gi, "");
    t = t.replace(/<analysis>[\s\S]*?<\/analysis>/gi, "");
    t = t.replace(/<reasoning>[\s\S]*?<\/reasoning>/gi, "");
    // Formats "Reasoning: ... Final: ..."
    t = t.replace(/(^|\n)reasoning\s*:\s*[\s\S]*?(?=(^|\n)final\s*:)/gi, "$1");
    t = t.replace(/(^|\n)final\s*:\s*/gi, "$1");
    return t.trim();
  }

  const cleFournisseurIaStockage = "resume_youtube_fournisseur_ia_v1";
  const cleCleOpenrouterStockage = "resume_youtube_cle_openrouter_v1";
  const cleModeleLmStudioStockage = "resume_youtube_modele_lm_studio_v1";
  const cleModeleOpenrouterStockage = "resume_youtube_modele_openrouter_v1";
  const cleTailleFenetreTokensStockage = "resume_youtube_taille_fenetre_tokens_v1";
  /** Base HTTP fixe du serveur LM Studio (non exposée dans l’interface). */
  const URL_BASE_LM_STUDIO_DEFAUT = "http://192.168.1.71:1234";
  /** Valeur initiale du champ clé OpenRouter (aucune clé réelle dans le dépôt ; l’utilisateur saisit la sienne). */
  const CLE_OPENROUTER_DEFAUT = "";

  type FournisseurIa = "lm_studio" | "openrouter" | "local";

  function lireCleOpenrouterInitial(): string {
    try {
      const brut = localStorage.getItem(cleCleOpenrouterStockage);
      if (brut !== null) {
        return brut.trim();
      }
    } catch {
      /* ignore */
    }
    return CLE_OPENROUTER_DEFAUT;
  }

  function lireTexteStockage(cle: string): string {
    try {
      return localStorage.getItem(cle) ?? "";
    } catch {
      return "";
    }
  }

  function lireFournisseurIaInitial(): FournisseurIa {
    try {
      const f = localStorage.getItem(cleFournisseurIaStockage);
      if (f === "openrouter" || f === "lm_studio" || f === "local") {
        return f;
      }
    } catch {
      /* ignore */
    }
    return "lm_studio";
  }

  /** Plafond aligné sur le backend (`commandes.rs`) pour n_ctx local. */
  const TAILLE_FENETRE_TOKENS_MAX = 131_072;
  const TAILLE_FENETRE_TOKENS_DEFAUT = 50_000;
  function lireTailleFenetreTokensInitial(): number {
    try {
      const brut = localStorage.getItem(cleTailleFenetreTokensStockage);
      if (brut) {
        const n = Number(brut);
        if (Number.isFinite(n) && n >= 512 && n <= TAILLE_FENETRE_TOKENS_MAX) {
          return Math.round(n);
        }
      }
    } catch {
      /* ignore */
    }
    return TAILLE_FENETRE_TOKENS_DEFAUT;
  }

  /** Modèles chargés depuis le serveur LM Studio local (liste `/v1/models`). */
  let modelesLmStudioDisponibles: string[] = [];
  /** Modèles gratuits OpenRouter (`:free` dans l’identifiant). */
  let modelesOpenrouterGratuits: string[] = [];
  let chargementModeles = false;
  const modeleParDefautLmStudio = "google/gemma-4-26b-a4b";

  /** Fournisseur utilisé pour tous les appels IA (résumés). */
  let fournisseurIa: FournisseurIa = lireFournisseurIaInitial();
  /** Onglet actuellement consulté dans les paramètres IA (indépendant du fournisseur actif). */
  let ongletParametresIa: FournisseurIa = fournisseurIa;
  let cleOpenrouter = lireCleOpenrouterInitial();
  let modeleLmStudio = lireTexteStockage(cleModeleLmStudioStockage) || modeleParDefautLmStudio;
  let modeleOpenrouter = lireTexteStockage(cleModeleOpenrouterStockage);
  let lmTemperature = 0.7;
  let tailleFenetreTokens = lireTailleFenetreTokensInitial();
  const formatSortie = "carte_mentale";

  type EtatModeleLocal = {
    identifiant: string;
    nomFichier: string;
    urlTelechargement: string;
    chemin: string;
    present: boolean;
    charge: boolean;
    tailleOctets: number | null;
    nCtxCourant: number | null;
  };
  let etatModeleLocal: EtatModeleLocal = {
    identifiant: "MARTHA-0.8B-Qwen3.5-Omni (Q8_0)",
    nomFichier: "MARTHA-0.8B-Qwen3.5-Omni.Q8_0.gguf",
    urlTelechargement: "",
    chemin: "",
    present: false,
    charge: false,
    tailleOctets: null,
    nCtxCourant: null,
  };
  let progressionTelechargementModeleLocal: { recus: number; total: number | null } | null = null;
  let telechargementModeleLocalEnCours = false;

  let dossierSortie: string | null = null;

  let logs: string[] = [];
  let progression: Progression | null = null;
  let erreur: string | null = null;
  let chargement = false;
  let etapeInfoTraitement: "transcript" | "modele" | null = null;
  /** Compteur pour ignorer les réponses obsolètes de « mettre à jour ». */
  let numeroRequeteMiseAJourVideos = 0;
  /** Message de chargement contextuel selon l’action en cours. */
  let messageChargementContextuel: string | null = null;
  /** Vidéo en cours de résumé LM (null = traitement global ou liste). */
  let idVideoEnTraitement: string | null = null;
  /** Texte libre en cours de résumé LM (pour le libellé du modal de chargement). */
  let idTexteEnResume: string | null = null;
  let logsOuverts = false;
  /** Verrouillages fins: éviter de geler toute l'UI sur un seul traitement. */
  $: estMiseAJourEnCours = chargement && messageChargementContextuel != null;
  $: estResumeVideoEnCours = chargement && idVideoEnTraitement != null;
  $: estResumeTexteEnCours = chargement && idTexteEnResume != null;
  $: estOperationGlobaleBloquante =
    chargement && !estMiseAJourEnCours && !estResumeVideoEnCours && !estResumeTexteEnCours;

  const cleChaineActiveStockage = "resume_youtube_url_chaine_active";
  const cleLmTemperatureStockage = "resume_youtube_lm_temperature_v1";
  const cleNbVideosStockage = "resume_youtube_nb_videos_v1";
  /** IDs YouTube des vidéos déjà résumées (persisté localement). */
  const cleIdsResumesFaits = "resume_youtube_ids_resumes_v1";
  const cleVideosParChaine = "resume_youtube_videos_par_chaine_v1";
  const cleStockageTextesLibres = "resume_youtube_textes_libres_v1";

  function chargerTemperatureDepuisStockage() {
    try {
      const brut = localStorage.getItem(cleLmTemperatureStockage);
      if (!brut) return;
      const n = Number(brut);
      if (Number.isFinite(n)) {
        lmTemperature = n;
      }
    } catch {
      /* ignore */
    }
  }

  function memoriserTemperatureDansStockage(val: number) {
    try {
      if (!Number.isFinite(val)) return;
      localStorage.setItem(cleLmTemperatureStockage, String(val));
    } catch {
      /* ignore */
    }
  }

  $: memoriserTemperatureDansStockage(lmTemperature);

  function chargerNbVideosDepuisStockage() {
    try {
      const brut = localStorage.getItem(cleNbVideosStockage);
      if (!brut) return;
      const n = Number(brut);
      if (Number.isFinite(n) && n >= 1 && n <= 50) {
        nbVideos = Math.round(n);
      }
    } catch {
      /* ignore */
    }
  }

  function memoriserNbVideosDansStockage(val: number) {
    try {
      if (!Number.isFinite(val) || val < 1 || val > 50) return;
      localStorage.setItem(cleNbVideosStockage, String(Math.round(val)));
    } catch {
      /* ignore */
    }
  }

  $: memoriserNbVideosDansStockage(nbVideos);

  function memoriserTailleFenetreTokensDansStockage(val: number) {
    try {
      if (!Number.isFinite(val) || val < 512 || val > TAILLE_FENETRE_TOKENS_MAX) return;
      localStorage.setItem(cleTailleFenetreTokensStockage, String(Math.round(val)));
    } catch {
      /* ignore */
    }
  }

  $: memoriserTailleFenetreTokensDansStockage(tailleFenetreTokens);

  function memoriserFournisseurIaEtClesDansStockage() {
    try {
      localStorage.setItem(cleFournisseurIaStockage, fournisseurIa);
      localStorage.setItem(cleCleOpenrouterStockage, cleOpenrouter);
      localStorage.setItem(cleModeleLmStudioStockage, modeleLmStudio);
      localStorage.setItem(cleModeleOpenrouterStockage, modeleOpenrouter);
    } catch {
      /* ignore */
    }
  }

  $: {
    fournisseurIa;
    cleOpenrouter;
    modeleLmStudio;
    modeleOpenrouter;
    memoriserFournisseurIaEtClesDansStockage();
  }

  let idsResumesFaits = new Set<string>();
  let carteMentaleExisteParVideoId: Record<string, boolean> = {};

  function lireIdsResumesDepuisStockage(): Set<string> {
    try {
      const brut = localStorage.getItem(cleIdsResumesFaits);
      if (!brut) return new Set();
      const tableau = JSON.parse(brut) as string[];
      return new Set(Array.isArray(tableau) ? tableau : []);
    } catch {
      return new Set();
    }
  }

  function ecrireIdsResumesVersStockage(ids: Set<string>) {
    try {
      localStorage.setItem(cleIdsResumesFaits, JSON.stringify([...ids]));
    } catch {
      /* ignore */
    }
  }

  function marquerResumeCommeFait(idVideo: string) {
    idsResumesFaits = new Set([...idsResumesFaits, idVideo]);
    ecrireIdsResumesVersStockage(idsResumesFaits);
  }

  type CacheChaine = { dateMaj: number; videos: VideoInfo[] };

  function lireCacheVideos(): Record<string, CacheChaine> {
    try {
      const brut = localStorage.getItem(cleVideosParChaine);
      if (!brut) return {};
      const obj = JSON.parse(brut) as Record<string, CacheChaine>;
      return obj && typeof obj === "object" ? obj : {};
    } catch {
      return {};
    }
  }

  function ecrireCacheVideos(cache: Record<string, CacheChaine>) {
    try {
      localStorage.setItem(cleVideosParChaine, JSON.stringify(cache));
    } catch {
      /* ignore */
    }
  }

  function videosDepuisCachePour(urlChaine: string): VideoInfo[] {
    const cache = lireCacheVideos();
    const entree = cache[urlChaine];
    return entree?.videos ?? [];
  }

  function enregistrerVideosDansCache(urlChaine: string, videos: VideoInfo[]) {
    const cache = lireCacheVideos();
    cache[urlChaine] = { dateMaj: Date.now(), videos };
    ecrireCacheVideos(cache);
  }

  function chargerListeTextesLibresDepuisStockage() {
    try {
      const brut = localStorage.getItem(cleStockageTextesLibres);
      if (!brut) return;
      const tableau = JSON.parse(brut) as TexteLibre[];
      if (!Array.isArray(tableau)) return;
      const valides = tableau.filter(
        (t): t is TexteLibre =>
          typeof t === "object" &&
          t != null &&
          typeof (t as TexteLibre).id === "string" &&
          typeof (t as TexteLibre).titre === "string" &&
          typeof (t as TexteLibre).contenu === "string",
      );
      listeTextesLibres = valides.map((t) => ({
        ...t,
        resumeCarteMentale:
          typeof t.resumeCarteMentale === "string" ? t.resumeCarteMentale : undefined,
      }));
    } catch {
      /* ignore */
    }
  }

  function enregistrerListeTextesLibresDansStockage(liste: TexteLibre[]) {
    try {
      localStorage.setItem(cleStockageTextesLibres, JSON.stringify(liste));
    } catch {
      /* ignore */
    }
  }

  function titreDepuisContenuTexte(contenu: string): string {
    const ligne =
      contenu
        .trim()
        .split(/\r?\n/)
        .find((l) => l.trim().length > 0) ?? "";
    const court = ligne.trim().slice(0, 72);
    return court.length > 0 ? court : "Texte sans titre";
  }

  function revenirAuContenuYoutube() {
    modeContenuPrincipal = "youtube";
    idTexteLibreEnCours = null;
  }

  function selectionnerTexteLibre(p: TexteLibre) {
    erreur = null;
    vue = "videos";
    modeContenuPrincipal = "texte_libre";
    idTexteLibreEnCours = p.id;
    videoSelectionneeId = null;
  }

  async function ajouterTexteLibre() {
    erreur = null;
    const contenu = saisieTexteLibre.trim();
    if (!contenu) {
      erreur = "Saisis un texte à résumer.";
      return;
    }
    const id =
      typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
        ? crypto.randomUUID()
        : `texte_${Date.now()}_${Math.random().toString(36).slice(2, 9)}`;
    const titre = titreDepuisContenuTexte(contenu);
    const entree: TexteLibre = { id, titre, contenu };
    listeTextesLibres = [
      entree,
      ...listeTextesLibres.filter((p) => p.contenu.trim() !== contenu),
    ];
    enregistrerListeTextesLibresDansStockage(listeTextesLibres);
    saisieTexteLibre = "";
    panneauAjoutTexteLibreOuvert = false;
    selectionnerTexteLibre(entree);
    await resumerTexteLibreEntree(entree);
  }

  async function supprimerTexteLibre(texte: TexteLibre) {
    const id = texte.id;
    const indexInitial = listeTextesLibres.findIndex((p) => p.id === id);
    const etaitSelectionne = idTexteLibreEnCours === id;
    if (invokeTauri) {
      try {
        await appeler<void>("supprimer_dossier_texte_libre", { idTexte: id });
      } catch {
        /* best-effort : on retire quand même l’entrée UI */
      }
    }
    listeTextesLibres = listeTextesLibres.filter((p) => p.id !== id);
    enregistrerListeTextesLibresDansStockage(listeTextesLibres);
    if (etaitSelectionne) {
      const voisin = listeTextesLibres[indexInitial] ?? listeTextesLibres[indexInitial - 1] ?? null;
      idTexteLibreEnCours = voisin?.id ?? null;
      if (!idTexteLibreEnCours) revenirAuContenuYoutube();
    }
  }

  /** Si la carte n’est pas dans le stockage local, tente `donnees/textes_libres/<id>/carte_mentale.md`. */
  async function hydraterCartesTextesLibresDepuisDisque() {
    if (!invokeTauri) return;
    let modifie = false;
    const suivant = await Promise.all(
      listeTextesLibres.map(async (p) => {
        if (p.resumeCarteMentale != null && p.resumeCarteMentale.trim().length > 0) {
          return p;
        }
        try {
          const md = await appeler<string | null>("lire_carte_mentale_texte_libre", {
            idTexte: p.id,
          });
          if (md != null && md.trim().length > 0) {
            modifie = true;
            return { ...p, resumeCarteMentale: md };
          }
        } catch {
          /* ignore */
        }
        return p;
      }),
    );
    if (modifie) {
      listeTextesLibres = suivant;
      enregistrerListeTextesLibresDansStockage(listeTextesLibres);
    }
  }

  /** Écrit sur disque tout ce qui est en mémoire (localStorage hydraté) : sauvegarde / migration. */
  async function synchroniserTextesLibresVersDisque() {
    if (!invokeTauri) return;
    if (listeTextesLibres.length === 0) return;
    try {
      await appeler<number>("synchroniser_textes_libres_vers_disque", {
        entrees: listeTextesLibres.map((p) => ({
          id: p.id,
          contenu: p.contenu,
          resumeCarteMentale: p.resumeCarteMentale,
        })),
      });
    } catch {
      /* ignore */
    }
  }

  function mettreAJourResumeTexteLibre(id: string, resumeMarkdown: string) {
    listeTextesLibres = listeTextesLibres.map((p) =>
      p.id === id ? { ...p, resumeCarteMentale: resumeMarkdown } : p,
    );
    enregistrerListeTextesLibresDansStockage(listeTextesLibres);
  }

  type FonctionInvoke = <T>(commande: string, args?: Record<string, unknown>) => Promise<T>;
  type FonctionListen = <T>(
    evenement: string,
    gestionnaire: (e: { payload: T }) => void,
  ) => Promise<() => void>;

  let invokeTauri: FonctionInvoke | null = null;
  let listenTauri: FonctionListen | null = null;

  /**
   * Sous Tauri : commande Rust `lire_texte_presse_papiers` (crate arboard, API système).
   * Hors Tauri (navigateur seul) : repli sur l’API Clipboard du navigateur.
   */
  async function lireTexteBrutDepuisPressePapiers(): Promise<string | null> {
    if (invokeTauri) {
      try {
        const t = await invokeTauri<string | null>("lire_texte_presse_papiers");
        if (t == null || t === "") return null;
        return t;
      } catch {
        return null;
      }
    }
    try {
      if (typeof navigator === "undefined" || !navigator.clipboard?.readText) return null;
      const t = await navigator.clipboard.readText();
      if (!t || !t.trim().length) return null;
      return t;
    } catch {
      return null;
    }
  }

  async function lirePressePapiersPourZoneVideo() {
    const t = await lireTexteBrutDepuisPressePapiers();
    if (t) {
      const u = t.trim();
      if (u.length > 0) urlVideoSeule = u;
    }
  }

  async function lirePressePapiersPourZoneChaine() {
    const t = await lireTexteBrutDepuisPressePapiers();
    if (t) {
      const u = t.trim();
      if (u.length > 0) urlNouvelleChaine = u;
    }
  }

  async function lirePressePapiersPourZoneTexte() {
    const t = await lireTexteBrutDepuisPressePapiers();
    if (t && t.trim().length > 0) saisieTexteLibre = t;
  }

  async function basculerPanneauAjoutChaine() {
    const ouvrir = !panneauAjoutOuvert;
    panneauAjoutOuvert = ouvrir;
    if (ouvrir) await lirePressePapiersPourZoneChaine();
  }

  async function basculerPanneauAjoutVideoSeule() {
    const ouvrir = !panneauAjoutVideoSeuleOuvert;
    panneauAjoutVideoSeuleOuvert = ouvrir;
    if (ouvrir) await lirePressePapiersPourZoneVideo();
  }

  async function basculerPanneauAjoutTexteLibre() {
    const ouvrir = !panneauAjoutTexteLibreOuvert;
    panneauAjoutTexteLibreOuvert = ouvrir;
    if (ouvrir) await lirePressePapiersPourZoneTexte();
  }

  async function gererValidationAjoutTexteAvecEntree(event: KeyboardEvent) {
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    if (chargement) return;
    await ajouterTexteLibre();
  }

  async function appeler<T>(commande: string, args?: Record<string, unknown>): Promise<T> {
    if (!invokeTauri) {
      throw new Error(
        "Backend Tauri indisponible (initialisation en cours ou mode sans IPC).",
      );
    }
    if (args === undefined) {
      return invokeTauri<T>(commande);
    }
    return invokeTauri<T>(commande, args);
  }

  function ajouterLog(ligne: string) {
    if (ligne.includes("[TRANSCRIPT]")) {
      etapeInfoTraitement = "transcript";
    } else if (ligne.includes("[MODELE]")) {
      etapeInfoTraitement = "modele";
    }
    logs = [...logs, ligne].slice(-200);
  }

  function effacerLogs() {
    logs = [];
    progression = null;
    etapeInfoTraitement = null;
  }

  /** Normalise la réponse IPC (snake_case ou camelCase) vers le format attendu par l’UI. */
  function normaliserEntreeDepuisIpc(entree: unknown): EntreeChaine {
    const o = entree as Record<string, unknown>;
    const nom = typeof o.nom === "string" ? o.nom : "";
    const url =
      typeof o.url_chaine === "string"
        ? o.url_chaine
        : typeof o.urlChaine === "string"
          ? o.urlChaine
          : "";
    return { nom, url_chaine: url };
  }

  /** Normalise une vidéo depuis l’IPC (snake_case côté Rust). */
  function normaliserVideoDepuisIpc(entree: unknown): VideoInfo {
    const o = entree as Record<string, unknown>;
    const id = typeof o.id === "string" ? o.id : "";
    const titre = typeof o.titre === "string" ? o.titre : "";
    const url = typeof o.url === "string" ? o.url : "";
    const datePublication =
      typeof o.datePublication === "string"
        ? o.datePublication
        : typeof o.date_publication === "string"
          ? o.date_publication
          : null;
    return { id, titre, url, datePublication };
  }

  /** En dev navigateur : lit le même `chaines.json` que le dépôt (middleware Vite). */
  async function chargerChainesDepuisNavigateur() {
    try {
      const reponse = await fetch("/chaines.json", { cache: "no-store" });
      if (!reponse.ok) return;
      const brut = (await reponse.json()) as unknown[];
      chaines = brut.map(normaliserEntreeDepuisIpc);
    } catch {
      /* ignore */
    }
  }

  async function chargerChaines() {
    if (!invokeTauri) {
      await chargerChainesDepuisNavigateur();
      return;
    }
    try {
      const brut = await invokeTauri<unknown[]>("lister_chaines");
      chaines = brut.map(normaliserEntreeDepuisIpc);
    } catch (e) {
      await chargerChainesDepuisNavigateur();
      if (chaines.length > 0) {
        erreur =
          "Aperçu : chaînes lues depuis le fichier local. Pour les vidéos et le backend, utilise la fenêtre lancée par « npm run dev » (pas seulement un onglet sur localhost).";
      } else {
        erreur = String(e);
      }
    }
  }

  function memoriserChaineActive(url: string) {
    try {
      localStorage.setItem(cleChaineActiveStockage, url);
    } catch {
      /* ignore */
    }
  }

  function urlChaineParDefaut(liste: EntreeChaine[]): string {
    try {
      const sauvegarde = localStorage.getItem(cleChaineActiveStockage);
      if (sauvegarde === CLE_URL_VIDEO_DIRECTE) {
        return CLE_URL_VIDEO_DIRECTE;
      }
      if (liste.length === 0) {
        return "";
      }
      if (sauvegarde && liste.some((c) => c.url_chaine === sauvegarde)) {
        return sauvegarde;
      }
    } catch {
      /* ignore */
    }
    return liste.length > 0 ? liste[0].url_chaine : "";
  }

  async function chargerDernieresVideosAuDemarrage() {
    if (!invokeTauri) return;
    urlChaineActive = urlChaineParDefaut(chaines);
    memoriserChaineActive(urlChaineActive);
    // Au démarrage, on affiche d’abord ce qu’on a déjà (cache) sans refetch.
    videos = videosDepuisCachePour(urlChaineActive.trim());
    void verifierResumesExistantsPourListe();
    if (!estModeVideoDirecte && urlChaineActive.trim() && videos.length === 0) {
      // Premier lancement (ou cache vidé) : charger automatiquement la chaîne active.
      await verifierNouvellesVideos();
    }
  }

  async function selectionnerChaine(c: EntreeChaine) {
    erreur = null;
    vue = "videos";
    revenirAuContenuYoutube();
    urlChaineActive = c.url_chaine;
    memoriserChaineActive(c.url_chaine);
    // Ne pas refetch au changement de chaîne : afficher le cache.
    videos = videosDepuisCachePour(urlChaineActive.trim());
    void verifierResumesExistantsPourListe();
    if (videos.length === 0) {
      await verifierNouvellesVideos();
    }
    await rechargerCarteVideoSelectionneeSiDisponible();
  }

  async function selectionnerModeVideosSeules() {
    erreur = null;
    vue = "videos";
    revenirAuContenuYoutube();
    urlChaineActive = CLE_URL_VIDEO_DIRECTE;
    memoriserChaineActive(CLE_URL_VIDEO_DIRECTE);
    videos = videosDepuisCachePour(CLE_URL_VIDEO_DIRECTE);
    void verifierResumesExistantsPourListe();
    await rechargerCarteVideoSelectionneeSiDisponible();
  }

  async function ajouterVideoSeule() {
    erreur = null;
    if (!invokeTauri) {
      erreur =
        "Backend Tauri indisponible. Lance l’application via la fenêtre Tauri pour ajouter une vidéo.";
      return;
    }
    if (!urlVideoSeule.trim()) {
      erreur = "URL de la vidéo requise.";
      return;
    }
    chargement = true;
    try {
      revenirAuContenuYoutube();
      const brut = await appeler<unknown>("infos_video_depuis_url", {
        videoUrl: urlVideoSeule.trim(),
      });
      const nv = normaliserVideoDepuisIpc(brut);
      const existantes = videosDepuisCachePour(CLE_URL_VIDEO_DIRECTE);
      const fusion = [nv, ...existantes.filter((x) => x.id !== nv.id)];
      enregistrerVideosDansCache(CLE_URL_VIDEO_DIRECTE, fusion);
      urlChaineActive = CLE_URL_VIDEO_DIRECTE;
      memoriserChaineActive(CLE_URL_VIDEO_DIRECTE);
      videos = fusion;
      urlVideoSeule = "";
      panneauAjoutVideoSeuleOuvert = false;
      void verifierResumesExistantsPourListe();
      await rechargerCarteVideoSelectionneeSiDisponible();
      ajouterLog(`Vidéo ajoutée : ${nv.titre}`);
    } catch (e) {
      erreur = String(e);
    } finally {
      chargement = false;
    }
  }

  async function ajouterChaine() {
    erreur = null;
    if (!urlNouvelleChaine.trim()) {
      erreur = "URL de chaîne requise.";
      return;
    }
    const urlSaisie = urlNouvelleChaine.trim();
    chargement = true;
    try {
      await appeler<void>("ajouter_chaine", {
        urlChaine: urlSaisie,
      });
      urlNouvelleChaine = "";
      await chargerChaines();
      urlChaineActive = urlSaisie;
      memoriserChaineActive(urlSaisie);
      panneauAjoutOuvert = false;
      vue = "videos";
      await verifierNouvellesVideos();
    } catch (e) {
      erreur = String(e);
    } finally {
      chargement = false;
    }
  }

  async function supprimerChaine(cible: EntreeChaine) {
    if (!invokeTauri) return;
    erreur = null;
    chargement = true;
    try {
      const compte = await appeler<number>("supprimer_chaine", {
        urlChaine: cible.url_chaine,
      });
      if (compte <= 0) {
        ajouterLog(`Aucune chaîne supprimée : ${cible.nom}`);
        return;
      }
      await chargerChaines();
      if (urlChaineActive === cible.url_chaine) {
        const urlSuivante = urlChaineParDefaut(chaines);
        urlChaineActive = urlSuivante;
        memoriserChaineActive(urlSuivante);
        if (urlSuivante === CLE_URL_VIDEO_DIRECTE) {
          videos = videosDepuisCachePour(CLE_URL_VIDEO_DIRECTE);
        } else if (urlSuivante) {
          videos = videosDepuisCachePour(urlSuivante.trim());
        } else {
          videos = [];
        }
      }
      ajouterLog(`Chaîne supprimée : ${cible.nom}`);
      void verifierResumesExistantsPourListe();
      await rechargerCarteVideoSelectionneeSiDisponible();
    } catch (e) {
      erreur = String(e);
    } finally {
      chargement = false;
    }
  }

  async function verifierNouvellesVideos() {
    erreur = null;
    messageChargementContextuel = null;
    const urlDemandee = urlChaineActive.trim();
    if (estModeVideoDirecte) {
      erreur = "« Vidéos plus récentes » s’applique uniquement à une chaîne YouTube.";
      return;
    }
    if (!urlDemandee) {
      erreur = "Sélectionne une chaîne dans la colonne de gauche.";
      return;
    }
    if (!invokeTauri) {
      // Mode navigateur (sans IPC) : on peut lire chaines.json mais pas appeler yt-dlp / le backend Rust.
      erreur =
        "Mode navigateur : le backend Tauri n’est pas disponible. Pour charger les vidéos, lance l’app via « npm run dev » et utilise la fenêtre Tauri.";
      return;
    }
    memoriserChaineActive(urlDemandee);
    const idRequeteCourante = ++numeroRequeteMiseAJourVideos;
    messageChargementContextuel = "Mise à jour en cours : récupération des vidéos les plus récentes de la chaîne.";
    chargement = true;
    try {
      const brut = await appeler<unknown[]>("lister_videos_chaine", {
        urlChaine: urlDemandee,
        nbVideos,
      });
      const nouvelles = brut.map(normaliserVideoDepuisIpc);
      // Toujours fusionner et mémoriser pour la chaîne demandée, même si l’utilisateur
      // a changé de chaîne entre-temps : cela évite de « perdre » la mise à jour.
      const existantes = videosDepuisCachePour(urlDemandee);
      const idsNouvelles = new Set(nouvelles.map((v) => v.id));
      const fusion = [...nouvelles, ...existantes.filter((v) => !idsNouvelles.has(v.id))];
      enregistrerVideosDansCache(urlDemandee, fusion);
      const estRequeteObsolete =
        idRequeteCourante !== numeroRequeteMiseAJourVideos || urlChaineActive.trim() !== urlDemandee;
      if (estRequeteObsolete) {
        ajouterLog(
          "Mise à jour terminée pour une autre chaîne (résultat conservé dans son cache local).",
        );
        return;
      }
      videos = fusion;
      ajouterLog(`Vérification terminée : ${nouvelles.length} vidéo(s) récupérée(s).`);
      void verifierResumesExistantsPourListe();
    } catch (e) {
      erreur = String(e);
    } finally {
      if (idRequeteCourante === numeroRequeteMiseAJourVideos) {
        messageChargementContextuel = null;
        chargement = false;
      }
    }
  }

  function lmPayload() {
    const base: Record<string, unknown> = {
      fournisseur: fournisseurIa,
      cleOpenrouter: cleOpenrouter.trim() || null,
      modele:
        fournisseurIa === "openrouter"
          ? modeleOpenrouter.trim() || null
          : fournisseurIa === "local"
            ? etatModeleLocal.identifiant
            : modeleLmStudio.trim() || null,
      temperature: lmTemperature,
    };
    if (fournisseurIa === "lm_studio") {
      base.baseUrl = URL_BASE_LM_STUDIO_DEFAUT;
      base.cleLmStudio = null;
    } else if (fournisseurIa === "local") {
      base.cleLmStudio = null;
      base.nCtx = Math.round(tailleFenetreTokens);
    } else {
      base.cleLmStudio = null;
    }
    return base;
  }

  async function rafraichirEtatModeleLocal() {
    if (!invokeTauri) return;
    try {
      const e = await appeler<EtatModeleLocal>("etat_modele_local");
      etatModeleLocal = e;
    } catch (err) {
      ajouterLog(`Impossible de lire l’état du modèle local: ${String(err)}`);
    }
  }

  async function telechargerModeleLocal() {
    if (!invokeTauri) {
      erreur = "Backend Tauri indisponible : impossible de télécharger le modèle.";
      return;
    }
    erreur = null;
    progressionTelechargementModeleLocal = { recus: 0, total: etatModeleLocal.tailleOctets };
    telechargementModeleLocalEnCours = true;
    try {
      const e = await appeler<EtatModeleLocal>("telecharger_modele_local");
      etatModeleLocal = e;
    } catch (err) {
      erreur = String(err);
    } finally {
      telechargementModeleLocalEnCours = false;
      progressionTelechargementModeleLocal = null;
    }
  }

  function annulerTelechargementModeleLocal() {
    void appeler<void>("demander_annulation").catch(() => {});
  }

  async function supprimerModeleLocal() {
    if (!invokeTauri) return;
    try {
      const e = await appeler<EtatModeleLocal>("supprimer_modele_local");
      etatModeleLocal = e;
      if (fournisseurIa === "local") {
        fournisseurIa = "lm_studio";
        ongletParametresIa = "lm_studio";
      }
    } catch (err) {
      erreur = String(err);
    }
  }

  function formaterTailleOctets(octets: number | null | undefined): string {
    if (octets == null || !Number.isFinite(octets) || octets < 0) return "—";
    const unites = ["o", "ko", "Mo", "Go", "To"];
    let val = octets;
    let i = 0;
    while (val >= 1024 && i < unites.length - 1) {
      val /= 1024;
      i += 1;
    }
    const arrondi = i === 0 ? Math.round(val) : Math.round(val * 10) / 10;
    return `${arrondi} ${unites[i]}`;
  }

  async function selectionnerVideo(v: VideoInfo, onglet: OngletVideo) {
    videoSelectionneeId = v.id;
    ongletPourVideoSelectionnee = onglet;
    await tick();
    void ouvrirVideoEtChargerResume(v);
  }

  async function ouvrirVideoEtChargerResume(v: VideoInfo) {
    if (!invokeTauri) return;
    try {
      const tous = await appeler<{
        carteMentale: string | null;
      }>("lire_tous_resumes_enregistres", { videoId: v.id, channelUrl: urlChainePourIpc() });
      if (tous.carteMentale != null) {
        const cle = cleCacheCartePourVideo(v.id);
        cacheCarteMentaleParVideo = { ...cacheCarteMentaleParVideo, [cle]: tous.carteMentale };
        await tick();
        remonterAscenseurCarteMentale();
      }
    } catch (e) {
      ajouterLog(`Impossible de lire la carte mentale (${v.id}): ${String(e)}`);
    }
  }

  /** Après changement de contexte chaîne / « direct », recharge la carte pour l’id encore sélectionné. */
  async function rechargerCarteVideoSelectionneeSiDisponible() {
    if (!invokeTauri) return;
    const id = videoSelectionneeId;
    if (!id) return;
    const v = videos.find((x) => x.id === id);
    if (v) await ouvrirVideoEtChargerResume(v);
  }

  // (lecture intégrée gérée directement par l’iframe)

  async function rafraichirModelesLmStudio() {
    if (!invokeTauri) return;
    chargementModeles = true;
    try {
      modelesLmStudioDisponibles = await appeler<string[]>("lister_modeles_lm_studio", {
        lm: {
          fournisseur: "lm_studio",
          temperature: lmTemperature,
          baseUrl: URL_BASE_LM_STUDIO_DEFAUT,
          cleLmStudio: null,
        },
      });
      if (modelesLmStudioDisponibles.includes(modeleParDefautLmStudio)) {
        modeleLmStudio = modeleParDefautLmStudio;
      } else if (!modeleLmStudio.trim() && modelesLmStudioDisponibles.length > 0) {
        modeleLmStudio = modelesLmStudioDisponibles[0];
      } else if (
        modeleLmStudio &&
        !modelesLmStudioDisponibles.includes(modeleLmStudio) &&
        modelesLmStudioDisponibles.length > 0
      ) {
        modeleLmStudio = modelesLmStudioDisponibles[0];
      }
    } catch (e) {
      ajouterLog(`Impossible de lister les modèles LM Studio: ${String(e)}`);
    } finally {
      chargementModeles = false;
    }
  }

  async function rafraichirModelesOpenrouterGratuits() {
    if (!invokeTauri) return;
    chargementModeles = true;
    try {
      modelesOpenrouterGratuits = await appeler<string[]>("lister_modeles_openrouter_gratuits");
      if (!modeleOpenrouter.trim() && modelesOpenrouterGratuits.length > 0) {
        modeleOpenrouter = modelesOpenrouterGratuits[0];
      } else if (
        modeleOpenrouter &&
        !modelesOpenrouterGratuits.includes(modeleOpenrouter) &&
        modelesOpenrouterGratuits.length > 0
      ) {
        modeleOpenrouter = modelesOpenrouterGratuits[0];
      }
    } catch (e) {
      ajouterLog(`Impossible de lister les modèles OpenRouter (gratuits): ${String(e)}`);
    } finally {
      chargementModeles = false;
    }
  }

  async function verifierCarteMentaleExistantePourVideo(v: VideoInfo) {
    if (!invokeTauri) return;
    try {
      const ok = await appeler<boolean>("resume_enregistre_existe", {
        videoId: v.id,
        format: "carte_mentale",
        channelUrl: urlChainePourIpc(),
      });
      const cle = cleCacheCartePourVideo(v.id);
      carteMentaleExisteParVideoId = { ...carteMentaleExisteParVideoId, [cle]: ok };
    } catch {
      const cle = cleCacheCartePourVideo(v.id);
      carteMentaleExisteParVideoId = { ...carteMentaleExisteParVideoId, [cle]: false };
    }
  }

  async function verifierResumesExistantsPourListe() {
    if (!invokeTauri) return;
    for (const v of videos) {
      // Best effort, sans bloquer l’UI
      void verifierCarteMentaleExistantePourVideo(v);
    }
  }

  async function resumerVideo(v: VideoInfo) {
    erreur = null;
    chargement = true;
    idVideoEnTraitement = v.id;
    effacerLogs();
    try {
      // Ne relance pas si déjà enregistré.
      const cleCarte = cleCacheCartePourVideo(v.id);
      if (carteMentaleExisteParVideoId[cleCarte]) {
        ajouterLog("Carte mentale déjà enregistrée : chargement depuis le disque.");
        videoSelectionneeId = v.id;
        ongletPourVideoSelectionnee = "carte_mentale";
        await tick();
        await ouvrirVideoEtChargerResume(v);
        return;
      }

      ajouterLog("[TRANSCRIPT] Début de l'étape transcript (cache disque puis extraction si nécessaire).");
      const texteCarte = await appeler<string>("resumer_video_direct", {
        videoUrl: v.url,
        format: formatSortie,
        channelUrl: urlChainePourIpc(),
        lm: lmPayload(),
      });
      cacheCarteMentaleParVideo = { ...cacheCarteMentaleParVideo, [cleCarte]: texteCarte };
      carteMentaleExisteParVideoId = { ...carteMentaleExisteParVideoId, [cleCarte]: true };
      // Après création, afficher immédiatement la carte mentale produite.
      videoSelectionneeId = v.id;
      ongletPourVideoSelectionnee = "carte_mentale";
      marquerResumeCommeFait(v.id);
      ajouterLog(`Résumé généré : ${v.titre}`);
    } catch (e) {
      erreur = String(e);
    } finally {
      idVideoEnTraitement = null;
      chargement = false;
    }
  }

  async function resumerTexteLibreEntree(p: TexteLibre) {
    erreur = null;
    if (!invokeTauri) {
      erreur =
        "Backend Tauri indisponible. Lance l’application via la fenêtre Tauri pour générer un résumé.";
      return;
    }
    if (!p.contenu.trim()) {
      erreur = "Texte vide.";
      return;
    }
    chargement = true;
    idTexteEnResume = p.id;
    effacerLogs();
    try {
      ajouterLog("[TRANSCRIPT] Texte utilisateur prêt pour le résumé.");
      ajouterLog("[MODELE] Début de l'appel du modèle IA.");
      const texteCarte = await appeler<string>("resumer_texte_libre", {
        idTexte: p.id,
        texte: p.contenu,
        format: formatSortie,
        lm: lmPayload(),
      });
      mettreAJourResumeTexteLibre(p.id, texteCarte);
      await tick();
      if (conteneurCarteMentaleTexteLibre) {
        conteneurCarteMentaleTexteLibre.scrollTop = 0;
      }
      ajouterLog(`Résumé généré pour : ${p.titre}`);
    } catch (e) {
      erreur = String(e);
    } finally {
      idTexteEnResume = null;
      chargement = false;
    }
  }

  async function supprimerVideo(v: VideoInfo) {
    if (!invokeTauri) return;

    erreur = null;
    chargement = true;
    effacerLogs();
    try {
      const compte = await appeler<number>("supprimer_video", {
        videoId: v.id,
        channelUrl: urlChainePourIpc(),
      });

      const cleCache = urlChaineActive.trim();
      videos = videos.filter((x) => x.id !== v.id);
      enregistrerVideosDansCache(cleCache, videos);

      // Mettre à jour l'état UI (carte mentale affichée et disponibilité).
      const cleSuppr = cleCacheCartePourVideo(v.id);
      const { [cleSuppr]: _, ...reste } = cacheCarteMentaleParVideo;
      cacheCarteMentaleParVideo = reste;
      carteMentaleExisteParVideoId = {
        ...carteMentaleExisteParVideoId,
        [cleSuppr]: false,
      };

      // Mettre à jour la persistance locale "déjà résumée".
      idsResumesFaits = new Set([...idsResumesFaits].filter((id) => id !== v.id));
      ecrireIdsResumesVersStockage(idsResumesFaits);

      if (videoSelectionneeId === v.id) {
        videoSelectionneeId = null;
        ongletPourVideoSelectionnee = "video";
      }

      ajouterLog(
        compte > 0
          ? `Suppression locale OK (${compte} élément(s)) : ${v.titre}`
          : `Rien à supprimer : ${v.titre}`,
      );
    } catch (e) {
      erreur = String(e);
    } finally {
      chargement = false;
    }
  }

  async function afficherResumePourVideo(v: VideoInfo) {
    videoSelectionneeId = v.id;
    ongletPourVideoSelectionnee = "carte_mentale";
    await tick();
    await ouvrirVideoEtChargerResume(v);
  }

  /** Texte carte pour la vidéo sélectionnée (dérivé réactif pour réactualiser le panneau après chargement du cache). */
  $: texteCarteMentaleSelection =
    videoSelectionneeId != null && videoSelectionneeId !== ""
      ? cacheCarteMentaleParVideo[cleCacheCartePourVideo(videoSelectionneeId)] ?? ""
      : "";

  /** Zone scrollable du Markdown (carte mentale) dans le panneau de droite. */
  let conteneurCarteMentale: HTMLDivElement | null = null;
  let conteneurCarteMentaleTexteLibre: HTMLDivElement | null = null;

  function remonterAscenseurCarteMentale() {
    const el = conteneurCarteMentale;
    if (!el) return;
    el.scrollTop = 0;
    requestAnimationFrame(() => {
      el.scrollTop = 0;
    });
  }

  /** Dès qu’on affiche la carte mentale ou que son texte change, remonter le défilement. */
  $: if (
    vue === "videos" &&
    ongletPourVideoSelectionnee === "carte_mentale" &&
    videoSelectionneeId
  ) {
    void texteCarteMentaleSelection.length;
    void tick().then(() => remonterAscenseurCarteMentale());
  }

  /**
   * Si le modèle enveloppe la carte dans ```markdown … ```, marked sinon voit un seul bloc de code (# visibles).
   * On enlève uniquement cette clôture **externe**. `lastIndexOf(\`\`\`)` est faux dès qu’un bloc interne existe
   * (ex. ```mermaid) : il faut apparier les fences avec une profondeur.
   */
  function ligneEstClotureBlocsCode(ligne: string, longueurMin: number): boolean {
    const t = ligne.trim();
    const m = t.match(/^(`+)(\s*)$/);
    return m !== null && m[1].length >= longueurMin;
  }

  function ligneEstOuvertureSousBlocs(ligne: string): boolean {
    const t = ligne.trim();
    if (/^`+\s*$/.test(t)) {
      return false;
    }
    return /^(`{3,})([^`].*)$/.test(t);
  }

  function retirerFenceMarkdownExterieure(source: string): string {
    const lignes = source.replace(/\r\n/g, "\n").replace(/^\uFEFF/, "").split("\n");
    if (lignes.length < 2) {
      return source;
    }
    const premiere = lignes[0].trim();
    const matchOuverture = premiere.match(/^(`{3,})(.*)$/);
    if (!matchOuverture) {
      return source;
    }
    const longueurFence = matchOuverture[1].length;
    const infoLangue = (matchOuverture[2] ?? "").trim();
    const premierMot = infoLangue.split(/\s/)[0]?.toLowerCase() ?? "";
    if (premierMot !== "" && premierMot !== "markdown" && premierMot !== "md") {
      return source;
    }

    let profondeur = 1;
    for (let i = 1; i < lignes.length; i++) {
      if (ligneEstClotureBlocsCode(lignes[i], longueurFence)) {
        profondeur -= 1;
        if (profondeur === 0) {
          return lignes.slice(1, i).join("\n");
        }
      } else if (ligneEstOuvertureSousBlocs(lignes[i])) {
        profondeur += 1;
      }
    }
    return source;
  }

  /**
   * Beaucoup de modèles ajoutent un **paragraphe d’intro**, puis un bloc ` ```markdown ` … ` ``` `.
   * Sans ce passage, marked ne voit qu’un seul bloc de code : tout le reste (titres, mermaid) reste brut.
   * On déballle **chaque** bloc `markdown` / `md` (indentation ≤ 3 espaces), avec la même logique de profondeur
   * que pour les blocs internes ` ```mermaid `.
   */
  function deballerBlocsEmballageMarkdown(source: string): string {
    const lignes = source.replace(/\r\n/g, "\n").replace(/^\uFEFF/, "").split("\n");
    const sortie: string[] = [];
    let i = 0;
    while (i < lignes.length) {
      const brute = lignes[i];
      const sansIndent = brute.replace(/^[\t ]{0,3}/, "");
      const matchOuverture = sansIndent.match(/^(`{3,})(.*)$/);
      if (!matchOuverture) {
        sortie.push(brute);
        i += 1;
        continue;
      }
      const longueurFence = matchOuverture[1].length;
      const infoLangue = (matchOuverture[2] ?? "").trim();
      const premierMot = infoLangue.split(/\s/)[0]?.toLowerCase() ?? "";
      if (premierMot !== "markdown" && premierMot !== "md") {
        sortie.push(brute);
        i += 1;
        continue;
      }

      const lignesInternes: string[] = [];
      let profondeur = 1;
      let j = i + 1;
      while (j < lignes.length && profondeur > 0) {
        if (ligneEstClotureBlocsCode(lignes[j], longueurFence)) {
          profondeur -= 1;
          if (profondeur === 0) {
            sortie.push(...lignesInternes);
            i = j + 1;
            break;
          }
          lignesInternes.push(lignes[j]);
        } else if (ligneEstOuvertureSousBlocs(lignes[j])) {
          profondeur += 1;
          lignesInternes.push(lignes[j]);
        } else {
          lignesInternes.push(lignes[j]);
        }
        j += 1;
      }
      if (profondeur === 0) {
        continue;
      }
      sortie.push(brute);
      i += 1;
    }
    return sortie.join("\n");
  }

  /**
   * Titres ATX : CommonMark exige un espace après les # (`## Titre`). Sinon le rendu reste du texte brut et les # s’affichent.
   * Gère aussi `> ##Titre` (citation).
   */
  function normaliserTitresAtxMarkdown(texte: string): string {
    return texte.replace(
      /^(\s{0,3})(>[\t ]*)?(#{1,6})([^\s#\n])/gm,
      (_, indent, cite, hashes, rest) => `${indent}${cite ?? ""}${hashes} ${rest}`,
    );
  }

  /**
   * Rend les commandes LaTeX de flèche (`\to`, `\rightarrow`, `\leftarrow`, `\uparrow`, `\downarrow`)
   * en symboles visuels, uniquement hors blocs de code Markdown pour préserver les exemples techniques.
   */
  function normaliserFlechesLatexMarkdown(texte: string): string {
    const lignes = texte.split("\n");
    let delimiteurBlocCode: string | null = null;
    const sortie = lignes.map((ligne) => {
      const matchFence = ligne.match(/^\s*(`{3,}|~{3,})/);
      if (matchFence) {
        const delimiteurCourant = matchFence[1][0];
        if (delimiteurBlocCode === null) {
          delimiteurBlocCode = delimiteurCourant;
        } else if (delimiteurBlocCode === delimiteurCourant) {
          delimiteurBlocCode = null;
        }
        return ligne;
      }
      if (delimiteurBlocCode !== null) {
        return ligne;
      }
      return ligne
        .replace(/\\(?:to|rightarrow)\b/g, "→")
        .replace(/\\leftarrow\b/g, "←")
        .replace(/\\uparrow\b/g, "↑")
        .replace(/\\downarrow\b/g, "↓")
        .replace(/\$\s*→\s*\$/g, "→")
        .replace(/\$\s*←\s*\$/g, "←")
        .replace(/\$\s*↑\s*\$/g, "↑")
        .replace(/\$\s*↓\s*\$/g, "↓");
    });
    return sortie.join("\n");
  }

  /** Normalise puces « • » et tirets typographiques pour des listes Markdown reconnues par marked. */
  function preparerMarkdownAvantRendu(source: string): string {
    let t = (source ?? "").replace(/\r\n/g, "\n").replace(/^\uFEFF/, "");
    t = deballerBlocsEmballageMarkdown(t);
    t = retirerFenceMarkdownExterieure(t);
    t = normaliserTitresAtxMarkdown(t);
    t = normaliserFlechesLatexMarkdown(t);
    const lignes = t.split("\n");
    const sortie = lignes.map((ligne) => {
      const debut = ligne.match(/^(\s*)/)?.[1] ?? "";
      const reste = ligne.slice(debut.length);
      if (reste.startsWith("•") || reste.startsWith("▪") || reste.startsWith("‣")) {
        const apres = reste.slice(1).trimStart();
        return `${debut}- ${apres}`;
      }
      if (reste.startsWith("– ") || reste.startsWith("— ")) {
        return `${debut}- ${reste.slice(2).trimStart()}`;
      }
      return ligne;
    });
    return sortie.join("\n");
  }

  function htmlPourMarkdown(texte: string): string {
    const prepare = preparerMarkdownAvantRendu(texte ?? "");
    const brut = marked.parse(prepare, { async: false }) as string;
    const htmlSanitise = DOMPurify.sanitize(brut);
    return rendreLatexAvecKatexSiNecessaire(htmlSanitise, prepare);
  }

  function markdownContientLatex(texte: string): boolean {
    return (
      /\$\$[\s\S]+?\$\$/.test(texte) ||
      /\$[^$\n]+\$/.test(texte) ||
      /\\\([\s\S]+?\\\)/.test(texte) ||
      /\\\[[\s\S]+?\\\]/.test(texte)
    );
  }

  function rendreLatexAvecKatexSiNecessaire(html: string, markdownSource: string): string {
    if (typeof document === "undefined" || !markdownContientLatex(markdownSource)) {
      return html;
    }
    const conteneur = document.createElement("div");
    conteneur.innerHTML = html;
    try {
      renderMathInElement(conteneur, {
        delimiters: [
          { left: "$$", right: "$$", display: true },
          { left: "\\[", right: "\\]", display: true },
          { left: "\\(", right: "\\)", display: false },
          { left: "$", right: "$", display: false },
        ],
        throwOnError: false,
      });
      return conteneur.innerHTML;
    } catch {
      return html;
    }
  }

  async function rafraichirDiagrammesMermaid() {
    await tick();
    try {
      await mermaid.run({ querySelector: ".diagramme-markdown", suppressErrors: true });
    } catch {
      /* diagramme invalide ou thème indisponible */
    }
  }

  $: if (vue === "videos") {
    void texteCarteMentaleSelection;
    void urlChaineActive;
    void modeContenuPrincipal;
    void idTexteLibreEnCours;
    void ongletPourVideoSelectionnee;
    void videoSelectionneeId;
    void listeTextesLibres;
    if (idTexteLibreEnCours) {
      const tlCourant = listeTextesLibres.find((p) => p.id === idTexteLibreEnCours);
      void tlCourant?.resumeCarteMentale;
      void tlCourant?.contenu;
    }
    void tick().then(() => rafraichirDiagrammesMermaid());
  }

  async function extraireTranscriptsChaine() {
    erreur = null;
    if (!urlChaineActive.trim() || estModeVideoDirecte) {
      erreur = "Sélectionne une chaîne YouTube (pas le mode « Vidéos seules »).";
      return;
    }
    chargement = true;
    effacerLogs();
    await appeler<void>("reinitialiser_annulation");
    try {
      await appeler<unknown[]>("extraire_transcripts_chaine", {
        channelUrl: urlChaineActive.trim(),
        nbVideos,
        dossierSortie,
        maxCaracteres: 25000,
      });
      ajouterLog("Extraction chaîne terminée.");
    } catch (e) {
      erreur = String(e);
    } finally {
      chargement = false;
    }
  }

  async function resumerChaine() {
    erreur = null;
    if (!urlChaineActive.trim() || estModeVideoDirecte) {
      erreur = "Sélectionne une chaîne YouTube (pas le mode « Vidéos seules »).";
      return;
    }
    chargement = true;
    effacerLogs();
    await appeler<void>("reinitialiser_annulation");
    try {
      await appeler<string[]>("resumer_chaine", {
        channelUrl: urlChaineActive.trim(),
        nbVideos,
        format: formatSortie,
        lm: lmPayload(),
        dossierSortie,
      });
      for (const v of videos) {
        marquerResumeCommeFait(v.id);
      }
      ajouterLog("Résumé chaîne terminé.");
    } catch (e) {
      erreur = String(e);
    } finally {
      chargement = false;
    }
  }

  async function choisirDossier() {
    erreur = null;
    try {
      dossierSortie = await appeler<string | null>("selectionner_dossier_sortie");
    } catch (e) {
      erreur = String(e);
    }
  }

  async function annuler() {
    await appeler<void>("demander_annulation");
    ajouterLog("Annulation demandée.");
  }

  function fermerElementAvecEchap(): boolean {
    if (panneauAjoutTexteLibreOuvert) {
      panneauAjoutTexteLibreOuvert = false;
      return true;
    }
    if (panneauAjoutVideoSeuleOuvert) {
      panneauAjoutVideoSeuleOuvert = false;
      return true;
    }
    if (panneauAjoutOuvert) {
      panneauAjoutOuvert = false;
      return true;
    }
    if (vue === "parametres") {
      vue = "videos";
      return true;
    }
    return false;
  }

  function gererRaccourciEchap(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    const aFerme = fermerElementAvecEchap();
    if (aFerme) {
      event.preventDefault();
      event.stopPropagation();
    }
  }

  onMount(async () => {
    window.addEventListener("keydown", gererRaccourciEchap);
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "loose",
      theme: "dark",
    });
    try {
      idsResumesFaits = lireIdsResumesDepuisStockage();
      chargerTemperatureDepuisStockage();
      chargerNbVideosDepuisStockage();
      chargerListeTextesLibresDepuisStockage();

      // Tauri 2 : `isTauri()` peut être faux alors que `invoke` est disponible (présence de
      // `window.__TAURI_INTERNALS__`). Sans `invoke`, les listes de modèles restent vides.
      try {
        const core = await import("@tauri-apps/api/core");
        const internesOk =
          typeof window !== "undefined" &&
          Boolean((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);
        const estTauri =
          typeof (core as any).isTauri === "function" && (core as any).isTauri();
        if (estTauri || internesOk) {
          invokeTauri = core.invoke as unknown as FonctionInvoke;
        }
      } catch {
        invokeTauri = null;
      }

      await chargerChaines();
      await chargerDernieresVideosAuDemarrage();
      await rafraichirModelesLmStudio();
      await rafraichirModelesOpenrouterGratuits();
      await rafraichirEtatModeleLocal();
      await hydraterCartesTextesLibresDepuisDisque();
      await synchroniserTextesLibresVersDisque();

      // Écoute des événements uniquement si l’API Tauri est disponible.
      if (invokeTauri) {
        try {
          const events = await import("@tauri-apps/api/event");
          listenTauri = events.listen as unknown as FonctionListen;

          const unlistenLog = await listenTauri<string>("job-log", (e) => {
            ajouterLog(e.payload);
          });
          const unlistenProg = await listenTauri<Progression>("job-progression", (e) => {
            progression = e.payload;
          });
          const unlistenModeleLocal = await listenTauri<{ recus: number; total: number | null }>(
            "modele-local-progression",
            (e) => {
              progressionTelechargementModeleLocal = {
                recus: Number(e.payload?.recus ?? 0),
                total:
                  e.payload?.total != null && Number.isFinite(Number(e.payload.total))
                    ? Number(e.payload.total)
                    : null,
              };
            },
          );

          return () => {
            window.removeEventListener("keydown", gererRaccourciEchap);
            unlistenLog();
            unlistenProg();
            unlistenModeleLocal();
          };
        } catch {
          listenTauri = null;
        }
      }
    } catch (e) {
      erreur = String(e);
    }
    return () => {
      window.removeEventListener("keydown", gererRaccourciEchap);
    };
  });
</script>

<div class="app-shell">
  <aside class="sidebar">
    <div class="entete-sidebar">
      <div class="groupe-titre-app">
        <button
          type="button"
          class="btn-icone-parametres"
          class:actif={vue === "parametres"}
          on:click={() => (vue = vue === "parametres" ? "videos" : "parametres")}
          title="Paramètres (IA, liste vidéos)"
          aria-label="Paramètres (IA, liste vidéos)"
        >
          <svg
            class="icone-engrenage"
            xmlns="http://www.w3.org/2000/svg"
            width="22"
            height="22"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.75"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <path
              d="M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z"
            />
            <path
              d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.6a1.65 1.65 0 0 0 1-1.51V2a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"
            />
          </svg>
        </button>
        <p class="brand">Resume YouTube</p>
      </div>
      <button
        type="button"
        class="btn secondaire btn-logs-entete"
        on:click={() => (logsOuverts = true)}
      >
        Logs
      </button>
    </div>

    {#if panneauAjoutOuvert}
      <form
        class="panneau-ajout"
        in:fly={transitionMonteeDepuisBouton}
        out:fly={transitionMonteeDepuisBouton}
        on:submit|preventDefault={() => void ajouterChaine()}
      >
        <!-- Une seule saisie : l’URL (le nom affiché vient du handle après @ côté backend). -->
        <div class="champ">
          <label for="url-chaine">URL de la chaîne</label>
          <input
            id="url-chaine"
            type="url"
            bind:value={urlNouvelleChaine}
            placeholder="https://www.youtube.com/@..."
            autocomplete="off"
          />
          <p class="petit-texte">Le nom affiché est dérivé du handle après @.</p>
        </div>
        <button
          type="submit"
          class="btn btn-plein"
          disabled={estOperationGlobaleBloquante}
        >
          Enregistrer dans chaines.json
        </button>
      </form>
    {/if}
    <button
      type="button"
      class="btn btn-plein btn-ajout"
      on:click={() => void basculerPanneauAjoutChaine()}
    >
      {panneauAjoutOuvert ? "Fermer l’ajout" : "+ Ajouter une chaîne"}
    </button>

    <p class="etiquette-liste">Chaînes enregistrées</p>
    <div class="liste-chaines">
      {#if chaines.length === 0}
        <p class="petit-texte">Aucune chaîne pour l’instant. Utilise le bouton ci-dessus pour en ajouter une.</p>
      {:else}
        {#each chaines as c, indexChaine (`${c.url_chaine}#${indexChaine}`)}
          <div class="ligne-chaine">
            <button
              type="button"
              class="chaine-item bouton-titre-chaine"
              class:actif={c.url_chaine === urlChaineActive}
              on:click={() => selectionnerChaine(c)}
              title={c.url_chaine}
              aria-label={`${c.nom} — ${c.url_chaine}`}
            >
              <span class="chaine-nom">{c.nom}</span>
            </button>
            <button
              type="button"
              class="btn-poubelle btn-poubelle-chaine"
              disabled={estOperationGlobaleBloquante}
              aria-label={`Supprimer la chaîne: ${c.nom}`}
              on:click|stopPropagation={() => void supprimerChaine(c)}
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.75"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
              >
                <path d="M3 6h18" />
                <path d="M8 6V4h8v2" />
                <path d="M19 6l-1 14H6L5 6" />
                <path d="M10 11v6" />
                <path d="M14 11v6" />
              </svg>
            </button>
          </div>
        {/each}
      {/if}
    </div>

    <p class="etiquette-liste etiquette-video-seule">Vidéo hors chaîne</p>
    <div class="zone-video-seule">
      <button
        type="button"
        class="chaine-item"
        class:actif={estModeVideoDirecte}
        on:click={() => selectionnerModeVideosSeules()}
        title="Données sous donnees/chaines/direct/videos/"
        aria-label="Vidéos seules (hors chaîne)"
      >
        <span class="chaine-nom">Vidéos seules</span>
      </button>
      <button
        type="button"
        class="btn secondaire btn-plein btn-video-seule"
        on:click={() => void basculerPanneauAjoutVideoSeule()}
      >
        {panneauAjoutVideoSeuleOuvert ? "Fermer" : "+ Ajouter une vidéo (hors chaîne)"}
      </button>
      {#if panneauAjoutVideoSeuleOuvert}
        <form
          class="panneau-ajout"
          in:fly={transitionDescenteDepuisBouton}
          out:fly={transitionDescenteDepuisBouton}
          on:submit|preventDefault={() => void ajouterVideoSeule()}
        >
          <div class="champ">
            <label for="url-video-seule">URL de la vidéo YouTube</label>
            <input
              id="url-video-seule"
              type="url"
              bind:value={urlVideoSeule}
              placeholder="https://www.youtube.com/watch?v=…"
              autocomplete="off"
            />
            <p class="petit-texte">Stockage local : dossier « direct », sans rattachement à une chaîne.</p>
          </div>
          <button
            type="submit"
            class="btn btn-plein"
            disabled={estOperationGlobaleBloquante}
          >
            Ajouter la vidéo
          </button>
        </form>
      {/if}
    </div>

    <p class="etiquette-liste etiquette-pages-web">Textes à résumer</p>
    <div class="zone-pages-web">
      {#if panneauAjoutTexteLibreOuvert}
        <form
          class="panneau-ajout"
          in:fly={transitionMonteeDepuisBouton}
          out:fly={transitionMonteeDepuisBouton}
          on:submit|preventDefault={() => void ajouterTexteLibre()}
        >
          <div class="champ">
            <label for="saisie-texte-libre">Contenu à résumer</label>
            <textarea
              id="saisie-texte-libre"
              class="textarea-texte-libre"
              bind:value={saisieTexteLibre}
              placeholder="Colle ici l’article, le courriel ou l’extrait à transformer en carte mentale…"
              rows="8"
              autocomplete="off"
              on:keydown={(event) => void gererValidationAjoutTexteAvecEntree(event)}
            ></textarea>
            <p class="petit-texte">
              Le texte est stocké localement. À la validation, un résumé (carte mentale) est généré automatiquement via LM
              Studio (paramètres : icône engrenage).
            </p>
          </div>
          <button
            type="submit"
            class="btn btn-plein"
            disabled={estOperationGlobaleBloquante}
          >
            Enregistrer le texte
          </button>
        </form>
      {/if}
      <button
        type="button"
        class="btn secondaire btn-plein"
        on:click={() => void basculerPanneauAjoutTexteLibre()}
      >
        {panneauAjoutTexteLibreOuvert ? "Fermer" : "+ Ajouter un texte"}
      </button>
      {#if listeTextesLibres.length > 0}
        <div class="liste-pages-web">
          {#each listeTextesLibres as p (p.id)}
            <div
              class="ligne-page-web"
              class:active={modeContenuPrincipal === "texte_libre" && idTexteLibreEnCours === p.id}
            >
              <button
                type="button"
                class="chaine-item bouton-titre-page-web"
                on:click={() => selectionnerTexteLibre(p)}
                title={p.contenu.slice(0, 200)}
                aria-label={`Ouvrir le texte: ${p.titre}`}
              >
                <span class="chaine-nom">{p.titre}</span>
              </button>
              <button
                type="button"
                class="btn-poubelle btn-poubelle-page-web"
                aria-label={`Supprimer le texte: ${p.titre}`}
                on:click|stopPropagation={() => void supprimerTexteLibre(p)}
              >
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.75"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  aria-hidden="true"
                >
                  <path d="M3 6h18" />
                  <path d="M8 6V4h8v2" />
                  <path d="M19 6l-1 14H6L5 6" />
                  <path d="M10 11v6" />
                  <path d="M14 11v6" />
                </svg>
              </button>
            </div>
          {/each}
        </div>
      {/if}
    </div>

  </aside>

  <main class="contenu" class:mode-videos={vue === "videos"}>
    {#if erreur}
      <div class="alerte">{erreur}</div>
    {/if}

    {#if vue === "videos"}
      <div class="vue-videos">
        <div>
          <div class="entete-chaine">
            <div class="titre-et-actualisation-chaine">
              <h2>
                {#if modeContenuPrincipal === "texte_libre"}
                  {listeTextesLibres.find((p) => p.id === idTexteLibreEnCours)?.titre ?? "Texte libre"}
                {:else}
                  {nomChaineCourante || "—"}
                {/if}
              </h2>
              {#if urlChaineActive && !estModeVideoDirecte && modeContenuPrincipal === "youtube"}
                <button
                  class="btn-icone-maj"
                  type="button"
                  disabled={estMiseAJourEnCours || estOperationGlobaleBloquante}
                  on:click={verifierNouvellesVideos}
                  title="Mettre à jour les vidéos de la chaîne"
                  aria-label="Mettre à jour les vidéos de la chaîne"
                >
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="18"
                    height="18"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.9"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                  >
                    <path d="M3 2v6h6" />
                    <path d="M21 22v-6h-6" />
                    <path d="M21 12a9 9 0 0 0-15.6-6.36L3 8" />
                    <path d="M3 12a9 9 0 0 0 15.6 6.36L21 16" />
                  </svg>
                </button>
              {/if}
            </div>
          </div>
        </div>

        {#if modeContenuPrincipal === "texte_libre"}
          <div class="page-web-vue-principale">
            <div class="carte panneau-page-web-plein panneau-texte-libre-detail">
              {#if idTexteLibreEnCours}
                {@const tl = listeTextesLibres.find((p) => p.id === idTexteLibreEnCours)}
                {#if tl}
                  <div class="barre-actions-texte-libre">
                    <button
                      type="button"
                      class="btn secondaire"
                      disabled={estResumeTexteEnCours || estOperationGlobaleBloquante}
                      on:click={() => resumerTexteLibreEntree(tl)}
                    >
                      Régénérer la carte mentale
                    </button>
                    <button
                      type="button"
                      class="btn danger"
                      disabled={estOperationGlobaleBloquante}
                      on:click={() => void supprimerTexteLibre(tl)}
                    >
                      Supprimer ce texte
                    </button>
                    <p class="petit-texte barre-actions-texte-libre-aide">
                      À l’enregistrement, le résumé est lancé automatiquement (IA, voir paramètres). Tu peux
                      régénérer ici si besoin. Le texte source et la carte s’affichent ci-dessous.
                    </p>
                  </div>
                  <div class="grille-texte-libre-deux-colonnes">
                    <section class="colonne-texte-libre colonne-texte-source" aria-labelledby="titre-source-texte">
                      <h3 id="titre-source-texte">Texte source</h3>
                      <div class="bloc-texte-source scrollable-markdown">
                        <div class="markdown-onglet-resume markdown-texte-source">
                          {@html htmlPourMarkdown(tl.contenu)}
                        </div>
                      </div>
                    </section>
                    <section class="colonne-texte-libre colonne-texte-resume" aria-labelledby="titre-resume-texte">
                      <h3 id="titre-resume-texte">Carte mentale</h3>
                      {#if (tl.resumeCarteMentale ?? "").trim().length > 0}
                        <div
                          class="markdown-onglet-resume scrollable-markdown"
                          bind:this={conteneurCarteMentaleTexteLibre}
                        >
                          {@html htmlPourMarkdown(
                            supprimerRaisonnementPourAffichage(tl.resumeCarteMentale ?? ""),
                          )}
                        </div>
                      {:else}
                        <div class="message-sans-lecteur message-resume-vide">
                          Aucune carte mentale pour l’instant (échec ou backend indisponible). Utilise « Régénérer la
                          carte mentale » ou ré-enregistre le texte.
                        </div>
                      {/if}
                    </section>
                  </div>
                {:else}
                  <div class="message-sans-lecteur">Texte introuvable.</div>
                {/if}
              {:else}
                <div class="message-sans-lecteur">Sélectionne un texte dans la colonne de gauche.</div>
              {/if}
            </div>
          </div>
        {:else if videos.length > 0}
          <div class="page-videos">
          <div class="carte panneau-liste-videos">
            <h3>Vidéos</h3>
            <div class="liste-videos-colonne">
              {#each videos as v}
                <div class="video-ligne" class:active={v.id === videoSelectionneeId}>
                  <button
                    type="button"
                    class="miniature-ligne"
                    on:click={() => selectionnerVideo(v, "video")}
                    aria-label={`Ouvrir la vidéo: ${v.titre}`}
                  >
                    <img
                      class="miniature"
                      src={`https://i.ytimg.com/vi/${v.id}/hqdefault.jpg`}
                      alt={v.titre}
                      loading="lazy"
                    />
                  </button>

                  <div class="video-ligne-contenu">
                    <button
                      type="button"
                      class="titre-ligne"
                      on:click={() => selectionnerVideo(v, "video")}
                      title={v.titre}
                    >
                      {v.titre}
                    </button>
                    <div class="meta meta-date-video">
                      {#if v.datePublication}
                        {#if v.datePublication.length === 8}
                          {v.datePublication.slice(6, 8)}-{v.datePublication.slice(4, 6)}-{v.datePublication.slice(0, 4)}
                        {:else}
                          {v.datePublication}
                        {/if}
                      {/if}
                    </div>
                    <div class="actions-ligne">
                      {#if !carteMentaleExisteParVideoId[cleCacheCartePourVideo(v.id)]}
                        {#if chargement && idVideoEnTraitement === v.id}
                          <button class="btn danger" type="button" on:click={annuler}>
                            Annuler
                          </button>
                        {:else}
                          <button
                            class="btn"
                            type="button"
                            disabled={estResumeVideoEnCours || estMiseAJourEnCours || estOperationGlobaleBloquante}
                            on:click={() => resumerVideo(v)}
                          >
                            Résumer
                          </button>
                        {/if}
                      {:else}
                        <button
                          class="btn secondaire"
                          type="button"
                          on:click={() => afficherResumePourVideo(v)}
                        >
                          Afficher
                        </button>
                      {/if}
                    </div>
                  </div>

                  <button
                    class="btn-poubelle"
                    type="button"
                    disabled={estOperationGlobaleBloquante || (estResumeVideoEnCours && idVideoEnTraitement === v.id)}
                    on:click={() => supprimerVideo(v)}
                    title="Supprimer (cache local : transcript + carte mentale)"
                    aria-label={`Supprimer la vidéo: ${v.titre}`}
                  >
                    <svg
                      xmlns="http://www.w3.org/2000/svg"
                      width="18"
                      height="18"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="1.75"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      aria-hidden="true"
                    >
                      <path d="M3 6h18" />
                      <path d="M8 6V4h8v2" />
                      <path d="M19 6l-1 14H6L5 6" />
                      <path d="M10 11v6" />
                      <path d="M14 11v6" />
                    </svg>
                  </button>
                </div>
              {/each}
            </div>
          </div>

          <div class="carte panneau-detail-video">
            {#if videoSelectionneeId}
              {@const v = videos.find((x) => x.id === videoSelectionneeId)}
              {#if v}
                <div class="barre-onglets-video" role="tablist" aria-label="Vidéo et carte mentale">
                  <button
                    type="button"
                    role="tab"
                    class="onglet-video"
                    class:actif={ongletPourVideoSelectionnee === "video"}
                    aria-selected={ongletPourVideoSelectionnee === "video"}
                    on:click={() => (ongletPourVideoSelectionnee = "video")}
                  >
                    Vidéo
                  </button>
                  <button
                    type="button"
                    role="tab"
                    class="onglet-video"
                    class:actif={ongletPourVideoSelectionnee === "carte_mentale"}
                    class:a-fichier={carteMentaleExisteParVideoId[cleCacheCartePourVideo(v.id)]}
                    aria-selected={ongletPourVideoSelectionnee === "carte_mentale"}
                    on:click={() => (ongletPourVideoSelectionnee = "carte_mentale")}
                  >
                    Carte mentale
                  </button>
                </div>

                <div class="panneau-onglet-video" role="tabpanel">
                  {#if ongletPourVideoSelectionnee === "video"}
                    <div class="lecteur-video">
                      <iframe
                        src={`https://www.youtube.com/embed/${v.id}?autoplay=1&rel=0`}
                        title={`Lecture: ${v.titre}`}
                        allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
                        allowfullscreen
                      ></iframe>
                    </div>
                  {:else if texteCarteMentaleSelection.length > 0}
                    <div
                      class="markdown-onglet-resume"
                      bind:this={conteneurCarteMentale}
                    >
                      {@html htmlPourMarkdown(
                        supprimerRaisonnementPourAffichage(texteCarteMentaleSelection),
                      )}
                    </div>
                  {:else}
                    <div class="message-sans-lecteur">
                      Aucune carte mentale enregistrée. Clique sur « Résumer » pour la générer.
                    </div>
                  {/if}
                </div>
              {:else}
                <div class="message-sans-lecteur">Sélectionne une vidéo dans la liste de gauche.</div>
              {/if}
            {:else}
              <div class="message-sans-lecteur">Sélectionne une vidéo dans la liste de gauche.</div>
            {/if}
          </div>
          </div>
        {:else if chargement}
          <p class="petit-texte">Chargement…</p>
        {:else if urlChaineActive && !chargement && videos.length === 0}
          <p class="petit-texte">
            {#if estModeVideoDirecte}
              Aucune vidéo seule : ajoute une URL avec le bouton « + Ajouter une vidéo (hors chaîne) » dans la colonne
              de gauche.
            {:else}
              Aucune vidéo (yt-dlp ou URL de chaîne). Utilise « Vidéos plus récentes ».
            {/if}
          </p>
        {:else if !urlChaineActive && !chargement && modeContenuPrincipal === "youtube"}
          <p class="petit-texte">Sélectionne une chaîne ou le mode « Vidéos seules » dans la colonne de gauche.</p>
        {/if}
      </div>

      <!-- Les transcripts/résumés par vidéo sont affichés directement sous le titre de chaque vidéo. -->
    {:else if vue === "parametres"}
      <h2>Paramètres</h2>
      <p class="petit-texte">Liste des vidéos, actions par lot et configuration IA (LM Studio ou OpenRouter).</p>

      <div class="carte">
        <h3>Liste des vidéos</h3>
        <p class="petit-texte">
          Nombre de vidéos à interroger sur YouTube (bouton « Vidéos plus récentes » et traitements par chaîne).
        </p>
        <div class="champ">
          <label for="nb-videos">Dernières vidéos à récupérer</label>
          <input id="nb-videos" type="number" min="1" max="50" bind:value={nbVideos} />
        </div>
        <div class="rangee-boutons">
          <button
            class="btn secondaire"
            type="button"
            disabled={estOperationGlobaleBloquante}
            on:click={choisirDossier}
          >
            Dossier de sortie
          </button>
          {#if dossierSortie}
            <span class="petit-texte">{dossierSortie}</span>
          {/if}
        </div>
      </div>

      <div class="carte carte-parametres-ia">
        <h3 class="titre-carte-param-ia">Fournisseur IA</h3>
        <p class="petit-texte">
          Choisis l’onglet pour configurer LM Studio (serveur réseau prédéfini dans l’application), OpenRouter (clé API
          + modèles gratuits) ou Local (modèle GGUF embarqué via llama.cpp).
        </p>
        <div class="barre-onglets-param-ia" role="tablist" aria-label="Fournisseur IA">
          <button
            type="button"
            role="tab"
            class="onglet-param-ia"
            class:actif={ongletParametresIa === "lm_studio"}
            aria-selected={ongletParametresIa === "lm_studio"}
            on:click={() => (ongletParametresIa = "lm_studio")}
          >
            <span class="marqueur-fournisseur" aria-hidden="true">
              <input
                type="checkbox"
                tabindex="-1"
                checked={fournisseurIa === "lm_studio"}
                on:click|stopPropagation={() => (fournisseurIa = "lm_studio")}
              />
            </span>
            <span>LM Studio</span>
          </button>
          <button
            type="button"
            role="tab"
            class="onglet-param-ia"
            class:actif={ongletParametresIa === "openrouter"}
            aria-selected={ongletParametresIa === "openrouter"}
            on:click={() => (ongletParametresIa = "openrouter")}
          >
            <span class="marqueur-fournisseur" aria-hidden="true">
              <input
                type="checkbox"
                tabindex="-1"
                checked={fournisseurIa === "openrouter"}
                on:click|stopPropagation={() => (fournisseurIa = "openrouter")}
              />
            </span>
            <span>OpenRouter</span>
          </button>
          <button
            type="button"
            role="tab"
            class="onglet-param-ia"
            class:actif={ongletParametresIa === "local"}
            aria-selected={ongletParametresIa === "local"}
            on:click={() => (ongletParametresIa = "local")}
          >
            <span class="marqueur-fournisseur" aria-hidden="true">
              <input
                type="checkbox"
                tabindex="-1"
                checked={fournisseurIa === "local"}
                disabled={!etatModeleLocal.present}
                title={etatModeleLocal.present
                  ? "Utiliser le modèle local pour les résumés"
                  : "Télécharge d'abord le modèle pour pouvoir l'activer"}
                on:click|stopPropagation={() => {
                  if (etatModeleLocal.present) fournisseurIa = "local";
                }}
              />
            </span>
            <span>Local</span>
          </button>
        </div>

        {#if ongletParametresIa === "lm_studio"}
          <div class="panneau-onglet-param-ia" role="tabpanel">
            <div class="champ">
              <label for="lm-modele">Modèle</label>
              <div class="rangee-boutons" style="justify-content: space-between;">
                <select id="lm-modele" bind:value={modeleLmStudio} style="flex:1; min-width: 0;">
                  {#if modelesLmStudioDisponibles.length === 0}
                    <option value="">(Aucun modèle détecté)</option>
                  {/if}
                  {#each modelesLmStudioDisponibles as m}
                    <option value={m}>{m}</option>
                  {/each}
                </select>
                <button
                  type="button"
                  class="btn secondaire"
                  disabled={chargementModeles}
                  on:click={rafraichirModelesLmStudio}
                >
                  {chargementModeles ? "Chargement…" : "Rafraîchir"}
                </button>
              </div>
            </div>
        </div>
        {:else if ongletParametresIa === "openrouter"}
          <div class="panneau-onglet-param-ia" role="tabpanel">
            <p class="petit-texte">
              Modèles listés : uniquement les identifiants se terminant par <code>:free</code> (API OpenRouter
              publique).
            </p>
            <div class="champ">
              <label for="cle-openrouter">Clé API OpenRouter</label>
              <input
                id="cle-openrouter"
                type="password"
                autocomplete="off"
                bind:value={cleOpenrouter}
                placeholder="sk-or-…"
              />
            </div>
            <div class="champ">
              <label for="or-modele">Modèle (gratuit)</label>
              <div class="rangee-boutons" style="justify-content: space-between;">
                <select id="or-modele" bind:value={modeleOpenrouter} style="flex:1; min-width: 0;">
                  {#if modelesOpenrouterGratuits.length === 0}
                    <option value="">(Aucun modèle gratuit listé)</option>
                  {/if}
                  {#each modelesOpenrouterGratuits as m}
                    <option value={m}>{m}</option>
                  {/each}
                </select>
                <button
                  type="button"
                  class="btn secondaire"
                  disabled={chargementModeles}
                  on:click={rafraichirModelesOpenrouterGratuits}
                >
                  {chargementModeles ? "Chargement…" : "Rafraîchir"}
                </button>
              </div>
            </div>
          </div>
        {:else}
          <div class="panneau-onglet-param-ia" role="tabpanel">
            <p class="petit-texte">
              Modèle GGUF exécuté en local (llama.cpp embarqué). Aucune requête réseau pour la génération une fois le
              fichier téléchargé.
            </p>
            {#if etatModeleLocal.present}
              <div class="champ">
                <div class="petit-texte">
                  <strong>Modèle utilisé :</strong> {etatModeleLocal.identifiant}
                </div>
                <div class="petit-texte">
                  Fichier : <code>{etatModeleLocal.chemin}</code>
                </div>
                <div class="petit-texte">
                  Taille : {formaterTailleOctets(etatModeleLocal.tailleOctets)} —
                  {etatModeleLocal.charge ? "chargé en mémoire" : "non chargé (chargement au prochain résumé)"}
                </div>
              </div>
              <div class="rangee-boutons">
                <button
                  type="button"
                  class="btn danger"
                  on:click={() => void supprimerModeleLocal()}
                >
                  Supprimer le modèle local
                </button>
              </div>
            {:else if telechargementModeleLocalEnCours}
              <div class="champ">
                <div class="petit-texte">
                  Téléchargement de {etatModeleLocal.identifiant} en cours…
                  {#if progressionTelechargementModeleLocal}
                    {formaterTailleOctets(progressionTelechargementModeleLocal.recus)} /
                    {formaterTailleOctets(progressionTelechargementModeleLocal.total)}
                  {/if}
                </div>
                <div class="barre-prog">
                  <div
                    style={progressionTelechargementModeleLocal &&
                    progressionTelechargementModeleLocal.total &&
                    progressionTelechargementModeleLocal.total > 0
                      ? `width:${Math.min(100, (progressionTelechargementModeleLocal.recus / progressionTelechargementModeleLocal.total) * 100)}%`
                      : "width:5%"}
                  ></div>
                </div>
              </div>
              <div class="rangee-boutons">
                <button
                  type="button"
                  class="btn danger"
                  on:click={annulerTelechargementModeleLocal}
                >
                  Annuler le téléchargement
                </button>
              </div>
            {:else}
              <div class="champ">
                <div class="petit-texte">
                  Modèle requis : <strong>{etatModeleLocal.identifiant}</strong> (~812 Mo, format GGUF Q8_0).
                </div>
                <div class="petit-texte">
                  Source : <code>{etatModeleLocal.urlTelechargement || "https://huggingface.co/mradermacher/MARTHA-0.8B-Qwen3.5-Omni-GGUF"}</code>
                </div>
              </div>
              <div class="rangee-boutons">
                <button
                  type="button"
                  class="btn btn-plein"
                  on:click={() => void telechargerModeleLocal()}
                >
                  Télécharger {etatModeleLocal.identifiant}
                </button>
              </div>
            {/if}
            <div class="champ">
              <label for="local-n-ctx">Fenêtre de tokens (n_ctx)</label>
              <input
                id="local-n-ctx"
                type="number"
                min="512"
                max={TAILLE_FENETRE_TOKENS_MAX}
                step="256"
                bind:value={tailleFenetreTokens}
              />
              <p class="petit-texte">
                Fenêtre de contexte allouée à chaque appel local (par défaut {TAILLE_FENETRE_TOKENS_DEFAUT}). Plus la
                valeur est grande, plus la mémoire utilisée est élevée.
              </p>
            </div>
          </div>
        {/if}

        <div class="champ">
          <label for="lm-temp">Température</label>
          <input id="lm-temp" type="number" step="0.1" bind:value={lmTemperature} />
        </div>
        <div class="champ">
          <div class="petit-texte">Format de sortie : Carte mentale (Markdown)</div>
        </div>
      </div>
  {/if}

    {#if progression}
      <div class="carte" style="margin-top:1rem;">
        <h3>Progression</h3>
        <p class="petit-texte">
          {progression.courant} / {progression.total} — {progression.titre}
        </p>
        <div class="barre-prog">
          <div
            style={`width:${Math.min(100, (progression.courant / progression.total) * 100)}%`}
          ></div>
        </div>
      </div>
    {/if}
  </main>
</div>

{#if chargement}
  <div
    class="toast-chargement"
    role="status"
    aria-busy="true"
    aria-live="polite"
    aria-labelledby="libelle-chargement-modal"
  >
    <div class="toast-chargement-carte">
      <p id="libelle-chargement-modal" class="texte-chargement-modal">
        {#if messageChargementContextuel}
          {messageChargementContextuel}
        {:else if etapeInfoTraitement === "transcript"}
          Récupération du transcript en cours.
        {:else if etapeInfoTraitement === "modele"}
          Envoi au modèle IA en cours.
        {:else}
          Traitement en cours…
        {/if}
      </p>
    </div>
  </div>
{/if}

{#if logsOuverts}
  <div class="overlay-logs" role="dialog" aria-label="Logs">
    <div class="modal-logs">
      <div class="entete-logs">
        <h3>Logs</h3>
        <div class="rangee-boutons">
          <button class="btn secondaire" type="button" on:click={effacerLogs}>Effacer</button>
          <button class="btn secondaire" type="button" on:click={() => (logsOuverts = false)}>
            Fermer
          </button>
        </div>
      </div>
      <div class="panneau-logs">{logs.join("\n") || "Aucun log pour le moment."}</div>
    </div>
  </div>
{/if}
