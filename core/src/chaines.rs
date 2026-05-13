//! Lecture / écriture du fichier `chaines.json` (liste de chaînes YouTube nommées).

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntreeChaine {
    pub nom: String,
    /// Fichier JSON : `url_chaine`. IPC Tauri peut exposer `urlChaine` (camelCase).
    #[serde(alias = "urlChaine")]
    pub url_chaine: String,
}

/// Remonte l’arborescence depuis `depuis` pour trouver un fichier `chaines.json` existant.
pub fn chercher_chaines_json_en_remontant(depuis: &Path) -> Option<PathBuf> {
    let mut repertoire = depuis.to_path_buf();
    for _ in 0..24 {
        let candidat = repertoire.join("chaines.json");
        if candidat.is_file() {
            return Some(candidat);
        }
        if !repertoire.pop() {
            break;
        }
    }
    None
}

/// Chemin utilisé pour lire/écrire `chaines.json` : d’abord la racine du workspace (parent du crate
/// `core`, fixé à la compilation — ne dépend pas du CWD du processus), puis recherche en remontant
/// depuis le répertoire courant.
pub fn resoudre_chemin_fichier_chaines() -> PathBuf {
    if let Some(depuis_workspace) = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("chaines.json"))
    {
        if depuis_workspace.is_file() {
            return depuis_workspace;
        }
    }
    let debut = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Some(trouve) = chercher_chaines_json_en_remontant(&debut) {
        return trouve;
    }
    if debut.file_name().and_then(|s| s.to_str()) == Some("src-tauri") {
        debut
            .parent()
            .unwrap_or(&debut)
            .join("chaines.json")
    } else {
        debut.join("chaines.json")
    }
}

/// Libellé dérivé du handle YouTube après `@` dans le chemin de l’URL (décodage pourcent si besoin).
pub fn nom_depuis_url_chaine(url_brut: &str) -> String {
    extraire_handle_apres_arobase(url_brut.trim()).unwrap_or_else(|| "Chaîne".to_string())
}

fn extraire_handle_apres_arobase(url_brut: &str) -> Option<String> {
    if let Ok(u) = Url::parse(url_brut) {
        let chemin = u.path();
        if let Some(n) = handle_depuis_chemin(chemin) {
            return Some(n);
        }
    }
    let idx = url_brut.find('@')?;
    let apres = &url_brut[idx + 1..];
    let segment = apres
        .split(|c| c == '/' || c == '?' || c == '#' || c == '&')
        .next()?;
    decoder_pourcent_ou_brut(segment)
}

fn handle_depuis_chemin(chemin: &str) -> Option<String> {
    let idx = chemin.find('@')?;
    let apres = &chemin[idx + 1..];
    let segment = apres
        .split(|c| c == '/' || c == '?' || c == '#')
        .next()?;
    decoder_pourcent_ou_brut(segment)
}

fn decoder_pourcent_ou_brut(segment: &str) -> Option<String> {
    let segment = segment.trim();
    if segment.is_empty() {
        return None;
    }
    match percent_decode_str(segment).decode_utf8() {
        Ok(c) => Some(c.to_string()),
        Err(_) => Some(segment.to_string()),
    }
}

/// Lit le fichier JSON des chaînes. Retourne une liste vide si le fichier n’existe pas.
pub fn lire_chaines(chemin: &Path) -> anyhow::Result<Vec<EntreeChaine>> {
    if !chemin.exists() {
        return Ok(Vec::new());
    }
    let contenu = std::fs::read_to_string(chemin)?;
    let liste: Vec<EntreeChaine> = serde_json::from_str(&contenu)?;
    Ok(liste)
}

pub fn ecrire_chaines(chemin: &Path, chaines: &[EntreeChaine]) -> anyhow::Result<()> {
    if let Some(parent) = chemin.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(chaines)?;
    std::fs::write(chemin, json)?;
    Ok(())
}

/// Garde la première entrée pour chaque `url_chaine` (clé stable pour l’UI, évite les doublons fichier).
pub fn dedoublonner_par_url(mut liste: Vec<EntreeChaine>) -> Vec<EntreeChaine> {
    use std::collections::HashSet;
    let mut vu: HashSet<String> = HashSet::new();
    liste.retain(|e| vu.insert(e.url_chaine.trim().to_string()));
    liste
}

pub fn ajouter_chaine(chemin: &Path, nom: String, url_chaine: String) -> anyhow::Result<()> {
    let mut liste = lire_chaines(chemin)?;
    liste.push(EntreeChaine { nom, url_chaine });
    ecrire_chaines(chemin, &liste)
}

/// Ajoute une chaîne en dérivant le nom affiché depuis l’URL (handle après `@`).
pub fn ajouter_chaine_depuis_url(chemin: &Path, url_chaine: String) -> anyhow::Result<()> {
    let nom = nom_depuis_url_chaine(&url_chaine);
    ajouter_chaine(chemin, nom, url_chaine)
}

/// Supprime toutes les entrées dont l'URL de chaîne correspond exactement (après trim).
/// Retourne le nombre d'entrées retirées.
pub fn supprimer_chaine_par_url(chemin: &Path, url_chaine: &str) -> anyhow::Result<usize> {
    let cible = url_chaine.trim();
    if cible.is_empty() {
        return Ok(0);
    }
    let mut liste = lire_chaines(chemin)?;
    let taille_avant = liste.len();
    liste.retain(|entree| entree.url_chaine.trim() != cible);
    let supprimees = taille_avant.saturating_sub(liste.len());
    if supprimees > 0 {
        ecrire_chaines(chemin, &liste)?;
    }
    Ok(supprimees)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nom_handle_youtube_standard() {
        assert_eq!(
            nom_depuis_url_chaine("https://www.youtube.com/@VisionIA-FR"),
            "VisionIA-FR"
        );
    }

    #[test]
    fn nom_avec_chemin_videos() {
        assert_eq!(
            nom_depuis_url_chaine("https://www.youtube.com/@Chaine/videos"),
            "Chaine"
        );
    }

    #[test]
    fn recherche_chaines_json_en_profondeur() {
        let racine = tempfile::tempdir().expect("tmp");
        let _ = std::fs::write(racine.path().join("chaines.json"), "[]");
        let profond = racine.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&profond).expect("mkdir");
        let trouve = chercher_chaines_json_en_remontant(&profond).expect("fichier");
        assert_eq!(trouve, racine.path().join("chaines.json"));
    }

    #[test]
    fn dedoublonner_garde_premiere_entree() {
        let liste = vec![
            EntreeChaine {
                nom: "A".into(),
                url_chaine: "https://youtube.com/@x".into(),
            },
            EntreeChaine {
                nom: "B".into(),
                url_chaine: "https://youtube.com/@x".into(),
            },
        ];
        let d = dedoublonner_par_url(liste);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].nom, "A");
    }
}
