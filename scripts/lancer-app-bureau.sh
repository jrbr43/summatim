#!/usr/bin/env bash
# Lance l’application graphique Tauri (fenêtre qui reste ouverte).
set -euo pipefail

racine_depot="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binaire_app="$racine_depot/target/release/resume_youtube_tauri"

if [[ ! -x "$binaire_app" ]]; then
  message="Application introuvable : $binaire_app
Compilez avec : (cd \"$racine_depot\" && env -u CI npm run build)"
  if command -v zenity >/dev/null 2>&1; then
    zenity --error --text="$message" --title="Resume YouTube"
  elif command -v kdialog >/dev/null 2>&1; then
    kdialog --error "$message"
  else
    echo "$message" >&2
    read -rp "Entrée pour fermer... " _
  fi
  exit 1
fi

cd "$racine_depot"
exec "$binaire_app" "$@"
