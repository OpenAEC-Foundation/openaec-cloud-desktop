#!/usr/bin/env bash
# Meting 7: hoeveel van een groot model gaat over het netwerk na samenvoegen?
# openaec-storage hergebruikt een blok van 8 MiB als het gelijk is aan het blok op dezelfde plek in de basisversie
# (`_herbruikbare_blokken` in api.py). Na samenvoegen is de basis de versie van hun. Hier: een model van 92 MB, Anna
# wijzigt één naam vlak bij het einde, Piet verplaatst één element vlak bij het begin. Niemand nummert opnieuw.
# Zwaar: maakt het bestand van 92 MB met de meettest van de bibliotheek, en voegt drie bestanden van 92 MB samen
# (ongeveer 2 GB geheugen). Draai het op aia met zwaar.sh.
. "$(dirname "$0")/gemeen.sh"

GROOT="$CRATE/target/meting/basis.ifc"
if [ ! -e "$GROOT" ]; then
  kop "Het grote bestand maken (meettest van de bibliotheek)"
  (cd "$CRATE" && cargo test --release --features cli --test meting meet_verschil -- --ignored --nocapture 2>&1 | grep -E '^(bestand|verschil)')
fi

kop "Invoer"
herkomst "$GROOT"
cp "$GROOT" v12.ifc

kop "Anna (vanaf versie 12): de naam van element 199500 (vlak bij het einde). Ze slaat als eerste op: versie 13."
"${IFC[@]}" vervang v12.ifc v13.ifc "'Element 199500'" "'Element 199500 (Anna)'"
echo "Een gewone upload van versie 13 op basis 12:"
"${IFC[@]}" blokken v13.ifc v12.ifc

kop "Piet (ook vanaf versie 12): element 10 schuift op (vlak bij het begin)."
"${IFC[@]}" vervang v12.ifc piet.ifc "IFCCARTESIANPOINT((10.,0.,0.));" "IFCCARTESIANPOINT((12.5,0.,0.));"
echo "Zonder Anna zou Piets upload op basis 12 dit versturen:"
"${IFC[@]}" blokken piet.ifc v12.ifc

kop "Piet slaat op. Zijn basis (12) is verouderd: samenvoegen met basis 12, ons Piet, hun 13."
samenvoegen v12.ifc piet.ifc v13.ifc v14.ifc
echo "De upload van versie 14 op basis 13 (de versie van hun):"
"${IFC[@]}" blokken v14.ifc v13.ifc

kop "Andersom: Anna slaat als tweede op (ons Anna, hun Piet als versie 13)."
samenvoegen v12.ifc v13.ifc piet.ifc v14b.ifc
echo "De upload van versie 14 op basis 13 (de versie van Piet):"
"${IFC[@]}" blokken v14b.ifc piet.ifc

kop "Wat Microsoft 365 hierover zegt"
m365 "https://support.microsoft.com/en-us/onedrive/restrictions-and-limitations-in-onedrive-and-sharepoint" \
  "Differential sync works for Microsoft Office files in the new file format, .docx, .pptx, and .xlsx." \
  "For other file types, differential sync is available globally."
echo "De bron noemt geen blokgrootte en geen meting. Een vergelijking in cijfers kan dus niet."
