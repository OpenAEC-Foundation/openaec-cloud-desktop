#!/usr/bin/env bash
# Draait alle scenarioproeven en schrijft per proef een log naar docs/samenwerken/scenario-m365/<naam>.txt, met
# EXIT:<code> als laatste regel. (De logs zijn .txt, want *.log staat in .gitignore.)
#
# Gebruik: alles.sh [werkkopie van openaec-storage]
#   Zonder werkkopie slaat het proef 08 (de tests van openaec-storage) over.
#   ZWAAR=<commando> zet dat commando voor de zware proeven 06 en 07 (op aia: /home/aia09/worktrees/zwaar.sh).
set -uo pipefail
HIER="$(cd "$(dirname "$0")" && pwd)"
LOGS="$HIER/../../docs/samenwerken/scenario-m365"
mkdir -p "$LOGS"

draai() {
  local naam="$1"
  shift
  "$@" > "$LOGS/$naam.txt" 2>&1
  echo "EXIT:$?" >> "$LOGS/$naam.txt"
  echo "$naam: $(tail -1 "$LOGS/$naam.txt")"
}

for s in 01-planning-samenvoegen 02-model-samenvoegen 03-conflict-zelfde-eigenschap 04-offline 05-terugzetten; do
  draai "$s" bash "$HIER/$s.sh"
done
# shellcheck disable=SC2086 # ZWAAR is een commando met eventuele opties.
draai 06-opslaan-lus ${ZWAAR:-} bash "$HIER/06-opslaan-lus.sh"
# shellcheck disable=SC2086
draai 07-blokken ${ZWAAR:-} bash "$HIER/07-blokken.sh"
if [ -n "${1:-}" ]; then
  draai 08-storage-tests bash "$HIER/08-storage-tests.sh" "$1"
fi
