# Bekende punten

Punten uit reviews die geen nieuwe fixronde vragen (VERVOLG): zeldzaam, alleen tekst, een ontbrekende test of
opruimen. Elk punt heeft een vast nummer; nummers worden nooit hergebruikt. De punten 1 tot en met 9 horen bij
PR #6 (verschil), de punten vanaf 10 bij PR #7 (samenvoegen), de punten 90 tot en met 99 bij de metingen
(`docs/samenwerken/metingen.md`). Zekerheid: **B** = bevestigd (met een test, een script of door de reviewer in de
code), **S** = afgeleid, niet getoetst.

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
90. **Een bestand van 4 GiB of groter loopt eerst het geheugen vol** (meting PR p4, 5 november 2026). De
    bibliotheek krijgt de inhoud als `&[u8]` en geeft bij 4 GiB of meer `Fout::TeGroot`. De opdrachtregel leest het
    bestand eerst helemaal in (`std::fs::read`). Met een grens van 2 GB stopte het proces op een bestand van 6,3 GB
    tijdens het inlezen (exit 137); de fout kwam nooit. Zonder grens vraagt dat eerst 6,3 GB. Een controle op de
    grootte vóór het lezen (`metadata().len()`) lost het op, ook in de client. Zie `docs/samenwerken/metingen.md`. — B
91. **Samenvoegen houdt vier versies helemaal in het geheugen** (meting PR p4, 5 november 2026). Gemeten tot 174 MB:
    het geheugen is ongeveer 3,9 × de bestandsgrootte + 670 bytes per knoop + 2,8 kB per object. Een dicht model van
    1 GB (zoals een gewone BIM-export) vraagt dan ongeveer 19 GB, een verschil ongeveer 9 GB. Op een laptop met 8 GB
    vrij ligt de grens voor samenvoegen rond 430 MB (dicht) tot 1,8 GB (veel geometrie). De getallen boven 174 MB zijn
    een schatting. Dat is het werk van de versie die streamt. — B (gemeten), S (geschat boven 174 MB)
92. **De tijden komen van een drukke machine** (meting PR p4, 5 november 2026). Tijdens de reeks draaiden zo'n 16
    andere agents (load 10 tot 60). Drie keer dezelfde samenvoeging gaf 5,0, 6,8 en 6,3 s; in de reeks staan
    uitschieters tot 9 s voor 21 MB. Het geheugen herhaalt zich binnen 1 %. Op een rustige machine opnieuw meten
    geeft betere tijden, maar geen andere conclusie. — B
93. **Het profiel is per fase, niet per functie** (meting PR p4, 5 november 2026). `perf` werkt hier niet zonder sudo
    (`kernel.perf_event_paranoid = 4`). De meetkopie meet daarom tijd en geheugen aan het eind van elke fase. Waar
    de tijd binnen een fase heen gaat (bijv. binnen "ankers en hashes"), is niet gemeten. — B
94. **Een planning van OPS wordt iets meer dan lineair trager** (meting PR p4, 5 november 2026). Samenvoegen kostte
    0,15 s per MB bij 11 MB en 0,19 s per MB bij 88 MB (5,8 miljoen objecten). Bij de andere modellen bleef de tijd
    per MB gelijk of daalde hij. Oorzaak niet nagegaan; vermoeden: de tabellen op GlobalId. Planningen van OPS zijn
    in de praktijk kleiner dan 1 MB. — S
