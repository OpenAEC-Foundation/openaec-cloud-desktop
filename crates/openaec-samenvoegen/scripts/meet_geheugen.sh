#!/usr/bin/env bash
# meet_geheugen.sh <commando…>
# Draait het commando en meldt de tijd en het hoogste geheugen, elke 0,2 s gemeten:
# - RssAnon: het eigen geheugen van het programma (tabellen, hashes);
# - RssFile: de delen van het bestand die via mmap in het geheugen staan. Die kan het besturingssysteem altijd weer
#   vrijgeven; onder een harde grens telt dit deel mee, maar het wordt dan kleiner in plaats van dat het programma stopt.
# Voor de metingen in docs/samenwerken/gb-schaal.md, met een harde grens van 2 GB:
#   systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 scripts/meet_geheugen.sh <commando…>
set -u
begin=$(date +%s.%N)
"$@" &
pid=$!
anon=0
bestand=0
while kill -0 "$pid" 2>/dev/null; do
  read -r a f < <(awk '/^RssAnon/ {a=$2} /^RssFile/ {f=$2} END {print a+0, f+0}' "/proc/$pid/status" 2>/dev/null || echo "0 0")
  [ "${a:-0}" -gt "$anon" ] && anon=$a
  [ "${f:-0}" -gt "$bestand" ] && bestand=$f
  sleep 0.2
done
wait "$pid"
code=$?
eind=$(date +%s.%N)
awk -v b="$begin" -v e="$eind" -v a="$anon" -v f="$bestand" -v c="$code" \
  'BEGIN {printf "meting: %.1f s, RssAnon max %d MB, RssFile max %d MB, exit %d\n", e - b, a / 1024, f / 1024, c}' >&2
exit "$code"
