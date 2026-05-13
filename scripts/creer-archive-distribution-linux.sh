#!/usr/bin/env bash
# Construit une archive .tar.xz avec les livrables Linux (app graphique, CLI, paquets).
set -euo pipefail

racine_depot="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="$(awk '/^\[workspace\.package\]/{f=1} f&&/^version =/{gsub(/"/,"",$3);print $3;exit}' "$racine_depot/Cargo.toml")"
nom_archive="resume-youtube-linux-${version}"
repertoire_staging="$(mktemp -d)"
trap 'rm -rf "$repertoire_staging"' EXIT

racine_sortie="$racine_depot/dist"
mkdir -p "$racine_sortie"

destination="$repertoire_staging/$nom_archive"
mkdir -p "$destination/bureau" "$destination/paquets" "$destination/cli" "$destination/scripts" "$destination/icons"

# Vérifications minimales
binaire_cli="$racine_depot/target/release/resume-youtube"
binaire_tauri="$racine_depot/target/release/resume_youtube_tauri"
appimage="$racine_depot/target/release/bundle/appimage/Resume YouTube_${version}_amd64.AppImage"
fichier_deb="$racine_depot/target/release/bundle/deb/Resume YouTube_${version}_amd64.deb"
fichier_rpm="$racine_depot/target/release/bundle/rpm/Resume YouTube-${version}-1.x86_64.rpm"

manquant=()
[[ -x "$binaire_cli" ]] || manquant+=("$binaire_cli")
[[ -x "$binaire_tauri" ]] || manquant+=("$binaire_tauri")
[[ -f "$appimage" ]] || manquant+=("$appimage")
[[ -f "$fichier_deb" ]] || manquant+=("$fichier_deb")
[[ -f "$fichier_rpm" ]] || manquant+=("$fichier_rpm")

if [[ ${#manquant[@]} -gt 0 ]]; then
  echo "Fichiers manquants — lancez d’abord depuis la racine du dépôt :" >&2
  echo "  cargo build --release -p resume_youtube_cli" >&2
  echo "  env -u CI npm run build" >&2
  echo "Manquants :" >&2
  printf '  %s\n' "${manquant[@]}" >&2
  exit 1
fi

cp -a "$appimage" "$destination/bureau/"
cp -a "$fichier_deb" "$fichier_rpm" "$destination/paquets/"
cp -a "$binaire_cli" "$destination/cli/resume-youtube"
cp -a "$binaire_tauri" "$destination/bureau/resume_youtube_tauri"
cp -a "$racine_depot/src-tauri/icons/128x128.png" "$destination/icons/"

for script in lancer-app-bureau.sh installer-raccourci-app-bureau.sh lancer-resume-youtube.sh installer-raccourci-cli.sh; do
  cp -a "$racine_depot/scripts/$script" "$destination/scripts/"
  chmod +x "$destination/scripts/$script"
done

cat > "$destination/chaines.json.exemple" << 'EOF'
[]
EOF

cat > "$destination/LISEZMOI.txt" << EOF
Resume YouTube — distribution Linux (version ${version})
============================================================

Contenu de l’archive
--------------------
bureau/
  Resume YouTube_${version}_amd64.AppImage   Application graphique (recommandé : double-clic)
  resume_youtube_tauri                      Même binaire (sans AppImage) ; dépendances système GTK/WebKit
paquets/
  Resume YouTube_${version}_amd64.deb       Installation Debian/Ubuntu : sudo apt install ./fichier.deb
  Resume YouTube-${version}-1.x86_64.rpm    Fedora/RHEL (selon votre gestionnaire)
cli/
  resume-youtube                            Ligne de commande (yt-dlp + LM Studio requis)
scripts/
  Lanceurs et installateurs de raccourcis menu (voir README du dépôt source)
icons/
  128x128.png                               Icône pour raccourci bureau
chaines.json.exemple                      Liste de chaînes vide ; copiez en chaines.json à côté de l’app si besoin

Prérequis
---------
- yt-dlp installé et dans le PATH (obligatoire pour YouTube / transcripts)
- Pour LM Studio : serveur HTTP local compatible OpenAPI (voir la doc du projet)
- Librairies GTK/WebKit : normalement couvertes par le .deb ; l’AppImage est plus autonome

Lancement rapide
----------------
1) Graphique : chmod +x ./bureau/Resume\\ YouTube_${version}_amd64.AppImage puis double-clic, ou
2) ./scripts/lancer-app-bureau.sh (depuis une copie du dépôt ; ici les scripts sont dans l’archive)

Documentation complète : dépôt source (README.md, HELP.md).
EOF

chemin_archive="$racine_sortie/${nom_archive}.tar.xz"
(
  cd "$repertoire_staging"
  tar -cJf "$chemin_archive" "$nom_archive"
)

echo "Archive créée : $chemin_archive"
ls -lh "$chemin_archive"
