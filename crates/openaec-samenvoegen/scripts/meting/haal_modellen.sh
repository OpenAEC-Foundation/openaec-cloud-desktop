#!/usr/bin/env bash
# haal_modellen.sh <lege map buiten de repo>
# Haalt de open voorbeeldmodellen voor de metingen op. Bron: buildingSMART/Sample-Test-Files, vaste commit.
# Licentie: CC BY 4.0, (c) buildingSMART International Ltd. Zie docs/samenwerken/metingen.md.
set -euo pipefail
map=${1:?geef een lege map}
mkdir -p "$map"
[ -z "$(ls -A "$map")" ] || { echo "$map is niet leeg" >&2; exit 2; }
commit=80d976a9b193a26a8e928c3e79bff67af1de68a8
basis="https://raw.githubusercontent.com/buildingSMART/Sample-Test-Files/$commit"
haal() { curl -fsSL --retry 3 -o "$map/$2" "$basis/$(printf '%s' "$1" | sed 's/ /%20/g')"; }
haal "IFC 2.3.0.1 (IFC 2x3 TC1)/Simple-Scene/Infra-Landscaping.ifc" ifc2x3-infra-landscaping.ifc
haal "IFC 4.0.2.1 (IFC 4 ADD2 TC1)/Simple-Scene/Infra-Landscaping.ifc" ifc4-infra-landscaping.ifc
haal "IFC 4.0.2.1 (IFC 4 ADD2 TC1)/Simple-Scene/Infra-Bridge.ifc" ifc4-infra-bridge.ifc
haal "IFC 4.3.2.0 (IFC 4.3 ADD2)/Simple-Scene/Infra-Bridge.ifc" ifc4x3-infra-bridge.ifc
ls -l "$map"
