#!/usr/bin/env bash
# draai.sh [--alleen] <naam> <script> [argumenten...]
#   Start de proefbench opnieuw (moto begint leeg), draait <script> (simulatie.py of last.py) via zwaar.sh, en schrijft
#   het log naar docs/samenwerken/proeven/<datum>-<naam>.log, met EXIT:<code> als laatste regel.
#   --alleen: via `zwaar.sh --alleen` (niets anders zwaars tegelijk), voor de runs met honderden clients.
#   Voorbeeld: draai.sh b-zaad1 simulatie.py --clients 8 --beurten 20 --zaad 1
set -uo pipefail
HIER=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HIER/../.." && pwd)
ALLEEN=()
[ "${1:-}" = "--alleen" ] && { ALLEEN=(--alleen); shift; }
[ $# -ge 2 ] || { sed -n '2,6p' "$0"; exit 2; }
NAAM=$1 SCRIPT=$2; shift 2
LOG="$REPO/docs/samenwerken/proeven/$(date +%Y-%m-%d)-$NAAM.log"
WERK=/home/aia09/bench-proef/werk/$NAAM
mkdir -p "$(dirname "$LOG")"
{
  echo "draai.sh: $NAAM, $(date '+%Y-%m-%d %H:%M:%S'), desktop $(git -C "$REPO" rev-parse --short HEAD)," \
    "server $(git -C /home/aia09/worktrees/storage-proef rev-parse --short HEAD) (integratie-4-4b)"
  echo "draai.sh: $SCRIPT $*"
  /home/aia09/worktrees/zwaar.sh "${ALLEEN[@]}" bash -c "'$HIER/bench.sh' start && '$HIER/bench.sh' python '$HIER/$SCRIPT' --werk '$WERK' $*"
  echo "EXIT:$?"
} > "$LOG" 2>&1
tail -1 "$LOG"
