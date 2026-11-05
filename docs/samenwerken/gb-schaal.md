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

De opdrachtregel `voeg-samen` gebruikt nog `Model` en kan hoogstens 4 GiB aan. Ze toetst de grootte nu vóór het lezen
en geeft dan meteen een fout met de grootte en de grens; vroeger las ze eerst het hele bestand en werd ze bij 6,3 GB
gedood (knownbug 90 van PR #16).

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
   GlobalId. Een verwijzing naar een nummer dat nog niet gelezen is (verderop in de tekst), geeft de instantie een
   bit; een tweede doorloop leest alleen die instanties opnieuw. Dezelfde fouten als `verschil`, in dezelfde
   volgorde: kapotte tekst, een nummer dat twee keer voorkomt, een verwijzing naar een nummer dat niet bestaat, een
   dubbele GlobalId.
2. **Kringen tussen knopen** (beide versies tegelijk). Een kring gaat altijd door een verwijzing naar verderop in de
   tekst. Is die er niet, dan zoeken we niet; anders zoals `Model`, vanaf elke knoop. "Bezocht" is een bit per
   instantie.
3. **Relaties koppelen en gemeen markeren**, op de sleutels.
4. **Ankers.** Alleen de identiteit van ankerknopen waar een regel naar kijkt: een knoop verwijst ernaar (`A` in de
   canonieke vorm), of meer objecten delen hem en dat telt (`D`, niet bij IfcOwnerHistory). Van alle andere
   ankerknopen telt alleen dát het een ankerknoop is. In een SET of BAG stemmen we af met de basis, zoals `Model`.
5. **Hashes per object** (beide versies tegelijk). De knopen eronder rekenen we uit als we ze tegenkomen, zonder recursie. Een knoop met meer
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

Gemeten op aia09-mp50 (Linux Mint 22.3, 8 kernen, 13,9 GB), release-modus, elke meting onder
`zwaar.sh --alleen` en met een harde grens van 2 GB (`systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0`).
Het meetscript is `crates/openaec-samenvoegen/scripts/meet_geheugen.sh`: de tijd, het hoogste eigen geheugen van het
programma (RssAnon) en het hoogste deel van de bestanden dat via de mmap in het geheugen stond (RssFile). Dat laatste
kan het besturingssysteem altijd weer vrijgeven; onder de grens van 2 GB krijgt het wat RssAnon overlaat. Het
script rekent MB als 1024 × 1024 bytes.

De bestanden komen uit `metingen-data/groot`: het IFC2X3-model `infra-landscaping` (SketchUp, 27 419 instanties)
k keer achter elkaar, met eigen GlobalIds en nummers per kopie. Ons en hun wijzigen elk 156 (1,2 GB) of 780 (6,3 GB)
objecten. "Index" is `verschil basis basis`: beide versies lezen en vergelijken, zonder verschil. "Verschil" is
`verschil basis ons`.

| Bestand | Wat | Tijd | Eigen geheugen | Bestand in geheugen | Uitkomst |
|---|---|---|---|---|---|
| 1,2 GB (23,2 miljoen instanties, 155 849 objecten) | index | 176 s | 373 MB | 2,3 GB | leeg |
| 1,2 GB | verschil | 164 s | 373 MB | 2,3 GB | 156 gewijzigd |
| 6,3 GB (116 miljoen instanties, 779 609 objecten) | index | 1303 s | 1701 MB | 3,2 GB | leeg |
| 6,3 GB | verschil | GETAL | GETAL | GETAL | GETAL |

Ter vergelijking: met `Model` kost een dicht model van 1 GB 2,9 GB om te lezen, 9,8 GB voor het verschil en ongeveer
20 GB voor samenvoegen (P4, desktop-PR #16, `349e3ec`; GB van 10^9 bytes). Het verschil van 1,2 GB past hier in
373 MB, het verschil van 6,3 GB in 1701 MB.

Waar het eigen geheugen heen gaat, op 6,3 GB (berekend uit de opzet, niet per onderdeel gemeten):

- de plekken en bits: 5 bytes per instantie per versie, samen ongeveer 1,16 GB (2 × 116 miljoen × 5 bytes);
- de bitmap per nummer en de tabellen `voor` en `basis`: samen ongeveer 30 MB;
- de objecten (GlobalId, sleutel, plek, hash): ongeveer 64 bytes per object, samen ongeveer 100 MB;
- de rest, ongeveer 0,5 GB: wat even nodig is bij de hashes (memo tot 2^18 knopen), de eigenaars van 254 219 losse
  knopen per versie, de stapels per thread en wat de allocator vasthoudt.

Per instantie is dat ongeveer 15 bytes voor beide versies samen. De grens van 2 GB houdt het daarom tot ongeveer
7,5 GB per bestand. Daarboven moeten de plekken (4 van de 5 bytes per instantie) kleiner worden of op schijf; dat is
nu niet gebouwd.

Hoe we hier kwamen (dezelfde bestanden, dezelfde grens):

| Versie | 1,2 GB index | 1,2 GB verschil | 6,3 GB |
|---|---|---|---|
| eerste (`330d350`): verwijzingen vooruit in een lijst | 226 s, 1589 MB | 288 s, 1522 MB | niet gemeten |
| `707b083`: een bit en een tweede doorloop | 166 s, 945 MB | 164 s, 905 MB | index gestopt door de OOM-killer bij 2 GB |
| `91ec27a`: eigenaars met een bit, kringen zonder lijst | 176 s, 373 MB | 164 s, 373 MB | index 1303 s, 1701 MB |

De tijd is nog hoog: ongeveer 145 s per GB op 1,2 GB en 205 s per GB op 6,3 GB, tegen ongeveer 37 s per GB voor `verschil` met `Model` (3,4 s op 92 MB).
We lezen elke knoop vier tot vijf keer (lezen, de tweede doorloop, kringen, hashes, eigenaars), en onder de grens
van 2 GB past het bestand niet in het geheugen, zodat het besturingssysteem delen opnieuw van de schijf leest.

## Grenzen

- **Nummers die niet oplopen in de tekst**, of die erg verspreid liggen (meer dan 64 nummers per instantie): dan
  valt de index terug op een HashMap, ongeveer 40 bytes per instantie in plaats van 5. De uitkomst blijft gelijk.
  Geen van de bestanden die we zagen, heeft dit; een programma dat bij het opslaan opnieuw nummert, schrijft de
  nummers op volgorde.
- **Verwijzingen naar verderop in de tekst** kosten een tweede doorloop over de instanties die zo verwijzen. In het
  SketchUp-model van de meting is dat 45 % van de verwijzingen. (De eerste versie onthield ze in een lijst: op
  1,2 GB was dat 1,2 GB. Zie knownbug 101.)
- **Kringen zoeken** kost een doorloop over alle knopen als er een verwijzing naar verderop is (in het
  SketchUp-model altijd).
- **De tijd.** Een gedeelde knoop die we vergaten, rekenen we opnieuw uit. Elke knoop lezen we minstens twee keer:
  bij het lezen en bij de hash. De hashes per object rekenen we in één thread per versie uit.
- **Losse knopen met verwijzingen** (bijv. een losse IfcStyledItem op geometrie): dan lopen we vanaf elk object door
  zijn knopen om de eigenaar te vinden, zoals `Model`. Dat is een doorloop extra; het geheugen is een bit per
  instantie, de knopen van één object en de knopen met meer eigenaars.

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
