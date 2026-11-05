#!/usr/bin/env bash
# Zaait het corpus van de fuzzdoelen uit de testbestanden (en uit $OPS_VOORBEELDEN als die map gezet is).
# lees: elk bestand. verschil: bestand NUL bestand. samenvoegen: bestand NUL bestand NUL bestand.
# Alleen bestanden tot 64 kB, zodat een ronde veel invoer haalt.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p corpus/lees corpus/verschil corpus/samenvoegen
for f in ../tests/bestanden/*.ifc ${OPS_VOORBEELDEN:+"$OPS_VOORBEELDEN"/*.ifc}; do
  [ "$(stat -c %s "$f")" -le 65536 ] || continue
  n=$(basename "$f" .ifc)
  cp "$f" "corpus/lees/$n"
  { cat "$f"; printf '\0'; cat "$f"; } > "corpus/verschil/$n"
  { cat "$f"; printf '\0'; cat "$f"; printf '\0'; cat "$f"; } > "corpus/samenvoegen/$n"
done
ls corpus/* | wc -l
