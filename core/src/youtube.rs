use crate::transcript::{parser_json3_vers_texte, parser_srt_vers_texte, parser_vtt_vers_texte};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use tempfile::tempdir;
use wait_timeout::ChildExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfo {
    pub id: String,
    pub titre: String,
    pub url: String,
    /// Date de publication brute telle que renvoyée par yt-dlp (ex: "20240115").
    /// Optionnelle pour rester tolérant selon les formats de sortie.
    #[serde(default)]
    pub date_publication: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ErreurYoutube {
    #[error("binaire externe `yt-dlp` introuvable ou inexécutable: {message}")]
    YtDlpIntrouvable { message: String },

    #[error("yt-dlp a échoué: {message}\nCommande: {commande}\nStderr: {stderr_tronque}")]
    EchecYtDlp {
        message: String,
        commande: String,
        stderr_tronque: String,
    },

    #[error("impossible d’extraire la liste de vidéos (yt-dlp a renvoyé zéro entrée)")]
    ListeVide,

    #[error("{0}")]
    TranscriptIntrouvable(String),
}

#[derive(Debug, Deserialize)]
struct EntreeDump {
    id: Option<String>,
    title: Option<String>,
    webpage_url: Option<String>,
    #[serde(default)]
    url: Option<Value>,
    /// Date de publication au format AAAAMMJJ (champ `upload_date` de yt-dlp).
    #[serde(default)]
    upload_date: Option<String>,
}

fn valeur_json_en_chaine(valeur: Option<Value>) -> Option<String> {
    let valeur = valeur?;
    match valeur {
        Value::String(s) if !s.trim().is_empty() => Some(s),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct SortiePlaylistDump {
    entries: Option<Vec<Option<EntreeDump>>>,
}

/// Binaire `yt-dlp` ou lancement via `interpreteur -m yt_dlp` (installations `pip install --user`).
#[derive(Clone, Debug)]
enum LancementYtDlp {
    Binaire(PathBuf),
    ModulePython { interpreteur: PathBuf },
}

impl LancementYtDlp {
    fn prefixe_debug(&self) -> String {
        match self {
            LancementYtDlp::Binaire(p) => p.display().to_string(),
            LancementYtDlp::ModulePython { interpreteur } => {
                format!("{} -m yt_dlp", interpreteur.display())
            }
        }
    }

    fn construire_commande_debug(&self, args: &[&str]) -> String {
        let mut resultat = self.prefixe_debug();
        for a in args {
            let a_escaped = if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.to_string()
            };
            resultat.push(' ');
            resultat.push_str(&a_escaped);
        }
        resultat
    }
}

fn construire_commande_yt_dlp(lanceur: &LancementYtDlp) -> Command {
    let mut commande = match lanceur {
        LancementYtDlp::Binaire(p) => Command::new(p),
        LancementYtDlp::ModulePython { interpreteur } => {
            let mut c = Command::new(interpreteur);
            c.arg("-m").arg("yt_dlp");
            c
        }
    };
    // En AppImage, ces variables peuvent pointer vers un Python embarqué incomplet
    // et casser yt-dlp (ModuleNotFoundError: encodings). On les neutralise.
    commande.env_remove("PYTHONHOME");
    commande.env_remove("PYTHONPATH");
    commande
}

/// Préfixe optionnel : `YT_DLP_EXTRACTOR_ARGS_YOUTUBE` (ex. `youtube:player_client=web`) pour contourner
/// certains HTTP 400 côté API iOS/Android. Laisser vide par défaut : un client imposé peut casser `-J` / métadonnées.
/// Forcer l’absence totale d’arguments d’extracteur : `YT_DLP_NO_EXTRACTOR_ARGS=1`.
fn arguments_yt_dlp_effectifs(args: &[&str]) -> Vec<String> {
    let mut v = Vec::new();
    if env::var("YT_DLP_NO_EXTRACTOR_ARGS").ok().as_deref() != Some("1") {
        if let Ok(s) = env::var("YT_DLP_EXTRACTOR_ARGS_YOUTUBE") {
            let t = s.trim();
            if !t.is_empty() {
                v.push("--extractor-args".into());
                v.push(t.to_string());
            }
        }
    }
    v.extend(args.iter().map(|s| (*s).to_string()));
    v
}

fn references_arguments_yt_dlp(effectifs: &[String]) -> Vec<&str> {
    effectifs.iter().map(|s| s.as_str()).collect()
}

fn executer_yt_dlp_avec_timeout(
    lanceur: &LancementYtDlp,
    args_effectifs_refs: &[&str],
    delai_max: Duration,
    commande_debug: &str,
) -> Result<Output, ErreurYoutube> {
    let dossier_temp_sorties = tempdir().map_err(|e| ErreurYoutube::YtDlpIntrouvable {
        message: e.to_string(),
    })?;
    let chemin_stdout = dossier_temp_sorties.path().join("yt_dlp_stdout.log");
    let chemin_stderr = dossier_temp_sorties.path().join("yt_dlp_stderr.log");
    let fichier_stdout = File::create(&chemin_stdout).map_err(|e| ErreurYoutube::YtDlpIntrouvable {
        message: e.to_string(),
    })?;
    let fichier_stderr = File::create(&chemin_stderr).map_err(|e| ErreurYoutube::YtDlpIntrouvable {
        message: e.to_string(),
    })?;

    let mut commande = construire_commande_yt_dlp(lanceur);
    commande
        .args(args_effectifs_refs)
        .stdout(fichier_stdout)
        .stderr(fichier_stderr);
    let mut enfant = commande
        .spawn()
        .map_err(|e| ErreurYoutube::YtDlpIntrouvable {
            message: e.to_string(),
        })?;

    match enfant
        .wait_timeout(delai_max)
        .map_err(|e| ErreurYoutube::YtDlpIntrouvable {
            message: e.to_string(),
        })? {
        Some(status) => {
            let stdout = fs::read(&chemin_stdout).unwrap_or_default();
            let stderr = fs::read(&chemin_stderr).unwrap_or_default();
            Ok(Output {
                status,
                stdout,
                stderr,
            })
        }
        None => {
            let _ = enfant.kill();
            let _ = enfant.wait();
            let stderr_contenu = fs::read(&chemin_stderr).unwrap_or_default();
            let stderr_tronque = String::from_utf8_lossy(&stderr_contenu)
                .chars()
                .take(1500)
                .collect();
            Err(ErreurYoutube::EchecYtDlp {
                message: format!(
                    "yt-dlp a dépassé le délai maximal ({}s)",
                    delai_max.as_secs()
                ),
                commande: commande_debug.to_string(),
                stderr_tronque,
            })
        }
    }
}

fn resoudre_executable_depuis_nom(nom: &str) -> Option<PathBuf> {
    let p = Path::new(nom);
    if p.is_absolute() || nom.contains('/') || nom.contains('\\') {
        return if p.is_file() {
            Some(p.to_path_buf())
        } else {
            None
        };
    }
    if let Some(path_var) = env::var_os("PATH") {
        for dossier in env::split_paths(&path_var) {
            let candidat = dossier.join(nom);
            if candidat.is_file() {
                return Some(candidat);
            }
            #[cfg(windows)]
            {
                let avec_exe = dossier.join(format!("{nom}.exe"));
                if avec_exe.is_file() {
                    return Some(avec_exe);
                }
            }
        }
    }
    None
}

fn interpreteur_supporte_module_yt_dlp(interpreteur: &Path) -> bool {
    Command::new(interpreteur)
        .args(["-m", "yt_dlp", "--version"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn candidats_interpreteurs_python() -> Vec<PathBuf> {
    let mut noms = Vec::new();
    if let Ok(v) = env::var("PYTHON") {
        let t = v.trim();
        if !t.is_empty() {
            noms.push(t.to_string());
        }
    }
    if let Ok(v) = env::var("PYTHON3") {
        let t = v.trim();
        if !t.is_empty() {
            noms.push(t.to_string());
        }
    }
    noms.push("python3".into());
    noms.push("python".into());
    let mut vu = HashSet::new();
    let mut sortie = Vec::new();
    for n in noms {
        if let Some(p) = resoudre_executable_depuis_nom(&n) {
            if vu.insert(p.clone()) {
                sortie.push(p);
            }
        }
    }
    sortie
}

fn trouver_lancement_yt_dlp() -> Result<LancementYtDlp, ErreurYoutube> {
    if let Some(chemin_env) = env::var_os("YT_DLP_CHEMIN").or_else(|| env::var_os("YTDLP_CHEMIN")) {
        let p = PathBuf::from(chemin_env);
        if p.exists() {
            return Ok(LancementYtDlp::Binaire(p));
        }
    }

    // Installations utilisateur courantes (pip install --user, etc.) souvent absentes du PATH
    // des lanceurs .desktop / AppImage.
    if let Ok(home) = env::var("HOME") {
        let home_path = Path::new(&home);
        for nom in ["yt-dlp", "yt-dlp.exe"] {
            let candidat = home_path.join(".local/bin").join(nom);
            if candidat.exists() {
                return Ok(LancementYtDlp::Binaire(candidat));
            }
        }
    }

    // 0) Priorité à un binaire local au projet/application (utile pour contourner les versions apt trop anciennes).
    let mut candidats_locaux: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        for dossier in cwd.ancestors() {
            candidats_locaux.push(dossier.join("bin").join("yt-dlp"));
            candidats_locaux.push(dossier.join("bin").join("yt-dlp.exe"));
        }
    }
    if let Ok(exe_courant) = env::current_exe() {
        if let Some(dossier_exe) = exe_courant.parent() {
            for dossier in dossier_exe.ancestors() {
                candidats_locaux.push(dossier.join("yt-dlp"));
                candidats_locaux.push(dossier.join("yt-dlp.exe"));
                candidats_locaux.push(dossier.join("bin").join("yt-dlp"));
                candidats_locaux.push(dossier.join("bin").join("yt-dlp.exe"));
            }
        }
    }
    for candidat in candidats_locaux {
        if candidat.exists() {
            return Ok(LancementYtDlp::Binaire(candidat));
        }
    }

    // 1) Chercher dans PATH
    if let Some(path_var) = env::var_os("PATH") {
        for dossier in env::split_paths(&path_var) {
            let exe = dossier.join("yt-dlp.exe");
            if exe.exists() {
                return Ok(LancementYtDlp::Binaire(exe));
            }
            let non_exe = dossier.join("yt-dlp");
            if non_exe.exists() {
                return Ok(LancementYtDlp::Binaire(non_exe));
            }
        }
    }

    // 2) Chercher dans %APPDATA%/Python/*/Scripts/
    if let Some(appdata) = env::var_os("APPDATA") {
        let base = PathBuf::from(appdata).join("Python");
        if let Ok(entries) = fs::read_dir(&base) {
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_dir() {
                    continue;
                }
                let scripts = p.join("Scripts");
                let exe = scripts.join("yt-dlp.exe");
                if exe.exists() {
                    return Ok(LancementYtDlp::Binaire(exe));
                }
                let non_exe = scripts.join("yt-dlp");
                if non_exe.exists() {
                    return Ok(LancementYtDlp::Binaire(non_exe));
                }
            }
        }
    }

    // 3) pip install sans script utilisable : `python3 -m yt_dlp` (même interpréteur que pip).
    for interpreteur in candidats_interpreteurs_python() {
        if interpreteur_supporte_module_yt_dlp(&interpreteur) {
            return Ok(LancementYtDlp::ModulePython { interpreteur });
        }
    }

    Err(ErreurYoutube::YtDlpIntrouvable {
        message: "yt-dlp introuvable (binaire) et `python3 -m yt_dlp` indisponible. Installe avec `python3 -m pip install --user -U yt-dlp`, vérifie `python3 -m yt_dlp --version`, ou définis YT_DLP_CHEMIN vers le script yt-dlp (ex. ~/.local/bin/yt-dlp). Sous Linux, ajoute ~/.local/bin au PATH du bureau ou exporte PYTHON=/chemin/vers/python."
            .to_string(),
    })
}

fn extraire_id_depuis_url(url: &str) -> Option<String> {
    // Cas simple: url avec ?v=...
    if let Some(pos) = url.find("v=") {
        let apres = &url[(pos + 2)..];
        let cle = apres.split('&').next().unwrap_or_default();
        if !cle.trim().is_empty() {
            return Some(cle.trim().to_string());
        }
    }

    // Cas /shorts/{id} ou /{id}
    let segments: Vec<&str> = url.split('/').filter(|s| !s.trim().is_empty()).collect();
    for i in 0..segments.len().saturating_sub(1) {
        if segments[i].eq_ignore_ascii_case("shorts") {
            return Some(segments[i + 1].to_string());
        }
    }

    // Fallback: tenter de récupérer dernier segment.
    segments
        .last()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn construire_url_video(id: &str) -> Option<String> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    Some(format!("https://www.youtube.com/watch?v={id}"))
}

fn selectionner_entrees_depuis_dump(entrees: Vec<EntreeDump>) -> Vec<VideoInfo> {
    let mut resultat: Vec<VideoInfo> = Vec::new();

    for e in entrees {
        let titre = e.title.unwrap_or_else(|| "Titre inconnu".to_string());
        let url_brute = e
            .webpage_url
            .or_else(|| valeur_json_en_chaine(e.url))
            .unwrap_or_default();
        let id = e.id.or_else(|| extraire_id_depuis_url(&url_brute)).unwrap_or_default();
        let url = if url_brute.trim().is_empty() {
            construire_url_video(&id).unwrap_or_default()
        } else if url_brute.starts_with("http://") || url_brute.starts_with("https://") {
            url_brute
        } else if let Some(id_depuis_url) = extraire_id_depuis_url(&url_brute) {
            construire_url_video(&id_depuis_url).unwrap_or(url_brute)
        } else {
            construire_url_video(&id).unwrap_or(url_brute)
        };

        if url.trim().is_empty() || id.trim().is_empty() {
            continue;
        }

        resultat.push(VideoInfo {
            id,
            titre,
            url,
            date_publication: e.upload_date,
        });
    }

    resultat
}

fn lancer_yt_dlp_json_sortie_ligne_par_ligne(
    args: &[&str],
) -> Result<Vec<EntreeDump>, ErreurYoutube> {
    let lanceur = trouver_lancement_yt_dlp()?;
    let args_effectifs = arguments_yt_dlp_effectifs(args);
    let refs = references_arguments_yt_dlp(&args_effectifs);
    let commande_debug = lanceur.construire_commande_debug(&refs);
    let sortie = executer_yt_dlp_avec_timeout(
        &lanceur,
        &refs,
        Duration::from_secs(90),
        &commande_debug,
    )?;

    let stdout = String::from_utf8_lossy(&sortie.stdout);
    let stderr = String::from_utf8_lossy(&sortie.stderr);
    let flux_combine = format!("{stdout}\n{stderr}");

    let mut entrees: Vec<EntreeDump> = Vec::new();
    for ligne in flux_combine.lines() {
        let l = ligne.trim();
        if !l.starts_with('{') {
            continue;
        }
        if let Ok(e) = serde_json::from_str::<EntreeDump>(l) {
            entrees.push(e);
            continue;
        }
        if let Ok(sortie_playlist) = serde_json::from_str::<SortiePlaylistDump>(l) {
            if let Some(elements) = sortie_playlist.entries {
                entrees.extend(elements.into_iter().flatten());
            }
        }
    }

    // Selon la version/plateforme, yt-dlp peut sortir un unique JSON au lieu d'une ligne par vidéo.
    if entrees.is_empty() {
        let brut = flux_combine.trim();
        if let Ok(e) = serde_json::from_str::<EntreeDump>(brut) {
            entrees.push(e);
        } else if let Ok(sortie_playlist) = serde_json::from_str::<SortiePlaylistDump>(brut) {
            if let Some(elements) = sortie_playlist.entries {
                entrees.extend(elements.into_iter().flatten());
            }
        }
    }

    if !entrees.is_empty() {
        return Ok(entrees);
    }

    if !sortie.status.success() {
        let stderr = String::from_utf8_lossy(&sortie.stderr).to_string();
        return Err(ErreurYoutube::EchecYtDlp {
            message: "échec d’exécution de yt-dlp".to_string(),
            commande: commande_debug,
            stderr_tronque: stderr.chars().take(1500).collect(),
        });
    }

    Ok(Vec::new())
}

fn lancer_yt_dlp_json_sortie_unique(args: &[&str]) -> Result<SortiePlaylistDump, ErreurYoutube> {
    let lanceur = trouver_lancement_yt_dlp()?;
    let args_effectifs = arguments_yt_dlp_effectifs(args);
    let refs = references_arguments_yt_dlp(&args_effectifs);
    let commande_debug = lanceur.construire_commande_debug(&refs);
    let sortie = executer_yt_dlp_avec_timeout(
        &lanceur,
        &refs,
        Duration::from_secs(90),
        &commande_debug,
    )?;

    let stdout = String::from_utf8_lossy(&sortie.stdout);
    if let Ok(sortie_playlist) = serde_json::from_str::<SortiePlaylistDump>(&stdout) {
        return Ok(sortie_playlist);
    }

    let stderr = String::from_utf8_lossy(&sortie.stderr);
    if let Ok(sortie_playlist) = serde_json::from_str::<SortiePlaylistDump>(&stderr) {
        return Ok(sortie_playlist);
    }

    let flux_combine = format!("{stdout}\n{stderr}");
    if let Ok(sortie_playlist) = serde_json::from_str::<SortiePlaylistDump>(&flux_combine) {
        return Ok(sortie_playlist);
    }

    if !sortie.status.success() {
        let stderr = String::from_utf8_lossy(&sortie.stderr).to_string();
        return Err(ErreurYoutube::EchecYtDlp {
            message: "échec d’exécution de yt-dlp".to_string(),
            commande: commande_debug,
            stderr_tronque: stderr.chars().take(1500).collect(),
        });
    }

    Err(ErreurYoutube::EchecYtDlp {
        message: "parsing JSON de liste invalide".to_string(),
        commande: commande_debug,
        stderr_tronque: String::new(),
    })
}

/// Récupère les `nb_videos` dernières vidéos d’une chaîne YouTube.
pub fn lister_dernieres_videos(
    channel_url: &str,
    nb_videos: usize,
) -> Result<Vec<VideoInfo>, ErreurYoutube> {
    let nb = nb_videos.max(1);
    let candidats: Vec<String> = {
        let c = channel_url.trim().to_string();
        if c.ends_with("/videos") {
            vec![c]
        } else {
            vec![c.clone(), format!("{}/videos", c.trim_end_matches('/'))]
        }
    };

    // Tentative A: `--dump-json` (entrées ligne par ligne) avec métadonnées complètes (dont `upload_date`).
    for url in candidats.iter() {
        let args = [
            "--no-warnings",
            "--dump-json",
            "--playlist-items",
            Box::leak(format!("1-{}", nb).into_boxed_str()),
            url.as_str(),
        ];

        let parse = lancer_yt_dlp_json_sortie_ligne_par_ligne(&args);
        if let Ok(entrees) = parse {
            let resultat = selectionner_entrees_depuis_dump(entrees)
                .into_iter()
                .take(nb)
                .collect::<Vec<_>>();
            if !resultat.is_empty() {
                return Ok(resultat);
            }
        }
    }

    // Tentative B: JSON unique (`-J`) avec métadonnées complètes (dont `upload_date`).
    for url in candidats.iter() {
        let args = [
            "--no-warnings",
            "-J",
            "--playlist-items",
            Box::leak(format!("1-{}", nb).into_boxed_str()),
            url.as_str(),
        ];

        let parse = lancer_yt_dlp_json_sortie_unique(&args);
        if let Ok(sortie) = parse {
            let mut entrees_flat: Vec<EntreeDump> = Vec::new();
            if let Some(entries) = sortie.entries {
                for e in entries.into_iter().flatten() {
                    entrees_flat.push(e);
                }
            }

            let resultat = selectionner_entrees_depuis_dump(entrees_flat)
                .into_iter()
                .take(nb)
                .collect::<Vec<_>>();
            if !resultat.is_empty() {
                return Ok(resultat);
            }
        }
    }

    Err(ErreurYoutube::ListeVide)
}

/// Métadonnées d’une vidéo depuis une URL YouTube (pas une chaîne / playlist).
pub fn infos_video_depuis_url(url: &str) -> Result<VideoInfo, ErreurYoutube> {
    let url = url.trim();
    if url.is_empty() {
        return Err(ErreurYoutube::ListeVide);
    }

    if let Ok(entrees) =
        lancer_yt_dlp_json_sortie_ligne_par_ligne(&[
            "--no-warnings",
            "--dump-json",
            "--no-playlist",
            url,
        ])
    {
        let v = selectionner_entrees_depuis_dump(entrees);
        if let Some(info) = v.into_iter().next() {
            return Ok(info);
        }
    }

    let lanceur = trouver_lancement_yt_dlp()?;
    let args_meta = ["--no-warnings", "-J", "--no-playlist", url];
    let args_effectifs = arguments_yt_dlp_effectifs(&args_meta);
    let refs = references_arguments_yt_dlp(&args_effectifs);
    let commande_debug = lanceur.construire_commande_debug(&refs);
    let sortie = executer_yt_dlp_avec_timeout(
        &lanceur,
        &refs,
        Duration::from_secs(90),
        &commande_debug,
    )?;

    let stdout = String::from_utf8_lossy(&sortie.stdout);
    let stderr = String::from_utf8_lossy(&sortie.stderr);
    let brut = stdout.trim();
    let brut_stderr = stderr.trim();
    let brut_combine = format!("{brut}\n{brut_stderr}");
    if brut.is_empty() {
        if !sortie.status.success() && brut_stderr.is_empty() {
            let stderr = String::from_utf8_lossy(&sortie.stderr).to_string();
            return Err(ErreurYoutube::EchecYtDlp {
                message: "échec yt-dlp (-J) pour cette URL".to_string(),
                commande: commande_debug,
                stderr_tronque: stderr.chars().take(1500).collect(),
            });
        }
        return Err(ErreurYoutube::ListeVide);
    }

    if let Ok(e) = serde_json::from_str::<EntreeDump>(brut) {
        let v = selectionner_entrees_depuis_dump(vec![e]);
        if let Some(info) = v.into_iter().next() {
            return Ok(info);
        }
    }

    if let Ok(e) = serde_json::from_str::<EntreeDump>(brut_stderr) {
        let v = selectionner_entrees_depuis_dump(vec![e]);
        if let Some(info) = v.into_iter().next() {
            return Ok(info);
        }
    }

    if let Ok(e) = serde_json::from_str::<EntreeDump>(&brut_combine) {
        let v = selectionner_entrees_depuis_dump(vec![e]);
        if let Some(info) = v.into_iter().next() {
            return Ok(info);
        }
    }

    if let Ok(playlist) = serde_json::from_str::<SortiePlaylistDump>(brut) {
        let mut plat: Vec<EntreeDump> = Vec::new();
        if let Some(entries) = playlist.entries {
            for opt in entries.into_iter().flatten() {
                plat.push(opt);
            }
        }
        let v = selectionner_entrees_depuis_dump(plat);
        if let Some(info) = v.into_iter().next() {
            return Ok(info);
        }
    }

    if let Ok(playlist) = serde_json::from_str::<SortiePlaylistDump>(brut_stderr) {
        let mut plat: Vec<EntreeDump> = Vec::new();
        if let Some(entries) = playlist.entries {
            for opt in entries.into_iter().flatten() {
                plat.push(opt);
            }
        }
        let v = selectionner_entrees_depuis_dump(plat);
        if let Some(info) = v.into_iter().next() {
            return Ok(info);
        }
    }

    if let Ok(playlist) = serde_json::from_str::<SortiePlaylistDump>(&brut_combine) {
        let mut plat: Vec<EntreeDump> = Vec::new();
        if let Some(entries) = playlist.entries {
            for opt in entries.into_iter().flatten() {
                plat.push(opt);
            }
        }
        let v = selectionner_entrees_depuis_dump(plat);
        if let Some(info) = v.into_iter().next() {
            return Ok(info);
        }
    }

    if !sortie.status.success() {
        let stderr = String::from_utf8_lossy(&sortie.stderr).to_string();
        return Err(ErreurYoutube::EchecYtDlp {
            message: "échec yt-dlp (-J) pour cette URL".to_string(),
            commande: commande_debug,
            stderr_tronque: stderr.chars().take(1500).collect(),
        });
    }

    Err(ErreurYoutube::ListeVide)
}

fn lister_fichiers_par_extension(
    dossier: &Path,
    extension: &str,
) -> Result<Vec<PathBuf>, ErreurYoutube> {
    let mut resultat = Vec::new();
    let entree = fs::read_dir(dossier).map_err(|e| ErreurYoutube::EchecYtDlp {
        message: format!("lecture du dossier temporaire impossible: {e}"),
        commande: "".to_string(),
        stderr_tronque: "".to_string(),
    })?;

    let extension_basse = extension.to_ascii_lowercase();

    for item in entree {
        let p = item
            .map_err(|e| ErreurYoutube::EchecYtDlp {
                message: format!("itération dossier temporaire impossible: {e}"),
                commande: "".to_string(),
                stderr_tronque: "".to_string(),
            })?
            .path();
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if ext == extension_basse {
            resultat.push(p);
        }
    }

    Ok(resultat)
}

fn selectionner_meilleur_transcript<F>(
    fichiers: &[PathBuf],
    max_caracteres_transcript: usize,
    parser: F,
) -> Option<String>
where
    F: Fn(&str) -> String,
{
    let mut meilleur: Option<String> = None;
    let mut meilleur_len: usize = 0;

    for p in fichiers {
        let contenu = match fs::read_to_string(p) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let texte = parser(&contenu);
        let tronque = texte
            .chars()
            .take(max_caracteres_transcript)
            .collect::<String>();
        if tronque.trim().is_empty() {
            continue;
        }

        let len = tronque.chars().count();
        if len > meilleur_len {
            meilleur_len = len;
            meilleur = Some(tronque);
        }
    }

    meilleur
}

/// 0 = ne pas passer `--sleep-subtitles` (plus rapide). Sinon 1–30 s entre sous-titres côté yt-dlp.
fn delai_sleep_sous_titres_secondes() -> u64 {
    env::var("YT_DLP_SLEEP_SOUS_TITRES_SEC")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| (0..=30).contains(&n))
        .unwrap_or(0)
}

fn lancer_yt_dlp_extraire_sous_titres(
    video_url: &str,
    dossier_temp: &Path,
    format_souhait: &str,
    langues: &str,
) -> Result<(), ErreurYoutube> {
    let output_pattern = format!("{}/%(id)s.%(ext)s", dossier_temp.display());
    let output_pattern_static = Box::leak(output_pattern.into_boxed_str());
    let pause_sous_titres = delai_sleep_sous_titres_secondes();

    let mut args_brutes: Vec<String> = vec!["--no-playlist".into()];
    if pause_sous_titres > 0 {
        args_brutes.push("--sleep-subtitles".into());
        args_brutes.push(pause_sous_titres.to_string());
    }
    args_brutes.extend([
        "--skip-download".into(),
        "--write-auto-subs".into(),
        "--write-subs".into(),
        "--sub-langs".into(),
        langues.to_string(),
        "--sub-format".into(),
        format_souhait.to_string(),
        "--output".into(),
        (*output_pattern_static).to_string(),
        video_url.to_string(),
    ]);
    let args_refs_brutes: Vec<&str> = args_brutes.iter().map(|s| s.as_str()).collect();

    let lanceur = trouver_lancement_yt_dlp()?;
    let args_effectifs = arguments_yt_dlp_effectifs(&args_refs_brutes);
    let refs = references_arguments_yt_dlp(&args_effectifs);
    let commande_debug = lanceur.construire_commande_debug(&refs);
    let mut derniere_erreur: Option<ErreurYoutube> = None;

    // Pauses sur 429 (YouTube) — moins de tentatives qu’avant pour ne pas bloquer des minutes.
    const PAUSES_429_SEC: [u64; 5] = [5, 14, 32, 60, 95];

    for tentative in 0..PAUSES_429_SEC.len() {
        let sortie = executer_yt_dlp_avec_timeout(
            &lanceur,
            &refs,
            Duration::from_secs(240),
            &commande_debug,
        );

        match sortie {
            Ok(s) if s.status.success() => return Ok(()),
            Ok(s) => {
                let stderr = String::from_utf8_lossy(&s.stderr).to_string();
                let est_429 = stderr.contains("429") || stderr.contains("Too Many Requests");
                derniere_erreur = Some(ErreurYoutube::EchecYtDlp {
                    message: format!("échec extraction sous-titres (tentative {})", tentative + 1),
                    commande: commande_debug.clone(),
                    stderr_tronque: stderr.chars().take(1500).collect(),
                });

                if est_429 && tentative + 1 < PAUSES_429_SEC.len() {
                    std::thread::sleep(Duration::from_secs(PAUSES_429_SEC[tentative]));
                    continue;
                }
            }
            Err(e) => {
                derniere_erreur = Some(e);
            }
        }
        break;
    }

    Err(derniere_erreur.unwrap_or_else(|| {
        ErreurYoutube::TranscriptIntrouvable(
            "yt-dlp n’a pas pu écrire de fichier de sous-titres (réponse vide ou erreur non détaillée)."
                .into(),
        )
    }))
}

fn lister_langues_auto_captions(video_url: &str) -> Vec<String> {
    let lanceur = match trouver_lancement_yt_dlp() {
        Ok(l) => l,
        Err(_) => return Vec::new(),
    };

    let args_liste: [&str; 4] = ["--no-playlist", "--skip-download", "--list-subs", video_url];
    let args_effectifs = arguments_yt_dlp_effectifs(&args_liste);
    let refs = references_arguments_yt_dlp(&args_effectifs);
    let commande_debug = lanceur.construire_commande_debug(&refs);
    let sortie = executer_yt_dlp_avec_timeout(
        &lanceur,
        &refs,
        Duration::from_secs(45),
        &commande_debug,
    );

    let Ok(sortie) = sortie else {
        return Vec::new();
    };

    let brut = {
        let mut s = String::new();
        s.push_str(&String::from_utf8_lossy(&sortie.stdout));
        s.push('\n');
        s.push_str(&String::from_utf8_lossy(&sortie.stderr));
        s
    };

    parser_codes_langues_depuis_sortie_liste_sous_titres(&brut)
}

fn est_en_tete_fausse_langue_liste_sous_titres(code: &str) -> bool {
    matches!(
        code,
        "Language" | "Name" | "Formats" | "Automatic" | "Subtitles" | "Available"
    ) || code.eq_ignore_ascii_case("info")
}

/// Détecte les codes langue dans la sortie de `--list-subs` : **sous-titres manuels d’abord**, puis auto-générés.
fn parser_codes_langues_depuis_sortie_liste_sous_titres(brut: &str) -> Vec<String> {
    enum SectionListeSousTitres {
        Aucune,
        Manuel,
        Automatique,
    }

    let re_code = Regex::new(r"^\s*([A-Za-z0-9][A-Za-z0-9.-]{0,39})\s+")
        .expect("regex codes langue list-subs");

    let mut section = SectionListeSousTitres::Aucune;
    let mut manuel: Vec<String> = Vec::new();
    let mut automatique: Vec<String> = Vec::new();
    let mut vu_manuel: HashSet<String> = HashSet::new();
    let mut vu_auto: HashSet<String> = HashSet::new();

    for ligne in brut.lines() {
        if ligne.contains("[info]") && ligne.contains("Available subtitles for") {
            section = SectionListeSousTitres::Manuel;
            continue;
        }
        if ligne.contains("[info]") && ligne.contains("Available automatic captions") {
            section = SectionListeSousTitres::Automatique;
            continue;
        }

        let l = ligne.trim_end();
        if !l.contains("vtt") && !l.contains("json3") && !l.contains("srv") {
            continue;
        }
        let debut = l.trim_start();
        let Some(cap) = re_code.captures(debut) else {
            continue;
        };
        let Some(m) = cap.get(1) else {
            continue;
        };
        let c = m.as_str();
        if c.len() < 2 || est_en_tete_fausse_langue_liste_sous_titres(c) {
            continue;
        }
        let code = c.to_string();
        match section {
            SectionListeSousTitres::Manuel => {
                if vu_manuel.insert(code.clone()) {
                    manuel.push(code);
                }
            }
            SectionListeSousTitres::Automatique => {
                if vu_auto.insert(code.clone()) {
                    automatique.push(code);
                }
            }
            SectionListeSousTitres::Aucune => {}
        }
    }

    let mut resultat = manuel;
    for c in automatique {
        if !resultat.iter().any(|x| x == &c) {
            resultat.push(c);
        }
    }
    if resultat.is_empty() {
        return parser_codes_langues_liste_sous_titres_plat(brut);
    }
    resultat
}

/// Repli si les en-têtes `[info] Available …` ne correspondent pas (locale / version yt-dlp).
fn parser_codes_langues_liste_sous_titres_plat(brut: &str) -> Vec<String> {
    let re_code = Regex::new(r"^\s*([A-Za-z0-9][A-Za-z0-9.-]{0,39})\s+")
        .expect("regex codes langue list-subs");
    let mut sortie: Vec<String> = Vec::new();
    let mut deja: HashSet<String> = HashSet::new();
    for ligne in brut.lines() {
        let l = ligne.trim_end();
        if !l.contains("vtt") && !l.contains("json3") && !l.contains("srv") {
            continue;
        }
        let debut = l.trim_start();
        let Some(cap) = re_code.captures(debut) else {
            continue;
        };
        let Some(m) = cap.get(1) else {
            continue;
        };
        let c = m.as_str();
        if c.len() < 2 || est_en_tete_fausse_langue_liste_sous_titres(c) {
            continue;
        }
        if deja.insert(c.to_string()) {
            sortie.push(c.to_string());
        }
    }
    sortie
}

fn construire_ordre_langues(disponibles: &[String], preferences: &[&str]) -> Vec<String> {
    let mut resultat: Vec<String> = Vec::new();

    // Ajoute d’abord les préférées (en acceptant les variantes `fr-orig`, `en-GB`, etc.).
    for pref in preferences {
        if let Some(matched) = disponibles
            .iter()
            .find(|l| *l == *pref || l.starts_with(&format!("{pref}-")))
        {
            resultat.push(matched.clone());
        }
    }

    // Puis ajoute le reste, dans l’ordre renvoyé par yt-dlp.
    for l in disponibles {
        if !resultat.iter().any(|x| x == l) {
            resultat.push(l.clone());
        }
    }

    resultat
}

/// Extrait et nettoie le transcript d’une vidéo YouTube via `yt-dlp`.
pub fn extraire_transcript(
    video_url: &str,
    max_caracteres_transcript: usize,
) -> Result<String, ErreurYoutube> {
    let disponibles = lister_langues_auto_captions(video_url);
    let preferences = [
        "fr", "en", "es", "de", "it", "pt", "ru", "ja", "nl", "pl", "ko", "zh", "hi", "ar", "tr",
    ];
    let codes_a_tester: Vec<String> = if disponibles.is_empty() {
        preferences.iter().map(|s| (*s).to_string()).collect()
    } else {
        let ordre = construire_ordre_langues(&disponibles, &preferences);
        ordre.into_iter().take(16).collect()
    };

    let mut dernier_echec_yt_dlp: Option<ErreurYoutube> = None;

    fn pause_si_429(erreur: &ErreurYoutube) {
        let msg = erreur.to_string();
        if msg.contains("429") || msg.contains("Too Many Requests") {
            std::thread::sleep(Duration::from_secs(8));
        }
    }

    /// Un seul yt-dlp : plusieurs langues en VTT (réduit les allers-retours YouTube).
    fn tenter_lot_vtt(
        video_url: &str,
        max_caracteres_transcript: usize,
        codes: &[String],
        limite: usize,
    ) -> Result<Option<String>, ErreurYoutube> {
        if codes.is_empty() {
            return Ok(None);
        }
        let lot: Vec<String> = codes.iter().take(limite).cloned().collect();
        if lot.is_empty() {
            return Ok(None);
        }
        let joined = lot.join(",");
        let dossier_temp = tempdir().map_err(|e| ErreurYoutube::EchecYtDlp {
            message: format!("création dossier temp impossible: {e}"),
            commande: "".to_string(),
            stderr_tronque: "".to_string(),
        })?;
        if lancer_yt_dlp_extraire_sous_titres(video_url, dossier_temp.path(), "vtt", &joined).is_ok()
        {
            let fichiers = lister_fichiers_par_extension(dossier_temp.path(), "vtt")?;
            if let Some(t) = selectionner_meilleur_transcript(
                &fichiers,
                max_caracteres_transcript,
                parser_vtt_vers_texte,
            ) {
                return Ok(Some(t));
            }
        }
        Ok(None)
    }

    // 1) Lot VTT : jusqu’à 8 langues prioritaires en une commande.
    if let Some(t) = tenter_lot_vtt(
        video_url,
        max_caracteres_transcript,
        &codes_a_tester,
        8,
    )? {
        return Ok(t);
    }

    // 2) VTT langue par langue (sans pause fixe ; pause seulement si 429).
    for code in codes_a_tester.iter().take(12) {
        let dossier_temp = tempdir().map_err(|e| ErreurYoutube::EchecYtDlp {
            message: format!("création dossier temp impossible: {e}"),
            commande: "".to_string(),
            stderr_tronque: "".to_string(),
        })?;
        if let Err(e) =
            lancer_yt_dlp_extraire_sous_titres(video_url, dossier_temp.path(), "vtt", code)
        {
            pause_si_429(&e);
            dernier_echec_yt_dlp = Some(e);
        }
        let fichiers_vtt = lister_fichiers_par_extension(dossier_temp.path(), "vtt")?;
        if let Some(transcript) = selectionner_meilleur_transcript(
            &fichiers_vtt,
            max_caracteres_transcript,
            parser_vtt_vers_texte,
        ) {
            return Ok(transcript);
        }
    }

    // 3) JSON3 puis SRT sur les 8 premières langues seulement.
    for code in codes_a_tester.iter().take(8) {
        let dossier_temp = tempdir().map_err(|e| ErreurYoutube::EchecYtDlp {
            message: format!("création dossier temp impossible: {e}"),
            commande: "".to_string(),
            stderr_tronque: "".to_string(),
        })?;
        if let Err(e) =
            lancer_yt_dlp_extraire_sous_titres(video_url, dossier_temp.path(), "json3", code)
        {
            pause_si_429(&e);
            dernier_echec_yt_dlp = Some(e);
        }
        let mut fichiers_json3 = lister_fichiers_par_extension(dossier_temp.path(), "json3")?;
        fichiers_json3.extend(lister_fichiers_par_extension(dossier_temp.path(), "json")?);
        if let Some(transcript) = selectionner_meilleur_transcript(
            &fichiers_json3,
            max_caracteres_transcript,
            parser_json3_vers_texte,
        ) {
            return Ok(transcript);
        }

        if let Err(e) =
            lancer_yt_dlp_extraire_sous_titres(video_url, dossier_temp.path(), "srt", code)
        {
            pause_si_429(&e);
            dernier_echec_yt_dlp = Some(e);
        }
        let fichiers_srt = lister_fichiers_par_extension(dossier_temp.path(), "srt")?;
        if let Some(transcript) = selectionner_meilleur_transcript(
            &fichiers_srt,
            max_caracteres_transcript,
            parser_srt_vers_texte,
        ) {
            return Ok(transcript);
        }
    }

    if let Some(e) = dernier_echec_yt_dlp {
        return Err(e);
    }

    Err(ErreurYoutube::TranscriptIntrouvable(
        "Aucun sous-titre exploitable (VTT/SRT/JSON) après plusieurs langues. La vidéo peut n’avoir aucun sous-titre (ni automatique), ou yt-dlp doit être mis à jour (`python3 -m pip install -U yt-dlp`, puis `python3 -m yt_dlp --version`). Variables utiles : YT_DLP_CHEMIN (script yt-dlp), PYTHON (interpréteur pour `python -m yt_dlp`)."
            .into(),
    ))
}

/// Helper: nettoyage “grossier” en cas de transcripts très bruyants.
#[allow(dead_code)]
pub fn nettoyer_transcript_supplementaire(texte: &str) -> String {
    let re_bruit = Regex::new(r"\[.*?\]").expect("regex valide");
    let re_whitespace = Regex::new(r"\s+").expect("regex valide");
    let sans = re_bruit.replace_all(texte, "");
    re_whitespace.replace_all(sans.trim(), " ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_liste_sans_section_info_repli_plat() {
        let brut = "Language Name Formats\nen       English                 vtt, json3\n";
        let codes = parser_codes_langues_depuis_sortie_liste_sous_titres(brut);
        assert_eq!(codes.as_slice(), &["en".to_string()]);
    }

    #[test]
    fn parse_liste_sous_titres_ignore_en_tete_language() {
        let brut = r"[info] Available subtitles for dQw4w9WgXcQ:
Language Name                    Formats
en       English                 vtt, ttml, srv3, json3
de-DE    German (Germany)        vtt, json3
[info] Available automatic captions for dQw4w9WgXcQ:
Language Name                    Formats
ab-en    Abkhazian from English vtt, json3
aa       Afar                  vtt, json3
";
        let codes = parser_codes_langues_depuis_sortie_liste_sous_titres(brut);
        assert_eq!(codes[0], "en");
        assert_eq!(codes[1], "de-DE");
        assert!(codes.contains(&"ab-en".into()));
        assert!(codes.contains(&"aa".into()));
        assert!(!codes.iter().any(|c| c == "Language"));
    }

    #[test]
    fn recuperation_infos_video_reelle_fonctionne() {
        let url_video = "https://www.youtube.com/watch?v=NUWgXaFhYr8";
        let resultat = infos_video_depuis_url(url_video)
            .expect("La récupération des infos vidéo doit fonctionner pour cette URL");

        assert_eq!(resultat.id, "NUWgXaFhYr8");
        assert!(!resultat.titre.trim().is_empty());
        assert!(
            resultat.url.contains("youtube.com/watch?v=NUWgXaFhYr8"),
            "URL inattendue: {}",
            resultat.url
        );
    }
}
