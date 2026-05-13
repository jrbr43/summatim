#!/usr/bin/env bash
# Installe un raccourci « Resume YouTube » (interface graphique) dans le menu applications.
set -euo pipefail

racine_depot="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lanceur="$racine_depot/scripts/lancer-app-bureau.sh"
icone="$racine_depot/src-tauri/icons/128x128.png"
dest_desktop="${XDG_DATA_HOME:-$HOME/.local/share}/applications/resume-youtube-bureau.desktop"

if [[ ! -x "$lanceur" ]]; then
  chmod +x "$lanceur"
fi

if [[ ! -f "$icone" ]]; then
  icone=""
fi

mkdir -p "$(dirname "$dest_desktop")"

{
  echo "[Desktop Entry]"
  echo "Version=1.0"
  echo "Type=Application"
  echo "Name=Resume YouTube"
  echo "Comment=Résumés YouTube et cartes mentales (interface graphique)"
  echo "Exec=$lanceur"
  echo "Path=$racine_depot"
  echo "Terminal=false"
  echo "Categories=Utility;AudioVideo;Education;"
  if [[ -n "$icone" ]]; then
    echo "Icon=$icone"
  fi
} > "$dest_desktop"

echo "Raccourci installé : $dest_desktop"
echo "Recherchez « Resume YouTube » dans le menu des applications."
echo ""
echo "Double-clic possible aussi sur : $lanceur"
echo "Ou sur l’AppImage : $racine_depot/target/release/bundle/appimage/Resume YouTube_0.1.0_amd64.AppImage"
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$(dirname "$dest_desktop")" 2>/dev/null || true
fi
