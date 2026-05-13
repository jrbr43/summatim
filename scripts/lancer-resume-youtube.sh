#!/usr/bin/env bash
# Lance le CLI dans un terminal visible (double-clic / raccourci bureau).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$REPO_ROOT/target/release/resume-youtube"

if [[ ! -x "$BIN" ]]; then
  echo "Binaire introuvable ou non exécutable : $BIN" >&2
  echo "Compilez depuis la racine du dépôt :" >&2
  echo "  cargo build --release -p resume_youtube_cli" >&2
  echo
  read -rp "Appuyez sur Entrée pour fermer... " _
  exit 1
fi

"$BIN" --help 2>&1 || true
echo
echo "--------------------------------------------------------------------"
echo "C'est un outil en ligne de commande : utilisez les sous-commandes"
echo "(resume-video, resume-channel, test-suite, …) depuis un terminal."
echo "Exemple : $BIN resume-video --help"
echo "--------------------------------------------------------------------"
echo
read -rp "Appuyez sur Entrée pour fermer... " _
