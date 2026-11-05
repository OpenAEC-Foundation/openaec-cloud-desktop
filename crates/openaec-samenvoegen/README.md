# openaec-samenvoegen

Deze bibliotheek vergelijkt twee versies van een IFC-bestand per object. Ze is de kern van "samenwerken aan
bestanden" in de OpenAEC SuperCloud: twee mensen wijzigen tegelijk hetzelfde model of dezelfde planning (Open
Planner Studio schrijft planningen als IFC). Het samenvoegen zelf (three-way merge) komt in een volgende stap.

Het ontwerp staat in openaec-storage, `docs/samenwerken/ontwerp.md` §5.6.

## Wat het doet

- Leest IFC als STEP-tekst (ISO 10303-21): HEADER en DATA, lijsten, teksten met codes (`''`, `\X2\…\X0\`, `\X\hh`,
  `\S\`), opsommingen, `$`, `*`, getypte waarden, verwijzingen, complexe instanties en commentaar. Ongeldige invoer
  geeft een fout met het regelnummer.
- Kent de schema's IFC2X3, IFC4 en IFC4X3 (uit FILE_SCHEMA). De tabellen komen uit de EXPRESS-schema's van
  buildingSMART (zie onder).
- Vergelijkt per object (`verschil`): wat erbij kwam, wat verdween en wat veranderde, per attribuut met de oude en
  de nieuwe waarde. Bij een lijst van verwijzingen (bijv. `RelatedObjects`) ook wat erbij en eraf ging.

## Hoe het vergelijkt

- Een **object** is een instantie van IfcRoot of een subtype ervan. Zijn sleutel is de GlobalId.
- Al het andere is een **knoop**: geometrie, plaatsing, eigenschapswaarden, IfcOwnerHistory, eenheden.
- De nummers `#n` doen nergens mee. Twee bestanden met dezelfde inhoud en een andere nummering zijn gelijk. Dat is
  nodig, want Open Planner Studio nummert bij elk opslaan alles opnieuw.
- Getallen vergelijken we als getal (`1.`, `1.0` en `1.E0` zijn gelijk), teksten na het decoderen van de codes.
- **Ankers.** Een knoop waar een object rechtstreeks naar verwijst (bijv. `ObjectPlacement`), is een anker van dat
  object. Een verwijzing naar het anker van een ánder object telt als dat anker, niet als zijn inhoud. Zo is "wand W
  hangt nu onder verdieping Q in plaats van P" een wijziging, ook als de plaatsingen van P en Q dezelfde inhoud
  hebben. Een knoop die meer objecten gebruiken, heeft als identiteit al zijn ankers samen. Splitst een versie zo'n
  knoop, dan is dat een wijziging.
- **Vluchtige velden** tellen niet mee: in IfcOwnerHistory de tijdstempels en de velden over de laatste wijziging,
  in IfcWorkPlan en IfcWorkSchedule `CreationDate` (Open Planner Studio schrijft daar bij elk opslaan het huidige
  tijdstip), en in de HEADER de tijdstempel van FILE_NAME. De auteur en de organisatie tellen wel mee.
- **Losse knopen**: knopen waar niets naar verwijst (bijv. IfcStyledItem, IfcIndexedColourMap). Die vergelijken we
  apart, op type, naam en wat ze aanwijzen.
- **Relaties met een nieuwe GlobalId.** Sommige programma's geven relaties (IfcRel*) bij elk opslaan een nieuwe
  GlobalId. Een nieuwe relatie koppelen we aan een verdwenen relatie van hetzelfde type, als ze dezelfde objecten
  aanwijzen in de attributen met één verwijzing (bijv. RelatingStructure), en er aan beide kanten precies één
  kandidaat is.

## Wat het nog niet kan

- Samenvoegen (komt in de volgende stap).
- `.ifczip` en `.ifcxml`.
- Schema's buiten IFC2X3, IFC4 en IFC4X3 (zoals IFC4X1): die vergelijken we met een heuristiek (een object herkennen
  aan een geldige GlobalId als eerste attribuut), en de statistiek meldt het.
- Bestanden van 4 GiB of groter.

## Opdrachtregel

```text
cargo build --release --features cli
openaec-samenvoegen verschil oud.ifc nieuw.ifc
openaec-samenvoegen verschil oud.ifc nieuw.ifc --json
```

Voorbeeld van de uitvoer:

```text
Gewijzigd (1):
  ~ IfcTask 'Clear out store' [0LopbjNfzor6Xb2xdRu7Nj]
      TaskTime.ScheduleFinish: '2027-03-03T16:00:00' -> '2027-03-04T16:00:00'
a: IFC4X3_ADD2, 165 objecten, 127 knopen
b: IFC4X3_ADD2, 165 objecten, 127 knopen
```

Exitcode: 0 = gelukt, 2 = fout.

## Meting

Gemeten op aia09-mp50 (Linux Mint 22.3), release-modus, met `tests/meting.rs`: een IFC4-bestand van 92 MB met
200 000 elementen, elk met een eigen plaatsing en geometrie (1,6 miljoen instanties).

| Wat | Tijd | Piekgeheugen |
|---|---|---|
| `verschil` (opdrachtregel, twee bestanden van 92 MB) | 2,7 s | 870 MB |

Zelf meten:

```text
cargo test --release --test meting -- --ignored --nocapture
```

## Bronnen

- De schematabellen in `src/schema/` zijn gemaakt met `scripts/genereer_schema.py` uit de officiële EXPRESS-schema's
  van buildingSMART International: `IFC2X3_TC1.exp`, `IFC4.exp` (IFC4 ADD2 TC1) en `IFC4X3_ADD2.exp`, uit
  [buildingSMART/Standards-builds](https://github.com/buildingSMART/Standards-builds/tree/main/IFC/RELEASE)
  (commit `9e87befeccde8e876b0f06325779617dedd44aa5`). © buildingSMART International Limited. Volgens hun
  copyrightvermelding mag de technische documentatie zonder voorafgaande toestemming in softwareontwikkeling worden
  gebruikt, mits de bron volledig wordt vermeld. We nemen alleen de namen van entiteiten, supertypes en attributen
  over, met de soort en de grenzen van aggregaten en de INVERSE-attributen.
- Testbestanden: zie `tests/bestanden/README.md`.

## Licentie

AGPL-3.0-only, zoals openaec-sync.
