#!/usr/bin/env bash
# Draait alle scenarioproeven en schrijft per proef een log naar docs/samenwerken/scenario-m365/<naam>.txt, met
# EXIT:<code> als laatste regel. (De logs zijn .txt, want *.log staat in .gitignore.)
#
# Gebruik: alles.sh [werkkopie van openaec-storage]
#   Zonder werkkopie slaat het proef 08 (de tests van openaec-storage) over.
#   ZWAAR=<commando> zet dat commando voor de zware proeven 06 en 07 (op aia: /home/aia09/worktrees/zwaar.sh).
#   OVERSLAAN="07-blokken ..." slaat die proeven over; hun oude log blijft staan.
# Eindigt met 1 zodra één proef mislukt (een EXIT anders dan 0).
set -uo pipefail
HIER="$(cd "$(dirname "$0")" && pwd)"
LOGS="$HIER/../../docs/samenwerken/scenario-m365"
mkdir -p "$LOGS"

fout=0
draai() {
  local naam="$1" code
  shift
  case " ${OVERSLAAN:-} " in
    *" $naam "*) echo "$naam: overgeslagen (oude log blijft)"; return ;;
  esac
  "$@" > "$LOGS/$naam.txt" 2>&1
  code=$?
  echo "EXIT:$code" >> "$LOGS/$naam.txt"
  echo "$naam: EXIT:$code"
  [ "$code" = 0 ] || fout=1
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
exit "$fout"
