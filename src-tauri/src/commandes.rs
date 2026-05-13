//! Commandes IPC Tauri : pont entre l’UI Svelte et `resume_youtube_core`.

use anyhow::Context;
use futures_util::StreamExt;
use resume_youtube_core::chaines::{self, EntreeChaine};
use resume_youtube_core::config::{ConfigurationIA, FormatSortie};
use resume_youtube_core::ia;
use resume_youtube_core::ia_local::{self, ModeleLocalCharge};
use resume_youtube_core::resume;
use resume_youtube_core::youtube::{self, VideoInfo};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as MutexCacheModele};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

/// État partagé : client HTTP et drapeau d’annulation best-effort.
pub struct EtatApplication {
    pub client_http: reqwest::Client,
    pub annulation: Arc<AtomicBool>,
    /// Racine des données utilisateur (`donnees/…`) : dossier du projet en dev, `app_data_dir` en release (AppImage, .deb).
    pub racine_donnees: PathBuf,
    /// Modèle GGUF local chargé en RAM (chargement paresseux au premier appel).
    /// `std::sync::Mutex` : le cache est lu/écrit depuis le même `spawn_blocking` que llama.cpp.
    pub modele_local: Arc<MutexCacheModele<Option<Arc<ModeleLocalCharge>>>>,
    /// Verrou global : `LlamaModel` / llama.cpp ne supportent pas plusieurs inférences
    /// concurrentes sur la même instance — sans ce verrou, l’app peut planter (SIGSEGV).
    pub verrou_inference_locale: Arc<Mutex<()>>,
}

/// Identifiant et nom de fichier du modèle local supporté nativement.
pub const IDENTIFIANT_MODELE_LOCAL: &str = "MARTHA-0.8B-Qwen3.5-Omni (Q8_0)";
pub const NOM_FICHIER_MODELE_LOCAL: &str = "MARTHA-0.8B-Qwen3.5-Omni.Q8_0.gguf";
pub const URL_MODELE_LOCAL: &str =
    "https://huggingface.co/mradermacher/MARTHA-0.8B-Qwen3.5-Omni-GGUF/resolve/main/MARTHA-0.8B-Qwen3.5-Omni.Q8_0.gguf";

/// Crée la racine `donnees` et la retourne (dev : à côté du crate ; prod : sous `app_data_dir`).
pub fn initialiser_racine_donnees(app: &AppHandle) -> Result<PathBuf, String> {
    let racine = if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or_else(|| "CARGO_MANIFEST_DIR sans parent".to_string())?
            .join("donnees")
    } else {
        app.path()
            .app_data_dir()
            .map_err(|e| format!("répertoire données application: {e}"))?
            .join("donnees")
    };
    std::fs::create_dir_all(&racine).map_err(|e| format!("création dossier données: {e}"))?;
    Ok(racine)
}

fn repertoire_modeles_locaux(racine_donnees: &Path) -> PathBuf {
    racine_donnees.join("modeles_locaux")
}

fn chemin_modele_local(racine_donnees: &Path) -> PathBuf {
    repertoire_modeles_locaux(racine_donnees).join(NOM_FICHIER_MODELE_LOCAL)
}

/// Emplacement de `chaines.json` : à la racine du projet en dev (comportement historique), sous `donnees/` en release.
fn chemin_fichier_chaines_json(racine_donnees: &Path) -> PathBuf {
    if cfg!(debug_assertions) {
        racine_donnees
            .parent()
            .map(|p| p.join("chaines.json"))
            .unwrap_or_else(|| racine_donnees.join("chaines.json"))
    } else {
        racine_donnees.join("chaines.json")
    }
}

/// Emplacements possibles d’un `chaines.json` « perdu » (release : binaire lancé depuis le dépôt, ancien fichier à la racine `app_data`, etc.).
fn chemins_fallback_pour_chaines_json(racine_donnees: &Path, chemin_principal: &Path) -> Vec<PathBuf> {
    let mut vu: HashSet<PathBuf> = HashSet::new();
    let mut sortie = Vec::new();
    let mut pousser = |p: PathBuf| {
        if p == chemin_principal {
            return;
        }
        if vu.insert(p.clone()) {
            sortie.push(p);
        }
    };
    if let Some(parent) = racine_donnees.parent() {
        pousser(parent.join("chaines.json"));
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..14 {
            let Some(ref d) = dir else {
                break;
            };
            pousser(d.join("chaines.json"));
            dir = d.parent().map(Path::to_path_buf);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(p) = chaines::chercher_chaines_json_en_remontant(&cwd) {
            pousser(p);
        }
    }
    sortie
}

/// Lit `chaines.json` à l’emplacement canonique ; si vide ou absent, tente des copies ailleurs puis **migre** vers le canonique.
fn lire_chaines_avec_migration(racine_donnees: &Path) -> anyhow::Result<Vec<EntreeChaine>> {
    let principal = chemin_fichier_chaines_json(racine_donnees);
    let liste = chaines::lire_chaines(&principal)?;
    if !liste.is_empty() {
        return Ok(chaines::dedoublonner_par_url(liste));
    }
    for alt in chemins_fallback_pour_chaines_json(racine_donnees, &principal) {
        if !alt.is_file() {
            continue;
        }
        match chaines::lire_chaines(&alt).with_context(|| format!("lecture {alt:?}")) {
            Ok(autre) if !autre.is_empty() => {
                let autre = chaines::dedoublonner_par_url(autre);
                chaines::ecrire_chaines(&principal, &autre)
                    .with_context(|| format!("écriture migrée {principal:?}"))?;
                return Ok(autre);
            }
            _ => {}
        }
    }
    Ok(chaines::dedoublonner_par_url(liste))
}

fn nettoyer_nom_dossier(nom: &str) -> String {
    let brut = nom.trim();
    if brut.is_empty() {
        return "inconnu".to_string();
    }
    let mut s = String::with_capacity(brut.len());
    for c in brut.chars() {
        let ok = c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.';
        s.push(if ok { c } else { '_' });
    }
    // Windows : évite les noms finissant par point/espace.
    let s = s.trim_matches([' ', '.']).to_string();
    if s.is_empty() {
        "inconnu".to_string()
    } else {
        s
    }
}

fn cle_chaine_depuis_url(url_chaine: &str) -> String {
    // Ex: https://www.youtube.com/@oxyad -> "oxyad"
    if let Some(pos) = url_chaine.find('@') {
        let apres = &url_chaine[(pos + 1)..];
        let handle = apres
            .split(|c| c == '/' || c == '?' || c == '&' || c == '#')
            .next()
            .unwrap_or_default();
        return nettoyer_nom_dossier(handle);
    }
    // Fallback : dernier segment.
    let dernier = url_chaine
        .split('/')
        .filter(|s| !s.trim().is_empty())
        .last()
        .unwrap_or("inconnu");
    nettoyer_nom_dossier(dernier)
}

fn repertoire_video(racine_donnees: &Path, channel_url: Option<&str>, id_video: &str) -> PathBuf {
    let cle_chaine = match channel_url {
        Some(u) if !u.trim().is_empty() => cle_chaine_depuis_url(u),
        _ => "direct".to_string(),
    };
    racine_donnees
        .join("chaines")
        .join(cle_chaine)
        .join("videos")
        .join(nettoyer_nom_dossier(id_video))
}

/// Dossier d’un texte libre : `donnees/textes_libres/<id>/` (carte mentale + texte source).
fn repertoire_texte_libre(racine_donnees: &Path, id_texte: &str) -> PathBuf {
    racine_donnees
        .join("textes_libres")
        .join(nettoyer_nom_dossier(id_texte))
}

fn chemin_transcript_cache(
    racine_donnees: &Path,
    channel_url: Option<&str>,
    id_video: &str,
) -> PathBuf {
    repertoire_video(racine_donnees, channel_url, id_video).join("transcript.txt")
}

fn charger_ou_extraire_transcript_et_mettre_en_cache(
    racine_donnees: &Path,
    channel_url: Option<&str>,
    id_video: &str,
    video_url: &str,
    max_caracteres: usize,
) -> Result<(String, bool), String> {
    let chemin = chemin_transcript_cache(racine_donnees, channel_url, id_video);
    if chemin.is_file() {
        let contenu = std::fs::read_to_string(&chemin).map_err(|e| e.to_string())?;
        if !contenu.trim().is_empty() {
            return Ok((contenu, true));
        }
    }

    let transcript =
        youtube::extraire_transcript(video_url, max_caracteres).map_err(|e| e.to_string())?;
    let dossier = repertoire_video(racine_donnees, channel_url, id_video);
    std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    std::fs::write(&chemin, &transcript).map_err(|e| e.to_string())?;
    Ok((transcript, false))
}

fn extraire_id_video_depuis_url(url: &str) -> Option<String> {
    if let Some(pos) = url.find("v=") {
        let apres = &url[(pos + 2)..];
        let cle = apres.split('&').next().unwrap_or_default();
        if !cle.trim().is_empty() {
            return Some(cle.trim().to_string());
        }
    }

    let segments: Vec<&str> = url.split('/').filter(|s| !s.trim().is_empty()).collect();
    for i in 0..segments.len().saturating_sub(1) {
        if segments[i].eq_ignore_ascii_case("shorts") {
            return Some(segments[i + 1].to_string());
        }
    }

    segments
        .last()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn enregistrer_resume_sur_disque(
    racine_donnees: &Path,
    id: &str,
    format: &str,
    contenu: &str,
    channel_url: Option<&str>,
) -> Result<PathBuf, String> {
    let suffixe = match format {
        "carte_mentale" => "carte_mentale.md",
        _ => "resume.txt",
    };
    let dossier = repertoire_video(racine_donnees, channel_url, id);
    std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let chemin = if suffixe == "carte_mentale.md" {
        dossier.join(suffixe)
    } else {
        dossier.join(format!("{id}_{suffixe}"))
    };
    std::fs::write(&chemin, contenu).map_err(|e| e.to_string())?;
    Ok(chemin)
}

#[tauri::command]
pub async fn resume_enregistre_existe(
    etat: State<'_, EtatApplication>,
    video_id: String,
    format: String,
    channel_url: Option<String>,
) -> Result<bool, String> {
    if format != "carte_mentale" && format != "bullets" {
        return Ok(false);
    }
    let racine = &etat.racine_donnees;
    let dossier = repertoire_video(racine, channel_url.as_deref(), &video_id);
    if dossier.join("carte_mentale.md").is_file() {
        return Ok(true);
    }

    // Fallback : si la clé de chaîne a changé, chercher dans toutes les chaînes.
    let base_chaines = racine.join("chaines");
    if base_chaines.is_dir() {
        if let Ok(chaines) = std::fs::read_dir(&base_chaines) {
            for c in chaines.flatten() {
                let p = c
                    .path()
                    .join("videos")
                    .join(nettoyer_nom_dossier(&video_id))
                    .join("carte_mentale.md");
                if p.is_file() {
                    return Ok(true);
                }
            }
        }
    }
    // Compat : ancien fichier au niveau racine `donnees/resumes/{id}_bullets.txt`.
    let base_ancien = racine.join("resumes");
    if base_ancien.join(format!("{video_id}_bullets.txt")).is_file() {
        return Ok(true);
    }
    Ok(false)
}

/// Supprime localement le cache de la vidéo (transcript + carte mentale).
/// Ne supprime pas la liste YouTube : la vidéo peut réapparaître au prochain rafraîchissement.
#[tauri::command]
pub fn supprimer_video(
    etat: State<'_, EtatApplication>,
    video_id: String,
    channel_url: Option<String>,
) -> Result<u64, String> {
    let mut compte: u64 = 0;
    let racine = &etat.racine_donnees;

    // Répertoire attendu dans la chaîne courante (ou “direct” si aucune chaîne).
    let dossier_cible = repertoire_video(racine, channel_url.as_deref(), &video_id);
    if dossier_cible.is_dir() {
        std::fs::remove_dir_all(&dossier_cible).map_err(|e| e.to_string())?;
        compte += 1;
    }

    // Fallback : si le handle de chaîne a changé, chercher dans toutes les chaînes.
    let base_chaines = racine.join("chaines");
    if base_chaines.is_dir() {
        if let Ok(chaines) = std::fs::read_dir(&base_chaines) {
            for c in chaines.flatten() {
                let videos_dir = c
                    .path()
                    .join("videos")
                    .join(nettoyer_nom_dossier(&video_id));
                if videos_dir.is_dir() && videos_dir != dossier_cible {
                    std::fs::remove_dir_all(&videos_dir).map_err(|e| e.to_string())?;
                    compte += 1;
                }
            }
        }
    }

    // Compat ancien : suppression du fichier bullets historique au niveau racine.
    let base_ancien = racine.join("resumes");
    let ancien = base_ancien.join(format!("{video_id}_bullets.txt"));
    if ancien.is_file() {
        std::fs::remove_file(&ancien).map_err(|e| e.to_string())?;
        compte += 1;
    }

    Ok(compte)
}

impl EtatApplication {
    pub fn nouveau(racine_donnees: PathBuf) -> Self {
        // Évite que le proxy système (souvent sous Windows) bloque les appels HTTP vers le serveur LM Studio (LAN).
        let client_http = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client_http,
            annulation: Arc::new(AtomicBool::new(false)),
            racine_donnees,
            modele_local: Arc::new(MutexCacheModele::new(None)),
            verrou_inference_locale: Arc::new(Mutex::new(())),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationLmPayload {
    /// `lm_studio` (défaut), `openrouter` ou `local`.
    #[serde(default = "fournisseur_lm_studio_defaut")]
    pub fournisseur: String,
    #[serde(default)]
    pub cle_openrouter: Option<String>,
    /// Compat : anciens clients / CLI peuvent encore préciser une URL LM Studio.
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub modele: Option<String>,
    /// Jeton Bearer pour le serveur LM Studio (optionnel, si l’API locale exige une authentification).
    #[serde(default)]
    pub cle_lm_studio: Option<String>,
    #[serde(default = "temperature_par_defaut")]
    pub temperature: f32,
    /// Taille de la fenêtre de contexte (utilisée uniquement par le fournisseur `local`).
    #[serde(default)]
    pub n_ctx: Option<u32>,
}

fn fournisseur_lm_studio_defaut() -> String {
    "lm_studio".to_string()
}

fn temperature_par_defaut() -> f32 {
    0.7
}

fn configuration_ia_depuis_payload(
    lm: &ConfigurationLmPayload,
    max_caracteres_transcript: usize,
    max_caracteres_reponse: usize,
) -> Result<ConfigurationIA, String> {
    const LM_BASE_DEFAUT: &str = "http://192.168.1.71:1234";
    const LM_ROUTE_DEFAUT: &str = "v1/chat/completions";
    const MODELE_DEFAUT: &str = "google/gemma-4-26b-a4b";

    let fournisseur = lm.fournisseur.trim();
    let fournisseur = if fournisseur.is_empty() {
        "lm_studio"
    } else {
        fournisseur
    };

    match fournisseur {
        "openrouter" => {
            let modele = lm
                .modele
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| MODELE_DEFAUT.to_string());
            let cle = lm
                .cle_openrouter
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            if cle.is_none() {
                return Err("Clé API OpenRouter requise (paramètres).".to_string());
            }
            Ok(ConfigurationIA {
                base_url: "https://openrouter.ai/api/v1".to_string(),
                route: "chat/completions".to_string(),
                modele: Some(modele),
                temperature: lm.temperature,
                max_caracteres_transcript,
                max_caracteres_reponse,
                cle_api_bearer: cle,
            })
        }
        _ => {
            let modele = lm
                .modele
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| MODELE_DEFAUT.to_string());
            let base = lm
                .base_url
                .as_deref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or(LM_BASE_DEFAUT);
            let route = lm
                .route
                .as_deref()
                .map(|s| s.trim().trim_start_matches('/'))
                .filter(|s| !s.is_empty())
                .unwrap_or(LM_ROUTE_DEFAUT);
            let cle_lm = lm
                .cle_lm_studio
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            Ok(ConfigurationIA {
                base_url: base.to_string(),
                route: route.to_string(),
                modele: Some(modele),
                temperature: lm.temperature,
                max_caracteres_transcript,
                max_caracteres_reponse,
                cle_api_bearer: cle_lm,
            })
        }
    }
}

fn format_depuis_chaine(nom: &str) -> Result<FormatSortie, String> {
    match nom {
        "carte_mentale" => Ok(FormatSortie::CarteMentale),
        // Compat : ancien libellé utilisé partout historiquement
        "bullets" => Ok(FormatSortie::CarteMentale),
        _ => Err(format!("format inconnu: {nom}")),
    }
}

/// Vrai si le `lm` payload demande explicitement le moteur local.
fn est_fournisseur_local(lm: &ConfigurationLmPayload) -> bool {
    lm.fournisseur.trim() == "local"
}

/// Taille de fenêtre de contexte par défaut quand l'UI n'en a pas envoyé.
const N_CTX_LOCAL_DEFAUT: u32 = 50_000;
/// Plafond côté IPC (aligné sur l'UI) ; le modèle peut refuser une valeur trop grande.
const N_CTX_LOCAL_MAX: u32 = 131_072;
/// Plafond approximatif du nombre de tokens générés (sortie) côté local.
const MAX_TOKENS_SORTIE_LOCALE: i32 = 4096;

/// Charge (au besoin) le modèle local en RAM puis exécute une génération chat
/// dans **un seul** `spawn_blocking` : llama.cpp / `LlamaModel` ne doivent pas être
/// manipulés depuis des threads du pool différents (sinon SIGSEGV possible).
/// `verrou_inference_locale` sérialise les appels concurrents.
async fn appeler_modele_local_chat(
    etat: &EtatApplication,
    app: &AppHandle,
    lm: &ConfigurationLmPayload,
    system: String,
    user: String,
    max_caracteres_reponse: usize,
) -> Result<String, String> {
    let _garde_inference = etat.verrou_inference_locale.lock().await;

    let chemin = chemin_modele_local(etat.racine_donnees.as_path());
    if !chemin.is_file() {
        return Err(format!(
            "Modèle local introuvable ({}). Téléchargez-le depuis l'onglet « Local » dans les paramètres.",
            chemin.display()
        ));
    }

    let n_ctx = lm
        .n_ctx
        .unwrap_or(N_CTX_LOCAL_DEFAUT)
        .clamp(512, N_CTX_LOCAL_MAX);
    let temperature = lm.temperature;

    let _ = app.emit(
        "job-log",
        format!(
            "[MODELE-LOCAL] Inférence locale (chargement si besoin puis génération), n_ctx={n_ctx} — {}.",
            chemin.display()
        ),
    );

    let cache_modele = etat.modele_local.clone();
    let chemin_bloquant = chemin.clone();
    let texte = tokio::task::spawn_blocking(move || {
        let arc_modele: Arc<ModeleLocalCharge> = {
            let mut garde_cache = cache_modele
                .lock()
                .map_err(|e| format!("verrou cache modèle local: {e}"))?;
            let recharger = match garde_cache.as_ref() {
                None => true,
                Some(a) => a.n_ctx_courant != n_ctx,
            };
            if recharger {
                *garde_cache = None;
                let charge = ia_local::charger_modele_local(&chemin_bloquant, n_ctx)
                    .map_err(|e| e.to_string())?;
                let nouveau = Arc::new(charge);
                *garde_cache = Some(nouveau.clone());
                nouveau
            } else {
                garde_cache
                    .as_ref()
                    .expect("modèle local présent dans le cache")
                    .clone()
            }
        };

        ia_local::generer_chat(
            &arc_modele,
            &system,
            &user,
            temperature,
            n_ctx,
            MAX_TOKENS_SORTIE_LOCALE,
            max_caracteres_reponse,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("tâche modèle local interrompue: {e}"))??;

    let _ = app.emit("job-log", "[MODELE-LOCAL] Génération terminée.");

    Ok(texte)
}

#[tauri::command]
pub fn lister_chaines(etat: State<'_, EtatApplication>) -> Result<Vec<EntreeChaine>, String> {
    lire_chaines_avec_migration(etat.racine_donnees.as_path()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ajouter_chaine(
    etat: State<'_, EtatApplication>,
    url_chaine: String,
) -> Result<(), String> {
    lire_chaines_avec_migration(etat.racine_donnees.as_path()).map_err(|e| e.to_string())?;
    let chemin = chemin_fichier_chaines_json(etat.racine_donnees.as_path());
    chaines::ajouter_chaine_depuis_url(&chemin, url_chaine).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn supprimer_chaine(
    etat: State<'_, EtatApplication>,
    url_chaine: String,
) -> Result<u32, String> {
    lire_chaines_avec_migration(etat.racine_donnees.as_path()).map_err(|e| e.to_string())?;
    let chemin = chemin_fichier_chaines_json(etat.racine_donnees.as_path());
    let compte = chaines::supprimer_chaine_par_url(&chemin, &url_chaine).map_err(|e| e.to_string())?;
    let compte = u32::try_from(compte).map_err(|_| "nombre de suppressions invalide".to_string())?;
    Ok(compte)
}

#[tauri::command]
pub fn chemin_fichier_chaines(etat: State<'_, EtatApplication>) -> String {
    chemin_fichier_chaines_json(etat.racine_donnees.as_path())
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
pub async fn lister_videos_chaine(
    url_chaine: String,
    nb_videos: usize,
) -> Result<Vec<VideoInfo>, String> {
    tokio::task::spawn_blocking(move || youtube::lister_dernieres_videos(&url_chaine, nb_videos))
        .await
        .map_err(|e| format!("tâche lister_videos_chaine interrompue: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn infos_video_depuis_url(video_url: String) -> Result<VideoInfo, String> {
    tokio::task::spawn_blocking(move || youtube::infos_video_depuis_url(&video_url))
        .await
        .map_err(|e| format!("tâche infos_video_depuis_url interrompue: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn extraire_transcript_video(
    _etat: State<'_, EtatApplication>,
    video_url: String,
    max_caracteres: usize,
) -> Result<String, String> {
    youtube::extraire_transcript(&video_url, max_caracteres).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lister_modeles_lm_studio(
    etat: State<'_, EtatApplication>,
    lm: ConfigurationLmPayload,
) -> Result<Vec<String>, String> {
    let f = lm.fournisseur.trim();
    let f = if f.is_empty() { "lm_studio" } else { f };
    if f == "openrouter" {
        return Err("Cette commande ne s’applique qu’au serveur LM Studio local.".to_string());
    }
    let config_ia = configuration_ia_depuis_payload(&lm, 25_000, 18_000)
        .map_err(|e| e.to_string())?;
    ia::lister_modeles_lm_studio(&etat.client_http, &config_ia)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lister_modeles_openrouter_gratuits(
    etat: State<'_, EtatApplication>,
) -> Result<Vec<String>, String> {
    ia::lister_modeles_openrouter_gratuits(&etat.client_http)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lire_resume_enregistre(
    video_id: String,
    format: String,
) -> Result<Option<String>, String> {
    let _ = format;
    let _ = video_id;
    Ok(None)
}

/// Tous les résumés déjà enregistrés sur disque pour une même vidéo (un fichier par format).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumesEnregistresPourVideo {
    pub carte_mentale: Option<String>,
}

#[tauri::command]
pub async fn lire_tous_resumes_enregistres(
    app: AppHandle,
    etat: State<'_, EtatApplication>,
    video_id: String,
    channel_url: Option<String>,
) -> Result<ResumesEnregistresPourVideo, String> {
    let _ = app.emit(
        "job-log",
        format!(
            "Lecture carte mentale: id={} channelUrl={}",
            video_id,
            channel_url.clone().unwrap_or_else(|| "(null)".to_string())
        ),
    );
    let racine = &etat.racine_donnees;
    let dossier = repertoire_video(racine, channel_url.as_deref(), &video_id);
    let chemin = dossier.join("carte_mentale.md");
    if chemin.is_file() {
        let contenu = std::fs::read_to_string(&chemin).map_err(|e| e.to_string())?;
        return Ok(ResumesEnregistresPourVideo {
            carte_mentale: Some(contenu),
        });
    }
    let _ = app.emit(
        "job-log",
        format!("Carte mentale introuvable au chemin attendu: {}", chemin.display()),
    );

    // Fallback : si la clé de chaîne a changé, chercher dans toutes les chaînes.
    let base_chaines = racine.join("chaines");
    if base_chaines.is_dir() {
        if let Ok(chaines) = std::fs::read_dir(&base_chaines) {
            for c in chaines.flatten() {
                let p = c
                    .path()
                    .join("videos")
                    .join(nettoyer_nom_dossier(&video_id))
                    .join("carte_mentale.md");
                if p.is_file() {
                    let contenu = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
                    let _ = app.emit(
                        "job-log",
                        format!("Carte mentale trouvée via fallback: {}", p.display()),
                    );
                    return Ok(ResumesEnregistresPourVideo {
                        carte_mentale: Some(contenu),
                    });
                }
            }
        }
    }

    // Compat ancien
    let base_ancien = racine.join("resumes");
    let ancien = base_ancien.join(format!("{video_id}_bullets.txt"));
    if ancien.is_file() {
        let contenu = std::fs::read_to_string(&ancien).map_err(|e| e.to_string())?;
        return Ok(ResumesEnregistresPourVideo {
            carte_mentale: Some(contenu),
        });
    }

    Ok(ResumesEnregistresPourVideo {
        carte_mentale: None,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct EntreeIndexTranscript {
    pub id: String,
    pub titre: String,
    pub url: String,
    pub ok: bool,
    pub nb_caracteres: Option<usize>,
    pub fichier_transcript: Option<String>,
}

#[tauri::command]
pub async fn extraire_transcripts_chaine(
    app: AppHandle,
    etat: State<'_, EtatApplication>,
    channel_url: String,
    nb_videos: usize,
    dossier_sortie: Option<String>,
    max_caracteres: usize,
) -> Result<Vec<EntreeIndexTranscript>, String> {
    let _ = &etat;
    etat.annulation.store(false, Ordering::SeqCst);

    let videos =
        youtube::lister_dernieres_videos(&channel_url, nb_videos).map_err(|e| e.to_string())?;
    let mut index: Vec<EntreeIndexTranscript> = Vec::new();

    for (i, video) in videos.iter().enumerate() {
        if etat.annulation.load(Ordering::SeqCst) {
            let _ = app.emit(
                "job-log",
                format!("Annulation demandée après {i} vidéo(s)."),
            );
            break;
        }

        let _ = app.emit(
            "job-progression",
            serde_json::json!({
                "courant": i + 1,
                "total": videos.len(),
                "titre": video.titre,
                "id": video.id,
            }),
        );
        let _ = app.emit(
            "job-log",
            format!("[{}/{}] Transcript: {}", i + 1, videos.len(), video.titre),
        );

        let (transcript, depuis_cache) = match charger_ou_extraire_transcript_et_mettre_en_cache(
            etat.racine_donnees.as_path(),
            Some(&channel_url),
            &video.id,
            &video.url,
            max_caracteres,
        ) {
            Ok(t) => t,
            Err(e) => {
                let _ = app.emit("job-log", format!("KO transcript: {e}"));
                index.push(EntreeIndexTranscript {
                    id: video.id.clone(),
                    titre: video.titre.clone(),
                    url: video.url.clone(),
                    ok: false,
                    nb_caracteres: None,
                    fichier_transcript: None,
                });
                continue;
            }
        };
        if depuis_cache {
            let _ = app.emit("job-log", format!("Transcript cache: {}", video.id));
        }

        let mut fichier_ecrit: Option<String> = None;
        if let Some(ref dossier) = dossier_sortie {
            let p = PathBuf::from(dossier);
            std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
            let fichier = format!("{}_transcript.txt", video.id);
            let chemin = p.join(&fichier);
            std::fs::write(&chemin, &transcript).map_err(|e| e.to_string())?;
            fichier_ecrit = chemin.file_name().map(|n| n.to_string_lossy().to_string());
        }

        index.push(EntreeIndexTranscript {
            id: video.id.clone(),
            titre: video.titre.clone(),
            url: video.url.clone(),
            ok: true,
            nb_caracteres: Some(transcript.chars().count()),
            fichier_transcript: fichier_ecrit,
        });
    }

    if let Some(dossier) = dossier_sortie {
        let p = PathBuf::from(&dossier);
        std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
        let chemin_index = p.join("index.json");
        let json = serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?;
        std::fs::write(&chemin_index, json).map_err(|e| e.to_string())?;
        let _ = app.emit(
            "job-log",
            format!("Index écrit: {}", chemin_index.display()),
        );
    }

    Ok(index)
}

#[tauri::command]
pub async fn resumer_video_direct(
    app: AppHandle,
    etat: State<'_, EtatApplication>,
    video_url: String,
    format: String,
    channel_url: Option<String>,
    lm: ConfigurationLmPayload,
) -> Result<String, String> {
    let format_sortie = format_depuis_chaine(&format)?;
    etat.annulation.store(false, Ordering::SeqCst);
    let video = VideoInfo {
        id: "video".to_string(),
        titre: "Vidéo YouTube".to_string(),
        url: video_url,
        date_publication: None,
    };

    let utiliser_local = est_fournisseur_local(&lm);
    let max_caracteres_transcript: usize = 25_000;
    let max_caracteres_reponse: usize = 18_000;
    let config_ia = if utiliser_local {
        // On ne configure pas LM Studio / OpenRouter quand le moteur local est actif :
        // les valeurs ci-dessous ne sont consultées que pour `max_caracteres_transcript`.
        ConfigurationIA {
            base_url: String::new(),
            route: String::new(),
            modele: None,
            temperature: lm.temperature,
            max_caracteres_transcript,
            max_caracteres_reponse,
            cle_api_bearer: None,
        }
    } else {
        configuration_ia_depuis_payload(&lm, max_caracteres_transcript, max_caracteres_reponse)?
    };

    let _ = app.emit(
        "job-log",
        "[TRANSCRIPT] Début de l'extraction (yt-dlp).",
    );

    let (tx_fini, mut rx_fini) = tokio::sync::watch::channel(false);

    let url_pour_extraction = video.url.clone();
    let max_pour_extraction = config_ia.max_caracteres_transcript;
    let id_pour_cache = extraire_id_video_depuis_url(&video.url).unwrap_or_else(|| "video".to_string());
    let chaine_pour_cache = channel_url.clone();
    let racine_transcript = etat.racine_donnees.clone();
    let tache_extraction = tokio::task::spawn_blocking(move || {
        charger_ou_extraire_transcript_et_mettre_en_cache(
            racine_transcript.as_path(),
            chaine_pour_cache.as_deref(),
            &id_pour_cache,
            &url_pour_extraction,
            max_pour_extraction,
        )
    });

    let app_heartbeat = app.clone();
    let tache_heartbeat = tokio::spawn(async move {
        // Évite de spammer les logs : une ligne ~toutes les 20 s avec le temps écoulé (l’extraction peut durer longtemps).
        let mut intervalle = tokio::time::interval(Duration::from_secs(20));
        intervalle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        intervalle.tick().await;
        let debut = Instant::now();
        loop {
            tokio::select! {
                _ = intervalle.tick() => {
                    let sec = debut.elapsed().as_secs();
                    let _ = app_heartbeat.emit(
                        "job-log",
                        format!("[TRANSCRIPT] Extraction en cours… ({sec}s)"),
                    );
                }
                Ok(()) = rx_fini.changed() => {
                    if *rx_fini.borrow() {
                        break;
                    }
                }
            }
        }
    });

    let (transcript, transcript_depuis_cache) = match tache_extraction
        .await
        .map_err(|e| format!("tâche extraction transcript interrompue: {e}"))?
    {
        Ok(t) => t,
        Err(e) => {
            let _ = tx_fini.send(true);
            let _ = tache_heartbeat.await;
            return Err(e);
        }
    };

    let _ = tx_fini.send(true);
    let _ = tache_heartbeat.await;

    if etat.annulation.load(Ordering::SeqCst) {
        let _ = app.emit(
            "job-log",
            "[TRANSCRIPT] Annulation demandée après extraction.",
        );
        return Err("Job annulé par l’utilisateur.".to_string());
    }

    if transcript_depuis_cache {
        let _ = app.emit("job-log", "[TRANSCRIPT] Chargé depuis le cache disque.");
    }
    let nb = transcript.chars().count();
    let _ = app.emit(
        "job-log",
        format!("[TRANSCRIPT] Prêt ({nb} caractères)."),
    );
    let _ = app.emit(
        "job-log",
        "[MODELE] Début de l'appel du modèle IA.",
    );

    let (system, user) =
        resume::construire_messages_resume(&transcript, Some(&video.titre), format_sortie);
    let contenu_ia = if utiliser_local {
        appeler_modele_local_chat(
            &etat,
            &app,
            &lm,
            system,
            user,
            max_caracteres_reponse,
        )
        .await?
    } else {
        let requete_ia =
            ia::appeler_lm_studio_chat(&etat.client_http, &config_ia, &system, &user);
        tokio::pin!(requete_ia);
        loop {
            tokio::select! {
                resultat = &mut requete_ia => {
                    break resultat.map_err(|e| e.to_string())?;
                }
                _ = tokio::time::sleep(Duration::from_millis(250)) => {
                    if etat.annulation.load(Ordering::SeqCst) {
                        let _ = app.emit(
                            "job-log",
                            "[MODELE] Annulation demandée pendant l'appel IA.",
                        );
                        return Err("Job annulé par l’utilisateur.".to_string());
                    }
                }
            }
        }
    };

    let _ = app.emit("job-log", "[MODELE] Réponse reçue, mise en forme du résumé…");

    let sortie = resume::parser_sortie_ia_vers_sortie(format_sortie, &contenu_ia)
        .map_err(|e| e.to_string())?;
    let texte = resume::formatter_sortie_resume_sortie(format_sortie, sortie);

    let id_pour_fichier = extraire_id_video_depuis_url(&video.url).unwrap_or_else(|| "video".to_string());
    if let Ok(chemin) = enregistrer_resume_sur_disque(
        etat.racine_donnees.as_path(),
        &id_pour_fichier,
        "carte_mentale",
        &texte,
        channel_url.as_deref(),
    ) {
        let _ = app.emit(
            "job-log",
            format!("[SORTIE] Résumé enregistré: {}", chemin.display()),
        );
    }

    Ok(texte)
}

/// Résumé LM Studio à partir d’un texte fourni par l’utilisateur (sans extraction YouTube).
/// Enregistre sous `donnees/textes_libres/<id_texte>/` : `texte_source.txt` et `carte_mentale.md`.
#[tauri::command]
pub async fn resumer_texte_libre(
    app: AppHandle,
    etat: State<'_, EtatApplication>,
    id_texte: String,
    texte: String,
    format: String,
    lm: ConfigurationLmPayload,
) -> Result<String, String> {
    let id_trim = id_texte.trim();
    if id_trim.is_empty() {
        return Err("identifiant texte manquant".to_string());
    }

    let format_sortie = format_depuis_chaine(&format)?;
    let transcript_brut = texte.trim().to_string();
    if transcript_brut.is_empty() {
        return Err("texte vide".to_string());
    }

    let max = 25_000usize;
    let transcript = if transcript_brut.chars().count() > max {
        let _ = app.emit(
            "job-log",
            format!("[TRANSCRIPT] Texte tronqué à {max} caractères avant appel du modèle."),
        );
        transcript_brut.chars().take(max).collect::<String>()
    } else {
        transcript_brut
    };

    let utiliser_local = est_fournisseur_local(&lm);
    let max_caracteres_reponse: usize = 18_000;
    let config_ia = if utiliser_local {
        ConfigurationIA {
            base_url: String::new(),
            route: String::new(),
            modele: None,
            temperature: lm.temperature,
            max_caracteres_transcript: max,
            max_caracteres_reponse,
            cle_api_bearer: None,
        }
    } else {
        configuration_ia_depuis_payload(&lm, max, max_caracteres_reponse)?
    };

    let nb = transcript.chars().count();
    let _ = app.emit(
        "job-log",
        format!("[TRANSCRIPT] Texte prêt ({nb} caractères)."),
    );
    let _ = app.emit(
        "job-log",
        "[MODELE] Début de l'appel du modèle IA.",
    );

    let (system, user) =
        resume::construire_messages_resume_texte_libre(&transcript, format_sortie);
    let contenu_ia = if utiliser_local {
        appeler_modele_local_chat(
            &etat,
            &app,
            &lm,
            system,
            user,
            max_caracteres_reponse,
        )
        .await?
    } else {
        ia::appeler_lm_studio_chat(&etat.client_http, &config_ia, &system, &user)
            .await
            .map_err(|e| e.to_string())?
    };

    let _ = app.emit("job-log", "[MODELE] Réponse reçue, mise en forme du résumé…");

    let sortie = resume::parser_sortie_ia_vers_sortie(format_sortie, &contenu_ia)
        .map_err(|e| e.to_string())?;
    let texte_sortie = resume::formatter_sortie_resume_sortie(format_sortie, sortie);

    let dossier = repertoire_texte_libre(etat.racine_donnees.as_path(), id_trim);
    std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    std::fs::write(dossier.join("texte_source.txt"), transcript.as_str())
        .map_err(|e| e.to_string())?;
    std::fs::write(dossier.join("carte_mentale.md"), texte_sortie.as_str())
        .map_err(|e| e.to_string())?;
    let chemin_carte = dossier.join("carte_mentale.md");
    let _ = app.emit(
        "job-log",
        format!("[SORTIE] Résumé enregistré: {}", chemin_carte.display()),
    );

    Ok(texte_sortie)
}

/// Lit la carte mentale d’un texte libre si le fichier existe (`donnees/textes_libres/<id>/carte_mentale.md`).
#[tauri::command]
pub fn lire_carte_mentale_texte_libre(
    etat: State<'_, EtatApplication>,
    id_texte: String,
) -> Result<Option<String>, String> {
    let id_trim = id_texte.trim();
    if id_trim.is_empty() {
        return Ok(None);
    }
    let chemin = repertoire_texte_libre(etat.racine_donnees.as_path(), id_trim).join("carte_mentale.md");
    if !chemin.is_file() {
        return Ok(None);
    }
    let contenu = std::fs::read_to_string(&chemin).map_err(|e| e.to_string())?;
    Ok(Some(contenu))
}

/// Supprime le dossier local d’un texte libre (`transcript` équivalent + carte mentale).
#[tauri::command]
pub fn supprimer_dossier_texte_libre(
    etat: State<'_, EtatApplication>,
    id_texte: String,
) -> Result<(), String> {
    let id_trim = id_texte.trim();
    if id_trim.is_empty() {
        return Ok(());
    }
    let dossier = repertoire_texte_libre(etat.racine_donnees.as_path(), id_trim);
    if dossier.is_dir() {
        std::fs::remove_dir_all(&dossier).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntreeTexteLibreSauvegarde {
    pub id: String,
    pub contenu: String,
    pub resume_carte_mentale: Option<String>,
}

/// Écrit `donnees/textes_libres/<id>/texte_source.txt` et, si fourni, `carte_mentale.md` à partir du stockage local (migration / sauvegarde).
#[tauri::command]
pub fn synchroniser_textes_libres_vers_disque(
    etat: State<'_, EtatApplication>,
    entrees: Vec<EntreeTexteLibreSauvegarde>,
) -> Result<u32, String> {
    let mut dossiers_ecrits = 0u32;
    for e in entrees {
        let id = e.id.trim();
        if id.is_empty() {
            continue;
        }
        let contenu = e.contenu.trim();
        if contenu.is_empty() {
            continue;
        }
        let dossier = repertoire_texte_libre(etat.racine_donnees.as_path(), id);
        std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
        std::fs::write(dossier.join("texte_source.txt"), contenu.as_bytes())
            .map_err(|e| e.to_string())?;
        if let Some(ref carte) = e.resume_carte_mentale {
            let c = carte.trim();
            if !c.is_empty() {
                std::fs::write(dossier.join("carte_mentale.md"), c).map_err(|e| e.to_string())?;
            }
        }
        dossiers_ecrits += 1;
    }
    Ok(dossiers_ecrits)
}

#[tauri::command]
pub async fn resumer_chaine(
    app: AppHandle,
    etat: State<'_, EtatApplication>,
    channel_url: String,
    nb_videos: usize,
    format: String,
    lm: ConfigurationLmPayload,
    dossier_sortie: Option<String>,
) -> Result<Vec<String>, String> {
    let format_sortie = format_depuis_chaine(&format)?;
    etat.annulation.store(false, Ordering::SeqCst);

    let utiliser_local = est_fournisseur_local(&lm);
    let max_caracteres_transcript: usize = 25_000;
    let max_caracteres_reponse: usize = 18_000;
    let config_ia = if utiliser_local {
        ConfigurationIA {
            base_url: String::new(),
            route: String::new(),
            modele: None,
            temperature: lm.temperature,
            max_caracteres_transcript,
            max_caracteres_reponse,
            cle_api_bearer: None,
        }
    } else {
        configuration_ia_depuis_payload(&lm, max_caracteres_transcript, max_caracteres_reponse)?
    };

    let videos =
        youtube::lister_dernieres_videos(&channel_url, nb_videos).map_err(|e| e.to_string())?;
    let mut resultats: Vec<String> = Vec::new();

    for (i, video) in videos.iter().enumerate() {
        if etat.annulation.load(Ordering::SeqCst) {
            let _ = app.emit("job-log", "Annulation du résumé chaîne.");
            break;
        }

        let _ = app.emit(
            "job-progression",
            serde_json::json!({
                "courant": i + 1,
                "total": videos.len(),
                "titre": video.titre,
                "id": video.id,
            }),
        );

        let (transcript, depuis_cache) = match charger_ou_extraire_transcript_et_mettre_en_cache(
            etat.racine_donnees.as_path(),
            Some(&channel_url),
            &video.id,
            &video.url,
            config_ia.max_caracteres_transcript,
        ) {
            Ok(t) => t,
            Err(e) => {
                let _ = app.emit("job-log", format!("KO transcript {}: {e}", video.id));
                continue;
            }
        };
        if depuis_cache {
            let _ = app.emit("job-log", format!("Transcript cache: {}", video.id));
        }

        let (system, user) =
            resume::construire_messages_resume(&transcript, Some(&video.titre), format_sortie);
        let resultat_ia = if utiliser_local {
            appeler_modele_local_chat(
                &etat,
                &app,
                &lm,
                system,
                user,
                max_caracteres_reponse,
            )
            .await
        } else {
            ia::appeler_lm_studio_chat(&etat.client_http, &config_ia, &system, &user)
                .await
                .map_err(|e| e.to_string())
        };
        let contenu_ia = match resultat_ia {
            Ok(c) => c,
            Err(e) => {
                let _ = app.emit("job-log", format!("KO IA {}: {e}", video.id));
                continue;
            }
        };

        let sortie = match resume::parser_sortie_ia_vers_sortie(format_sortie, &contenu_ia) {
            Ok(s) => s,
            Err(e) => {
                let _ = app.emit("job-log", format!("KO parse résumé {}: {e}", video.id));
                continue;
            }
        };

        let texte = resume::formatter_sortie_resume_sortie(format_sortie, sortie);
        resultats.push(format!("=== {} ===\n{}", video.titre, texte));

        if let Some(ref dossier) = dossier_sortie {
            let p = PathBuf::from(dossier);
            std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
            let fichier = format!(
                "{}_{}.{}",
                video.id,
                format_sortie.nom_pour_prompt(),
                format_sortie.ext_fichier()
            );
            let chemin = p.join(fichier);
            std::fs::write(&chemin, &texte).map_err(|e| e.to_string())?;
        }

        if let Ok(chemin) = enregistrer_resume_sur_disque(
            etat.racine_donnees.as_path(),
            &video.id,
            "carte_mentale",
            &texte,
            Some(&channel_url),
        ) {
            let _ = app.emit(
                "job-log",
                format!("Résumé enregistré: {}", chemin.display()),
            );
        }
    }

    Ok(resultats)
}

#[tauri::command]
pub async fn lancer_test_suite(
    app: AppHandle,
    etat: State<'_, EtatApplication>,
    channel_url: Option<String>,
    video_url: Option<String>,
    lm: Option<ConfigurationLmPayload>,
) -> Result<String, String> {
    let (video_source, titre_source) = match (channel_url, video_url) {
        (Some(c), None) => (c, "chaîne"),
        (None, Some(v)) => (v, "vidéo"),
        (Some(_), Some(_)) => {
            return Err("Fournis seulement channel_url ou video_url.".to_string());
        }
        (None, None) => {
            return Err("Fournis channel_url ou video_url.".to_string());
        }
    };

    let _ = app.emit("job-log", format!("Test-suite (source: {titre_source})"));

    let video_cible: VideoInfo = match titre_source {
        "chaîne" => {
            let videos =
                youtube::lister_dernieres_videos(&video_source, 10).map_err(|e| e.to_string())?;
            videos
                .first()
                .cloned()
                .ok_or_else(|| "Chaîne sans vidéo".to_string())?
        }
        _ => VideoInfo {
            id: "video".to_string(),
            titre: "Vidéo YouTube".to_string(),
            url: video_source,
            date_publication: None,
        },
    };

    let _ = app.emit("job-log", "Extraction transcript...".to_string());
    let transcript_ok =
        youtube::extraire_transcript(&video_cible.url, 25_000).map_err(|e| e.to_string())?;
    if transcript_ok.trim().is_empty() {
        return Err("Transcript vide.".to_string());
    }
    let _ = app.emit(
        "job-log",
        format!(
            "OK transcript ({} caractères).",
            transcript_ok.chars().count()
        ),
    );

    let mut rapport = String::new();
    let config_ia_opt = match lm {
        Some(ref l) => Some(configuration_ia_depuis_payload(l, 25_000, 18_000)?),
        None => None,
    };

    for format_sortie in [FormatSortie::CarteMentale] {
        let label = format!("{:?}", format_sortie);
        let _ = app.emit("job-log", format!("Test format: {label}"));

        match &config_ia_opt {
            None => {
                rapport.push_str(&format!("{label}: IA ignorée (pas de config LM).\n"));
            }
            Some(config_ia) => {
                let (system, user) = resume::construire_messages_resume(
                    &transcript_ok,
                    Some(&video_cible.titre),
                    format_sortie,
                );
                let contenu_ia =
                    ia::appeler_lm_studio_chat(&etat.client_http, config_ia, &system, &user).await;

                match contenu_ia {
                    Err(e) => {
                        rapport.push_str(&format!("{label}: Erreur IA: {e}\n"));
                        let _ = app.emit("job-log", format!("Erreur IA {label}: {e}"));
                    }
                    Ok(contenu) => {
                        match resume::parser_sortie_ia_vers_sortie(format_sortie, &contenu) {
                            Ok(_) => {
                                rapport.push_str(&format!("{label}: OK\n"));
                                let _ = app.emit("job-log", format!("OK {label}"));
                            }
                            Err(e) => {
                                rapport.push_str(&format!("{label}: Sortie invalide: {e}\n"));
                                let _ = app.emit("job-log", format!("KO parse {label}: {e}"));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(rapport)
}

#[tauri::command]
pub fn demander_annulation(etat: State<'_, EtatApplication>) {
    etat.annulation.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub fn reinitialiser_annulation(etat: State<'_, EtatApplication>) {
    etat.annulation.store(false, Ordering::SeqCst);
}

/// Presse-papiers texte via l’API système (`arboard`), sans plugin navigateur ni permission Chromium.
#[tauri::command]
pub fn lire_texte_presse_papiers() -> Result<Option<String>, String> {
    let mut presse = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    match presse.get_text() {
        Ok(t) => {
            if t.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(t))
            }
        }
        Err(_) => Ok(None),
    }
}

#[tauri::command]
pub async fn selectionner_dossier_sortie(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let dossier = app
        .dialog()
        .file()
        .set_title("Dossier de sortie")
        .blocking_pick_folder();
    Ok(dossier.map(|p| p.to_string()))
}

/// État du modèle local (présent sur disque et/ou chargé en mémoire).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EtatModeleLocal {
    pub identifiant: String,
    pub nom_fichier: String,
    pub url_telechargement: String,
    pub chemin: String,
    pub present: bool,
    pub charge: bool,
    pub taille_octets: Option<u64>,
    pub n_ctx_courant: Option<u32>,
}

#[tauri::command]
pub async fn etat_modele_local(
    etat: State<'_, EtatApplication>,
) -> Result<EtatModeleLocal, String> {
    let chemin = chemin_modele_local(etat.racine_donnees.as_path());
    let present = chemin.is_file();
    let taille_octets = if present {
        std::fs::metadata(&chemin).ok().map(|m| m.len())
    } else {
        None
    };
    let guard = etat
        .modele_local
        .lock()
        .map_err(|e| format!("verrou cache modèle local: {e}"))?;
    let charge = guard.is_some();
    let n_ctx_courant = guard.as_ref().map(|m| m.n_ctx_courant);
    Ok(EtatModeleLocal {
        identifiant: IDENTIFIANT_MODELE_LOCAL.to_string(),
        nom_fichier: NOM_FICHIER_MODELE_LOCAL.to_string(),
        url_telechargement: URL_MODELE_LOCAL.to_string(),
        chemin: chemin.to_string_lossy().to_string(),
        present,
        charge,
        taille_octets,
        n_ctx_courant,
    })
}

/// Télécharge le GGUF dans `donnees/modeles_locaux/`. Émet régulièrement un
/// évènement `modele-local-progression` ({recus, total}). Le drapeau d'annulation
/// existant peut interrompre le téléchargement (bouton « Annuler »).
#[tauri::command]
pub async fn telecharger_modele_local(
    app: AppHandle,
    etat: State<'_, EtatApplication>,
) -> Result<EtatModeleLocal, String> {
    etat.annulation.store(false, Ordering::SeqCst);
    let dossier = repertoire_modeles_locaux(etat.racine_donnees.as_path());
    std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let chemin_final = chemin_modele_local(etat.racine_donnees.as_path());
    let chemin_partiel = dossier.join(format!("{NOM_FICHIER_MODELE_LOCAL}.partiel"));

    if chemin_final.is_file() {
        let _ = app.emit(
            "job-log",
            format!(
                "[MODELE-LOCAL] Modèle déjà présent ({}). Téléchargement ignoré.",
                chemin_final.display()
            ),
        );
        return etat_modele_local(etat).await;
    }

    let _ = app.emit(
        "job-log",
        format!("[MODELE-LOCAL] Téléchargement depuis {URL_MODELE_LOCAL}"),
    );

    let reponse = etat
        .client_http
        .get(URL_MODELE_LOCAL)
        .send()
        .await
        .map_err(|e| format!("téléchargement: {e}"))?;

    let statut = reponse.status();
    if !statut.is_success() {
        return Err(format!("HTTP {statut} lors du téléchargement du modèle"));
    }
    let total: Option<u64> = reponse.content_length();

    if chemin_partiel.is_file() {
        let _ = std::fs::remove_file(&chemin_partiel);
    }
    let mut fichier = std::fs::File::create(&chemin_partiel).map_err(|e| e.to_string())?;
    use std::io::Write;

    let mut flux = reponse.bytes_stream();
    let mut recus: u64 = 0;
    let mut dernier_emit = Instant::now();
    let _ = app.emit(
        "modele-local-progression",
        serde_json::json!({"recus": 0u64, "total": total}),
    );

    while let Some(morceau) = flux.next().await {
        if etat.annulation.load(Ordering::SeqCst) {
            let _ = std::fs::remove_file(&chemin_partiel);
            let _ = app.emit("job-log", "[MODELE-LOCAL] Téléchargement annulé.");
            return Err("Téléchargement annulé.".to_string());
        }
        let bytes = morceau.map_err(|e| format!("flux téléchargement: {e}"))?;
        fichier.write_all(&bytes).map_err(|e| e.to_string())?;
        recus = recus.saturating_add(bytes.len() as u64);
        if dernier_emit.elapsed() >= Duration::from_millis(250) {
            dernier_emit = Instant::now();
            let _ = app.emit(
                "modele-local-progression",
                serde_json::json!({"recus": recus, "total": total}),
            );
        }
    }
    fichier.flush().map_err(|e| e.to_string())?;
    drop(fichier);

    std::fs::rename(&chemin_partiel, &chemin_final).map_err(|e| e.to_string())?;

    if let Ok(mut garde) = etat.modele_local.lock() {
        *garde = None;
    }

    let _ = app.emit(
        "modele-local-progression",
        serde_json::json!({"recus": recus, "total": total.or(Some(recus))}),
    );
    let _ = app.emit(
        "job-log",
        format!(
            "[MODELE-LOCAL] Téléchargement terminé ({} octets) → {}",
            recus,
            chemin_final.display()
        ),
    );

    etat_modele_local(etat).await
}

/// Supprime le fichier GGUF local et libère la mémoire associée.
#[tauri::command]
pub async fn supprimer_modele_local(
    etat: State<'_, EtatApplication>,
) -> Result<EtatModeleLocal, String> {
    let _garde_inference = etat.verrou_inference_locale.lock().await;
    {
        let mut garde = etat
            .modele_local
            .lock()
            .map_err(|e| format!("verrou cache modèle local: {e}"))?;
        *garde = None;
    }
    let chemin = chemin_modele_local(etat.racine_donnees.as_path());
    if chemin.is_file() {
        std::fs::remove_file(&chemin).map_err(|e| e.to_string())?;
    }
    let dossier = repertoire_modeles_locaux(etat.racine_donnees.as_path());
    if dossier.is_dir() {
        if let Ok(restes) = std::fs::read_dir(&dossier) {
            for entree in restes.flatten() {
                let p = entree.path();
                if p.extension().and_then(|s| s.to_str()) == Some("partiel") {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
    }
    drop(_garde_inference);
    etat_modele_local(etat).await
}
