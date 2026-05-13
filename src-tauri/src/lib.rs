mod commandes;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let racine = commandes::initialiser_racine_donnees(&app.handle())?;
            app.manage(commandes::EtatApplication::nouveau(racine));
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commandes::lister_chaines,
            commandes::ajouter_chaine,
            commandes::supprimer_chaine,
            commandes::chemin_fichier_chaines,
            commandes::lister_videos_chaine,
            commandes::infos_video_depuis_url,
            commandes::extraire_transcript_video,
            commandes::extraire_transcripts_chaine,
            commandes::lister_modeles_lm_studio,
            commandes::lister_modeles_openrouter_gratuits,
            commandes::lire_resume_enregistre,
            commandes::lire_tous_resumes_enregistres,
            commandes::resume_enregistre_existe,
            commandes::supprimer_video,
            commandes::resumer_video_direct,
            commandes::resumer_texte_libre,
            commandes::lire_carte_mentale_texte_libre,
            commandes::supprimer_dossier_texte_libre,
            commandes::synchroniser_textes_libres_vers_disque,
            commandes::resumer_chaine,
            commandes::lancer_test_suite,
            commandes::demander_annulation,
            commandes::reinitialiser_annulation,
            commandes::selectionner_dossier_sortie,
            commandes::lire_texte_presse_papiers,
            commandes::etat_modele_local,
            commandes::telecharger_modele_local,
            commandes::supprimer_modele_local,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l’application Tauri");
}
