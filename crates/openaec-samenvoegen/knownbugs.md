# Bekende punten

Punten uit reviews die geen nieuwe fixronde vragen (VERVOLG): zeldzaam, alleen tekst, een ontbrekende test of
opruimen. Elk punt heeft een vast nummer; nummers worden nooit hergebruikt. De punten 1 tot en met 9 horen bij
PR #6 (verschil), de punten vanaf 10 bij PR #7 (samenvoegen). Zekerheid: **B** = bevestigd (met een test, een
script of door de reviewer in de code), **S** = afgeleid, niet getoetst.

1. **De voorwaarden voor de IFC2X3-tabel zijn niet uitgezocht** (review PR #6, bevinding 8, 5 november 2026). De kop
   van `IFC2X3_TC1.exp` zegt: "All rights reserved. No part of this documentation may be reproduced ... without the
   prior written permission of the owner." `src/schema/ifc2x3.txt` bevat alleen namen en structuur uit dat
   bestand. Of dat onder die zin valt, is niet nagegaan; dat is een besluit voor de eigenaar. Voor IFC4X3 heeft het
   bestand geen kop; de README van Standards-builds noemt CC BY-ND 4.0 voor "Final IFC", maar niet met zoveel
   woorden voor de EXPRESS-bestanden. De voorwaarden per schema staan in de README. — B
2. **Een element vooraan in een LIST verschuift de ankers erachter** (eigen bevinding, 5 november 2026). In een LIST
   of een enkel attribuut is de positie van een anker het hoeveelste `#n`. Voegt een versie vooraan een element toe
   (bijv. in `IfcTypeProduct.RepresentationMaps`), dan krijgen de knopen erachter een andere identiteit. Een
   diepere verwijzing ernaar (bijv. `IfcMappedItem.MappingSource`) telt dan als gewijzigd. Dat geeft een wijziging
   of conflict te veel, geen verlies. — S
3. **Gesloten: twee gelijke elementen in een SET of BAG kregen hun anker in tekstvolgorde** (eigen bevinding,
   5 november 2026; Sol ronde 3 op 4fd159d, bevinding 1, BLOKKEERT). Een wand die naar een gelijke context ging en
   tegelijk een andere volgorde, verdween uit het rapport. Nu koppelen we leden alleen als dat eenduidig kan
   (ontwerp §5.6, `stem_ankers_af`). Tests: `wand_naar_een_gelijke_context`, `gelijke_contexten_omwisselen`,
   `gelijke_nieuwe_context_vooraan`. — B
4. **Gesloten: de codetabel van `\P?\` geldt tot het einde van de tekst** (eigen bevinding, 5 november 2026).
   `src/tekst.rs` begint elke tekst met tabel A (ISO 8859-1). Sol bevestigde in de review van 1456355 dat de
   keuze alleen binnen dezelfde tekst geldt (ISO 10303-21 §6.4.3.2). — B
5. **Vluchtige waarden van een losse IfcOwnerHistory staan niet in het rapport** (review PR #6 fixronde 2,
   bevinding 4, 5 november 2026). Verwijst niets naar een IfcOwnerHistory, dan vergelijken we hem als losse knoop,
   zonder zijn vluchtige velden. Verandert alleen bijv. zijn CreationDate (1700000000 -> 1700000001), dan noemt
   `Verschil::vluchtig` dat niet. Het verschil blijft verder juist (vluchtig telt niet als wijziging). — B
6. **Gesloten: veranderden twee elementen van hetzelfde type in één SET, dan verloren hun ankers hun identiteit**
   (eigen bevinding, 5 november 2026; Sol ronde 3 op 4fd159d, bevinding 2, BLOKKEERT). Twee gewijzigde contexten, of
   een gewijzigde naast een nieuwe, maakten elk object met een vorm gewijzigd. Nu koppelen we per type op dezelfde
   verwijzende objecten en de meeste gelijke attributen (§5.6). Tests: `twee_gewijzigde_contexten`,
   `gewijzigde_naast_nieuwe_context`, `koppeling_alleen_bij_een_wederzijdse_beste_keuze`. — B
7. **Gaat een object naar een SET-lid met dezelfde inhoud, dan zijn ook de andere objecten aan die leden
   gewijzigd** (Sol ronde 3 op 4fd159d, test_07 en test_08; 5 november 2026). Het project heeft twee contexten met
   dezelfde inhoud; W hangt aan de ene, V aan de andere. Gaat W naar de context van V, dan verwijzen er daarna
   andere objecten naar de contexten. Er is dan geen eenduidige koppeling, dus beide contexten zijn nieuw, en het
   rapport noemt W én V. Dat is een wijziging te veel, geen verlies (§5.6). In het voorbeeld van Sol noemt het
   rapport de wand, de opening en het raam. — B
8. **Gesloten: een diep geneste getypte SET gaf een stack overflow** (Sol ronde 3 op 4fd159d, bevinding 3,
   5 november 2026). 1500 keer `IFCPROPERTYSETDEFINITIONSET((…))` in elkaar liet de canonisatie te diep
   terugroepen, en het proces stopte (SIGABRT). Nu geeft de lezer bij meer dan 256 niveaus in één attribuut een
   `Fout::Ongeldig` met het regelnummer. Tests: `diepe_getypte_set_is_een_fout`, `diepe_lijst_is_een_fout`. — B
10. **Twee kanten die dezelfde relatie door een gelijke nieuwe vervangen, geven een conflict** (eigen bevinding,
    5 november 2026). De regel uit de review van PR #7 (bevinding 2): verdween een relatie aan beide kanten en
    kwam er aan beide kanten een nieuwe van hetzelfde type voor dezelfde objecten, dan is dat een conflict. Dat
    geldt ook als de twee nieuwe relaties dezelfde inhoud hebben en alleen hun GlobalId verschilt. Dat is een
    conflict te veel, geen verlies. OPS schrijft de GlobalId van een relatie vast (bewaard of uit een hash), dus
    daar komt het niet voor. — S
11. **Gesloten: de meting na fixronde 1** (eigen bevinding, 5 november 2026). Op 5 november 2026 opnieuw gemeten
    op `6a27723`, drie runs per opdracht; de cijfers en de commando's staan in de README. `verschil` op twee
    bestanden van 92 MB: 2,8 s en 870 MB werd 3,4 s en 910 MB. `voeg-samen` op drie bestanden van 92 MB: 9,5 s en
    1,9 GB werd 9,7 s en 1,9 GB. — B
12. **Faalt het lezen van de huidige versie in de opslaan-lus, dan is dat een fout en geen nieuwe poging**
    (eigen bevinding, 5 november 2026). `sla_op` leest na "verouderd" de huidige versie. Is die intussen weer
    vervangen en opgeruimd, dan geeft `lees` een fout en stopt de lus met `LusFout::Opslag`, in plaats van de dan
    huidige versie op te vragen. Er gaat niets verloren: er is niets geschreven. — S
13. **Het regeleinde van ons komt uit de eerste 64 KiB** (eigen bevinding, 5 november 2026). De schrijver kiest
    `\r\n` voor nieuwe regels als die in de eerste 64 KiB van ons staat. Een bestand met `\n` in het begin en `\r\n`
    verderop krijgt dan nieuwe regels met `\n`. ISO 10303-21-lezers aanvaarden beide; het bestand blijft geldig. — S
14. **De groep van een losse knoop op een vorm van meer objecten kent die objecten niet** (eigen bevinding bij de
    review van PR #7 op `6a27723`, 5 november 2026). De sleutel van een losse knoop (bijv. een IfcStyledItem) noemt
    bij een vorm die bij meer objecten hoort alleen "meer" (`wortel_sleutel_van`), niet welke objecten. Twee
    stijlen op twee gedeelde vormen met dezelfde inhoud (bijv. die van W en V, en die van X en Y) vallen dan in één
    groep. De schrijver kiest de vorm wel goed (dezelfde objecten); alleen het besluit per groep is grover. Wijzigen
    beide kanten zo'n groep, dan kan dat een conflict te veel geven, geen verlies. — S
15. **Botsende vluchtige waarden in een gedeelde knoop geven een conflict, geen splitsing** (eigen bevinding,
    5 november 2026). Vragen objecten die in het resultaat één knoop delen een andere vluchtige waarde, en kan het
    object de eigen knoop van de andere kant niet krijgen (de knoop verschilt verder, bijv. een andere
    OwningApplication bij ons), dan is dat een conflict. Een eigen kopie voor dat object zou het oplossen. Het komt
    alleen voor als de ene kant de knoop inhoudelijk wijzigt en de andere hem anders deelt. Tests:
    `botsende_vluchtige_waarden_in_een_gedeelde_knoop`, `twee_objecten_vragen_elk_een_andere_waarde`. — B
16. **Oudere tests vergelijken met de eigen vergelijker** (review PR #7 op `6a27723`, opmerking, 5 november 2026).
    Een deel van de tests in `tests/samenvoegen.rs` controleert het resultaat met `zelfde`, dus met `verschil` uit
    deze bibliotheek. Een fout die in het verschil en in het samenvoegen samen zit, zien die tests niet. De tests
    uit de reviews gaan na met `Feiten` (een losse lezer). Die oudere tests kunnen ook naar `Feiten` over. — B

### De stromende weg (`verschil_stromend`, PR "Verschil op GB-schaal", nummers 100 tot en met 109)

100. **Nummers die niet oplopen in de tekst kosten 40 bytes per instantie** (eigen bevinding, 5 november 2026). De
     compacte index (5 bytes per instantie) werkt als de nummers in de tekst oplopen, ook met gaten. Lopen ze niet
     op, of liggen ze erg verspreid (meer dan 64 nummers per instantie), dan valt de index terug op een HashMap.
     De uitkomst blijft gelijk (de tests met `hernummer` lopen zo), alleen het geheugen niet. — B
101. **Gesloten: verwijzingen naar verderop in de tekst kostten 32 bytes per stuk** (eigen bevinding bij de meting,
     5 november 2026). De eerste versie onthield elke verwijzing naar een nummer dat nog niet gelezen was, tot het
     einde van het lezen. In het SketchUp-model van de meting wijst 45 % van de verwijzingen naar verderop (een
     plaatsing naar haar assenstelsel, een vlak naar zijn rand); op 1,2 GB was dat ongeveer 1,2 GB. Nu krijgt zo'n
     instantie een bit, en een tweede doorloop leest alleen die instanties opnieuw. — B
102. **Een kring zoekt een tweede keer door alle knopen** (eigen bevinding, 5 november 2026). Is er een kring, dan
     zoeken we opnieuw zoals `Model`, voor dezelfde eerste knoop in de foutmelding. Dat kost een doorloop extra (tijd,
     geen geheugen: "bezocht" is een bit). Het resultaat is altijd een fout. — S
103. **De verwijzende objecten van een gedeeld lid van een SET doorzoeken het hele bestand** (eigen bevinding,
     5 november 2026). `koppel_leden` vraagt welke objecten naar een lid verwijzen. Heeft het lid één verwijzer, dan
     is dat het object zelf. Anders lezen we het hele bestand, per laag van knopen, per lid (met een geheugen per
     lid). Met veel gedeelde leden die veranderden, kost dat veel tijd. — S
104. **"Gedeeld met" van een IfcOwnerHistory staat niet in de details** (eigen bevinding, 5 november 2026). Van een
     ankerknoop die niet door een knoop gedeeld wordt en IfcOwnerHistory is, rekenen we de ankers niet uit. Wordt
     een attribuut van een object aan de ene kant een gedeelde plaatsing en aan de andere kant een gedeelde
     IfcOwnerHistory (een ander type), dan toont de wijziging "niet gedeeld" in plaats van de lijst van objecten.
     Alleen tekst; of het object gewijzigd is, klopt. — S
105. **Samenvoegen op GB-schaal is er nog niet** (eigen bevinding, 5 november 2026). `voeg_samen` gebruikt nog drie
     keer `Model`. Het plan staat in `docs/samenwerken/gb-schaal.md`. — B
106. **Losse knopen met verwijzingen zoeken een eigenaar door het hele model** (eigen bevinding, 5 november 2026).
     Verwijst een losse knoop naar een knoop, dan zoeken we per object welke knopen erbij horen, met een tabel van
     alle gedeelde knopen die we tegenkomen. Dat kost tijd en geheugen naar de grootte van het model. — S
