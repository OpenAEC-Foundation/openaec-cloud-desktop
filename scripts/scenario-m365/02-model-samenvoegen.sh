#!/usr/bin/env bash
# Scenario 2: Anna en Piet wijzigen tegelijk een echt bouwmodel (buildingSMART, IFC2X3). Er hangt een opmerking
# aan de wand die Anna verplaatst.
# Verwacht van onze onderlaag: één nieuwe versie met beide wijzigingen; het verschil zegt dat de wand met de
# opmerking gewijzigd is. De opmerking zelf is nog niet gebouwd (server, §5.9 van het ontwerp).
. "$(dirname "$0")/gemeen.sh"

WAND='1AQAupaRP1txwK1AGiN61V'     # IfcWall 'house - outer wall - house right front': hier hangt de opmerking
ANDERE_WAND='3wdauVJT5Fx9drrREiDqA$' # IfcWall 'house - outer wall - house right back'
VLOERSET='3IPHMao9vAlQSQdn4mt1Bm'  # Pset_SlabCommon van de vloer, met FireRating REI30

kop "Invoer"
herkomst "$MODEL"
cp "$MODEL" v12.ifc

kop "Anna (vanaf versie 12): de voorgevelwand rechts schuift 250 mm op. Ze slaat als eerste op: versie 13."
"${IFC[@]}" plaats v12.ifc anna.tmp "$WAND" 4100. 2050. 0.
sla_op_als_ops anna.tmp v13.ifc 100000

kop "Piet (ook vanaf versie 12): de brandwerendheid van de vloer gaat van REI30 naar REI60."
"${IFC[@]}" vervang v12.ifc piet.tmp \
  "IFCPROPERTYSINGLEVALUE('FireRating',\$,IFCLABEL('REI30'),\$);" \
  "IFCPROPERTYSINGLEVALUE('FireRating',\$,IFCLABEL('REI60'),\$);"
sla_op_als_ops piet.tmp piet.ifc 200000

kop "Piet slaat op. Zijn basis (12) is verouderd: samenvoegen met basis 12, ons Piet, hun 13."
samenvoegen v12.ifc piet.ifc v13.ifc v14.ifc

kop "Controle: versie 14 bevat beide wijzigingen, en verder niets"
verschil v12.ifc v14.ifc v12-v14
"${IFC[@]}" controleer v12-v14.json --gewijzigd "$WAND,$VLOERSET" --waarden "$HIER/verwacht/02-v12-v14.txt"

kop "De opmerking aan de wand: wat het verschil over versie 14 zegt"
"${IFC[@]}" status v12-v14.json "$WAND"
"${IFC[@]}" status v12-v14.json "$ANDERE_WAND"
echo "Opmerkingen zelf (opslaan, anker_status in de API) zijn nog niet gebouwd: dat is serverwerk (§5.9)."

kop "Wat Microsoft 365 in dit geval doet"
m365 "https://learn.microsoft.com/en-us/troubleshoot/sharepoint/sync/troubleshoot-sync-issues" \
  "If a conflict occurred with a non-Office file type, OneDrive automatically keeps both versions. The online version keeps the original file name and is downloaded, and the copy on your computer has your device name appended to the file name such as Report-JOHNS-SURFACE.txt."
m5_voorbehoud
m365 "https://support.microsoft.com/en-us/onedrive/see-comments-and-activity-on-your-onedrive-files" \
  "OneDrive now lets you @mention someone in the details pane of any non-Office file that you've saved in OneDrive for home, including photos, videos, PDFs, and more." \
  "@mentions for non-office file types are not yet available for the OneDrive sync app or OneDrive for work or school accounts."
echo "Dus: bij een .ifc twee bestanden naast elkaar. Een opmerking kan alleen over het hele bestand gaan, niet over één wand."

kop "Uitkomst"
echo "Onze onderlaag: één versie (14) met beide wijzigingen, en het verschil meldt de wand met de opmerking als gewijzigd."
