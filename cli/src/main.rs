mod cli;

use clap::Parser;
use reqwest::Client;
use serde::Serialize;

use resume_youtube_core::config::{ConfigurationIA, FormatSortie};
use resume_youtube_core::ia;
use resume_youtube_core::resume;
use resume_youtube_core::youtube;
use resume_youtube_core::youtube::VideoInfo;

use crate::cli::{Arguments, Commande};

const MODELE_PAR_DEFAUT: &str = "google/gemma-4-26b-a4b";

fn tronquer_texte_pour_diag(texte: &str, max_caracteres: usize) -> String {
    texte.chars().take(max_caracteres).collect()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    let client_http = Client::new();

    match arguments.commande {
        Commande::ResumeChannel {
            channel_url,
            nb_videos,
            lm_base_url,
            lm_route,
            lm_modele,
            temperature,
            out,
        } => {
            let config_ia = ConfigurationIA {
                base_url: lm_base_url,
                route: lm_route,
                modele: Some(
                    lm_modele
                        .unwrap_or_else(|| MODELE_PAR_DEFAUT.to_string()),
                ),
                temperature,
                max_caracteres_transcript: 25_000,
                max_caracteres_reponse: 18_000,
                cle_api_bearer: None,
            };

            let videos = youtube::lister_dernieres_videos(&channel_url, nb_videos)?;
            for (index, video) in videos.iter().enumerate() {
                eprintln!(
                    "\n[{}/{}] Vidéo: {} ({})",
                    index + 1,
                    nb_videos,
                    video.titre,
                    video.id
                );

                match traiter_une_video(
                    &client_http,
                    video,
                    FormatSortie::CarteMentale,
                    &config_ia,
                    out.as_deref(),
                )
                    .await
                {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("Erreur: {e}");
                    }
                }
            }
        }
        Commande::ResumeVideo {
            video_url,
            lm_base_url,
            lm_route,
            lm_modele,
            temperature,
            out,
        } => {
            let config_ia = ConfigurationIA {
                base_url: lm_base_url,
                route: lm_route,
                modele: Some(
                    lm_modele
                        .unwrap_or_else(|| MODELE_PAR_DEFAUT.to_string()),
                ),
                temperature,
                max_caracteres_transcript: 25_000,
                max_caracteres_reponse: 18_000,
                cle_api_bearer: None,
            };

            let video = VideoInfo {
                id: "video".to_string(),
                titre: "Vidéo YouTube".to_string(),
                url: video_url,
                date_publication: None,
            };

            traiter_une_video(
                &client_http,
                &video,
                FormatSortie::CarteMentale,
                &config_ia,
                out.as_deref(),
            )
            .await?;
        }
        Commande::TestSuite {
            channel_url,
            video_url,
            lm_base_url,
            lm_route,
            lm_modele,
            temperature,
        } => {
            let (video_source, titre_source) = match (channel_url, video_url) {
                (Some(c), None) => (c, "chaîne".to_string()),
                (None, Some(v)) => (v, "vidéo".to_string()),
                (Some(_), Some(_)) => {
                    anyhow::bail!("Fournis seulement `channel_url` ou `video_url`, pas les deux.");
                }
                (None, None) => {
                    anyhow::bail!("Fournis `--channel-url` ou `--video-url` pour le test-suite.");
                }
            };

            eprintln!("Test-suite démarré (source: {titre_source}).");

            let video_cible: VideoInfo = match titre_source.as_str() {
                "chaîne" => {
                    let videos = youtube::lister_dernieres_videos(&video_source, 10)?;
                    videos
                        .first()
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("Chaîne sans vidéo"))?
                }
                "vidéo" => VideoInfo {
                    id: "video".to_string(),
                    titre: "Vidéo YouTube".to_string(),
                    url: video_source,
                    date_publication: None,
                },
                _ => unreachable!(),
            };

            let config_ia_opt = lm_base_url.map(|b| ConfigurationIA {
                base_url: b,
                route: lm_route,
                modele: Some(
                    lm_modele
                        .unwrap_or_else(|| MODELE_PAR_DEFAUT.to_string()),
                ),
                temperature,
                max_caracteres_transcript: 25_000,
                max_caracteres_reponse: 18_000,
                cle_api_bearer: None,
            });

            eprintln!("\nExtraction transcript...");
            let transcript_ok = youtube::extraire_transcript(&video_cible.url, 25_000)?;
            if transcript_ok.trim().is_empty() {
                anyhow::bail!("Transcript vide après extraction.");
            }
            eprintln!(
                "OK transcript ({} caractères).",
                transcript_ok.chars().count()
            );

            let mut tout_ok = true;

            for format in [FormatSortie::CarteMentale] {
                eprintln!("\nFormat: {format:?}");

                match &config_ia_opt {
                    None => {
                        eprintln!("IA skip (pas de --lm-base-url).");
                    }
                    Some(config_ia) => {
                        let (system, user) = resume::construire_messages_resume(
                            &transcript_ok,
                            Some(&video_cible.titre),
                            format,
                        );
                        let contenu_ia =
                            ia::appeler_lm_studio_chat(&client_http, config_ia, &system, &user)
                                .await;

                        match contenu_ia {
                            Err(e) => {
                                eprintln!("Erreur IA: {e}");
                                tout_ok = false;
                            }
                            Ok(contenu) => {
                                let resultat =
                                    resume::parser_sortie_ia_vers_sortie(format, &contenu);
                                match resultat {
                                    Ok(sortie) => {
                                        eprintln!("OK: sortie valide ({:?}).", format);
                                        let _ = sortie;
                                    }
                                    Err(e) => {
                                        let brut_tronque = tronquer_texte_pour_diag(&contenu, 1200);
                                        eprintln!(
                                            "KO: sortie invalide: {e}\nRéponse IA (tronquée):\n{brut_tronque}"
                                        );
                                        tout_ok = false;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if config_ia_opt.is_some() && !tout_ok {
                anyhow::bail!("test-suite: au moins un format IA a échoué.");
            }
        }

        Commande::TranscriptChannel {
            channel_url,
            nb_videos,
            out,
            max_caracteres,
        } => {
            let videos = youtube::lister_dernieres_videos(&channel_url, nb_videos)?;

            #[derive(Debug, Clone, Serialize)]
            struct EntreeIndexTranscript {
                id: String,
                titre: String,
                url: String,
                ok: bool,
                nb_caracteres: Option<usize>,
                fichier_transcript: Option<String>,
            }

            let mut index_transcripts: Vec<EntreeIndexTranscript> = Vec::new();

            for (index, video) in videos.iter().enumerate() {
                eprintln!(
                    "\n[{}/{}] Extraction transcript: {} ({})",
                    index + 1,
                    nb_videos,
                    video.titre,
                    video.id
                );

                let transcript = match youtube::extraire_transcript(&video.url, max_caracteres) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("KO transcript: {e}");
                        index_transcripts.push(EntreeIndexTranscript {
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

                let apercu = transcript.chars().take(1200).collect::<String>();
                println!(
                    "\n=== Transcript ({:?}) pour {} ===\n{}\n",
                    transcript.len(),
                    video.titre,
                    apercu
                );

                let mut fichier_ecrit: Option<String> = None;
                if let Some(dossier_sortie) = out.as_deref() {
                    std::fs::create_dir_all(dossier_sortie)?;
                    let fichier = format!("{}_transcript.txt", video.id);
                    let chemin = dossier_sortie.join(fichier);
                    std::fs::write(&chemin, &transcript)?;
                    eprintln!("Enregistré: {}", chemin.display());
                    fichier_ecrit = chemin.file_name().map(|n| n.to_string_lossy().to_string());
                }

                index_transcripts.push(EntreeIndexTranscript {
                    id: video.id.clone(),
                    titre: video.titre.clone(),
                    url: video.url.clone(),
                    ok: true,
                    nb_caracteres: Some(transcript.chars().count()),
                    fichier_transcript: fichier_ecrit,
                });
            }

            if let Some(dossier_sortie) = out.as_deref() {
                std::fs::create_dir_all(dossier_sortie)?;
                let chemin_index = dossier_sortie.join("index.json");
                let json = serde_json::to_string_pretty(&index_transcripts)?;
                std::fs::write(&chemin_index, json)?;
                eprintln!("Index écrit: {}", chemin_index.display());
            }
        }

        Commande::TranscriptVideo {
            video_url,
            out,
            max_caracteres,
        } => {
            let video = VideoInfo {
                id: "video".to_string(),
                titre: "Vidéo YouTube".to_string(),
                url: video_url,
                date_publication: None,
            };

            eprintln!("\nExtraction transcript: {} ({})", video.titre, video.id);
            let transcript = youtube::extraire_transcript(&video.url, max_caracteres)?;

            let apercu = transcript.chars().take(1200).collect::<String>();
            println!(
                "\n=== Transcript ({:?}) pour {} ===\n{}\n",
                transcript.len(),
                video.titre,
                apercu
            );

            if let Some(dossier_sortie) = out.as_deref() {
                std::fs::create_dir_all(dossier_sortie)?;
                let fichier = "transcript.txt";
                let chemin = dossier_sortie.join(fichier);
                std::fs::write(&chemin, transcript)?;
                eprintln!("Enregistré: {}", chemin.display());
            }
        }

        Commande::ListeVideos {
            channel_url,
            nb_videos,
            out,
        } => {
            let videos = youtube::lister_dernieres_videos(&channel_url, nb_videos)?;

            for (i, v) in videos.iter().enumerate() {
                println!("{:>2}. {} | {} | {}", i + 1, v.id, v.titre, v.url);
            }

            if let Some(chemin) = out.as_deref() {
                #[derive(Debug, Clone, Serialize)]
                struct EntreeVideo {
                    id: String,
                    titre: String,
                    url: String,
                }
                let liste = videos
                    .into_iter()
                    .map(|v| EntreeVideo {
                        id: v.id,
                        titre: v.titre,
                        url: v.url,
                    })
                    .collect::<Vec<_>>();
                let json = serde_json::to_string_pretty(&liste)?;
                std::fs::write(chemin, json)?;
                eprintln!("Écrit: {}", chemin.display());
            }
        }
    }

    Ok(())
}

async fn traiter_une_video(
    client_http: &Client,
    video: &VideoInfo,
    format: FormatSortie,
    config_ia: &ConfigurationIA,
    out: Option<&std::path::Path>,
) -> Result<(), anyhow::Error> {
    let transcript = youtube::extraire_transcript(&video.url, config_ia.max_caracteres_transcript)?;

    let (system, user) =
        resume::construire_messages_resume(&transcript, Some(&video.titre), format);
    let contenu_ia = ia::appeler_lm_studio_chat(client_http, config_ia, &system, &user).await;

    let contenu_ia = match contenu_ia {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Erreur IA: {e}");
            return Err(anyhow::anyhow!("{e}"));
        }
    };

    match resume::parser_sortie_ia_vers_sortie(format, &contenu_ia) {
        Ok(sortie) => {
            let chaine = resume::formatter_sortie_resume_sortie(format, sortie);
            println!(
                "\n=== Résumé ({:?}) pour {} ===\n{}\n",
                format, video.titre, chaine
            );

            if let Some(dossier_sortie) = out {
                std::fs::create_dir_all(dossier_sortie)?;
                let fichier = format!(
                    "{}_{}.{}",
                    video.id,
                    format.nom_pour_prompt(),
                    format.ext_fichier()
                );
                let chemin = dossier_sortie.join(fichier);
                std::fs::write(&chemin, chaine)?;
                eprintln!("Enregistré: {}", chemin.display());
            }
            Ok(())
        }
        Err(e) => {
            let brut_tronque = tronquer_texte_pour_diag(&contenu_ia, 2000);
            Err(anyhow::anyhow!(
                "sortie IA invalide ({format:?}): {e}\nRéponse IA (tronquée):\n{brut_tronque}"
            ))
        }
    }
}
