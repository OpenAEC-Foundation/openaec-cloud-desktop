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
| Een relatie (voorganger, opvolger) | `IfcRelSequence` met `SequenceType` en eventueel een `IfcLagTime` (r. 1675). Een relatie met een fase geldt voor elke taak in die fase (`expandSummaryRelations.ts`) |
| Wat bij een relatie hoort, maar niet in `IfcLagTime` past | De eigenschappenset `OPS_Sequences` op het werkschema, met de GlobalId van de relatie als sleutel (r. 1691): het record van een lag in dagen met een breuk (`DayMeasureLags`, bijv. een halve dag als `PT4H0M0S` plus `[0.5,240]`), dat van een lag in minuten met een breuk (`ExactMinuteLags`, 150,4 minuten als `PT2H30M0S` plus `[150.4,150]`) en de P6-grens (`P6StartAtPredecessorFinishBoundarySequenceGuids`) |
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
| Duur van taak A naar 20 dagen | Naam van A | Beide | De `IfcTaskTime` hoort alleen bij A. Die voegen we in twee delen samen: de tijden van ons, de naam van hun (regel 1, "taaktijd in twee delen") | `andere_duur_bij_ons_en_andere_naam_bij_hun` |
| Duur van A naar 20 dagen | Duur van A naar 18 dagen | Conflict op `TaskTime.ScheduleDuration` (20 en 18) | Twee waarden voor hetzelfde veld | `allebei_een_andere_duur_voor_dezelfde_taak` |
| Duur van A | Start van A | Conflict op `TaskTime.ScheduleDuration` en `TaskTime.ScheduleStart` | Alle tijden van een taak zijn één blok (regel 1) | `taaktijd_in_twee_delen` |
| Duur van A | Duur van B, in dezelfde keten | Conflict | De afgeleide velden (datums van de opvolgers, de projecteinddatum) zijn aan beide kanten anders. Bij A en B wijzigde de ene kant de duur en de andere kant de datums, in hetzelfde blok. Er gaat niets verloren, maar een planner verwacht beide. Zie "Afgeleide velden" | `andere_duur_voor_twee_taken_in_een_keten` |
| Taak B wissen | Nieuwe relatie naar B | Conflict | De relatie van hun wijst naar een taak die er niet meer is | `ons_wist_een_taak_en_hun_legt_er_een_relatie_naar` |
| Taak B wissen | Naam van taak A | Beide | Andere objecten | `wissen_en_hernoemen` |
| A → B (eind-start) | A → B (eind-start), eigen GlobalId | Eén relatie, met de GlobalId van ons | Twee keer dezelfde relatie is één relatie (regel 2, "dezelfde nieuwe relatie"). Zo staat hij er niet dubbel in | `allebei_dezelfde_nieuwe_relatie` |
| A → B (start-start) | A → B (eind-start) | Conflict | Dezelfde relatie, een ander type: welke geldt, weet alleen een planner | `dezelfde_nieuwe_relatie_met_een_ander_type` |
| A → B, lag 150 minuten | A → B, lag 150,4 minuten | Conflict op `OPS_Sequences` | In IFC allebei `PT2H30M0S`; alleen hun heeft een record in `OPS_Sequences` (regel 2) | `dezelfde_nieuwe_relatie_met_andere_gegevens_van_ops` |
| A → B, lag 240 minuten | A → B, lag een halve dag | Conflict op `OPS_Sequences` | In IFC allebei `PT4H0M0S`; alleen hun heeft het record `[0.5,240]` (regel 2) | idem |
| A → B | A → B met de P6-grens | Conflict op `OPS_Sequences` | Alleen hun staat in de lijst van de P6-grens (regel 2) | idem |
| A → B, lag een halve dag | A → B, lag een halve dag | Conflict op de set `OPS_Sequences` | Hetzelfde record, maar onder een andere GlobalId. We voegen de JSON niet samen. Er gaat niets verloren | idem |
| A → B | B → A | Conflict (kring in de volgorde) | Elke kant apart is goed, samen is het een kring. OPS kan die niet doorrekenen (`CPMSolver.ts` r. 1669: een fout en geen berekende taken). Ook een langere kring via relaties uit de basis, en ook als een kant elders al een kring had (regel 3) | `nieuwe_kring_ons_a_naar_b_en_hun_b_naar_a`, `nieuwe_kring_over_een_bestaande_keten`, `nieuwe_kring_naast_een_oude_kring` |
| Fase P → B | B → A, en A zit in P | Conflict (kring in de volgorde) | OPS rekent P → B door als A → B (regel 3) | `nieuwe_kring_via_een_fase` |
| Taak K onder fase P | P → L, en in de basis loopt L via twee taken naar K | Conflict (kring in de volgorde) | Met de nesting van het resultaat geldt P → L ook voor K: K → L → … → K (regel 3) | `nieuwe_kring_door_verhangen_en_een_relatie_met_de_fase` |
| Taak K onder fase P | P → K | Beide | Een relatie tussen een fase en een taak eronder telt in OPS niet mee; geen kring | `relatie_met_de_eigen_fase_is_geen_kring` |
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

Drie regels. Geen ervan is algemeen: elk past op één soort gegevens, en de grenzen en de tegenvoorbeelden van Sol
staan erbij. Ze zijn bedoeld als invoer voor ontwerp §5.6.

1. **Taaktijd in twee delen.** Wijzigden beide kanten het attribuut `TaskTime` van een taak, en is dat een
   `IfcTaskTime` die alleen bij die taak hoort (één anker), dan voegen we die in twee delen samen: de naam (attribuut
   0) en alle tijden samen (de rest: soort duur, duur, start, eind, de berekende datums, speling, voortgang). Reden:
   OPS schrijft de taaknaam ook in de `IfcTaskTime` ("… Time"), dus "ons de duur, hun de naam" gaf anders altijd een
   conflict. Wijzigden beide kanten de tijden anders, dan is dat een conflict, ook als het andere velden zijn (ons de
   duur, hun de start): de tijden hangen samen, en een mengsel kan een eind geven dat niet bij start en duur past. Het
   rapport noemt per veld wat verschilt (`TaskTime.ScheduleDuration`).
   - Grenzen: alleen `IfcTaskTime` (niet `IfcTaskTimeRecurring`), alleen onder `IfcTask.TaskTime`, en niet als de
     knoop gedeeld is. Elke andere knoop blijft een conflict op het hele attribuut, zoals ontwerp §5.6 zegt. Test los
     van OPS, in IFC4 en IFC4X3: `taaktijd_in_twee_delen` (IFC2X3 kent geen `IfcTaskTime`).
   - Tegenvoorbeeld van Sol, waarom de regel niet algemeen is: een `IfcObjective` die alleen bij één
     `IfcRelAssociatesConstraint` hoort, in de basis `HARD` met `UserDefinedGrade` 'Special'. Ons wist de
     `UserDefinedGrade`, hun zet `ConstraintGrade` op `USERDEFINED`. Per attribuut samenvoegen gaf `USERDEFINED` zonder
     `UserDefinedGrade`, en dat breekt een WHERE-regel van IFC (`IfcConstraint` WR11). Nu een conflict op
     `RelatingConstraint`: `andere_eigen_knoop_blijft_een_conflict`.
2. **Dezelfde nieuwe relatie.** Voegen ons en hun allebei een relatie toe die alleen enkele verwijzingen naar
   objecten heeft (bijv. `IfcRelSequence` van A naar B), met hetzelfde type en dezelfde objecten, en is er aan beide
   kanten precies één, dan is het dezelfde relatie: hij houdt de GlobalId van ons. Gelijke inhoud geeft er één; een
   andere inhoud (bijv. een ander `SequenceType` of een andere `IfcLagTime`) is een conflict (`beide_toegevoegd`).
   OPS bewaart een deel van een relatie buiten de relatie, onder haar GlobalId, in `OPS_Sequences` (zie boven).
   Verschilt wat daar onder de GlobalId van ons en die van hun staat, dan is het ook een conflict, met
   `attribuut` `OPS_Sequences` en beide waarden in het rapport. Anders zou het record van hun los raken, want de
   relatie heeft na het samenvoegen de GlobalId van ons.
   - Grenzen: een relatie met een SET of LIST van objecten (bijv. `IfcRelContainedInSpatialStructure`) valt er niet
     onder: twee nieuwe daarvan kunnen naast elkaar bestaan. Van gegevens buiten de relatie kent de bibliotheek alleen
     de drie eigenschappen van OPS in `OPS_Sequences`. Bewaart een ander programma iets onder de GlobalId van een
     relatie, dan ziet de bibliotheek dat niet. Staat aan beide kanten hetzelfde record (allebei een halve dag), dan
     blijft het een conflict op de set `OPS_Sequences`: de sleutels verschillen en we voegen de JSON niet samen.
   - Tegenvoorbeelden van Sol, nu een conflict: ons 150 en hun 150,4 minuten; ons 240 minuten en hun een halve dag;
     alleen hun met de P6-grens. Test: `dezelfde_nieuwe_relatie_met_andere_gegevens_van_ops`, in beide richtingen.
3. **Kring in de volgorde.** Na het schrijven bouwen we de graaf van de volgorde zoals OPS hem doorrekent
   (`expandSummaryRelations` in OPS), met de nesting (`IfcRelNests`) van het resultaat. Een relatie met een fase geldt
   voor elke bladtaak eronder. Om het aantal randen klein te houden, krijgt een fase P twee hulpknopen: elke bladtaak
   onder P wijst naar "uit P", en "in P" wijst naar elke bladtaak onder P. Een relatie tussen een fase en een taak
   eronder (of de fase zelf) laten we weg, zoals OPS. Dan zoeken we de sterk samenhangende delen. Een relatie ligt in
   een kring als haar twee einden elkaar bereiken. Bereikten die einden elkaar in ons en in hun niet (elk met de eigen
   nesting), dan is de kring nieuw: een conflict (`volgorde_kring`), één per kring. De controle kijkt alleen naar
   relaties en nesting, niet naar datums; de rekenkern van OPS zit niet in de bibliotheek.
   - Grenzen: de nesting komt alleen uit `IfcRelNests`. OPS leest voor bestanden van andere programma's ook een
     `ParentGuid` als vangnet (`ifcReader.ts` r. 2455); die kennen we niet. Een fase zonder taken eronder telt bij ons
     als taak; OPS laat een relatie ermee weg. OPS laat ook relaties weg boven 50.000 uitgebreide relaties
     (`MAX_EXPANDED_RELATIONS`). In die gevallen kan de bibliotheek een kring melden die OPS niet ziet, nooit andersom.
     Andere redenen waarom OPS niet kan doorrekenen, controleren we niet.
   - Tegenvoorbeelden van Sol, nu een conflict: fase P bevat A, ons P → B, hun B → A (`nieuwe_kring_via_een_fase`);
     ons verhangt een taak in een fase, hun legt een relatie met die fase
     (`nieuwe_kring_door_verhangen_en_een_relatie_met_de_fase`). Zonder kring gaat het goed:
     `relatie_met_de_eigen_fase_is_geen_kring`.

## Afgeleide velden

Twee planners die elk een duur of een relatie wijzigen, krijgen bijna altijd een conflict op afgeleide velden, ook als
hun wijzigingen niet botsen. Er gaat niets verloren (een conflictkopie), maar een planner verwacht beide wijzigingen.
Dat conflict blijft. Een lijst velden die bij een botsing de waarde van ons krijgen, bouwen we niet: dan zou een
bestand datums bevatten die bij geen van beide kanten horen.

Wat OPS zelf uitrekent (`src/types/task.ts` r. 269, `TaskTimeComputed`): `EarlyStart`, `EarlyFinish`, `LateStart`,
`LateFinish`, `FreeFloat`, `TotalFloat` en `IsCritical`. Gemeten komen daar bij: de duur en de datums van een fase
(uit de taken eronder), `StartTime` en `FinishTime` van het plan en het werkschema, en `OPS_Analysis.ExactFloat`.
Niet afgeleid zijn de invoer van de planner: `ScheduleStart`, `ScheduleFinish`, de duur en de soort duur van een taak
(`TaskTimeInput`, r. 262), en de werkelijke datums en de voortgang.

Een latere weg, alleen voor OPS: de invoer samenvoegen en daarna de planning doorrekenen met de rekenkern van OPS,
voordat het resultaat terugkomt of wordt opgeslagen. Dan zijn de afgeleide velden geen conflict meer, want ze komen
uit de samengevoegde invoer. Waarom dat nu niet kan:

- De bibliotheek bevat geen rekenkern van OPS, en dat is zo bedoeld. Doorrekenen moet in OPS of in een dienst die OPS
  aanroept.
- Of doorrekenen mag, hangt af van het bestand. OPS rekent een planning bij het openen zelf opnieuw door
  (`src/state/documentActivation.ts` r. 265-281, `prepareLoadedPayload`), behalve in de modus "datums zoals
  opgeslagen" (r. 284-317). Die modus staat aan na een import uit een ander pakket, en blijft aan zolang
  `OPS_ImportProvenance` op het werkschema `UnchangedSinceImport` of een `SourceFormat` heeft (`ifcWriter.ts`
  r. 1023). In die modus zijn de opgeslagen datums de bron; doorrekenen zou ze veranderen.
- Een algemene vlag "veilig om door te rekenen" bestaat niet, in IFC niet en in OPS niet. Andere programma's (bijv.
  Synchro) lezen de opgeslagen datums wel.

Dit is een besluit voor de eigenaar: of en waar die latere weg komt. De WBS-codes bij twee nieuwe taken in dezelfde
fase horen bij dezelfde vraag.

## Hoe getest

- `scripts/ops/ops.ts` en `ops.mjs`: een kleine hulp die de lezer (`readIFC`), de rekenkern (`solveProject`) en de
  schrijver (`writeIFC`) van OPS gebruikt, gebundeld met esbuild buiten de OPS-repo. Er verandert niets in OPS.
  - `wijzig <in> <uit> <json>` doet één wijziging zoals de app (bijv. `{"duur": "Structural steel, extension",
    "dagen": 20}`, of een relatie met `"lagMinuten": 150.4`, `"lagDagen": 0.5` of `"p6": true`), rekent door en slaat
    op met de GlobalIds uit het ingelezen bestand.
  - `lees <bestand>…` leest elk bestand met de lezer van OPS en rekent het door. Het controleert dat de lezer elke
    `IfcTask` en `IfcRelSequence` uit de tekst heeft, dat elke ouder bestaat, dat geen relatie dubbel is, en dat
    doorrekenen lukt (geen kring).
- `scripts/ops/maak_planningen.sh <ops-repo>` maakt de bestanden in `tests/bestanden/planning/` opnieuw, uit
  `13-supermarkt-albert-heijn.ifc` (25 taken in 5 fases, 20 relaties): 23 bestanden.
- `cargo test --test planning` voegt samen en controleert de inhoud met een losse lezer (`Feiten`).
- De resultaten door OPS en IfcOpenShell:

  ```text
  SAMENVOEGEN_UITVOER=/tmp/resultaten cargo test --test planning
  node scripts/ops/ops.mjs <ops-repo> lees /tmp/resultaten/planning-*.ifc
  uv run --no-project --python 3.12 --with ifcopenshell --with pytest python -I scripts/valideer_met_ifcopenshell.py --regels /tmp/resultaten
  ```
