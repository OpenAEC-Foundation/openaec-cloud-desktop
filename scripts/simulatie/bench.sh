#!/usr/bin/env bash
# Een tweede, lichte proefbench voor de simulatie (bewijs 2), naast /home/aia09/bench-samenwerken.
#
# Wat hij deelt met de eerste bench (alleen lezen): de MariaDB (127.0.0.1:3407) en Valkey (127.0.0.1:6479), de map
# apps/frappe en de venv (symlinks). Wat eigen is: sites/ met twee eigen sites, logs/, pycache/, en
# apps/openaec_storage -> de servercode van de proef. PYTHONPATH zet die code vóór de .pth-regel van de eerste bench,
# en FRAPPE_BENCH_ROOT laat Frappe deze bench gebruiken voor logs en wachtrijnamen.
#
#   bench.sh opzet                 maak de bench en de sites (eenmalig; daarna doet het niets meer)
#   bench.sh start                 moto (S3) en gunicorn als user-units; wacht tot beide antwoorden
#   bench.sh stop                  stop beide units
#   bench.sh test <module>         één testmodule van openaec-storage op de testsite (via zwaar.sh)
#   bench.sh python <args...>      de Python van de venv, met de omgeving van deze bench
#   bench.sh frappe <args...>      een Frappe-opdracht op de simulatiesite (bijv. migrate)
set -euo pipefail

P=${BENCH_PROEF:-/home/aia09/bench-proef}
EERSTE=/home/aia09/bench-samenwerken
SERVER=${SERVER_CODE:-/home/aia09/worktrees/storage-proef}
SIM_SITE=sim.localhost
TEST_SITE=proeftest.localhost
WEB_POORT=${WEB_POORT:-18200}
S3_POORT=${S3_POORT:-18210}
# Het S3-adres in de ondertekende links: daar luistert de proxy van de chaosproef (proxy.py). Zonder proxy: S3_POORT.
S3_PUBLIEK=${S3_PUBLIEK:-18211}
FB=$P/frappe-bench
ZWAAR=/home/aia09/worktrees/zwaar.sh  # new-site, migrate, tests: zware runs, hoogstens twee tegelijk op aia

export PATH="$EERSTE/mariadb/bin:$HOME/.local/bin:$PATH"
export PYTHONPATH="$SERVER"
export PYTHONPYCACHEPREFIX="$P/pycache"
export FRAPPE_BENCH_ROOT="$FB"
export AWS_ACCESS_KEY_ID=proef AWS_SECRET_ACCESS_KEY=proef AWS_DEFAULT_REGION=eu-central-1

frappe_op() {  # <site> <opdracht...>: zoals `bench --site`, maar zonder de bench-CLI (die zoekt zelf zijn paden)
  local site=$1; shift
  (cd "$FB/sites" && "$FB/env/bin/python" -m frappe.utils.bench_helper frappe --site "$site" "$@")
}

opzet() {
  if [ -f "$P/.opgezet" ]; then echo "bench.sh: $P staat er al"; return 0; fi
  mkdir -p "$FB/apps" "$FB/sites" "$FB/logs" "$P/pycache" "$P/wsgi"
  ln -sfn "$EERSTE/frappe-bench/apps/frappe" "$FB/apps/frappe"
  ln -sfn "$EERSTE/frappe-bench/env" "$FB/env"
  ln -sfn "$SERVER" "$FB/apps/openaec_storage"
  printf 'frappe\nopenaec_storage\n' > "$FB/sites/apps.txt"
  cat > "$FB/sites/common_site_config.json" <<'JSON'
{
 "bench_id": "bench-proef",
 "db_host": "127.0.0.1",
 "db_port": 3407,
 "redis_cache": "redis://127.0.0.1:6479",
 "redis_queue": "redis://127.0.0.1:6479",
 "redis_socketio": "redis://127.0.0.1:6479",
 "use_redis_auth": false,
 "serve_default_site": true,
 "default_site": "sim.localhost"
}
JSON
  cat > "$P/wsgi/proef_wsgi.py" <<PY
# WSGI-ingang voor gunicorn: elke aanvraag gaat naar site $SIM_SITE, ongeacht de Host-header.
import frappe.app

frappe.app._site = "$SIM_SITE"
application = frappe.app.application
PY
  for site in "$SIM_SITE" "$TEST_SITE"; do
    (cd "$FB/sites" && "$ZWAAR" "$FB/env/bin/python" -m frappe.utils.bench_helper frappe new-site "$site" \
      --db-root-username root --db-root-password bench --admin-password admin \
      --mariadb-user-host-login-scope=% --install-app openaec_storage)
  done
  frappe_op "$TEST_SITE" set-config allow_tests true
  frappe_op "$SIM_SITE" set-config openaec_storage_bucket sim
  frappe_op "$SIM_SITE" set-config openaec_storage_endpoint "http://127.0.0.1:$S3_POORT"
  frappe_op "$SIM_SITE" set-config openaec_storage_endpoint_publiek "http://127.0.0.1:$S3_PUBLIEK"
  schrijf_readme
  touch "$P/.opgezet"
  echo "bench.sh: $P opgezet"
}

schrijf_readme() {
  cat > "$P/README.md" <<EOF
# Proefbench voor de simulatie

Gemaakt door \`scripts/simulatie/bench.sh opzet\` in openaec-cloud-desktop (branch samenwerken/p2-simulatie).
Een lichte tweede bench naast \`$EERSTE\`, zodat lange simulaties de tests van anderen niet blokkeren.

- Gedeeld met de eerste bench, alleen lezen: MariaDB 127.0.0.1:3407, Valkey 127.0.0.1:6479, \`apps/frappe\` en de
  venv (symlinks). Niets in de eerste bench wordt gewijzigd.
- Eigen: \`frappe-bench/sites/\` met \`$SIM_SITE\` (simulatie) en \`$TEST_SITE\` (tests), \`logs/\`, \`pycache/\`,
  \`apps/openaec_storage\` -> \`$SERVER\`.
- \`PYTHONPATH=$SERVER\` gaat vóór de .pth-regel van de eerste bench, dus \`import openaec_storage\` laadt de proefcode.
  \`FRAPPE_BENCH_ROOT\` en \`bench_id\` houden logs en wachtrijen gescheiden van de eerste bench.
- Units: \`bench-proef-s3\` (moto, 127.0.0.1:$S3_POORT) en \`bench-proef-web\` (gunicorn, 127.0.0.1:$WEB_POORT).
  Ondertekende links wijzen naar 127.0.0.1:$S3_PUBLIEK (de S3-proxy van de chaosproef).
- Gebruik: \`bench.sh start | stop | test <module> | python ... | frappe ...\`.
EOF
}

start() {
  "$EERSTE/start.sh"  # MariaDB en Valkey (doet niets als ze al draaien)
  # Op deze bench draait geen worker: de taken van Frappe zelf (bijv. create_contact bij elke opslag van een gebruiker)
  # stapelen zich op, en bij 600 weigert Frappe nieuwe (QueueOverloaded). Alleen de wachtrijen van deze bench gaan leeg.
  local vk="$EERSTE/valkey/usr/bin/valkey-cli -h 127.0.0.1 -p 6479"
  export LD_LIBRARY_PATH="$EERSTE/valkey/usr/lib/x86_64-linux-gnu"
  for q in $($vk --scan --pattern 'rq:queue:*bench-proef*'); do
    for j in $($vk lrange "$q" 0 -1); do $vk del "rq:job:$j" >/dev/null; done
    $vk del "$q" >/dev/null
  done
  grep -q '"max_queued_jobs"' "$FB/sites/$SIM_SITE/site_config.json" || frappe_op "$SIM_SITE" set-config max_queued_jobs 100000
  # De lastproef maakt honderden proefgebruikers; Frappe remt na 60 per minuut (throttle_user_creation).
  grep -q '"throttle_user_limit"' "$FB/sites/$SIM_SITE/site_config.json" || frappe_op "$SIM_SITE" set-config throttle_user_limit 10000
  for unit in bench-proef-s3 bench-proef-web; do
    systemctl --user stop "$unit" 2>/dev/null || true
    systemctl --user reset-failed "$unit" 2>/dev/null || true
  done
  systemd-run --user --quiet --unit=bench-proef-s3 "$FB/env/bin/moto_server" -H 127.0.0.1 -p "$S3_POORT"
  systemd-run --user --quiet --unit=bench-proef-web -p WorkingDirectory="$FB/sites" \
    -p Environment=PYTHONPATH="$SERVER" -p Environment=PYTHONPYCACHEPREFIX="$P/pycache" \
    -p Environment=FRAPPE_BENCH_ROOT="$FB" -p Environment=AWS_ACCESS_KEY_ID=proef \
    -p Environment=AWS_SECRET_ACCESS_KEY=proef -p Environment=AWS_DEFAULT_REGION=eu-central-1 \
    "$FB/env/bin/gunicorn" -b "127.0.0.1:$WEB_POORT" -w "${WERKERS:-4}" -t 120 --pythonpath "$P/wsgi" \
    proef_wsgi:application
  for _ in $(seq 90); do
    if curl -fsS "http://127.0.0.1:$WEB_POORT/api/method/ping" 2>/dev/null | grep -q pong \
       && curl -s -o /dev/null "http://127.0.0.1:$S3_POORT/"; then
      echo "bench.sh: web op 127.0.0.1:$WEB_POORT, S3 op 127.0.0.1:$S3_POORT"; return 0
    fi
    sleep 1
  done
  echo "bench.sh: geen antwoord na 90 s; zie journalctl --user -u bench-proef-web / bench-proef-s3" >&2
  return 1
}

stop() {
  for unit in bench-proef-web bench-proef-s3; do
    systemctl --user stop "$unit" 2>/dev/null || true
    systemctl --user reset-failed "$unit" 2>/dev/null || true
  done
}

case "${1:-}" in
  opzet) opzet ;;
  start) start ;;
  stop) stop ;;
  test)
    [ $# -eq 2 ] || { echo "gebruik: $0 test <module>" >&2; exit 2; }
    "$EERSTE/start.sh"
    "$ZWAAR" bash -c "cd '$FB/sites' && '$FB/env/bin/python' -m frappe.utils.bench_helper frappe --site '$TEST_SITE' \
      migrate >/dev/null && '$FB/env/bin/python' -m frappe.utils.bench_helper frappe --site '$TEST_SITE' run-tests \
      --app openaec_storage --module '$2'"
    ;;
  python) shift; exec "$FB/env/bin/python" "$@" ;;
  frappe) shift; (cd "$FB/sites" && "$ZWAAR" "$FB/env/bin/python" -m frappe.utils.bench_helper frappe --site "$SIM_SITE" "$@") ;;
  *) sed -n '2,15p' "$0"; exit 2 ;;
esac
