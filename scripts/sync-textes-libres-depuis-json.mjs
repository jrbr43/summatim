#!/usr/bin/env node
/**
 * Écrit `donnees/textes_libres/<id>/texte_source.txt` et `carte_mentale.md` à partir d’un export
 * de la clé localStorage `resume_youtube_textes_libres_v1` (tableau JSON).
 *
 * Usage : node scripts/sync-textes-libres-depuis-json.mjs <fichier.json>
 */
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const racine = path.resolve(__dirname, "..");

/** Aligné sur `nettoyer_nom_dossier` (Rust, commandes.rs). */
function nettoyerNomDossier(nom) {
  const brut = String(nom).trim();
  if (!brut) return "inconnu";
  let s = "";
  for (const c of brut) {
    const ok =
      /[a-zA-Z0-9]/.test(c) || c === "-" || c === "_" || c === ".";
    s += ok ? c : "_";
  }
  let t = s;
  while (t.length && (t.endsWith(" ") || t.endsWith("."))) {
    t = t.slice(0, -1);
  }
  while (t.length && (t.startsWith(" ") || t.startsWith("."))) {
    t = t.slice(1);
  }
  return t.length ? t : "inconnu";
}

const fichier = process.argv[2];
if (!fichier) {
  console.error(
    "Usage: node scripts/sync-textes-libres-depuis-json.mjs <fichier.json>",
  );
  process.exit(1);
}
const brut = fs.readFileSync(fichier, "utf8");
const arr = JSON.parse(brut);
if (!Array.isArray(arr)) {
  console.error("Le JSON doit être un tableau (valeur de resume_youtube_textes_libres_v1).");
  process.exit(1);
}
const base = path.join(racine, "donnees", "textes_libres");
let n = 0;
for (const e of arr) {
  if (!e || typeof e.id !== "string" || typeof e.contenu !== "string") continue;
  if (!e.contenu.trim()) continue;
  const dir = path.join(base, nettoyerNomDossier(e.id));
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, "texte_source.txt"), e.contenu, "utf8");
  if (
    typeof e.resumeCarteMentale === "string" &&
    e.resumeCarteMentale.trim().length > 0
  ) {
    fs.writeFileSync(
      path.join(dir, "carte_mentale.md"),
      e.resumeCarteMentale,
      "utf8",
    );
  }
  n++;
}
console.log(`OK : ${n} dossier(s) écrit(s) sous donnees/textes_libres/`);
