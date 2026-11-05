#!/usr/bin/env bash
# Proef 8: de bestaande tests van openaec-storage (main) voor de regels van de tabel die al op de server werken:
# conflictkopie, verwijderen en herstellen, slot per bestand, delen met wachtwoord en vervaldatum, blokken van
# 8 MiB, en de melding bij delen. Geen nieuwe servercode: dit draait de tests zoals ze op main staan.
#
# Gebruik: 08-storage-tests.sh <werkkopie van openaec-storage>
# Vraagt een Frappe-bench met een testsite. Op aia: /home/aia09/bench-samenwerken/test.sh (zet BENCH_TEST anders).
set -uo pipefail

WK="${1:?gebruik: $0 <werkkopie van openaec-storage>}"
BENCH_TEST="${BENCH_TEST:-/home/aia09/bench-samenwerken/test.sh}"

echo "openaec-storage: $(git -C "$WK" log -1 --format='%h %s')"
fout=0
for module in test_api test_vergrendelen_delen test_delta test_gedeeld_met_mij; do
  printf '\n== openaec_storage.tests.%s\n' "$module"
  "$BENCH_TEST" "$WK" "openaec_storage.tests.$module" 2>&1 | grep -E '^(test_|Ran |OK|FAILED|ERROR|FAIL:)' || true
  code=${PIPESTATUS[0]}
  echo "exitcode $code"
  [ "$code" = 0 ] || fout=1
done
exit "$fout"
