#!/usr/bin/env bash
# Scenario 3: Anna en Piet wijzigen tegelijk dezelfde eigenschap van dezelfde vloer, elk anders. Anna verschuift
# daarnaast een wand.
# Verwacht van onze onderlaag: geen nieuwe versie en geen stille keuze, maar een rapport per object en attribuut
# met de waarden van basis, ons en hun. De client maakt dan een conflictkopie, zoals nu (openaec-storage).
. "$(dirname "$0")/gemeen.sh"

WAND='1AQAupaRP1txwK1AGiN61V'
REI30="IFCPROPERTYSINGLEVALUE('FireRating',\$,IFCLABEL('REI30'),\$);"

kop "Invoer"
herkomst "$MODEL"
cp "$MODEL" v12.ifc

kop "Anna (vanaf versie 12): de vloer wordt REI60, en de voorgevelwand schuift 250 mm op. Versie 13."
"${IFC[@]}" vervang v12.ifc anna.1 "$REI30" "${REI30/REI30/REI60}"
"${IFC[@]}" plaats anna.1 anna.tmp "$WAND" 4100. 2050. 0.
sla_op_als_ops anna.tmp v13.ifc 100000

kop "Piet (ook vanaf versie 12): de vloer wordt REI90."
"${IFC[@]}" vervang v12.ifc piet.tmp "$REI30" "${REI30/REI30/REI90}"
sla_op_als_ops piet.tmp piet.ifc 200000

kop "Piet slaat op. Zijn basis (12) is verouderd: samenvoegen met basis 12, ons Piet, hun 13."
code=0
samenvoegen v12.ifc piet.ifc v13.ifc v14.ifc || code=$?

kop "Controle: een conflict, geen uitvoerbestand, en het rapport noemt de drie waarden"
[ "$code" = 1 ] || { echo "verwacht exitcode 1 (conflict), kreeg $code"; exit 1; }
[ ! -e v14.ifc ] || { echo "er is toch een uitvoerbestand"; exit 1; }
python3 -I - v14.ifc.rapport.json <<'EOF'
import json, sys
r = json.load(open(sys.argv[1]))
assert not r["gelukt"], r
c = [c for c in r["conflicten"] if c.get("attribuut")]
assert len(c) == 1, c
c = c[0]
print(f"conflict: {c['soort']} op {c['type']} [{c['globalid']}], attribuut {c['attribuut']}")
print(f"  basis {c['basis']}, ons (Piet) {c['ons']}, hun (Anna) {c['hun']}")
for kant, waarde in (("basis", "REI30"), ("ons", "REI90"), ("hun", "REI60")):
    assert waarde in (c[kant] or ""), (kant, c[kant])
EOF
echo "Daarna: de client slaat Piets versie op als conflictkopie, zoals openaec-storage nu doet"
echo "(test_api.py: test_verouderde_basisversie_geeft_conflictkopie). Anna's versie 13 blijft de huidige."

kop "Wat Microsoft 365 in dit geval doet"
m365 "https://learn.microsoft.com/en-us/troubleshoot/sharepoint/sync/troubleshoot-sync-issues" \
  "If a conflict occurred with a non-Office file type, OneDrive automatically keeps both versions."
m365 "https://support.microsoft.com/en-us/excel/get-started/collaborate-on-excel-workbooks-at-the-same-time-with-co-authoring" \
  "In general, the last change that is saved, either with the Save button or automatically with AutoSave, is the one that \"wins.\""
m365 "https://support.microsoft.com/en-us/word/troubleshoot-common-coauthoring-errors" \
  "Why this error happened: Your document can't be saved to the server because your edits conflict with edits from others." \
  "Accept or reject each change."
echo "Dus: bij een .ifc twee bestanden zonder uitleg wat er botst; in Excel wint de laatste opslag; in Word kiest de gebruiker per wijziging."

kop "Uitkomst"
echo "Onze onderlaag: geen stille keuze. Het rapport noemt het object, het attribuut en de drie waarden."
echo "Een scherm om per conflict te kiezen (zoals Word) is er nog niet (§8 van het ontwerp)."
