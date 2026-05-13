#!/usr/bin/env bash
# Crée le dépôt public github.com/jrbr43/summatim et pousse la branche main (nécessite gh authentifié).
set -euo pipefail
racine_projet="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$racine_projet"

if gh auth status -h github.com &>/dev/null; then
  echo "Création du dépôt summatim et envoi des commits…"
  gh repo create summatim --public --source=. --remote=origin --push
  echo "Dépôt : https://github.com/jrbr43/summatim"
else
  echo "Erreur : GitHub CLI (gh) n’est pas authentifié pour github.com."
  echo "Exécutez : gh auth login -h github.com -p ssh -s \"repo,read:org,gist\""
  echo "Puis relancez ce script."
  exit 1
fi
