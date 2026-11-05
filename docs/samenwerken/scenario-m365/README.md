# Scenarioproeven: onze onderlaag naast Microsoft 365

Deze proeven horen bij hoofdstuk 4a van het ontwerp (openaec-storage, `docs/samenwerken/ontwerp.md`, branch
`samenwerken/ontwerp-m365`). Dat hoofdstuk vergelijkt per functie onze onderlaag met Microsoft 365 (M365). De
nummers F1 tot en met F25 komen uit die tabel.

Het vaste voorbeeld: Anna en Piet wijzigen tegelijk één IFC-bestand, vanaf versie 12. Het script doet de bewerking
zelf en nummert het bestand daarna opnieuw, zoals OPS bij elk opslaan doet. Elk script zegt ook wat M365 in dat geval
doet, letterlijk uit een bron van Microsoft. Een script stopt met een fout zodra onze onderlaag iets anders doet dan
verwacht.

| Log | Wat het laat zien | Regel |
|---|---|---|
| `01-planning-samenvoegen.txt` | Een planning van OPS (260 taken): twee taken, twee mensen, één versie | F1, F4, F8 |
| `02-model-samenvoegen.txt` | Een model van buildingSMART (IFC2X3): een wand en een vloer; de status van de wand met de opmerking | F1, F8, F10 |
| `03-conflict-zelfde-eigenschap.txt` | Dezelfde eigenschap, twee waarden: geen keuze, een rapport met de drie waarden | F5 |
| `04-offline.txt` | Anna werkt offline, Piet maakt twee versies: één versie met alle vier de wijzigingen | F7 |
| `05-terugzetten.txt` | Piet zet versie 12 terug terwijl Anna doorwerkt: beide blijven | F16 |
| `06-opslaan-lus.txt` | De tests van de opslaan-lus (PR #7), met acht schrijvers tegelijk | F2, F25 |
| `07-blokken.txt` | Hoeveel blokken van 8 MiB van een model van 92 MB over het netwerk gaan, met en zonder samenvoegen | F24 |
| `08-storage-tests.txt` | De bestaande tests van openaec-storage (main) voor conflictkopie, slot, delen, versies en blokken | F5, F13, F15, F17, F19, F21, F24 |

Code van de bibliotheek: proef 01 tot en met 06 draaiden op 5 november op de basis `e9e601a` van PR #7 (na de
merge in deze branch), proef 07 op `0655eda`. Proef 08 draaide op de code van openaec-storage `main` (`e87b870`);
de log noemt `9393365`, de ontwerp-branch, die buiten `docs/` gelijk is aan main.

Alles draait zonder server, behalve proef 08 (een Frappe-bench met een testsite). De weg van de desktop-app naar
openaec-storage is voor samenvoegen nog niet gebouwd; deze proeven bewijzen die dus niet.

## Zelf draaien

```text
(cd crates/openaec-samenvoegen && cargo build --release --features cli)
scripts/scenario-m365/alles.sh [werkkopie van openaec-storage]
```

Invoer: de planning `showcase-appartementencomplex.ifc` uit `examples/` van open-planner-studio (zet
`OPS_VOORBEELDEN` als die map niet in `~/projects/open-planner-studio/examples` staat), en
`crates/openaec-samenvoegen/tests/bestanden/ifc2x3-building-architecture.ifc`. Op aia zet je `ZWAAR` op
`/home/aia09/worktrees/zwaar.sh`: proef 06 bouwt tests, en proef 07 voegt drie bestanden van 92 MB samen (ongeveer
2 GB geheugen).
