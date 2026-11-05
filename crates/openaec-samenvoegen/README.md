# openaec-samenvoegen

Deze bibliotheek vergelijkt en voegt versies van een IFC-bestand samen, per object. Ze is de kern van "samenwerken
aan bestanden" in de OpenAEC SuperCloud: twee mensen wijzigen tegelijk hetzelfde model of dezelfde planning (Open
Planner Studio schrijft planningen als IFC). Lukt het samenvoegen niet zeker, dan krijgt de client een rapport en
maakt hij een conflictkopie, zoals nu.

Het ontwerp staat in openaec-storage, `docs/samenwerken/ontwerp.md` §5.6.

## Wat het doet

- Leest IFC als STEP-tekst (ISO 10303-21): HEADER en DATA, lijsten, teksten met codes (`''`, `\X2\…\X0\`, `\X\hh`,
  `\S\`), opsommingen, `$`, `*`, getypte waarden, verwijzingen, complexe instanties en commentaar. Ongeldige invoer
  geeft een fout met het regelnummer.
- Kent de schema's IFC2X3, IFC4 en IFC4X3 (uit FILE_SCHEMA). De tabellen komen uit de EXPRESS-schema's van
  buildingSMART (zie onder).
- `verschil(a, b)`: wat erbij kwam, wat verdween en wat veranderde, per attribuut met de oude en de nieuwe waarde.
- `voeg_samen(basis, ons, hun, &opties)`: three-way samenvoegen. Geeft het nieuwe bestand (of niets), de conflicten
  met per conflict het object, het attribuut en de waarden van basis, ons en hun, en cijfers.
- `sla_op(opslag, basis, ons, &opties)`: de opslaan-lus. Schrijf; is de basis verouderd, lees de basis en de huidige
  versie, voeg samen en schrijf opnieuw. Een slot van een ander is een aparte fout: dan voegen we niet samen. Elke
  opslag die de trait `Opslag` (`huidige`, `lees`, `schrijf_als`) heeft, past.
- `kan_samenvoegen(naam)`: alleen `.ifc`.

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
  per groep: het type, de naam en wat ze aanwijzen van een object (bijv. de geometrie onder een kleurkaart).
- **Relaties met een nieuwe GlobalId.** Sommige programma's geven relaties (IfcRel*) bij elk opslaan een nieuwe
  GlobalId. Een nieuwe relatie koppelen we aan een verdwenen relatie van hetzelfde type, als ze dezelfde objecten
  aanwijzen in de attributen met één verwijzing (bijv. RelatingStructure), en er aan beide kanten precies één
  kandidaat is.

## Hoe het samenvoegt

| Situatie | Uitkomst |
|---|---|
| Alleen ons of alleen hun wijzigde, voegde toe of verwijderde een object | Die kant |
| Beide wijzigden, andere attributen, of hetzelfde attribuut op dezelfde manier | Beide wijzigingen |
| Beide wijzigden een SET of BAG van verwijzingen | Wat erbij kwam en wat eraf ging, van beide kanten |
| Beide wijzigden een eigenschappenset of hoeveelhedenset | Per eigenschap (op naam); twee nieuwe waarden = conflict |
| Beide wijzigden een ander attribuut of een LIST anders | Conflict |
| De ene kant verwijderde, de andere wijzigde | Conflict |
| Beide voegden een object met dezelfde GlobalId toe, met een andere inhoud | Conflict |
| Beide wijzigden dezelfde losse knopen (bijv. een kleur) of hetzelfde HEADER-veld anders | Conflict |

Na het samenvoegen controleren we, en elk probleem is een conflict:

- Iets verwijst naar een object dat er niet meer is (ook via een plaatsing).
- Een object heeft meer relaties dan een INVERSE met een bovengrens toestaat (bijv. een wand in twee verdiepingen),
  terwijl ons en hun elk binnen de grens bleven.
- Een aggregaat valt buiten de grenzen uit het schema (bijv. een lege `RelatedElements`).
- **De koppelregel.** Een definitie (eigenschappenset, type, materiaal, classificatie, document) gold in de basis
  voor een object. Bij de ene kant geldt hij niet meer (koppeling weg, uit de `HasPropertySets` van het type, ander
  type, of object verwijderd), en de andere kant wijzigde hem terwijl hij daar nog gold. Een definitie geldt ook
  via een andere definitie, bijv. de sets van het type. Komen het weghalen en het wijzigen van dezelfde kant, dan is
  het geen conflict.
- **De boomregel.** In IfcRelAggregates, IfcRelNests en IfcRelContainedInSpatialStructure zit geen kring, en wat in
  ons én in hun vanaf het project bereikbaar was, is dat nog steeds.
- **Zelfcontrole.** We lezen het geschreven bestand opnieuw. Elk object, elke groep losse knopen en de HEADER moeten
  zijn wat we bedoelden.
- Een onbekend schema, versies met een ander schema, onbekende types of een geval dat we niet kennen: conflict,
  zonder samen te voegen.

Met `bij_conflict = weiger` (standaard) is er bij een conflict geen resultaat. Met `ons` of `hun` wint die kant per
conflict: de betrokken objecten komen in hun geheel van die kant. Het rapport noemt de conflicten dan toch, met de
gekozen kant.

**Schrijven.** Het resultaat begint bij de tekst van ons. Alleen de regels van gewijzigde objecten en hun
plaatsing of geometrie veranderen. Wat van hun komt, krijgt nieuwe nummers boven het hoogste nummer van ons. Een
knoop van ons met dezelfde inhoud gebruiken we opnieuw. Knopen waar niets meer naar verwijst, halen we weg. De
HEADER blijft van ons, behalve de velden die alleen hun wijzigde. Wijzigde een kant niets, dan is het resultaat de
andere kant, byte voor byte.

## Wat het nog niet kan

- `.ifczip` en `.ifcxml`.
- Schema's buiten IFC2X3, IFC4 en IFC4X3 (zoals IFC4X1): `verschil` werkt met een heuristiek, `voeg_samen` geeft een
  conflict.
- Twee kanten die hetzelfde attribuut van een knoop anders wijzigen (bijv. ons de x, hun de y van hetzelfde punt):
  dat is een conflict op het attribuut van het object, geen samenvoeging binnen de knoop.
- Bestanden van 4 GiB of groter.

## Opdrachtregel

```text
cargo build --release --features cli
openaec-samenvoegen verschil oud.ifc nieuw.ifc [--json]
openaec-samenvoegen voeg-samen basis.ifc ons.ifc hun.ifc -o uit.ifc [--rapport rapport.json] [--bij-conflict weiger|ons|hun]
```

Voorbeeld:

```text
$ openaec-samenvoegen voeg-samen basis.ifc ons.ifc hun.ifc -o uit.ifc
Conflicten (1):
  ! beide_gewijzigd IfcBuildingElementProxy 'W-ons' [0WandW0000000000000000]: beide kanten wijzigden Name anders
      Name: basis 'W', ons 'W-ons', hun 'W-hun'
Niet samengevoegd; er is geen uitvoerbestand geschreven.
```

Exitcode: 0 = gelukt, 1 = conflicten (bij `voeg-samen`), 2 = fout.

## Meting

Gemeten op aia09-mp50 (Linux Mint 22.3, 8 kernen), release-modus. Het grote bestand komt uit `tests/meting.rs`: IFC4,
92 MB, 200 000 elementen met elk een eigen plaatsing en geometrie (1,6 miljoen instanties). Ons wijzigt de naam van
200 elementen, hun verplaatst 200 elementen en verwijdert er 50.

| Wat | Tijd | Piekgeheugen |
|---|---|---|
| `verschil` (opdrachtregel, twee bestanden van 92 MB) | 2,8 s | 870 MB |
| `voeg-samen` (opdrachtregel, drie bestanden van 92 MB) | 9,5 s | 1,9 GB |
| `voeg-samen` op een planning van OPS (52 kB, 39 taken) | 0,02 s | 7 MB |

Het geheugen is ongeveer 20 keer de bestandsgrootte: vier modellen (basis, ons, hun en het resultaat voor de
zelfcontrole). Zelf meten:

```text
cargo test --release --test meting -- --ignored --nocapture
```

## Controle met IfcOpenShell

Los van de Rust-tests (die hangen er niet van af) kun je de resultaten van de tests laten controleren door
IfcOpenShell: het schema, verplichte attributen, de grenzen van aggregaten en de INVERSE's.

```text
SAMENVOEGEN_UITVOER=/tmp/resultaten cargo test --test samenvoegen
uv run --no-project --python 3.12 --with ifcopenshell python -I scripts/valideer_met_ifcopenshell.py /tmp/resultaten
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
