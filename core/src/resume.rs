use crate::config::FormatSortie;
use thiserror::Error;

#[derive(Debug, Clone)]
pub enum SortieResume {
    CarteMentaleMarkdown(String),
}

#[derive(Debug, Error)]
pub enum ErreurSortieResume {
    #[error("sortie IA invalide pour le format {format:?}: {message}")]
    Invalide {
        format: FormatSortie,
        message: String,
    },
}

/// Construit les messages (system + user) pour l’IA.
pub fn construire_messages_resume(
    transcript: &str,
    titre_video: Option<&str>,
    _format: FormatSortie,
) -> (String, String) {
    // Version la plus simple : pas de rôle système, une seule consigne.
    // On garde néanmoins la signature (system, user) pour ne pas refactorer tout l’appelant.
    let system = String::new();

    let titre_bloc = match titre_video {
        Some(t) if !t.trim().is_empty() => format!("Titre de la vidéo: {}\n", t.trim()),
        _ => String::new(),
    };

    let user = format!(
        "Réalise une carte mentale de cette vidéo :\n\n{}Transcript :\n---\n{}\n---",
        titre_bloc,
        transcript.trim(),
    );

    (system, user)
}

/// Messages pour un texte collé manuellement (pas de vidéo YouTube).
pub fn construire_messages_resume_texte_libre(
    texte: &str,
    _format: FormatSortie,
) -> (String, String) {
    let system = String::new();
    let user = format!(
        "Réalise une carte mentale (Markdown) à partir du texte suivant :\n\n---\n{}\n---",
        texte.trim()
    );
    (system, user)
}

fn nettoyer_texte_simple(contenu: &str) -> String {
    // Normalise les retours à la ligne accidentels.
    contenu.trim().replace("\r\n", "\n")
}

fn valider_non_vide(contenu: &str) -> bool {
    !contenu.trim().is_empty()
}

/// Transforme la sortie brute de l’IA en sortie typée (avec validation).
pub fn parser_sortie_ia_vers_sortie(
    format: FormatSortie,
    contenu_ia: &str,
) -> Result<SortieResume, ErreurSortieResume> {
    let contenu = nettoyer_texte_simple(contenu_ia);

    match format {
        FormatSortie::CarteMentale => {
            if !valider_non_vide(&contenu) {
                return Err(ErreurSortieResume::Invalide {
                    format,
                    message: "sortie vide".to_string(),
                });
            }
            Ok(SortieResume::CarteMentaleMarkdown(contenu))
        }
    }
}

/// Prépare une version “string” de la sortie typée.
pub fn formatter_sortie_resume_sortie(format: FormatSortie, sortie: SortieResume) -> String {
    match (format, sortie) {
        (FormatSortie::CarteMentale, SortieResume::CarteMentaleMarkdown(t)) => t,
    }
}
