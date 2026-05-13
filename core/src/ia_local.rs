//! Inférence locale via la crate `llama-cpp-2` (llama.cpp embarqué).
//!
//! Charge un fichier GGUF (Qwen3-like, ChatML) et expose une fonction
//! `generer_chat` réutilisable depuis le backend Tauri.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use once_cell::sync::OnceCell;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErreurIaLocale {
    #[error("backend llama.cpp indisponible: {0}")]
    Backend(String),
    #[error("chargement du modèle GGUF échoué: {0}")]
    Chargement(String),
    #[error("création du contexte llama.cpp échouée: {0}")]
    Contexte(String),
    #[error("tokenisation échouée: {0}")]
    Tokenisation(String),
    #[error("appel decode() échoué: {0}")]
    Decodage(String),
    #[error("la fenêtre de contexte (n_ctx={n_ctx}) est trop petite pour le prompt ({n_prompt} tokens) et la sortie demandée ({n_max_sortie}).")]
    ContexteTropPetit {
        n_ctx: i32,
        n_prompt: i32,
        n_max_sortie: i32,
    },
    #[error("conversion token→texte échouée: {0}")]
    TokenVersTexte(String),
}

/// Backend llama.cpp partagé par tout le processus (init unique).
/// `LlamaBackend::init` ne peut être appelé qu'une seule fois.
fn backend_partage() -> Result<&'static Arc<LlamaBackend>, ErreurIaLocale> {
    static BACKEND: OnceCell<Arc<LlamaBackend>> = OnceCell::new();
    BACKEND.get_or_try_init(|| {
        LlamaBackend::init()
            .map(Arc::new)
            .map_err(|e| ErreurIaLocale::Backend(e.to_string()))
    })
}

/// Modèle GGUF chargé en mémoire et conservé entre les appels.
pub struct ModeleLocalCharge {
    pub chemin: PathBuf,
    pub n_ctx_courant: u32,
    pub backend: Arc<LlamaBackend>,
    pub modele: LlamaModel,
}

impl std::fmt::Debug for ModeleLocalCharge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModeleLocalCharge")
            .field("chemin", &self.chemin)
            .field("n_ctx_courant", &self.n_ctx_courant)
            .finish()
    }
}

/// Charge le GGUF en RAM. Le `n_ctx` n'est utilisé que côté contexte (par appel),
/// mais on le mémorise ici pour permettre à l'appelant de détecter un changement.
pub fn charger_modele_local(
    chemin: &Path,
    n_ctx: u32,
) -> Result<ModeleLocalCharge, ErreurIaLocale> {
    let backend = backend_partage()?.clone();
    let parametres_modele = LlamaModelParams::default();
    let modele = LlamaModel::load_from_file(&backend, chemin, &parametres_modele)
        .map_err(|e| ErreurIaLocale::Chargement(e.to_string()))?;
    Ok(ModeleLocalCharge {
        chemin: chemin.to_path_buf(),
        n_ctx_courant: n_ctx,
        backend,
        modele,
    })
}

/// Construit un prompt ChatML adapté à Qwen / MARTHA-Qwen3.5-Omni.
///
/// On évite `apply_chat_template` (qui dépend du nom du template baked dans le GGUF)
/// pour rester robuste sur cette famille de modèles, dont certains GGUF n'incluent
/// pas le template.
fn construire_prompt_chatml(system: &str, user: &str) -> String {
    let mut p = String::new();
    p.push_str("<|im_start|>system\n");
    p.push_str(system.trim());
    p.push_str("<|im_end|>\n");
    p.push_str("<|im_start|>user\n");
    p.push_str(user.trim());
    p.push_str("<|im_end|>\n");
    p.push_str("<|im_start|>assistant\n");
    p
}

/// Supprime les blocs `<think>...</think>` éventuellement émis par le modèle.
fn supprimer_blocs_pensee(texte: &str) -> String {
    let re = regex::Regex::new(r"(?is)<think>.*?</think>").expect("regex valide");
    re.replace_all(texte, "").trim().to_string()
}

/// Génère une réponse via inférence locale (synchrone, à exécuter dans
/// `tokio::task::spawn_blocking`).
pub fn generer_chat(
    modele_charge: &ModeleLocalCharge,
    system: &str,
    user: &str,
    temperature: f32,
    n_ctx: u32,
    max_tokens_sortie: i32,
    max_caracteres_reponse: usize,
) -> Result<String, ErreurIaLocale> {
    let backend = &modele_charge.backend;
    let modele = &modele_charge.modele;

    // Ne jamais dépasser le contexte d’entraînement du GGUF : au-delà, llama.cpp peut
    // mal se comporter ou consommer une quantité de RAM disproportionnée (OOM / crash).
    let n_ctx_plafond_modele = modele.n_ctx_train().max(512);
    let n_ctx_utilisateur = n_ctx.max(512);
    let n_ctx_effectif_brut = n_ctx_utilisateur.min(n_ctx_plafond_modele);
    let n_ctx_effectif = NonZeroU32::new(n_ctx_effectif_brut).unwrap_or_else(|| {
        NonZeroU32::new(2048).expect("constante non nulle")
    });

    // Tokeniser avant `new_context` : llama.cpp exige `n_tokens <= n_batch` à chaque
    // `decode` ; le défaut `n_batch=2048` faisait planter (assert) dès un prompt > 2048.
    let prompt = construire_prompt_chatml(system, user);
    let tokens_prompt = modele
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| ErreurIaLocale::Tokenisation(e.to_string()))?;
    let n_prompt = tokens_prompt.len() as i32;
    let n_max_sortie = max_tokens_sortie.max(64);
    let n_ctx_limite_avant_pad = n_ctx_effectif.get() as i32;
    if n_prompt + n_max_sortie > n_ctx_limite_avant_pad {
        return Err(ErreurIaLocale::ContexteTropPetit {
            n_ctx: n_ctx_limite_avant_pad,
            n_prompt,
            n_max_sortie,
        });
    }

    // Plancher aligné sur `llama_context_default_params().n_batch` (2048).
    const N_BATCH_DEFAUT_LLAMA: u32 = 2048;
    let n_batch_decode = (n_prompt as u32)
        .max(N_BATCH_DEFAUT_LLAMA)
        .min(n_ctx_effectif.get());

    let parametres_contexte = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx_effectif))
        .with_n_batch(n_batch_decode);

    let mut contexte = modele
        .new_context(backend, parametres_contexte)
        .map_err(|e| ErreurIaLocale::Contexte(e.to_string()))?;

    let n_ctx_total = contexte.n_ctx() as i32;
    if n_prompt + n_max_sortie > n_ctx_total {
        return Err(ErreurIaLocale::ContexteTropPetit {
            n_ctx: n_ctx_total,
            n_prompt,
            n_max_sortie,
        });
    }

    let taille_lot = n_prompt.max(512) as usize;
    let mut lot = LlamaBatch::new(taille_lot, 1);
    let dernier_index_prompt = n_prompt - 1;
    for (i, tok) in (0_i32..).zip(tokens_prompt.iter().copied()) {
        let est_dernier = i == dernier_index_prompt;
        lot.add(tok, i, &[0], est_dernier)
            .map_err(|e| ErreurIaLocale::Decodage(e.to_string()))?;
    }
    contexte
        .decode(&mut lot)
        .map_err(|e| ErreurIaLocale::Decodage(e.to_string()))?;

    let graine: u32 = 1234;
    let temperature_clamp = if temperature.is_finite() && temperature > 0.0 {
        temperature
    } else {
        0.7
    };
    let mut echantilloneur = LlamaSampler::chain_simple([
        LlamaSampler::top_k(40),
        LlamaSampler::top_p(0.95, 1),
        LlamaSampler::temp(temperature_clamp),
        LlamaSampler::dist(graine),
    ]);

    let mut decodeur_utf8 = encoding_rs::UTF_8.new_decoder();
    let mut sortie = String::new();
    let mut n_curseur = lot.n_tokens();
    let mut n_decodes: i32 = 0;

    while n_decodes < n_max_sortie && n_curseur < n_ctx_total {
        // llama.h : « sample from the logits of the last token in the batch » → idx = -1.
        let tok = echantilloneur.sample(&contexte, -1);
        echantilloneur.accept(tok);

        if modele.is_eog_token(tok) {
            break;
        }

        let morceau = modele
            .token_to_piece(tok, &mut decodeur_utf8, false, None)
            .map_err(|e| ErreurIaLocale::TokenVersTexte(e.to_string()))?;
        sortie.push_str(&morceau);

        if sortie.chars().count() >= max_caracteres_reponse {
            break;
        }

        lot.clear();
        lot.add(tok, n_curseur, &[0], true)
            .map_err(|e| ErreurIaLocale::Decodage(e.to_string()))?;
        contexte
            .decode(&mut lot)
            .map_err(|e| ErreurIaLocale::Decodage(e.to_string()))?;

        n_curseur += 1;
        n_decodes += 1;
    }

    let propre = supprimer_blocs_pensee(&sortie);
    let tronque: String = propre.chars().take(max_caracteres_reponse).collect();
    Ok(tronque)
}
