#!/usr/bin/env bash
# meet_groot.sh <meetkopie-programma> <uitmap> <opdracht> <bestand>...
# Meet één opdracht van de meetkopie (zie meetkopie.patch) op grote bestanden, één run tegelijk, met
# `zwaar.sh --alleen` en een harde grens van 2 GB. /usr/bin/time staat buiten de grens: stopt de grens het programma,
# dan blijven de tijd, de piek en de exitcode bewaard. De fasen (FASE-regels op stderr) laten zien hoe ver het kwam.
# <opdracht> is `tel` (één bestand per run) of `verschil` (telkens basis.ifc en ons.ifc uit de map van het bestand).
set -u
prog=${1:?}; uit=${2:?}; opdracht=${3:?}; shift 3
mkdir -p "$uit"
for f in "$@"; do
  naam="$(basename "$(dirname "$f")")-$opdracht"
  if [ "$opdracht" = verschil ]; then args=(verschil "$(dirname "$f")/basis.ifc" "$(dirname "$f")/ons.ifc"); else args=(tel "$f"); fi
  /home/aia09/worktrees/zwaar.sh --alleen /usr/bin/time -v -o "$uit/$naam.tijd" \
    systemd-run --user --scope -q -p MemoryMax=2G -p MemorySwapMax=0 "$prog" "${args[@]}" \
    > "$uit/$naam.uit" 2> "$uit/$naam.fasen"
  code=$?
  echo "$naam: exit $code, $(grep -E 'Elapsed' "$uit/$naam.tijd" | sed 's/.*: //') , piek $(grep -E 'Maximum resident' "$uit/$naam.tijd" | sed 's/.*: //') kB, laatste fase: $(grep FASE "$uit/$naam.fasen" | tail -1 | tr '\t' ' ')"
done
