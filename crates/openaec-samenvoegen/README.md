# openaec-samenvoegen

Deze bibliotheek vergelijkt en voegt versies van een IFC-bestand samen, per object. Ze is de kern van "samenwerken
aan bestanden" in de OpenAEC SuperCloud: twee mensen wijzigen tegelijk hetzelfde model of dezelfde planning (Open
Planner Studio schrijft planningen als IFC). Lukt het samenvoegen niet zeker, dan krijgt de client een rapport en
maakt hij een conflictkopie, zoals nu.

Het ontwerp staat in openaec-storage, `docs/samenwerken/ontwerp.md` §5.6.

## Wat het doet

- Leest IFC als STEP-tekst (ISO 10303-21): HEADER en DATA, lijsten, teksten met codes (`''`, `\\`, `\S\`, de
  codetabellen `\PA\` tot en met `\PI\` (ISO 8859-1 tot en met 9), `\X\hh`, `\X2\…\X0\` en `\X4\…\X0\`),
  opsommingen, `$`, `*`, getypte waarden, verwijzingen, complexe instanties en commentaar. Ongeldige invoer, ook een
  kapotte code in een tekst, geeft een fout met het regelnummer.
- Kent de schema's IFC2X3, IFC4 en IFC4X3 (uit FILE_SCHEMA). De tabellen komen uit de EXPRESS-schema's van
  buildingSMART (zie onder).
- `verschil(a, b)`: wat erbij kwam, wat verdween en wat veranderde, per attribuut met de oude en de nieuwe waarde.
  Vluchtige velden die veranderden, staan apart (zie onder).
- `voeg_samen(basis, ons, hun)`: three-way samenvoegen. Geeft het nieuwe bestand, of bij een conflict niets. Het
  rapport noemt per conflict het object, het attribuut en de waarden van basis, ons en hun, en geeft cijfers.
- `sla_op(opslag, basis, ons, &opties)`: de opslaan-lus (zie onder). Elke opslag die de trait `Opslag` (`huidige`,
  `lees`, `schrijf_als`) heeft, past; openaec-storage past er nu op.
- `kan_samenvoegen(naam)`: alleen `.ifc`.

## Hoe het vergelijkt

- Een **object** is een instantie van IfcRoot of een subtype ervan. Zijn sleutel is de GlobalId.
- Al het andere is een **knoop**: geometrie, plaatsing, eigenschapswaarden, IfcOwnerHistory, eenheden.
- De nummers `#n` doen nergens mee. Twee bestanden met dezelfde inhoud en een andere nummering zijn gelijk. Dat is
  nodig, want Open Planner Studio nummert bij elk opslaan alles opnieuw.
- Getallen vergelijken we precies op hun waarde, zonder afronden: `1.`, `1.0` en `1.E0` zijn gelijk,
  `9007199254740992.` en `9007199254740993.` niet. Teksten vergelijken we na het decoderen van de codes.
- De volgorde in een SET of BAG telt niet (bijv. de eigenschappen in `HasProperties`); in een LIST wel. Dat geldt
  ook voor een getypte waarde van een TYPE dat een SET is: `IFCPROPERTYSETDEFINITIONSET((#1,#2))`.
- Regeleinden en witruimte tussen de waarden tellen niet: een bestand met `\r\n` (zoals veel Windows-programma's
  schrijven) is gelijk aan hetzelfde bestand met `\n`.
- **Ankers.** Een knoop waar een object rechtstreeks naar verwijst (bijv. `ObjectPlacement`), is een anker van dat
  object. Een verwijzing naar het anker van een ánder object telt als dat anker, niet als zijn inhoud. Zo is "wand W
  hangt nu onder verdieping Q in plaats van P" een wijziging, ook als de plaatsingen van P en Q dezelfde inhoud
  hebben. Een knoop die meer objecten gebruiken, heeft als identiteit al zijn ankers samen. Daarin tellen alleen
  objecten die in beide versies staan: een nieuw object aan een gedeelde IfcOwnerHistory verandert de andere
  objecten niet.
- **Ankers in een SET of BAG** (ontwerp §5.6). Een SET heeft geen volgorde, dus de leden hebben geen vaste plek.
  We koppelen een lid aan een lid van de basis alleen als dat eenduidig kan: eerst op gelijke inhoud die aan beide
  kanten één keer voorkomt, daarna per type op dezelfde objecten die ernaar verwijzen en de meeste gelijke
  attributen, als elk van beide de enige beste keuze van de ander is. Een lid zonder koppeling is nieuw; elk object
  dat er dieper naar verwijst, telt als gewijzigd. Zo verbergt een andere volgorde nooit een wijziging. Een andere
  Precision van een context, of een nieuwe context ernaast, is alleen een wijziging van het project.
- **Delen telt.** Delen twee objecten een knoop (bijv. W en V één plaatsing) en krijgt V in de andere versie een
  eigen kopie met dezelfde inhoud, dan zijn W en V gewijzigd (`ObjectPlacement (gedeeld)`: "gedeeld met V" ->
  "niet gedeeld"). Bij IfcOwnerHistory telt delen niet: sommige programma's geven een gewijzigd object een eigen
  IfcOwnerHistory.
- **Vluchtige velden** maken een object niet gewijzigd: in IfcOwnerHistory de tijdstempels en de velden over de
  laatste wijziging, in IfcWorkPlan en IfcWorkSchedule `CreationDate` (Open Planner Studio schrijft daar bij elk
  opslaan het huidige tijdstip, behalve bij een baseline), en in de HEADER de tijdstempel van FILE_NAME. Het
  verschil noemt ze wel, apart onder `vluchtig`. De auteur, de organisatie en FILE_SCHEMA tellen gewoon mee.
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
| Beide wijzigden de IfcTaskTime van een taak, die alleen bij die taak hoort (bijv. ons de duur, hun de naam) | In twee delen: de naam en alle tijden samen. Beide kanten de tijden anders (ook ons de duur, hun de start) = conflict per veld dat verschilt (`TaskTime.ScheduleDuration`). Elke andere knoop: conflict op het hele attribuut |
| Beide voegden dezelfde relatie toe met een andere GlobalId (een relatie met alleen enkele verwijzingen naar objecten, bijv. IfcRelSequence van A naar B) | Gelijke inhoud: één relatie (die van ons); een andere inhoud (bijv. een ander SequenceType), of andere gegevens van Open Planner Studio onder de GlobalId in `OPS_Sequences` (een lag met een breuk, de P6-grens): conflict |
| Beide wijzigden een ander attribuut of een LIST anders | Conflict |
| De ene kant verwijderde, de andere wijzigde | Conflict |
| Beide voegden een object met dezelfde GlobalId toe, met een andere inhoud | Conflict |
| Beide wijzigden dezelfde losse knopen (bijv. een kleur) of hetzelfde HEADER-veld anders | Conflict |
| Beide vervingen dezelfde relatie door een nieuwe (bijv. een ander materiaal, met een nieuwe GlobalId) | Conflict |
| Een vluchtig veld (tijdstempels in IfcOwnerHistory en FILE_NAME, `CreationDate` van een planning) | Alleen hun wijzigde: hun; anders ons, welke kant de rest van het object ook levert. Geen conflict, behalve als objecten die één knoop delen elk een andere waarde vragen |

Na het samenvoegen controleren we, en elk probleem is een conflict:

- Iets verwijst naar een object dat er niet meer is (ook via een plaatsing).
- Een object heeft meer relaties dan een INVERSE met een bovengrens toestaat (bijv. een wand in twee verdiepingen),
  terwijl ons en hun elk binnen de grens bleven. Voor knopen tellen we hoeveel er over de grens gaan (bijv. twee
  stijlen op één vorm: `StyledByItem`); zijn dat er meer dan in ons en in hun, dan is het een conflict.
- Een aggregaat valt buiten de grenzen uit het schema (bijv. een lege `RelatedElements`).
- **De koppelregel.** Een definitie (eigenschappenset, type, materiaal, classificatie, document) gold in de basis
  voor een object. Bij de ene kant geldt hij niet meer (koppeling weg, uit de `HasPropertySets` van het type, ander
  type, of object verwijderd), en de andere kant wijzigde hem terwijl hij daar nog gold. Een definitie geldt ook
  via een andere definitie, bijv. de sets van het type. Komen het weghalen en het wijzigen van dezelfde kant, dan is
  het geen conflict.
- **De boomregel.** In IfcRelAggregates, IfcRelNests en IfcRelContainedInSpatialStructure zit geen kring, en wat in
  ons én in hun vanaf het project bereikbaar was, is dat nog steeds.
- **Volgorde van taken.** Geen nieuwe kring in `IfcRelSequence` (bijv. ons legt A -> B, hun B -> A). De graaf is die
  van Open Planner Studio: een relatie met een fase geldt voor elke taak eronder (nesting via `IfcRelNests` in het
  resultaat), en een relatie tussen een fase en een taak eronder telt niet mee. Een relatie ligt in een kring als
  haar twee einden elkaar bereiken; nieuw is de kring als ze elkaar in ons en in hun niet bereikten. Een kring die
  een kant elders al had, verbergt zo geen nieuwe. Zie `docs/samenwerken/planning.md`.
- **Plaatsingen.** Geen kring in `PlacementRelTo` (bijv. ons zet P relatief aan Q, hun Q relatief aan P). Een
  kring die ons of hun zelf al had, melden we niet.
- **Zelfcontrole.** We lezen het geschreven bestand opnieuw. Elk object, elke groep losse knopen, de HEADER en de
  vluchtige velden moeten zijn wat we bedoelden.
- Een onbekend schema, versies met een ander FILE_SCHEMA (ook IFC4 tegenover IFC4_ADD2), onbekende types of een
  geval dat we niet kennen: conflict, zonder samen te voegen.

Bij een conflict is er geen resultaat. Een keuze "ons wint" of "hun wint" is er niet: de client bewaart de eigen
versie als conflictkopie, met het rapport erbij.

**Schrijven.** Het resultaat begint bij de tekst van ons. Alleen de regels van gewijzigde objecten en hun
plaatsing of geometrie veranderen. Wat van hun komt, krijgt nieuwe nummers boven het hoogste nummer van ons. Een
knoop van ons met dezelfde inhoud gebruiken we opnieuw, maar alleen als hij bij dezelfde objecten hoort en nog niet
voor een andere knoop van hun gebruikt is; een gelijk STEP-nummer zegt niets. Hoort een knoop bij meer objecten,
dan moeten het dezelfde objecten zijn: alleen "allebei bij meer objecten" is niet genoeg (bijv. de vorm van X en Y
is niet die van W en V, ook al is de inhoud gelijk).

**Vluchtige velden van een knoop** (bijv. een IfcOwnerHistory) tellen per object dat de knoop gebruikt, niet per
knoop. Zo komt een tijdstempel van hun ook over als ons het object een eigen IfcOwnerHistory gaf. We kijken alleen
naar de ankers waar ons en hun verschillen, en zetten de gekozen waarde in de knoop die het object in het resultaat
heeft. Geeft de kant waarvan de waarde komt het object een eigen knoop, en is de knoop verder gelijk, dan krijgt
het object die eigen knoop. Is dat niet mogelijk en vragen objecten die in het resultaat één knoop delen een andere
waarde, of verandert de waarde voor een object dat hem bij ons en hun gelijk had, dan is dat een conflict. Een veld
dat niet over te nemen is, laten we nooit stil weg. Knopen waar niets meer naar
verwijst, halen we weg. De HEADER blijft van ons, behalve de velden die alleen hun wijzigde. Het regeleinde (`\n` of
`\r\n`) is dat van ons. Wijzigde een kant niets, dan is het resultaat de andere kant, byte voor byte.

## De opslaan-lus

`sla_op` schrijft de eigen versie met de basis waarop hij gemaakt is. De basis is het bestand plus een id van de
versie dat nooit terugkomt en altijd dezelfde inhoud aanwijst: bij openaec-storage `basis_bestand` en
`basis_versie_id`. Een versienummer is alleen voor mensen. Een bestand dat op
dezelfde plek opnieuw is gemaakt, is een ander bestand.

De opslag kan het schrijven op drie manieren weigeren:

- **Verouderd**: de basis hoort bij dit bestand, maar er is een nieuwere versie. De lus leest de basis en de huidige
  versie, voegt samen en schrijft opnieuw, hoogstens `max_pogingen` keer (standaard 5).
- **Onbekende basis**: de basis hoort niet (meer) bij dit bestand, of de opslag weet het niet zeker. De lus leest
  niets en voegt niet samen: dat is een conflict. Zegt een opslag "verouderd" met een ander bestand, dan behandelt
  de lus dat ook zo.
- **Vergrendeld**: een slot van een ander. Daar helpt samenvoegen niet; de lus geeft een fout.

Kan de lus de basis niet lezen (bijv. de versie is opgeruimd), dan voegt hij ook niet samen: conflict. Bij elk
conflict is er niets geschreven.

## Wat het nog niet kan

- `.ifczip` en `.ifcxml`.
- Schema's buiten IFC2X3, IFC4 en IFC4X3 (zoals IFC4X1): `verschil` werkt met een heuristiek, `voeg_samen` geeft een
  conflict.
- Twee kanten die hetzelfde attribuut van een knoop anders wijzigen (bijv. ons de x, hun de y van hetzelfde punt):
  dat is een conflict op het attribuut van het object, geen samenvoeging binnen de knoop. Alleen de IfcTaskTime van
  één taak voegen we in twee delen samen (de naam en de tijden).
- Afgeleide velden van een planning (datums en speling na doorrekenen) geven een conflict; zie
  `docs/samenwerken/planning.md`.
- Knopen die zonder object ertussen in een kring naar elkaar verwijzen, ook via ankers (bijv. twee plaatsingen
  relatief aan elkaar): `verschil` geeft een fout (`Fout::Kring`), `voeg_samen` een conflict.
- Waarden die dieper dan 256 niveaus in elkaar zitten (lijsten en getypte waarden in één attribuut): dat geeft een
  fout (`Fout::Ongeldig`) met het regelnummer.
- Bestanden van 4 GiB of groter.

Bekende kleinere punten staan in `knownbugs.md`.

## Opdrachtregel

```text
cargo build --release --features cli
openaec-samenvoegen verschil oud.ifc nieuw.ifc [--json]
openaec-samenvoegen voeg-samen basis.ifc ons.ifc hun.ifc -o uit.ifc [--rapport rapport.json]
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
| `verschil` (opdrachtregel, twee bestanden van 92 MB) | 3,4 s (3,3 tot 3,7) | 910 MB |
| `voeg-samen` (opdrachtregel, drie bestanden van 92 MB) | 9,7 s (9,4 tot 10,3) | 1,9 GB |
| `voeg-samen` op een planning van OPS (52 kB, 39 taken) | 0,02 s | 7 MB |

Gemeten op 5 november 2026, na fixronde 1 (commit `6a27723`). Na de review daarvan (vluchtige velden per object,
eigenaars per object) opnieuw vergeleken met de vorige versie (`422f9cb`), om en om onder dezelfde belasting:
`voeg-samen` 9,99 s tegen 9,86 s, beide 1,9 GB, met hetzelfde resultaat. Wijzigt hun ook de gedeelde
IfcOwnerHistory van alle elementen (een vluchtige waarde op 200 000 ankers): 11,5 s tegen 11,1 s. Na fixronde 3
van #6 (de koppeling van SET-leden, `4010e80`) om en om vergeleken met `0655eda`: `voeg-samen` 10,4 s (10,4 tot
10,8) tegen 10,7 s (10,5 tot 11,1), beide 1,9 GB; `verschil` 4,1 s (3,8 tot 4,1) tegen 3,9 s (3,7 tot 4,2), beide
0,9 GB. Het resultaat en het verschil zijn byte voor byte gelijk. Die run draaide met `nice` naast andere processen,
dus alle tijden liggen iets hoger dan in de tabel. De tijd is de middelste van drie runs, met de
spreiding erbij; het piekgeheugen is de "Maximum resident set size" van `/usr/bin/time -v`. Vóór fixronde 1 was
het 2,8 s en 870 MB voor `verschil`, en 9,5 s en 1,9 GB voor `voeg-samen`. Het geheugen is ongeveer 20 keer de
bestandsgrootte: vier modellen (basis, ons, hun en het resultaat voor de zelfcontrole). Zelf meten:

```text
cargo build --release --features cli
# Maakt target/meting/{basis,ons,hun}.ifc en meet in het proces.
cargo test --release --features cli --test meting -- --ignored --nocapture --test-threads=1
/usr/bin/time -v target/release/openaec-samenvoegen verschil target/meting/basis.ifc target/meting/ons.ifc
/usr/bin/time -v target/release/openaec-samenvoegen voeg-samen target/meting/basis.ifc target/meting/ons.ifc \
    target/meting/hun.ifc -o uit.ifc
```

## Controle met IfcOpenShell

Los van de Rust-tests (die hangen er niet van af) kun je de resultaten van de tests laten controleren door
IfcOpenShell: het schema, verplichte attributen, de grenzen van aggregaten en de INVERSE's.

```text
SAMENVOEGEN_UITVOER=/tmp/resultaten cargo test --test samenvoegen
uv run --no-project --python 3.12 --with ifcopenshell --with pytest python -I scripts/valideer_met_ifcopenshell.py [--regels] /tmp/resultaten
```

Met `--regels` controleert IfcOpenShell ook de regels uit het EXPRESS-schema (WHERE-regels en functies); dat is
trager.

Laatste run (5 november 2026, fixronde 1): 121 van 121 resultaten geldig (waarvan 6 met `\r\n` als
regeleinde), met IfcOpenShell 0.9.0.post1. Na werkstroom 7 (planningen, 5 november 2026, met de basis op `4010e80`): 188 van 188, met de resultaten van
`tests/planning.rs` erbij (`cargo test --features cli`).

## Controle met Open Planner Studio

`docs/samenwerken/planning.md` (in de root van deze repo) zegt wat samen kan in een planning van OPS. De bestanden in
`tests/bestanden/planning/` maakt OPS zelf (`scripts/ops/maak_planningen.sh <ops-repo>`). De resultaten van
`tests/planning.rs` leest de lezer van OPS terug:

```text
SAMENVOEGEN_UITVOER=/tmp/resultaten cargo test --test planning
node scripts/ops/ops.mjs <ops-repo> lees /tmp/resultaten/planning-*.ifc
```

Daarvoor is een kloon van OPS nodig met `npm ci` (of `NODE_PATH` naar zijn `node_modules`); er verandert niets in die
kloon.

## Bronnen

- De schematabellen in `src/schema/` zijn gemaakt met `scripts/genereer_schema.py` uit de officiële EXPRESS-schema's
  van buildingSMART International, uit
  [buildingSMART/Standards-builds](https://github.com/buildingSMART/Standards-builds/tree/main/IFC/RELEASE)
  (commit `9e87befeccde8e876b0f06325779617dedd44aa5`). We nemen alleen de namen van entiteiten, supertypes en
  attributen over, met de soort en de grenzen van aggregaten en de INVERSE-attributen. De bestanden zelf staan niet
  in deze repo. De voorwaarden verschillen per schema:

  | Tabel | Bron | Voorwaarden in de bron |
  |---|---|---|
  | `ifc2x3.txt` | `IFC2x3/TC1/EXPRESS/IFC2X3_TC1.exp` | Kop van het bestand: "Copyright by: buildingSMART International Ltd., 1996-2020. All rights reserved. No part of this documentation may be reproduced, stored in a retrieval system, or transmitted in any form or by any means, without the prior written permission of the owner." |
  | `ifc4.txt` | `IFC4/ADD2_TC1/EXPRESS/IFC4.exp` | Kop van het bestand: de technische documentatie "may be photocopied, used in software development, or translated into another computer language without prior written consent from buildingSMART International Limited provided that full attribution is given. Prior written consent is required if changes are made to the technical specification." |
  | `ifc4x3_add2.txt` | `IFC4_3/HTML/IFC4X3_ADD2.exp` | Het bestand heeft geen copyrightkop. De README van Standards-builds zegt: "Copyright © buildingSMART International Ltd. All Rights reserved until formal release. Final IFC is licensed under the Creative Commons Attribution-NoDerivatives 4.0 International License (CC BY-ND 4.0)." |

  Voor IFC2X3 vraagt de bron dus voorafgaande schriftelijke toestemming voor elke reproductie. Of een tabel met
  alleen namen en structuur daaronder valt, is niet nagegaan (zie `knownbugs.md`).
- Testbestanden: zie `tests/bestanden/README.md`.

## Licentie

AGPL-3.0-only, zoals openaec-sync.
