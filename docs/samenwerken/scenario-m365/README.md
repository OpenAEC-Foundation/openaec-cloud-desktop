# Scenarioproeven: onze onderlaag naast Microsoft 365

Deze proeven horen bij hoofdstuk 4a van het ontwerp (openaec-storage, `docs/samenwerken/ontwerp.md`, branch
`samenwerken/ontwerp-m365`). Dat hoofdstuk vergelijkt per functie onze onderlaag met Microsoft 365 (M365). De
nummers F1 tot en met F26 (F5 gesplitst in F5a, F5b en F5c) komen uit die tabel. Tegelijk bewerken in de browser (de
werkplek, F26) toetsen deze proeven niet: daar krijgt de tweede nu alleen-lezen.

Het vaste voorbeeld: Anna en Piet wijzigen tegelijk één IFC-bestand, vanaf versie 12. Het script doet de bewerking
zelf en nummert het bestand daarna opnieuw, zoals OPS bij elk opslaan doet. Elk script zegt ook wat M365 in dat geval
doet, letterlijk uit een bron van Microsoft. Een script stopt met een fout zodra onze onderlaag iets anders doet dan
verwacht.

| Log | Wat het laat zien | Regel |
|---|---|---|
| `01-planning-samenvoegen.txt` | Een planning van OPS (260 taken): twee taken, twee mensen, één versie | F1, F4, F8 |
| `02-model-samenvoegen.txt` | Een model van buildingSMART (IFC2X3): een wand en een vloer; de status van de wand met de opmerking | F1, F8, F10 |
| `03-conflict-zelfde-eigenschap.txt` | Dezelfde eigenschap, twee waarden: geen keuze, een rapport met de drie waarden | F5a, F5b, F5c |
| `04-offline.txt` | Anna werkt offline, Piet maakt twee versies: één versie met alle vier de wijzigingen | F7 |
| `05-terugzetten.txt` | Piet zet versie 12 terug terwijl Anna doorwerkt: beide blijven | F16 |
| `06-opslaan-lus.txt` | De tests van de opslaan-lus (PR #7), met acht schrijvers tegelijk | F2, F25 |
| `07-blokken.txt` | Hoeveel blokken van 8 MiB van een model van 92 MB over het netwerk gaan, met en zonder samenvoegen | F24 |
| `08-storage-tests.txt` | De bestaande tests van openaec-storage (main) voor conflictkopie, slot, delen, een verlopen link, versies en blokken | F5a, F13, F15, F17, F19, F21, F24 |

## Wat de controles eisen

- `ifc.py controleer` eist dat het verschil precies de genoemde objecten noemt, dat de HEADER en de losse knopen
  gelijk blijven, en dat elke gewijzigde waarde letterlijk in een bestand in `scripts/scenario-m365/verwacht/` staat
  (een regel per waarde). Elk bestand zegt welke bewerking welke waarde geeft, bijv. "+14 dagen". Zonder bestand mag
  er geen enkele waarde gewijzigd zijn: zo toetst het script na elke opslag "zoals OPS" een leeg verschil.
- Proef 03 eist het conflictrapport letterlijk: soort, object, attribuut en de drie waarden.
- Proef 08 bewaart per module de echte exitcode van de testrun. `alles.sh` eindigt met 1 zodra één proef mislukt.

Dat de controles kunnen falen (gedraaid op 5 november, fixronde 1):

- Eén datum in `verwacht/01-v12-v14.txt` een dag later (`2027-03-15` wordt `2027-03-16`): proef 01 stopt met
  "waarden kloppen niet" en exitcode 1.
- `BENCH_TEST=false`: proef 08 geeft per module exitcode 1 en eindigt met 1, en `alles.sh` eindigt dan ook met 1.

## De bron over conflicten in OneDrive

De citaten uit "Resolve sync issues in OneDrive for work or school" (learn.microsoft.com) gaan over een conflict bij
een ander soort bestand. Die pagina noemt de oude client `groove.exe`, en die synchroniseert sinds 1 februari 2021
geen bestanden in M365 meer. Een bron over de huidige app (`OneDrive.exe`) vonden we niet. Daarom staat na elk citaat
uit die pagina een voorbehoud, en heet de vergelijking in 4a daar "niet bewezen".

## Wanneer gedraaid

STAND_VAN_DE_RUN

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
2 GB geheugen). `OVERSLAAN="07-blokken"` slaat een proef over; zijn oude log blijft dan staan.
