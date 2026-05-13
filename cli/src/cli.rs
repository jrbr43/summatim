use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "resume-youtube",
    about = "Récupère le transcript de vidéos YouTube et le résume via LM Studio (local)."
)]
pub struct Arguments {
    #[command(subcommand)]
    pub commande: Commande,
}

#[derive(Debug, Subcommand)]
pub enum Commande {
    #[command(name = "resume-channel")]
    ResumeChannel {
        /// URL de la chaîne YouTube (ex: https://www.youtube.com/@nom)
        #[arg(long)]
        channel_url: String,

        /// Nombre de vidéos à traiter (par défaut 10)
        #[arg(long, default_value_t = 10)]
        nb_videos: usize,

        /// Sortie : carte mentale (Markdown)

        /// Base URL LM Studio (ex: http://localhost:1234)
        #[arg(long)]
        lm_base_url: String,

        /// Route LM Studio (OpenAI-compatible chat completions par défaut)
        #[arg(long, default_value = "/v1/chat/completions")]
        lm_route: String,

        /// Modèle LM (optionnel, ex: "local-model-name")
        #[arg(long)]
        lm_modele: Option<String>,

        /// Température (par défaut 0.3)
        #[arg(long, default_value_t = 0.3)]
        temperature: f32,

        /// Dossier de sortie (optionnel). Si fourni, les résumés sont enregistrés par vidéo.
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(name = "resume-video")]
    ResumeVideo {
        /// URL de la vidéo YouTube
        #[arg(long)]
        video_url: String,

        /// Sortie : carte mentale (Markdown)

        /// Base URL LM Studio (ex: http://localhost:1234)
        #[arg(long)]
        lm_base_url: String,

        /// Route LM Studio (OpenAI-compatible chat completions par défaut)
        #[arg(long, default_value = "/v1/chat/completions")]
        lm_route: String,

        /// Modèle LM (optionnel)
        #[arg(long)]
        lm_modele: Option<String>,

        /// Température (par défaut 0.3)
        #[arg(long, default_value_t = 0.3)]
        temperature: f32,

        /// Dossier de sortie (optionnel)
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(name = "test-suite")]
    TestSuite {
        /// URL de la chaîne YouTube (optionnel)
        #[arg(long)]
        channel_url: Option<String>,

        /// URL de la vidéo YouTube (optionnel). Si `channel_url` est absent, il faut fournir `video_url`.
        #[arg(long)]
        video_url: Option<String>,

        /// Base URL LM Studio (optionnel). Si absent, le test exécute seulement la partie transcript.
        #[arg(long)]
        lm_base_url: Option<String>,

        /// Route LM Studio
        #[arg(long, default_value = "/v1/chat/completions")]
        lm_route: String,

        /// Modèle LM (optionnel)
        #[arg(long)]
        lm_modele: Option<String>,

        /// Température (par défaut 0.3)
        #[arg(long, default_value_t = 0.3)]
        temperature: f32,
    },

    #[command(name = "transcript-channel")]
    TranscriptChannel {
        /// URL de la chaîne YouTube
        #[arg(long)]
        channel_url: String,

        /// Nombre de vidéos à traiter (défaut 10)
        #[arg(long, default_value_t = 10)]
        nb_videos: usize,

        /// Dossier de sortie (optionnel). Si fourni, les transcripts sont enregistrés par vidéo.
        #[arg(long)]
        out: Option<PathBuf>,

        /// Limite de taille du transcript en caractères (défaut 25000)
        #[arg(long, default_value_t = 25_000)]
        max_caracteres: usize,
    },

    #[command(name = "transcript-video")]
    TranscriptVideo {
        /// URL de la vidéo YouTube
        #[arg(long)]
        video_url: String,

        /// Dossier de sortie (optionnel)
        #[arg(long)]
        out: Option<PathBuf>,

        /// Limite de taille du transcript en caractères (défaut 25000)
        #[arg(long, default_value_t = 25_000)]
        max_caracteres: usize,
    },

    #[command(name = "liste-videos")]
    ListeVideos {
        /// URL de la chaîne YouTube
        #[arg(long)]
        channel_url: String,

        /// Nombre de vidéos à lister (défaut 10)
        #[arg(long, default_value_t = 10)]
        nb_videos: usize,

        /// Sortie JSON dans un fichier (optionnel)
        #[arg(long)]
        out: Option<PathBuf>,
    },
}
