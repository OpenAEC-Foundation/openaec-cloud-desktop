#!/usr/bin/env bash
# Proef 6: de opslaan-lus van de bibliotheek, met de tests van PR #7. Acht schrijvers tegelijk vanaf dezelfde
# versie (een echte planning van OPS), een slot van een ander, een conflict, een verdwenen basis.
# De lus werkt hier tegen een opslag in het geheugen achter de trait `Opslag`, niet tegen de server.
# Zwaar: bouwt de tests. Draai het op aia met zwaar.sh.
. "$(dirname "$0")/gemeen.sh"

cd "$CRATE"
kop "Tests van de opslaan-lus (tests/samenwerken.rs)"
cargo test --release --features cli --test samenwerken 2>&1 | grep -E '^(test |test result|running)'

kop "Samenvoegen op echte bestanden (tests/samenvoegen.rs)"
cargo test --release --features cli --test samenvoegen -- \
  verschillende_objecten_op_echte_bestanden taakdatum_en_taaknaam_in_een_planning \
  hetzelfde_attribuut_anders_is_een_conflict verwijderd_en_gewijzigd 2>&1 | grep -E '^(test |test result|running)'

kop "Wat Microsoft 365 hierover zegt"
m365 "https://learn.microsoft.com/en-us/sharepoint/governance/configure-the-maximum-number-of-co-authoring-authors" \
  "The CoauthoringMaxAuthors property lets you specify the maximum number of users who can co-author a Word document or PowerPoint presentation at the same time in SharePoint Server 2013."
echo "Voor Microsoft 365 zelf vonden we geen getal voor het aantal schrijvers tegelijk."
