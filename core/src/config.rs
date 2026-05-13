use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Format de sortie demandé par l’utilisateur.
#[derive(Debug, Clone, Copy, ValueEnum, Serialize, Deserialize)]
pub enum FormatSortie {
    /// Carte mentale (en Markdown).
    #[serde(rename = "carte_mentale")]
    #[value(name = "carte_mentale")]
    CarteMentale,
}

impl FormatSortie {
    pub fn ext_fichier(self) -> &'static str {
        match self {
            FormatSortie::CarteMentale => "md",
        }
    }

    pub fn nom_pour_prompt(self) -> &'static str {
        match self {
            FormatSortie::CarteMentale => "carte_mentale",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigurationIA {
    pub base_url: String,
    pub route: String,
    pub modele: Option<String>,
    pub temperature: f32,
    pub max_caracteres_transcript: usize,
    pub max_caracteres_reponse: usize,
    /// Jeton Bearer (ex. clé API OpenRouter). LM Studio local n’en a pas besoin.
    pub cle_api_bearer: Option<String>,
}
