#!/usr/bin/env bash
# Scenario 4: Anna werkt offline aan de planning en slaat twee keer lokaal op. Piet maakt intussen online twee
# nieuwe versies. Anna komt weer online.
# Verwacht van onze onderlaag: één nieuwe versie met alle vier de wijzigingen.
. "$(dirname "$0")/gemeen.sh"

FUNDERING='3Lz$0hQ_JxtHXIVlnG3JSI'   # Pour foundation beams (shared)
DAK_A='0AOKES1U_N1S7hr$6tCHXH'       # Install roof structure — Tower A
KELDERVLOER='1FPwlytwv2VWjV9PVP_VP4' # Pour basement floor — Tower C
DAK_B='0_4IdJo_cQzpN_Ek772XwL'       # Install roof structure — Tower B

kop "Invoer"
herkomst "$PLANNING"
cp "$PLANNING" v12.ifc

kop "Anna gaat offline met versie 12. Ze slaat twee keer lokaal op."
"${IFC[@]}" schuif v12.ifc anna.1 "$FUNDERING" 14
sla_op_als_ops anna.1 anna-lokaal-1.ifc 100000
"${IFC[@]}" schuif anna-lokaal-1.ifc anna.2 "$DAK_A" 7
sla_op_als_ops anna.2 anna-lokaal-2.ifc 300000

kop "Piet werkt online: versie 13 (keldervloer toren C 8 dagen) en versie 14 (dak toren B 3 dagen later)."
"${IFC[@]}" duur v12.ifc piet.1 "$KELDERVLOER" 8
sla_op_als_ops piet.1 v13.ifc 200000
"${IFC[@]}" schuif v13.ifc piet.2 "$DAK_B" 3
sla_op_als_ops piet.2 v14.ifc 400000

kop "Anna is weer online. Haar basis is 12, de server staat op 14: samenvoegen met basis 12, ons Anna, hun 14."
samenvoegen v12.ifc anna-lokaal-2.ifc v14.ifc v15.ifc

kop "Controle: versie 15 bevat alle vier de wijzigingen, en verder niets"
verschil v12.ifc v15.ifc v12-v15 > /dev/null
"${IFC[@]}" controleer v12-v15.json --gewijzigd "$FUNDERING,$DAK_A,$KELDERVLOER,$DAK_B"
verschil v14.ifc v15.ifc v14-v15 > /dev/null
"${IFC[@]}" controleer v14-v15.json --gewijzigd "$FUNDERING,$DAK_A"

kop "Wat Microsoft 365 in dit geval doet"
m365 "https://support.microsoft.com/en-gb/sharepoint/get-started-with-sharepoint/document-collaboration-and-co-authoring" \
  "If there are others editing the same document, they won't be able to see your changes while you're offline. When you go back online, you'll be notified of any changes that are available, and others will see that you have changes." \
  "When you go offline with Word, it's possible to work on the same paragraph as someone else in your group. When you save your changes, you'll get a pop-up message saying there's a conflict."
m365 "https://learn.microsoft.com/en-us/troubleshoot/sharepoint/sync/troubleshoot-sync-issues" \
  "If a conflict occurred with a non-Office file type, OneDrive automatically keeps both versions."
echo "Dus: voor Word een melding en een conflict per alinea; voor een .ifc twee bestanden naast elkaar."

kop "Uitkomst"
echo "Onze onderlaag: één versie (15) met de twee wijzigingen van Anna en de twee van Piet."
echo "Wat hier niet getoetst is: de desktop-sync die dit tegen de server doet. Die hangt aan stap 4 op de server"
echo "(storage-PR #9, een voorstel; de server ligt stil tot de kern gelezen is)."
