# Samenvoegen van planningen van Open Planner Studio

Stand: 5 november 2026. Hoort bij ontwerp §5.6 (openaec-storage, `docs/samenwerken/ontwerp.md`) en de bibliotheek
`crates/openaec-samenvoegen`.

Twee planners werken aan dezelfde planning, elk in een eigen kopie. Open Planner Studio (OPS) slaat de planning op als
IFC. De bibliotheek voegt de twee versies samen met de basis. Dit document zegt welke wijzigingen samen kunnen en
welke een conflict geven, met voorbeelden uit OPS. Alles hieronder is getest op planningen die OPS zelf schreef
(zie "Hoe getest").

## Wat OPS in een planning schrijft

Bron: OPS commit `604dd2940` (3 november 2026), `src/services/ifc/ifcWriter.ts` en `ifcTaskSlots.ts`.

| Wat in OPS | In de IFC |
|---|---|
| Het project, het plan en het werkschema | `IfcProject`, `IfcWorkPlan`, `IfcWorkSchedule` (`.PLANNED.`) en een `IfcRelAggregates` ertussen |
| Een taak | `IfcTask` met een eigen `IfcTaskTime` (duur, datums, speling). De naam van de `IfcTaskTime` is de taaknaam plus " Time" (`ifcTaskSlots.ts` r. 95) |
| De WBS (fases en taken) | Per samenvatting een `IfcRelNests` met de kinderen als LIST, en een `IfcRelNests` van het werkschema naar de bovenste taken (r. 505, 1600). De WBS-code staat in `IfcTask.Identification` |
| Alle taken in het werkschema | Eén `IfcRelAssignsToControl` met alle taken als SET (r. 551) |
| Een relatie (voorganger, opvolger) | `IfcRelSequence` met `SequenceType` en eventueel een `IfcLagTime` (r. 1675) |
| Middelen en toewijzingen | `IfcConstructionResource`-subtypen, `IfcRelAssignsToProcess` |
| Kalenders | `IfcWorkCalendar` met `IfcWorkTime` |
| Een baseline | Een `IfcWorkSchedule` (`.BASELINE.`) zonder taken (r. 472), en alle baselines samen als JSON in één eigenschap `Baselines` van de set `OPS_Baselines` op het werkschema (r. 973) |
| Wat IFC niet kent | Eigenschappensets `OPS_*`, bijv. `OPS_TaskIdentity` en `OPS_Analysis` per taak |

**GlobalIds blijven gelijk.** OPS bewaart de GlobalId uit het bestand dat het opende. Een nieuw object krijgt een
GlobalId uit een hash van zijn eigen id (`issueGuid`, r. 244). Een nieuwe relatie krijgt dus bij elke planner een
andere GlobalId, ook als het dezelfde relatie is. De nummers `#n` kiest OPS bij elk opslaan opnieuw.

**Vluchtige velden.** Gemeten: OPS opnieuw opslaan zonder wijziging verandert precies vier velden. Dat zijn de
tijdstempel in `FILE_NAME` (r. 345), `IfcOwnerHistory.CreationDate` (r. 394) en de `CreationDate` van het
`IfcWorkPlan` en het `IfcWorkSchedule` (r. 452 en 457). Een baseline houdt haar eigen `CreationDate`. De bibliotheek
behandelt die velden als vluchtig (ontwerp §5.6): ze geven nooit een conflict. Test:
`opnieuw_opslaan_verandert_alleen_vluchtige_velden`.

**Afgeleide velden.** Na elke wijziging rekent OPS de planning door. Dan verandert meer dan wat de planner deed:

- in de `IfcTaskTime` van de taak en van alle opvolgers: `EarlyStart`, `EarlyFinish`, `LateStart`, `LateFinish`,
  `FreeFloat`, `TotalFloat` en `IsCritical`;
- bij een andere projecteinddatum: `LateStart`, `LateFinish` en `TotalFloat` van bijna elke taak, ook buiten de keten;
- de `IfcTaskTime` van de fase erboven (duur en datums zijn de som van de taken eronder);
- `StartTime` en `FinishTime` van het `IfcWorkPlan` en het `IfcWorkSchedule`;
- de exacte speling in `OPS_Analysis.ExactFloat`;
- bij verplaatsen, invoegen of wissen: de WBS-codes (`Identification`) van de taken erna.

Voorbeeld uit de test: de staalconstructie van 5 naar 20 dagen verandert 24 van de 25 `IfcTaskTime`'s.

## Wat samen kan en wat een conflict is

| Ons | Hun | Uitkomst | Waarom | Test (`tests/planning.rs`) |
|---|---|---|---|---|
| Duur van taak A naar 20 dagen | Naam van A | Beide | De `IfcTaskTime` hoort alleen bij A. Die voegen we per attribuut samen: de duur en de datums van ons, de naam van hun (regel "eigen knoop", nieuw) | `andere_duur_bij_ons_en_andere_naam_bij_hun` |
| Duur van A naar 20 dagen | Duur van A naar 18 dagen | Conflict op `TaskTime.ScheduleDuration` (20 en 18) | Twee waarden voor hetzelfde veld | `allebei_een_andere_duur_voor_dezelfde_taak` |
| Duur van A | Duur van B, in dezelfde keten | Conflict | De afgeleide velden (datums van de opvolgers, de projecteinddatum) zijn aan beide kanten anders. Er gaat niets verloren, maar een planner verwacht beide. Zie "Open vraag" | `andere_duur_voor_twee_taken_in_een_keten` |
| Taak B wissen | Nieuwe relatie naar B | Conflict | De relatie van hun wijst naar een taak die er niet meer is | `ons_wist_een_taak_en_hun_legt_er_een_relatie_naar` |
| Taak B wissen | Naam van taak A | Beide | Andere objecten | `wissen_en_hernoemen` |
| A → B (eind-start) | A → B (eind-start), eigen GlobalId | Eén relatie, met de GlobalId van ons | Een relatie die alleen enkele verwijzingen naar objecten heeft, is door die objecten bepaald. Twee keer dezelfde relatie is één relatie (regel "dezelfde nieuwe relatie", nieuw). Zo staat hij er niet dubbel in | `allebei_dezelfde_nieuwe_relatie` |
| A → B (start-start) | A → B (eind-start) | Conflict | Dezelfde relatie, een ander type: welke geldt, weet alleen een planner | `dezelfde_nieuwe_relatie_met_een_ander_type` |
| A → B | B → A | Conflict (kring in de volgorde) | Elke kant apart is goed, samen is het een kring. OPS kan die niet doorrekenen (`CPMSolver.ts` r. 1669: een fout en geen berekende taken). Ook een langere kring via relaties uit de basis, en ook als een kant elders al een kring had (regel "kring in de volgorde", nieuw) | `nieuwe_kring_ons_a_naar_b_en_hun_b_naar_a`, `nieuwe_kring_over_een_bestaande_keten`, `nieuwe_kring_naast_een_oude_kring` |
| Taak K onder fase 3 | Taak K onder fase 4 | Conflict | K krijgt aan beide kanten een andere WBS-code, en zou in twee fases hangen (`Nests` is hoogstens 1) | `taak_aan_beide_kanten_naar_een_andere_samenvatting` |
| Taak K onder fase 3 | Naam van taak A | Beide | Andere objecten; de WBS-codes veranderen alleen bij ons | `verplaatsen_en_hernoemen` |
| Nieuwe taak in fase 3 | Nieuwe taak in fase 4 | Beide | Elk een eigen `IfcRelNests`; de SET van `IfcRelAssignsToControl` voegen we samen | `nieuwe_taken_in_verschillende_fases` |
| Nieuwe taak in fase 3 | Nieuwe taak in fase 3 | Conflict | De kinderen van een fase zijn een LIST; twee kanten wijzigen die anders. Zie knownbugs.md punt 31 | `nieuwe_taken_in_dezelfde_fase` |
| Baseline vastleggen | Naam van taak A | Beide | Alleen ons wijzigt `OPS_Baselines` | `baseline_bij_ons_en_naam_bij_hun` |
| Baseline vastleggen | Baseline vastleggen | Conflict | Alle baselines staan in één JSON-eigenschap; beide kanten maakten die anders. Zie knownbugs.md punt 32 | `allebei_een_baseline` |
| Opnieuw opslaan, verder niets | Naam van taak A | Hun naam | Alleen vluchtige velden verschillen | `opnieuw_opslaan_verandert_alleen_vluchtige_velden` |

Elk geslaagd resultaat gaat door een eigen controle van de taakstructuur (de testhulp `Taken`): elke taak heeft een
`IfcTaskTime` en hangt in precies één fase of onder het werkschema, er is geen kring in de WBS, elke relatie loopt van
taak naar taak en staat er één keer in, en het werkschema bestuurt precies alle taken. Daarna leest de lezer van OPS
zelf elk resultaat (zie "Hoe getest").

## De nieuwe regels in de bibliotheek

Alle drie zijn algemeen; er is geen aparte planningmodus.

1. **Eigen knoop per attribuut.** Wijzigden beide kanten hetzelfde attribuut van een object, en is dat een verwijzing
   naar een knoop die alleen bij dat object hoort (één anker: dit attribuut van dit object), dan voegen we die knoop
   per attribuut samen, zoals een object. Twee andere waarden voor hetzelfde attribuut zijn een conflict, met het veld
   erbij (`TaskTime.ScheduleDuration`). De regel geldt niet als de knoop gedeeld is (de knoop van ons herschrijven zou
   een ander object raken), als het type een vluchtig veld heeft, of als een attribuut dat van hun komt een verwijzing
   bevat. Dan blijft het een conflict op het hele attribuut, zoals in ontwerp §5.6. Test los van OPS, in IFC4 en
   IFC4X3: `eigen_knoop_per_attribuut` (IFC2X3 kent geen `IfcTaskTime`).
2. **Dezelfde nieuwe relatie.** Voegen ons en hun allebei een relatie toe die alleen enkele verwijzingen naar objecten
   heeft (bijv. `IfcRelSequence` van A naar B), met hetzelfde type en dezelfde objecten, en is er aan beide kanten
   precies één, dan is het dezelfde relatie. Gelijke inhoud geeft er één (die van ons). Een andere inhoud is een
   conflict (`beide_toegevoegd`). Een relatie met een SET of LIST van objecten (bijv.
   `IfcRelContainedInSpatialStructure`) valt er niet onder: twee nieuwe daarvan kunnen naast elkaar bestaan.
3. **Kring in de volgorde.** Na het schrijven zoeken we de sterk samenhangende delen van de graaf van
   `IfcRelSequence` (van `RelatingProcess` naar `RelatedProcess`). Een relatie van A naar B ligt in een kring als A en
   B elkaar bereiken. Bereikten A en B elkaar in ons en in hun niet, dan is de kring nieuw: een conflict
   (`volgorde_kring`), één per kring. De controle kijkt alleen naar de relaties, niet naar datums; de rekenkern van OPS
   zit niet in de bibliotheek.

## Open vraag: afgeleide velden

Twee planners die elk een duur of een relatie wijzigen, krijgen bijna altijd een conflict op afgeleide velden, ook als
hun wijzigingen niet botsen. Er gaat niets verloren (een conflictkopie), maar een planner verwacht beide wijzigingen.

OPS rekent een planning bij het openen zelf opnieuw door (`src/state/documentActivation.ts` r. 265-281,
`prepareLoadedPayload` met `solveProject`). De opgeslagen datums gebruikt OPS dan niet, behalve in de modus "datums
zoals opgeslagen" (na een import uit een ander pakket, `OPS_ImportProvenance`, r. 296-317). Voor een gewone
OPS-planning zijn de afgeleide velden dus een kopie van wat OPS bij openen opnieuw uitrekent.

Een mogelijke volgende stap, nog niet gebouwd: een lijst afgeleide velden (de velden hierboven), die bij een botsing
de waarde van ons krijgen en in het rapport als "opnieuw doorrekenen" staan, maar alleen als het bestand niet in de
modus "datums zoals opgeslagen" staat. Andere programma's (bijv. Synchro) lezen de datums wel; die zien tot het
volgende opslaan in OPS oude datums. Dat is een besluit voor de eigenaar. De WBS-codes bij twee nieuwe taken in
dezelfde fase horen bij dezelfde vraag.

## Hoe getest

- `scripts/ops/ops.ts` en `ops.mjs`: een kleine hulp die de lezer (`readIFC`), de rekenkern (`solveProject`) en de
  schrijver (`writeIFC`) van OPS gebruikt, gebundeld met esbuild buiten de OPS-repo. Er verandert niets in OPS.
  - `wijzig <in> <uit> <json>` doet één wijziging zoals de app (bijv. `{"duur": "Structural steel, extension",
    "dagen": 20}`), rekent door en slaat op met de GlobalIds uit het ingelezen bestand.
  - `lees <bestand>…` leest elk bestand met de lezer van OPS en rekent het door. Het controleert dat de lezer elke
    `IfcTask` en `IfcRelSequence` uit de tekst heeft, dat elke ouder bestaat, dat geen relatie dubbel is, en dat
    doorrekenen lukt (geen kring).
- `scripts/ops/maak_planningen.sh <ops-repo>` maakt de bestanden in `tests/bestanden/planning/` opnieuw, uit
  `13-supermarkt-albert-heijn.ifc` (25 taken in 5 fases, 20 relaties).
- `cargo test --test planning` voegt samen en controleert de inhoud met een losse lezer (`Feiten`).
- De resultaten door OPS en IfcOpenShell:

  ```text
  SAMENVOEGEN_UITVOER=/tmp/resultaten cargo test --test planning
  node scripts/ops/ops.mjs <ops-repo> lees /tmp/resultaten/planning-*.ifc
  uv run --no-project --python 3.12 --with ifcopenshell python -I scripts/valideer_met_ifcopenshell.py /tmp/resultaten
  ```
