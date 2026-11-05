# Samenvoegen op GB-schaal

Stand: donderdag 5 november 2026, branch `samenvoegen/gb-schaal` (draft-PR op `samenvoegen/2-voeg-samen`).

## Het probleem

De bibliotheek `openaec-samenvoegen` leest een versie helemaal in het geheugen ([`Model`]). Op een bestand van 92 MB
kost het verschil 3,4 s en 910 MB, het samenvoegen 9,7 s en 1,9 GB. Dat groeit lineair: een model van 1 GB vraagt
ongeveer 10 tot 20 GB, een model van 5 GB vijf keer zoveel. Dat kan op geen laptop.

## Wat er nu is: `verschil_stromend`

Eén functie erbij: `verschil_stromend(a: &[u8], b: &[u8]) -> Result<Verschil, Fout>`. Ze geeft precies dezelfde
uitkomst als `verschil`, met begrensd geheugen. De opdrachtregel (`openaec-samenvoegen verschil`) gebruikt haar, met
een mmap van beide bestanden.

### De opzet

- **Het bestand blijft waar het is.** De invoer is een `&[u8]`; de aanroeper geeft een mmap. Het besturingssysteem
  leest de delen die nodig zijn en kan ze weer vrijgeven. Het bestand komt nooit in een `String` of een `Vec`.
- **Per instantie 5 bytes.** Per instantie onthouden we de plek in het bestand (4 bytes, als afstand tot het begin
  van een blok van 4096 instanties) en 7 bits: object, er verwijst iets naar, meer dan één verwijzer, een object
  verwijst ernaar (ankerknoop), meer dan één object verwijst ernaar, een knoop verwijst ernaar, het type heeft
  vluchtige velden. Een nummer `#n` vinden we terug met één bit per nummer en een telling per 512 nummers (de rang).
  Dat werkt als de nummers oplopen in de tekst, ook met gaten (zoals in elk bestand dat we zagen).
- **Per object 64 bytes:** het nummer, de sleutel (128 bits uit BLAKE3 van de GlobalId) en de inhoudshash.
- **Opnieuw lezen in plaats van onthouden.** Attributen en verwijzingen lezen we opnieuw uit de tekst als we ze nodig
  hebben (`ontleden::instantie_op`), met een kleine cache van 8 instanties.
- **Dezelfde regels, dezelfde code.** De canonieke vorm, de ankers, de posities in een SET, de weergave en het verschil
  per object staan één keer in de bibliotheek: in de trait `Bron` en in `verschil.rs`. `Model` en de nieuwe `Versie`
  zijn allebei een `Bron`. Er is dus geen tweede kopie van de regels die uit de pas kan lopen.

### De doorlopen

1. **Lezen** (beide versies tegelijk, elk in een eigen thread): per instantie de plek en de bits, per object de
   GlobalId. Dezelfde fouten als `verschil`, in dezelfde volgorde: kapotte tekst, een nummer dat twee keer voorkomt,
   een verwijzing naar een nummer dat niet bestaat, een dubbele GlobalId.
2. **Kringen tussen knopen.** Elke kring gaat door een knoop die naar een knoop verderop in de tekst verwijst. We
   zoeken alleen vanaf die knopen. Is er een kring, dan zoeken we opnieuw zoals `Model`, voor dezelfde foutmelding.
3. **Relaties koppelen en gemeen markeren**, op de sleutels.
4. **Ankers.** Alleen de identiteit van ankerknopen waar een regel naar kijkt: een knoop verwijst ernaar (`A` in de
   canonieke vorm), of meer objecten delen hem en dat telt (`D`, niet bij IfcOwnerHistory). Van alle andere
   ankerknopen telt alleen dát het een ankerknoop is. In een SET of BAG stemmen we af met de basis, zoals `Model`.
5. **Hashes per object.** De knopen eronder rekenen we uit als we ze tegenkomen, zonder recursie. Een knoop met meer
   verwijzers onthouden we, tot ongeveer een miljoen hashes (50 MB); daarna beginnen we opnieuw.
6. **Vergelijken** op sleutel. Alleen bij een andere hash lezen we het object en zijn knopen opnieuw, voor de details.

## Gelijkwaardigheid

- **Elke test van het verschil draait op beide wegen.** De hulpfunctie `verschil` in `tests/gemeen` roept `verschil`
  en `verschil_stromend` aan en faalt als de uitkomsten (of de fouten) verschillen, met de eerste afwijkende regel in
  JSON. De tests in `tests/verschil.rs`, `tests/samenvoegen.rs` en `tests/samenwerken.rs` gebruiken die functie.
- **Ook bij elk samenvoegen.** In `tests/samenvoegen.rs` vergelijkt `voeg` de drie paren (basis-ons, basis-hun,
  ons-hun), en `gelukt` het resultaat met de basis en met ons. Dat zijn dezelfde resultaten die het
  IfcOpenShell-script controleert (`SAMENVOEGEN_UITVOER`).
- **Het corpus** (`tests/stromend.rs`): elk testbestand met zes varianten (opnieuw genummerd, andere getallen, andere
  GlobalIds zodat relaties gekoppeld worden, `\r\n`), in beide richtingen, en met het volgende bestand. Met
  `IFC_VOORBEELDEN=<map>:<map>` ook elk `.ifc`-bestand in die mappen, bijv. `examples/` van Open Planner Studio.
- **Fouten**: kapotte tekst, een dubbele GlobalId, een kring, een verwijzing naar zichzelf en een verwijzing naar een
  nummer dat niet bestaat geven dezelfde fout op dezelfde regel.

## Getallen

GETALLEN VOLGEN

## Grenzen

- **Nummers die niet oplopen in de tekst**, of die erg verspreid liggen (meer dan 64 nummers per instantie): dan
  valt de index terug op een HashMap, ongeveer 40 bytes per instantie in plaats van 5. De uitkomst blijft gelijk.
  Geen van de bestanden die we zagen, heeft dit; een programma dat bij het opslaan opnieuw nummert, schrijft de
  nummers op volgorde.
- **Verwijzingen naar verderop in de tekst** onthouden we tot het einde van het lezen: 25 bytes per verwijzing. In de
  bestanden die we zagen, is dat ongeveer één per object (een plaatsing die naar haar assenstelsel verwijst).
- **Een kring** is een fout. Voor dezelfde foutmelding als `verschil` zoeken we dan door alle knopen, met een set
  van alle bezochte knopen. Dat kost op een groot kapot bestand veel geheugen.
- **De tijd.** Een gedeelde knoop die we vergaten, rekenen we opnieuw uit. Elke knoop lezen we minstens twee keer:
  bij het lezen en bij de hash. De hashes per object rekenen we in één thread per versie uit.
- **Losse knopen met verwijzingen** (bijv. een losse IfcShapeRepresentation): dan zoeken we per object welke knopen
  erbij horen, met een tabel van alle gedeelde knopen.

## Samenvoegen op GB-schaal: het plan

`voeg_samen` gebruikt nog drie keer `Model`. Op GB-schaal is het plan:

1. **Drie indexen** (basis, ons, hun) als `Versie`, en de twee verschillen basis-ons en basis-hun daaruit. Dat werkt
   nu al: het is `verschil_stromend`, twee keer.
2. **Besluiten per object.** De regels in `samenvoegen.rs` (eigenaars van knopen, koppelregel, vluchtige velden,
   losse knopen, de controle op de boom na afloop) lezen nu `Model`. Net als bij het verschil maken we ze generiek
   over `Bron`. Ze lezen dan alleen de gewijzigde objecten en hun knopen uit het bestand.
3. **Stromend schrijven.** De tekst van ons kopiëren we in blokken uit de mmap naar het nieuwe bestand. Een regel van
   een object dat hun veranderde, slaan we over; de nieuwe regels (het object van hun en zijn knopen, met nummers
   boven het hoogste nummer van ons) komen vóór `ENDSEC;`. Een knoop die alleen bij een verwijderd object hoorde,
   slaan we ook over; welke knopen dat zijn, volgt uit de eigenaar per knoop (een extra doorloop, 1 bit per instantie).
4. **De controle na afloop** (geen kring, niets wat bereikbaar was raakt los, K18) draait op de index van het
   resultaat, met dezelfde bits.

Wat er daarvoor nog nodig is:

- De regels van `samenvoegen.rs` en `schrijven.rs` generiek over `Bron` (zoals in deze PR voor het verschil).
- De eigenaar per knoop zonder een tabel van 4 bytes per instantie: alleen voor knopen onder gewijzigde objecten.
- **Blokken op objectgrenzen.** De opslag deelt een versie in blokken van 8 MiB met BLAKE3. Schuift een wijziging
  vooraan de rest van het bestand op, dan zijn alle blokken erna nieuw. Nieuwe regels achteraan zetten (zoals
  hierboven) helpt; een blokgrens op de grens van een instantie (inhoudsafhankelijk knippen) helpt meer.
- **Een model deels laden** in de apps: alleen de objecten die veranderden. Dat vraagt een API in de kern (zie het
  eindrapport, "Behoefte van de kern").
