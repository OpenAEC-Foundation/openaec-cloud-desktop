#!/usr/bin/env bash
# Proef stap 5b (ontwerp §5.6): twee gebruikers in één ruimte, elk met een eigen syncmap en een eigen sync-client
# (openaec-storage), tegen een echte openaec-storage-server met moto als S3. Een echt IFC-bestand uit Open Planner
# Studio, en per geval een controle van de uitkomst op de server zelf.
#
#   (a) verschillende taken tegelijk gewijzigd     -> één samengevoegde versie, geen conflictkopie
#   (b) dezelfde taak verschillend gewijzigd       -> conflictkopie met rapport, niets verloren
#   (c) opslaan terwijl de sync samenvoegt         -> beide wijzigingen in de volgende versie
#   (d) N uploads tegelijk op dezelfde basis       -> precies één wint, de rest geweigerd of kopie, geen versie weg
#   (e) bestand weg en opnieuw gemaakt (offline)   -> onbekende_basis, conflictkopie
#
# Gebruik: scripts/proef-twee-clients.sh <werkkopie openaec-storage> > docs/samenwerken/proef-twee-clients.log 2>&1
# Bouw eerst de client: cargo build --features cli (in crates/openaec-sync). De laatste regel is EXIT:<code>.
#
# Op aia, in /home/aia09/bench-samenwerken: een eigen site (proef.localhost), eigen units (proef-5b-web op
# 127.0.0.1:18310, proef-5b-s3 op 127.0.0.1:18311), het slot van test.sh (.testlock) en een zware plek zolang de
# proef loopt.
# Daarna: units weg, site weg, apps/openaec_storage terug naar waar hij stond.
set -uo pipefail
B=/home/aia09/bench-samenwerken
. "$B/env.sh"
SITE=proef.localhost
POORT=18310
S3POORT=18311
ADRES="http://127.0.0.1:$POORT"
REPO=$(cd "$(dirname "$0")/.." && pwd)
CLI="$REPO/crates/openaec-sync/target/debug/openaec-storage"
HULP="$REPO/scripts/proef-twee-clients.py"
PY="$BENCH_DIR/env/bin/python"
VOORBEELD=/home/aia09/projects/open-planner-studio/examples/09-rioolvervanging-delft.ifc
N_RACE=8

if [ $# -lt 1 ]; then echo "gebruik: $0 <werkkopie openaec-storage>" >&2; exit 2; fi
WK=$(realpath "$1") || exit 2
[ -x "$CLI" ] || { echo "eerst bouwen: cargo build --features cli (in crates/openaec-sync)" >&2; exit 2; }
[ -f "$VOORBEELD" ] || { echo "voorbeeld ontbreekt: $VOORBEELD" >&2; exit 2; }

code=99
WERK=$(mktemp -d /tmp/proef-5b.XXXXXX)
LINK="$BENCH_DIR/apps/openaec_storage"
# Wat de opruimstap mag terugdraaien: alleen wat dit script zelf veranderde of startte.
OUD_LINK=""
DB_GESTART=0
SITE_GEMAAKT=0

opruimen() {
	echo "== opruimen"
	systemctl --user stop proef-5b-web proef-5b-s3 2>/dev/null
	systemctl --user reset-failed proef-5b-web proef-5b-s3 2>/dev/null
	if [ "$SITE_GEMAAKT" = 1 ] && [ -d "$BENCH_DIR/sites/$SITE" ]; then
		(cd "$BENCH_DIR" && bench drop-site "$SITE" --db-root-password bench --no-backup --force >/dev/null 2>&1) \
			&& echo "site $SITE verwijderd" || echo "LET OP: site $SITE niet te verwijderen"
	fi
	[ "$SITE_GEMAAKT" = 1 ] && rm -rf "$BENCH_DIR/archived/sites/$SITE" "$BENCH_DIR/archived_sites/$SITE"
	if [ -n "$OUD_LINK" ]; then
		ln -sfn "$OUD_LINK" "$LINK" && echo "apps/openaec_storage weer -> $OUD_LINK"
	fi
	if [ "$DB_GESTART" = 1 ]; then
		systemctl --user stop bench-samen-mariadb bench-samen-valkey 2>/dev/null && echo "mariadb en valkey weer gestopt"
	fi
	rm -rf "$WERK"
	flock -u 8 2>/dev/null
	flock -u 9 2>/dev/null
}
trap 'opruimen; echo "EXIT:$code"' EXIT

# --- opzet --------------------------------------------------------------------------------------------------------

exec 9>"$B/.testlock"
if ! flock -n 9; then
	echo "wachten op het slot van test.sh ($B/.testlock)..."
	flock 9
fi
# Een zware run op aia: ook een van de twee zware plekken, en minstens 2 GB vrij geheugen (zoals test.sh, in dezelfde
# volgorde: eerst het testslot, dan de plek).
while :; do
	for plek in /home/aia09/worktrees/.zwaar.lock /home/aia09/worktrees/.zwaar.2; do
		exec 8>"$plek"
		flock -n 8 && break 2
		exec 8>&-
	done
	sleep 5
done
while [ "$(awk '/MemAvailable/ {print $2}' /proc/meminfo)" -lt 2000000 ]; do sleep 10; done
echo "== proef 5b, $(date -Iseconds)"
echo "client: $(git -C "$REPO" rev-parse --abbrev-ref HEAD) $(git -C "$REPO" rev-parse --short HEAD)"
echo "server: $(git -C "$WK" rev-parse --abbrev-ref HEAD) $(git -C "$WK" rev-parse --short HEAD)$(git -C "$WK" diff --quiet HEAD && echo ", schoon" || echo ", MET LOKALE WIJZIGINGEN")"
echo "voorbeeld: $VOORBEELD ($(grep -c 'IFCTASK(' "$VOORBEELD") taken)"

systemctl --user is-active --quiet bench-samen-mariadb || DB_GESTART=1
"$B/start.sh" || exit
OUD_LINK=$(readlink "$LINK")
ln -sfn "$WK" "$LINK"
cd "$BENCH_DIR" || exit

echo "== site $SITE"
SITE_GEMAAKT=1
bench new-site "$SITE" --db-root-password bench --admin-password proef --mariadb-user-host-login-scope=% \
	--install-app openaec_storage >"$WERK/new-site.log" 2>&1 || { tail -30 "$WERK/new-site.log"; exit; }
for kv in "openaec_storage_endpoint=http://127.0.0.1:$S3POORT" openaec_storage_bucket=proef openaec_storage_region=eu-central-1; do
	bench --site "$SITE" set-config "${kv%%=*}" "${kv#*=}" >/dev/null || exit
done

echo "== moto (S3) en de server"
systemd-run --user --quiet --unit=proef-5b-s3 "$BENCH_DIR/env/bin/moto_server" -H 127.0.0.1 -p "$S3POORT" || exit
cat >"$WERK/proef_wsgi.py" <<EOF
import frappe.app

frappe.app._site = "$SITE"
application = frappe.app.application
EOF
systemd-run --user --quiet --unit=proef-5b-web -p WorkingDirectory="$BENCH_DIR/sites" \
	-p Environment=PYTHONPYCACHEPREFIX="$B/pycache" -p Environment=AWS_ACCESS_KEY_ID=proef \
	-p Environment=AWS_SECRET_ACCESS_KEY=proef \
	"$BENCH_DIR/env/bin/gunicorn" -b "127.0.0.1:$POORT" -w 4 -t 120 --pythonpath "$WERK" proef_wsgi:application || exit
for _ in $(seq 90); do
	curl -fsS "$ADRES/api/method/ping" 2>/dev/null | grep -q pong && ss -tln | grep -q "127.0.0.1:$S3POORT " && break
	sleep 1
done
curl -fsS "$ADRES/api/method/ping" | grep -q pong || { echo "de server antwoordt niet"; exit; }

uit=$(cd "$BENCH_DIR/sites" && AWS_ACCESS_KEY_ID=proef AWS_SECRET_ACCESS_KEY=proef "$PY" "$HULP" opzet "$BENCH_DIR/sites" "$SITE" "http://127.0.0.1:$S3POORT") \
	|| { echo "$uit"; exit; }
KA=$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["a"])' "$uit")
KB=$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["b"])' "$uit")
RUIMTE=$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["ruimte"])' "$uit")
echo "ruimte $RUIMTE, gebruikers proef-a en proef-b (schrijver)"

# Per gebruiker een eigen configuratie (XDG_CONFIG_HOME) en een eigen syncmap.
client() {  # client <a|b> <opdracht...>
	local wie=$1; shift
	XDG_CONFIG_HOME="$WERK/$wie/config" "$CLI" "$@"
}
for wie in a b; do
	mkdir -p "$WERK/$wie/config" "$WERK/$wie/map"
	sleutel=$KA; [ "$wie" = b ] && sleutel=$KB
	OPENAEC_SLEUTEL=$sleutel client "$wie" inloggen "$ADRES" --sleutel >/dev/null || exit
	client "$wie" koppel "$RUIMTE" "$WERK/$wie/map" || exit
done
A="$WERK/a/map"
Bm="$WERK/b/map"

sync() {  # sync <a|b>: één ronde, met de uitvoer van de client
	echo "  sync $1: $(client "$1" sync 2>&1 | tr '\n' ' ')"
}
srv() {  # srv <opdracht...>: vraag aan de server (als gebruiker A)
	"$PY" "$HULP" server "$ADRES" "$KA" "$RUIMTE" "$@"
}
wijzig() {  # wijzig <bestand> <globalid> <naam>: een planner geeft een taak een andere naam en slaat op
	"$PY" "$HULP" wijzig "$1" "$2" naam "$3"
}

declare -A UITKOMST
GEVAL=""
geval() {
	GEVAL=$1
	UITKOMST[$GEVAL]=OK
	echo
	echo "== geval ($1): $2"
}
eis() {  # eis <omschrijving> <python-expressie over j> <json>
	if python3 -c "import json,sys; j=json.loads(sys.argv[1]); sys.exit(0 if ($2) else 1)" "$3" 2>/dev/null; then
		echo "  ok: $1"
	else
		echo "  MISLUKT: $1"
		echo "    gegeven: $3"
		UITKOMST[$GEVAL]=MISLUKT
	fi
}
taak() {  # taak <pad> <globalid> [versie_id]: de taak in die versie op de server
	srv taak "$@"
}
afgeleid() {  # afgeleid <map> <pad>: staat het bestand in de toestand als "afgeleid"?
	python3 -c 'import json,sys; t=json.load(open(sys.argv[1]+"/.openaec/staat.json")); print(json.dumps({"afgeleid": "afgeleid" in t["bestanden"].get(sys.argv[2], {})}))' "$1" "$2"
}

mapfile -t GID < <(grep -oP "IFCTASK\('\K[^']+" "$VOORBEELD" | head -4)

echo
echo "== beginstand: A zet vijf planningen in de ruimte, B haalt ze op"
for g in a b c d e; do cp "$VOORBEELD" "$A/$g.ifc"; done
sync a
sync b
eis "B heeft de vijf bestanden" "j['n'] == 5" "{\"n\": $(ls "$Bm"/*.ifc | wc -l)}"

# --- (a) -----------------------------------------------------------------------------------------------------------
geval a "A en B wijzigen tegelijk een andere taak"
wijzig "$A/a.ifc" "${GID[0]}" "a: taak van A"
wijzig "$Bm/a.ifc" "${GID[1]}" "a: taak van B"
sync a
sync b
v=$(srv versies a.ifc); echo "  versies: $v"
eis "drie versies; de nieuwste is samengevoegd uit 1 en 2" \
	"len(j['versies']) == 3 and j['versies'][0]['samengevoegd_uit'] in ({'basis': 1, 'hun': 2}, '{\"basis\": 1, \"hun\": 2}')" "$v"
eis "taak 1 heeft de naam van A" "j['naam'] == 'a: taak van A'" "$(taak a.ifc "${GID[0]}")"
eis "taak 2 heeft de naam van B" "j['naam'] == 'a: taak van B'" "$(taak a.ifc "${GID[1]}")"
p=$(srv paden | python3 -c 'import json,sys; print(json.dumps([l for l in sys.stdin.read().splitlines() if l.startswith("a ")]))')
eis "geen conflictkopie van a.ifc" "j == []" "$p"
sync a
eis "A heeft hier de samengevoegde versie" "j['naam'] == 'a: taak van B'" "$("$PY" "$HULP" taak "$A/a.ifc" "${GID[1]}")"
eis "B heeft hier de samengevoegde versie" "j['naam'] == 'a: taak van A'" "$("$PY" "$HULP" taak "$Bm/a.ifc" "${GID[0]}")"

# --- (b) -----------------------------------------------------------------------------------------------------------
geval b "A en B geven dezelfde taak een andere naam"
wijzig "$A/b.ifc" "${GID[0]}" "b: naam van A"
wijzig "$Bm/b.ifc" "${GID[0]}" "b: naam van B"
sync a
sync b
sync b
paden=$(srv paden); echo "  op de server: $(echo "$paden" | grep '^b ' | tr '\n' '|')"
kopie=$(echo "$paden" | grep -E '^b .*\.ifc$' | head -1)
rapport=$(echo "$paden" | grep -E '^b .*\.ifc\.samenvoegrapport\.json$' | head -1)
eis "op b.ifc de naam van A" "j['naam'] == 'b: naam van A'" "$(taak b.ifc "${GID[0]}")"
eis "een conflictkopie met de naam van B" "j.get('naam') == 'b: naam van B'" "$([ -n "$kopie" ] && taak "$kopie" "${GID[0]}" || echo '{}')"
eis "het rapport staat naast de conflictkopie op de server" "j['rapport'] == j['kopie'] + '.samenvoegrapport.json'" \
	"$(python3 -c 'import json,sys; print(json.dumps({"kopie": sys.argv[1], "rapport": sys.argv[2]}))' "$kopie" "$rapport")"
r=$(cat "$Bm/$rapport" 2>/dev/null || echo '{}'); echo "  rapport: $(echo "$r" | python3 -c 'import json,sys; j=json.load(sys.stdin); print({k: j.get(k) for k in ("bestand", "conflictkopie", "reden", "basis", "huidige")})')"
eis "het rapport noemt de reden en het conflict" \
	"j.get('reden') == 'conflicten' and j.get('bestand') == 'b.ifc' and len(j['samenvoeging']['conflicten']) >= 1" "$r"

# --- (c) -----------------------------------------------------------------------------------------------------------
geval c "B slaat op terwijl de sync samenvoegt"
wijzig "$A/c.ifc" "${GID[0]}" "c: taak van A"
sync a
wijzig "$Bm/c.ifc" "${GID[1]}" "c: taak van B"
"$PY" "$HULP" wacht-en-wijzig "$Bm" "$Bm/c.ifc" "${GID[2]}" naam "c: later opgeslagen door B" >"$WERK/wacht.log" 2>&1 &
wachter=$!
sleep 1
sync b
wait $wachter; echo "  wachter: $(tr '\n' ' ' <"$WERK/wacht.log")"
eis "de sync zette het resultaat niet over het opgeslagen werk heen (afgeleid)" "j['afgeleid']" "$(afgeleid "$Bm" c.ifc)"
eis "het opgeslagen werk staat er nog" "j['naam'] == 'c: later opgeslagen door B'" "$("$PY" "$HULP" taak "$Bm/c.ifc" "${GID[2]}")"
sync b
v=$(srv versies c.ifc); echo "  versies: $v"
eis "vier versies" "len(j['versies']) == 4" "$v"
eis "taak 1: A" "j['naam'] == 'c: taak van A'" "$(taak c.ifc "${GID[0]}")"
eis "taak 2: B" "j['naam'] == 'c: taak van B'" "$(taak c.ifc "${GID[1]}")"
eis "taak 3: tijdens het samenvoegen opgeslagen" "j['naam'] == 'c: later opgeslagen door B'" "$(taak c.ifc "${GID[2]}")"
p=$(srv paden | python3 -c 'import json,sys; print(json.dumps([l for l in sys.stdin.read().splitlines() if l.startswith("c ")]))')
eis "geen conflictkopie van c.ifc" "j == []" "$p"
eis "B is daarna niet meer afgeleid" "not j['afgeleid']" "$(afgeleid "$Bm" c.ifc)"

# --- (d) -----------------------------------------------------------------------------------------------------------
geval d "$N_RACE uploads tegelijk op dezelfde basis"
if uit=$(srv race d.ifc "$N_RACE" 2>&1); then echo "$uit" | sed 's/^/  /'; else echo "$uit" | sed 's/^/  /'; UITKOMST[d]=MISLUKT; fi

# --- (e) -----------------------------------------------------------------------------------------------------------
geval e "A verwijdert e.ifc en maakt een nieuw; B werkte offline door"
wijzig "$Bm/e.ifc" "${GID[0]}" "e: offline werk van B"
rm "$A/e.ifc"
sync a
cp "$VOORBEELD" "$A/e.ifc"
wijzig "$A/e.ifc" "${GID[1]}" "e: nieuw bestand van A"
sync a
sync b
sync b
paden=$(srv paden); echo "  op de server: $(echo "$paden" | grep '^e ' | tr '\n' '|')"
kopie=$(echo "$paden" | grep -E '^e .*\.ifc$' | head -1)
eis "op e.ifc het nieuwe bestand van A (één versie)" "len(j['versies']) == 1" "$(srv versies e.ifc)"
eis "het nieuwe bestand heeft de naam van A" "j['naam'] == 'e: nieuw bestand van A'" "$(taak e.ifc "${GID[1]}")"
eis "het offline werk van B staat in een conflictkopie" "j.get('naam') == 'e: offline werk van B'" "$([ -n "$kopie" ] && taak "$kopie" "${GID[0]}" || echo '{}')"
r=$(cat "$Bm/$kopie.samenvoegrapport.json" 2>/dev/null || echo '{}')
eis "het rapport zegt onbekende_basis" "j.get('reden') == 'onbekende_basis'" "$r"
eis "het rapport staat ook op de server" "j['n'] == 1" "{\"n\": $(echo "$paden" | grep -cxF "$kopie.samenvoegrapport.json")}"

# --- uitkomst ------------------------------------------------------------------------------------------------------
echo
echo "== uitkomst"
code=0
for g in a b c d e; do
	echo "  ($g) ${UITKOMST[$g]:-NIET GEDRAAID}"
	[ "${UITKOMST[$g]:-}" = OK ] || code=1
done
exit $code
