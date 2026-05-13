use crate::config::ConfigurationIA;
use reqwest::Client;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErreurIA {
    #[error("échec requête LM Studio: {message}")]
    Requete { message: String },
    #[error("réponse LM Studio inattendue: {message}")]
    Reponse { message: String },
    #[error("LM Studio a renvoyé un statut HTTP {statut}: {corps}")]
    StatutHTTP { statut: u16, corps: String },
}

#[derive(Debug, Serialize)]
struct MessageChat<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct DemandeChat<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    messages: Vec<MessageChat<'a>>,
    temperature: f32,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct DemandeChargementModele<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    flash_attention: Option<bool>,
    echo_load_config: bool,
}

#[derive(Debug, Deserialize)]
struct ReponseChat {
    choices: Vec<Choix>,
}

#[derive(Debug, Deserialize)]
struct Choix {
    message: MessageChatReponse,
}

#[derive(Debug, Deserialize)]
struct MessageChatReponse {
    content: String,
}

fn supprimer_blocs_pensee(texte: &str) -> String {
    // Enlève les blocs de “raisonnement” fréquemment renvoyés par certains modèles.
    // Exemples courants: <think>...</think>
    let re_think = Regex::new(r"(?is)<think>.*?</think>").expect("regex valide");
    let sans_think = re_think.replace_all(texte, "");
    sans_think.trim().to_string()
}

/// Appelle LM Studio (format OpenAI-compatible) et retourne le texte brut de la réponse.
pub async fn appeler_lm_studio_chat(
    client: &Client,
    configuration: &ConfigurationIA,
    system: &str,
    user: &str,
) -> Result<String, ErreurIA> {
    let base = configuration.base_url.trim_end_matches('/');
    let route = configuration.route.trim_start_matches('/');
    let url_complete = format!("{base}/{route}");

    let system_message = MessageChat {
        role: "system",
        content: system,
    };
    let user_message = MessageChat {
        role: "user",
        content: user,
    };

    let model = configuration.modele.as_deref();

    let demande = DemandeChat {
        model,
        messages: vec![system_message, user_message],
        temperature: configuration.temperature,
        stream: false,
    };

    async fn envoyer_requete_chat(
        client: &Client,
        configuration: &ConfigurationIA,
        url_complete: &str,
        demande: &DemandeChat<'_>,
    ) -> Result<(u16, String), ErreurIA> {
        let mut requete = client.post(url_complete).json(demande);
        if let Some(ref cle) = configuration.cle_api_bearer {
            let t = cle.trim();
            if !t.is_empty() {
                requete = requete.header("Authorization", format!("Bearer {t}"));
            }
        }
        if configuration.base_url.contains("openrouter.ai") {
            requete = requete
                .header("Referer", "https://resume-youtube.app")
                .header("X-Title", "Resume YouTube");
        }
        let reponse = requete.send().await.map_err(|e| ErreurIA::Requete {
            message: e.to_string(),
        })?;
        let statut = reponse.status().as_u16();
        let corps = reponse.text().await.map_err(|e| ErreurIA::Requete {
            message: e.to_string(),
        })?;
        Ok((statut, corps))
    }

    async fn tenter_charger_modele_lm_studio(
        client: &Client,
        configuration: &ConfigurationIA,
        modele: &str,
    ) -> Result<(), ErreurIA> {
        let base = configuration.base_url.trim_end_matches('/');
        let url_charge = format!("{base}/api/v1/models/load");
        let corps_charge = DemandeChargementModele {
            model: modele,
            context_length: None,
            flash_attention: None,
            echo_load_config: false,
        };

        let mut requete = client.post(url_charge).json(&corps_charge);
        if let Some(ref cle) = configuration.cle_api_bearer {
            let t = cle.trim();
            if !t.is_empty() {
                requete = requete.header("Authorization", format!("Bearer {t}"));
            }
        }
        let reponse = requete.send().await.map_err(|e| ErreurIA::Requete {
            message: e.to_string(),
        })?;
        let statut = reponse.status().as_u16();
        let texte = reponse.text().await.map_err(|e| ErreurIA::Requete {
            message: e.to_string(),
        })?;
        if !(200..300).contains(&statut) {
            return Err(ErreurIA::StatutHTTP {
                statut,
                corps: texte.chars().take(1500).collect(),
            });
        }
        Ok(())
    }

    fn erreur_modele_charge_annulee(statut: u16, corps: &str) -> bool {
        if statut != 400 {
            return false;
        }
        let c = corps.to_ascii_lowercase();
        c.contains("failed to load model") && c.contains("operation canceled")
    }

    let (mut statut, mut corps) =
        envoyer_requete_chat(client, configuration, &url_complete, &demande).await?;
    if erreur_modele_charge_annulee(statut, &corps) {
        if !configuration.base_url.contains("openrouter.ai") {
            if let Some(m) = configuration.modele.as_deref().filter(|m| !m.trim().is_empty()) {
                let _ = tenter_charger_modele_lm_studio(client, configuration, m).await;
            }
        }
        // LM Studio peut annuler la première tentative pendant le chargement du modèle.
        tokio::time::sleep(Duration::from_millis(900)).await;
        let (s2, c2) = envoyer_requete_chat(client, configuration, &url_complete, &demande).await?;
        statut = s2;
        corps = c2;
    }

    if !(200..300).contains(&statut) {
        return Err(ErreurIA::StatutHTTP {
            statut,
            corps: corps.chars().take(1500).collect(),
        });
    }

    let reponse_chat: ReponseChat =
        serde_json::from_str(&corps).map_err(|e| ErreurIA::Reponse {
            message: format!("impossible de parser la réponse JSON: {e}"),
        })?;

    let premier = reponse_chat
        .choices
        .first()
        .ok_or_else(|| ErreurIA::Reponse {
            message: "aucune choice dans la réponse".to_string(),
        })?;

    let contenu = premier.message.content.clone();
    let limite = configuration.max_caracteres_reponse;
    let tronque: String = contenu.chars().take(limite).collect();
    Ok(supprimer_blocs_pensee(&tronque))
}

#[derive(Debug, Deserialize)]
struct ModeleItemOpenRouter {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ReponseModelesOpenRouter {
    data: Vec<ModeleItemOpenRouter>,
}

/// Extrait les identifiants depuis une réponse LM Studio : format OpenAI (`data[]`) et/ou REST (`models[]`).
fn extraire_identifiants_modeles_depuis_json(corps: &str) -> Result<Vec<String>, ErreurIA> {
    let valeur: serde_json::Value = serde_json::from_str(corps).map_err(|e| ErreurIA::Reponse {
        message: format!("impossible de parser la réponse JSON: {e}"),
    })?;

    let mut ids = Vec::new();

    fn pousser_id_depuis_objet(obj: &serde_json::Map<String, serde_json::Value>, ids: &mut Vec<String>) {
        const CLES: &[&str] = &["key", "id", "model", "name", "root"];
        for cle in CLES {
            if let Some(serde_json::Value::String(s)) = obj.get(*cle) {
                if !s.is_empty() {
                    ids.push(s.clone());
                    return;
                }
            }
        }
    }

    fn collecter_depuis_tableau_data(arr: &[serde_json::Value], ids: &mut Vec<String>) {
        for item in arr {
            match item {
                serde_json::Value::String(s) if !s.is_empty() => ids.push(s.clone()),
                serde_json::Value::Object(obj) => pousser_id_depuis_objet(obj, ids),
                _ => {}
            }
        }
    }

    if let Some(arr) = valeur.get("data").and_then(|v| v.as_array()) {
        collecter_depuis_tableau_data(arr, &mut ids);
    }
    if ids.is_empty() {
        if let Some(arr) = valeur.pointer("/data/models").and_then(|v| v.as_array()) {
            collecter_depuis_tableau_data(arr, &mut ids);
        }
    }
    if let Some(arr) = valeur.get("models").and_then(|v| v.as_array()) {
        for item in arr {
            match item {
                serde_json::Value::String(s) if !s.is_empty() => ids.push(s.clone()),
                serde_json::Value::Object(obj) => pousser_id_depuis_objet(obj, &mut ids),
                _ => {}
            }
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn appliquer_entete_bearer_lm(
    mut requete: reqwest::RequestBuilder,
    configuration: &ConfigurationIA,
) -> reqwest::RequestBuilder {
    if let Some(ref cle) = configuration.cle_api_bearer {
        let t = cle.trim();
        if !t.is_empty() {
            requete = requete.header("Authorization", format!("Bearer {t}"));
        }
    }
    requete
}

/// Liste les modèles proposés par LM Studio (`GET /v1/models` OpenAI-compatible, repli `GET /api/v1/models`).
///
/// Si `/v1/models` répond **404**, on utilise uniquement l’API REST. Si elle répond **200** mais sans
/// identifiants exploitables (liste vide ou format inattendu), on tente aussi l’API REST — certains builds
/// renvoient une liste OpenAI-compatible vide alors que `/api/v1/models` est rempli.
pub async fn lister_modeles_lm_studio(
    client: &Client,
    configuration: &ConfigurationIA,
) -> Result<Vec<String>, ErreurIA> {
    let base = configuration.base_url.trim_end_matches('/');
    let url_openai = format!("{base}/v1/models");

    let requete = appliquer_entete_bearer_lm(client.get(&url_openai), configuration);
    let reponse = requete.send().await.map_err(|e| ErreurIA::Requete {
        message: e.to_string(),
    })?;

    let statut = reponse.status().as_u16();
    let corps = reponse.text().await.map_err(|e| ErreurIA::Requete {
        message: e.to_string(),
    })?;

    if statut == 404 {
        let url_rest = format!("{base}/api/v1/models");
        let requete2 = appliquer_entete_bearer_lm(client.get(&url_rest), configuration);
        let reponse2 = requete2.send().await.map_err(|e| ErreurIA::Requete {
            message: e.to_string(),
        })?;
        let statut2 = reponse2.status().as_u16();
        let corps2 = reponse2.text().await.map_err(|e| ErreurIA::Requete {
            message: e.to_string(),
        })?;
        if !(200..300).contains(&statut2) {
            return Err(ErreurIA::StatutHTTP {
                statut: statut2,
                corps: corps2.chars().take(1500).collect(),
            });
        }
        return extraire_identifiants_modeles_depuis_json(&corps2);
    }

    if !(200..300).contains(&statut) {
        return Err(ErreurIA::StatutHTTP {
            statut,
            corps: corps.chars().take(1500).collect(),
        });
    }

    let mut ids = extraire_identifiants_modeles_depuis_json(&corps)?;
    if ids.is_empty() {
        let url_rest = format!("{base}/api/v1/models");
        let requete_rest = appliquer_entete_bearer_lm(client.get(&url_rest), configuration);
        let reponse_rest = requete_rest.send().await.map_err(|e| ErreurIA::Requete {
            message: e.to_string(),
        })?;
        let statut_rest = reponse_rest.status().as_u16();
        let corps_rest = reponse_rest.text().await.map_err(|e| ErreurIA::Requete {
            message: e.to_string(),
        })?;
        if (200..300).contains(&statut_rest) {
            ids = extraire_identifiants_modeles_depuis_json(&corps_rest)?;
        } else if statut_rest != 404 {
            return Err(ErreurIA::StatutHTTP {
                statut: statut_rest,
                corps: corps_rest.chars().take(1500).collect(),
            });
        }
    }

    Ok(ids)
}

/// Liste les modèles **gratuits** OpenRouter (`:free` dans l’identifiant), via l’API publique.
pub async fn lister_modeles_openrouter_gratuits(client: &Client) -> Result<Vec<String>, ErreurIA> {
    let url_complete = "https://openrouter.ai/api/v1/models";
    let reponse = client
        .get(url_complete)
        .send()
        .await
        .map_err(|e| ErreurIA::Requete {
            message: e.to_string(),
        })?;

    let statut = reponse.status().as_u16();
    let corps = reponse.text().await.map_err(|e| ErreurIA::Requete {
        message: e.to_string(),
    })?;

    if !(200..300).contains(&statut) {
        return Err(ErreurIA::StatutHTTP {
            statut,
            corps: corps.chars().take(1500).collect(),
        });
    }

    let reponse_modeles: ReponseModelesOpenRouter =
        serde_json::from_str(&corps).map_err(|e| ErreurIA::Reponse {
            message: format!("impossible de parser la réponse JSON: {e}"),
        })?;

    let mut ids = reponse_modeles
        .data
        .into_iter()
        .map(|m| m.id)
        .filter(|id| id.contains(":free"))
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    Ok(ids)
}
