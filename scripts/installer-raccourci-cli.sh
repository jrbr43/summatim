#!/usr/bin/env bash
# Installe un raccourci « Resume YouTube (CLI) » dans le menu applications.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LANCER="$REPO_ROOT/scripts/lancer-resume-youtube.sh"
DEST="${XDG_DATA_HOME:-$HOME/.local/share}/applications/resume-youtube-cli.desktop"

if [[ ! -x "$LANCER" ]]; then
  chmod +x "$LANCER"
fi

mkdir -p "$(dirname "$DEST")"

cat > "$DEST" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=Resume YouTube (CLI)
Comment=Récupère des transcripts YouTube et résume via LM Studio (terminal)
Exec=$LANCER
Path=$REPO_ROOT
Terminal=true
Categories=Utility;ConsoleOnly;
EOF

echo "Raccourci installé : $DEST"
echo "Recherchez « Resume YouTube » dans le menu, ou double-cliquez ce fichier :"
echo "  $LANCER"
