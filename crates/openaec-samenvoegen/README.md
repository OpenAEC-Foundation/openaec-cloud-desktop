# openaec-samenvoegen

Deze bibliotheek vergelijkt twee versies van een IFC-bestand per object. Ze is de kern van "samenwerken aan
bestanden" in de OpenAEC SuperCloud: twee mensen wijzigen tegelijk hetzelfde model of dezelfde planning (Open
Planner Studio schrijft planningen als IFC). Het samenvoegen zelf (three-way merge) komt in een volgende stap.

Het ontwerp staat in openaec-storage, `docs/samenwerken/ontwerp.md` §5.6.

## Wat het doet

- Leest IFC als STEP-tekst (ISO 10303-21): HEADER en DATA, lijsten, teksten met codes (`''`, `\\`, `\S\`, de
  codetabellen `\PA\` tot en met `\PI\` (ISO 8859-1 tot en met 9), `\X\hh`, `\X2\…\X0\` en `\X4\…\X0\`),
  opsommingen, `$`, `*`, getypte waarden, verwijzingen, complexe instanties en commentaar. Ongeldige invoer, ook een
  kapotte code in een tekst, geeft een fout met het regelnummer.
- Kent de schema's IFC2X3, IFC4 en IFC4X3 (uit FILE_SCHEMA). De tabellen komen uit de EXPRESS-schema's van
  buildingSMART (zie onder).
- Vergelijkt per object (`verschil`): wat erbij kwam, wat verdween en wat veranderde, per attribuut met de oude en
  de nieuwe waarde. Bij een lijst van verwijzingen (bijv. `RelatedObjects`) ook wat erbij en eraf ging. Vluchtige
  velden die veranderden, staan apart (zie onder).

## Hoe het vergelijkt

- Een **object** is een instantie van IfcRoot of een subtype ervan. Zijn sleutel is de GlobalId.
- Al het andere is een **knoop**: geometrie, plaatsing, eigenschapswaarden, IfcOwnerHistory, eenheden.
- De nummers `#n` doen nergens mee. Twee bestanden met dezelfde inhoud en een andere nummering zijn gelijk. Dat is
  nodig, want Open Planner Studio nummert bij elk opslaan alles opnieuw.
- Getallen vergelijken we precies op hun waarde, zonder afronden: `1.`, `1.0` en `1.E0` zijn gelijk,
  `9007199254740992.` en `9007199254740993.` niet. Teksten vergelijken we na het decoderen van de codes.
- De volgorde in een SET of BAG telt niet (bijv. de eigenschappen in `HasProperties`); in een LIST wel.
- **Ankers.** Een knoop waar een object rechtstreeks naar verwijst (bijv. `ObjectPlacement`), is een anker van dat
  object. Een verwijzing naar het anker van een ánder object telt als dat anker, niet als zijn inhoud. Zo is "wand W
  hangt nu onder verdieping Q in plaats van P" een wijziging, ook als de plaatsingen van P en Q dezelfde inhoud
  hebben. Een knoop die meer objecten gebruiken, heeft als identiteit al zijn ankers samen. Daarin tellen alleen
  objecten die in beide versies staan: een nieuw object aan een gedeelde IfcOwnerHistory verandert de andere
  objecten niet.
- **Delen telt.** Delen twee objecten een knoop (bijv. W en V één plaatsing) en krijgt V in de andere versie een
  eigen kopie met dezelfde inhoud, dan zijn W en V gewijzigd (`ObjectPlacement (gedeeld)`: "gedeeld met V" ->
  "niet gedeeld"). Bij IfcOwnerHistory telt delen niet: sommige programma's geven een gewijzigd object een eigen
  IfcOwnerHistory.
- **Vluchtige velden** maken een object niet gewijzigd: in IfcOwnerHistory de tijdstempels en de velden over de
  laatste wijziging, in IfcWorkPlan en IfcWorkSchedule `CreationDate` (Open Planner Studio schrijft daar bij elk
  opslaan het huidige tijdstip, behalve bij een baseline), en in de HEADER de tijdstempel van FILE_NAME. Het
  verschil noemt ze wel, apart onder `vluchtig`. De auteur, de organisatie en FILE_SCHEMA tellen gewoon mee.
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
- Knopen die zonder object ertussen in een kring naar elkaar verwijzen: dat geeft een fout (`Fout::Kring`), want
  hun hash zou afhangen van de volgorde in het bestand.
- Bestanden van 4 GiB of groter.

Bekende kleinere punten staan in `knownbugs.md`.

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
