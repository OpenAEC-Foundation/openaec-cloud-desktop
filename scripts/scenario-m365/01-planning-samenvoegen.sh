#!/usr/bin/env bash
# Scenario 1: Anna en Piet wijzigen tegelijk een echte planning van OPS. Ze raken elk een andere taak.
# Verwacht van onze onderlaag: één nieuwe versie met beide wijzigingen, zonder conflictkopie.
. "$(dirname "$0")/gemeen.sh"

FUNDERING='3Lz$0hQ_JxtHXIVlnG3JSI'   # Pour foundation beams (shared)
KELDERVLOER='1FPwlytwv2VWjV9PVP_VP4' # Pour basement floor — Tower C

kop "Invoer"
herkomst "$PLANNING"
cp "$PLANNING" v12.ifc

kop "Anna (vanaf versie 12): de funderingsbalken schuiven 2 weken op. Ze slaat als eerste op: versie 13."
"${IFC[@]}" schuif v12.ifc anna.tmp "$FUNDERING" 14
sla_op_als_ops anna.tmp v13.ifc 100000

kop "Piet (ook vanaf versie 12): de keldervloer van toren C duurt 8 dagen in plaats van 5."
"${IFC[@]}" duur v12.ifc piet.tmp "$KELDERVLOER" 8
sla_op_als_ops piet.tmp piet.ifc 200000

kop "Piet slaat op. Zijn basis (12) is verouderd: de opslaan-lus voegt samen met basis 12, ons Piet, hun 13."
samenvoegen v12.ifc piet.ifc v13.ifc v14.ifc

kop "Controle: versie 14 bevat beide wijzigingen, en verder niets"
verschil v12.ifc v14.ifc v12-v14
"${IFC[@]}" controleer v12-v14.json --gewijzigd "$FUNDERING,$KELDERVLOER" --waarden "$HIER/verwacht/01-v12-v14.txt"
verschil v13.ifc v14.ifc v13-v14
"${IFC[@]}" controleer v13-v14.json --gewijzigd "$KELDERVLOER" --waarden "$HIER/verwacht/01-v13-v14.txt"

kop "Wat Microsoft 365 in dit geval doet (een .ifc is geen Office-bestand)"
m365 "https://support.microsoft.com/en-gb/sharepoint/get-started-with-sharepoint/document-collaboration-and-co-authoring" \
  "Co-authoring is only supported on modern file formats including: .docx (Word), .pptx (PowerPoint), and .xlsx (Excel)."
m365 "https://learn.microsoft.com/en-us/troubleshoot/sharepoint/sync/troubleshoot-sync-issues" \
  "If a conflict occurred with a non-Office file type, OneDrive automatically keeps both versions. The online version keeps the original file name and is downloaded, and the copy on your computer has your device name appended to the file name such as Report-JOHNS-SURFACE.txt."
m5_voorbehoud
echo "Dus: twee bestanden naast elkaar, en iemand voegt de taken met de hand samen."

kop "Uitkomst"
echo "Onze onderlaag: één versie (14) met beide wijzigingen. Geen conflictkopie."
