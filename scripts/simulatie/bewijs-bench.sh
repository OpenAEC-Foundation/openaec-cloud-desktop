#!/usr/bin/env bash
# bewijs-bench.sh: de proefbench werkt naast bench-samenwerken (bewijs 2, stap A).
#   Houdt één zware plek (zwaar.sh) en draait daarin steeds test_namen op de proefbench, tot test.sh op
#   bench-samenwerken (die zelf de andere plek neemt) test_zoek_pad heeft gedraaid. Zo overlappen de twee runs zeker.
#   Log: docs/samenwerken/proeven/<datum>-a-twee-benches.log, met EXIT:<code> als laatste regel.
set -uo pipefail
HIER=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HIER/../.." && pwd)
LOG="$REPO/docs/samenwerken/proeven/$(date +%Y-%m-%d)-a-twee-benches.log"
E=/home/aia09/bench-proef/werk/a-eerste.log
FB=/home/aia09/bench-proef/frappe-bench
mkdir -p "$(dirname "$E")"
rm -f "$E"
export E FB
{
  echo "a-twee-benches: $(date '+%Y-%m-%d %H:%M:%S'), server $(git -C /home/aia09/worktrees/storage-proef rev-parse --short HEAD)"
  /home/aia09/worktrees/zwaar.sh bash -c '
    export PYTHONPATH=/home/aia09/worktrees/storage-proef PYTHONPYCACHEPREFIX=/home/aia09/bench-proef/pycache FRAPPE_BENCH_ROOT=$FB
    ( echo "bench-samenwerken start $(date +%T)"
      /home/aia09/bench-samenwerken/test.sh /home/aia09/worktrees/storage-proef openaec_storage.tests.test_zoek_pad
      echo "EXIT:$?"; echo "bench-samenwerken eind $(date +%T)" ) > "$E" 2>&1 &
    n=0
    while ! grep -q "^EXIT:" "$E" && [ $n -lt 40 ]; do
      n=$((n + 1))
      echo "proefbench ronde $n start $(date +%T); bench-samenwerken: $(grep -m1 -o "run-tests\|apps/openaec_storage -> [^ ]*" "$E" || echo wacht nog)"
      (cd "$FB/sites" && "$FB/env/bin/python" -m frappe.utils.bench_helper frappe --site proeftest.localhost run-tests \
        --app openaec_storage --module openaec_storage.tests.test_namen 2>&1 | grep -E "^Ran |^OK|FAILED|Error")
      echo "proefbench ronde $n eind $(date +%T)"
    done
    wait
    echo "--- test.sh op bench-samenwerken:"
    tr "\r" "\n" < "$E" | grep -E "start|eind|EXIT|test.sh:|^Ran |^OK|FAILED"
    grep -q "^EXIT:0" "$E"
  '
  echo "EXIT:$?"
} > "$LOG" 2>&1
tail -n 1 "$LOG"
