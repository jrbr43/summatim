use regex::Regex;
use serde_json::Value;

fn supprimer_doublons_consecutifs(morceaux: Vec<String>) -> Vec<String> {
    let mut resultat: Vec<String> = Vec::with_capacity(morceaux.len());
    for m in morceaux {
        let t = m.trim();
        if t.is_empty() {
            continue;
        }
        if resultat.last().is_some_and(|dernier| dernier == t) {
            continue;
        }
        resultat.push(t.to_string());
    }
    resultat
}

/// Parse un fichier VTT (WebVTT) en texte “lisible” pour l’IA.
///
/// Objectif: supprimer les timestamps, tags HTML/VTTCUE et condenser les espaces.
pub fn parser_vtt_vers_texte(vtt: &str) -> String {
    // Timestamp typique: 00:00:12.345 --> 00:00:15.678
    let re_timestamps =
        Regex::new(r"^\s*\d{2}:\d{2}:\d{2}\.\d{3}\s+-->\s+\d{2}:\d{2}:\d{2}\.\d{3}\s*$")
            .expect("regex valide");
    let re_tags = Regex::new(r"<[^>]+>").expect("regex valide");
    let re_whitespace = Regex::new(r"\s+").expect("regex valide");

    let mut morceaux: Vec<String> = Vec::new();

    for ligne in vtt.lines() {
        let l = ligne.trim();
        if l.is_empty() {
            continue;
        }
        if l.starts_with("WEBVTT") {
            continue;
        }
        if re_timestamps.is_match(l) {
            continue;
        }
        if l.contains("-->") {
            continue;
        }

        let sans_tags = re_tags.replace_all(l, "");
        let nettoye = sans_tags.trim();
        if !nettoye.is_empty() {
            morceaux.push(nettoye.to_string());
        }
    }

    let morceaux = supprimer_doublons_consecutifs(morceaux);
    let texte = morceaux.join(" ");
    re_whitespace.replace_all(texte.trim(), " ").to_string()
}

/// Fallback SRT vers texte.
///
/// Ce parsing est volontairement simple (utile si `yt-dlp` ne fournit pas de VTT).
pub fn parser_srt_vers_texte(srt: &str) -> String {
    let re_timestamp = Regex::new(r"\d{2}:\d{2}:\d{2},\d{3}\s+-->\s+\d{2}:\d{2}:\d{2},\d{3}")
        .expect("regex valide");
    let re_tags = Regex::new(r"<[^>]+>").expect("regex valide");
    let re_whitespace = Regex::new(r"\s+").expect("regex valide");

    let mut morceaux: Vec<String> = Vec::new();
    for ligne in srt.lines() {
        let l = ligne.trim();
        if l.is_empty() {
            continue;
        }
        if re_timestamp.is_match(l) {
            continue;
        }
        if l.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let sans_tags = re_tags.replace_all(l, "");
        let nettoye = sans_tags.trim();
        if !nettoye.is_empty() {
            morceaux.push(nettoye.to_string());
        }
    }

    let morceaux = supprimer_doublons_consecutifs(morceaux);
    let texte = morceaux.join(" ");
    re_whitespace.replace_all(texte.trim(), " ").to_string()
}

/// Parse un fichier `yt-dlp --sub-format json3` en texte.
///
/// Le format JSON3 de `yt-dlp` contient généralement un tableau `events`,
/// avec une liste de `segs` (dont les segments contiennent `utf8`).
pub fn parser_json3_vers_texte(json3: &str) -> String {
    let valeur: Value = match serde_json::from_str(json3) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    let mut morceaux: Vec<String> = Vec::new();

    if let Some(events) = valeur.get("events").and_then(|v| v.as_array()) {
        for event in events {
            let mut segs_utf8: Vec<String> = Vec::new();
            if let Some(segs) = event.get("segs").and_then(|v| v.as_array()) {
                for seg in segs {
                    if let Some(utf8) = seg.get("utf8").and_then(|v| v.as_str()) {
                        let t = utf8.trim();
                        if !t.is_empty() {
                            segs_utf8.push(t.to_string());
                        }
                    }
                }
            }
            if !segs_utf8.is_empty() {
                let ligne = segs_utf8.join(" ");
                if !ligne.trim().is_empty() {
                    morceaux.push(ligne.trim().to_string());
                }
            }
        }
    }

    // Fallback: recherche récursive d’objets { "utf8": "..." }.
    if morceaux.is_empty() {
        fn parcourir(valeur: &Value, morceaux: &mut Vec<String>) {
            if let Some(obj) = valeur.as_object() {
                for (k, v) in obj {
                    if k == "utf8" {
                        if let Some(s) = v.as_str() {
                            let t = s.trim();
                            if !t.is_empty() {
                                morceaux.push(t.to_string());
                            }
                        }
                    } else {
                        parcourir(v, morceaux);
                    }
                }
            } else if let Some(arr) = valeur.as_array() {
                for v in arr {
                    parcourir(v, morceaux);
                }
            }
        }

        parcourir(&valeur, &mut morceaux);
    }

    let morceaux = supprimer_doublons_consecutifs(morceaux);
    let re_whitespace = Regex::new(r"\s+").expect("regex valide");
    let texte = morceaux.join(" ");
    re_whitespace.replace_all(texte.trim(), " ").to_string()
}
