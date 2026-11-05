# Gedeeld deel van de scenarioproeven. Wordt ingelezen door elk script (`. gemeen.sh`), niet los gedraaid.
#
# Het vaste voorbeeld: Anna en Piet werken tegelijk aan één IFC-bestand. Ze beginnen allebei bij versie 12.
# De bewerkingen doet het script zelf (een datum, een duur, een plaats, een eigenschap); daarna nummert het elk
# bestand opnieuw, zoals Open Planner Studio (OPS) bij elk opslaan doet. Elke proef stopt met een fout als
# onze onderlaag iets anders doet dan verwacht.

set -euo pipefail

HIER="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HIER/../.." && pwd)"
CRATE="$REPO/crates/openaec-samenvoegen"
CLI="$CRATE/target/release/openaec-samenvoegen"
IFC=(python3 -I "$HIER/ifc.py")

# Een echte planning van OPS (260 taken) en een echt bouwmodel van buildingSMART (IFC2X3, met wanden en een vloer).
OPS_VOORBEELDEN="${OPS_VOORBEELDEN:-$HOME/projects/open-planner-studio/examples}"
PLANNING="$OPS_VOORBEELDEN/showcase-appartementencomplex.ifc"
MODEL="$CRATE/tests/bestanden/ifc2x3-building-architecture.ifc"

if [ ! -x "$CLI" ]; then
  echo "De opdrachtregel van openaec-samenvoegen ontbreekt. Bouw hem eerst:" >&2
  echo "  (cd $CRATE && cargo build --release --features cli)" >&2
  exit 2
fi

# Elke proef werkt in een eigen lege map onder target/ (niet in git).
WERK="$CRATE/target/scenario-m365/$(basename "$0" .sh)"
rm -rf "$WERK"
mkdir -p "$WERK"
cd "$WERK"

kop() { printf '\n== %s\n' "$*"; }

# Wat Microsoft 365 in dit geval doet, letterlijk uit de bron.
m365() {
  printf 'Microsoft 365 volgens de bron (%s):\n' "$1"
  shift
  for zin in "$@"; do printf '  "%s"\n' "$zin"; done
}

# Herkomst van een invoerbestand: naam, grootte en sha256.
herkomst() {
  printf '%s: %s bytes, sha256 %s\n' "$(basename "$1")" "$(stat -c %s "$1")" "$(sha256sum "$1" | cut -c1-16)"
}

# samenvoegen BASIS ONS HUN UIT: voegt samen met de opdrachtregel, meet de tijd en geeft de exitcode van de
# opdrachtregel terug (0 = gelukt, 1 = conflict, 2 = fout).
samenvoegen() {
  local begin eind code=0
  begin=$(date +%s%N)
  "$CLI" voeg-samen "$1" "$2" "$3" -o "$4" --rapport "$4.rapport.json" || code=$?
  eind=$(date +%s%N)
  printf 'tijd: %s ms (exitcode %s)\n' $(( (eind - begin) / 1000000 )) "$code"
  return "$code"
}

# verschil A B NAAM: het verschil als tekst, en als JSON in NAAM.json voor de controle.
verschil() {
  "$CLI" verschil "$1" "$2"
  "$CLI" verschil "$1" "$2" --json > "$3.json"
}

# Een bestand opslaan "zoals OPS": opnieuw genummerd. Het verschil met het origineel moet leeg zijn.
sla_op_als_ops() {
  "${IFC[@]}" hernummer "$1" "$2" "$3"
  "$CLI" verschil "$1" "$2" --json > "$2.nummers.json"
  "${IFC[@]}" controleer "$2.nummers.json" > /dev/null
}
