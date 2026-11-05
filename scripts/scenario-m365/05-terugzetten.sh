#!/usr/bin/env bash
# Scenario 5: Piet zet versie 12 terug, terwijl Anna nog vanaf versie 13 werkt. Daarna slaat Anna op.
# Terugzetten op de server (`zet_terug`) is nog niet gebouwd; hier is de teruggezette versie een kopie van de
# inhoud van versie 12, zoals §5.5 het beschrijft ("nieuwe versie die gelijk is aan een oude").
# Verwacht van onze onderlaag: Anna's wijziging blijft, en het terugzetten van Piet ook.
. "$(dirname "$0")/gemeen.sh"

FUNDERING='3Lz$0hQ_JxtHXIVlnG3JSI'   # Pour foundation beams (shared)
KELDERVLOER='1FPwlytwv2VWjV9PVP_VP4' # Pour basement floor — Tower C

kop "Invoer"
herkomst "$PLANNING"
cp "$PLANNING" v12.ifc

kop "Piet maakt versie 13: de keldervloer van toren C duurt 8 dagen."
"${IFC[@]}" duur v12.ifc piet.1 "$KELDERVLOER" 8
sla_op_als_ops piet.1 v13.ifc 200000

kop "Anna haalt versie 13 op en schuift de funderingsbalken 2 weken op (nog niet opgeslagen)."
"${IFC[@]}" schuif v13.ifc anna.1 "$FUNDERING" 14
sla_op_als_ops anna.1 anna.ifc 100000

kop "Piet zet versie 12 terug: versie 14 heeft precies de inhoud van versie 12."
cp v12.ifc v14.ifc
cmp v12.ifc v14.ifc && echo "versie 14 is byte voor byte gelijk aan versie 12"

kop "Anna slaat op. Haar basis (13) is verouderd: samenvoegen met basis 13, ons Anna, hun 14."
samenvoegen v13.ifc anna.ifc v14.ifc v15.ifc

kop "Controle: versie 15 is versie 12 plus de wijziging van Anna"
verschil v12.ifc v15.ifc v12-v15
"${IFC[@]}" controleer v12-v15.json --gewijzigd "$FUNDERING"

kop "Wat Microsoft 365 in dit geval doet"
m365 "https://support.microsoft.com/en-us/onedrive/restore-a-previous-version-of-a-file-stored-in-onedrive" \
  "The document version you selected becomes the current version. The previous current version becomes the previous version in the list."
m365 "https://support.microsoft.com/en-us/sharepoint/documents-and-library/restore-a-previous-version-of-an-item-or-file-in-sharepoint" \
  "SharePoint doesn't remove the earlier version you just restored. It creates a copy and makes it the latest version."
m365 "https://learn.microsoft.com/en-us/troubleshoot/sharepoint/sync/troubleshoot-sync-issues" \
  "If a conflict occurred with a non-Office file type, OneDrive automatically keeps both versions."
echo "Dus (afgeleid, niet zo beschreven): slaat Anna daarna een .ifc op via de sync-app, dan komt er een tweede"
echo "bestand naast het teruggezette."

kop "Uitkomst"
echo "Onze onderlaag: het terugzetten van Piet en de wijziging van Anna staan allebei in versie 15."
echo "Nog niet gebouwd: terugzetten in één stap op de server (zet_terug, §5.5). Nu: de oude versie downloaden en"
echo "opnieuw uploaden."
