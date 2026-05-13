#!/usr/bin/env bash
set -euo pipefail

repertoire_script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fichier_appimage="$repertoire_script/Resume-YouTube"

if [[ ! -f "$fichier_appimage" ]]; then
  echo "Fichier introuvable: $fichier_appimage" >&2
  exit 1
fi

chmod +x "$fichier_appimage"

# Fallback sans FUSE: utile sur certaines machines.
if "$fichier_appimage" "$@" >/dev/null 2>&1; then
  exit 0
fi

exec "$fichier_appimage" --appimage-extract-and-run "$@"
