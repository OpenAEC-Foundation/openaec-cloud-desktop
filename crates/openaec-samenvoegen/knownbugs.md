# Bekende punten

Punten uit reviews die geen nieuwe fixronde vragen (VERVOLG): zeldzaam, alleen tekst, een ontbrekende test of
opruimen. Elk punt heeft een vast nummer; nummers worden nooit hergebruikt. De punten 1 tot en met 9 horen bij
PR #6 (verschil), de punten 10 tot en met 29 bij PR #7 (samenvoegen), de punten 30 tot en met 39 bij werkstroom 7
(planningen van Open Planner Studio). Zekerheid: **B** = bevestigd (met een test, een
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
30. **Afgeleide velden van een planning geven een conflict** (eigen bevinding bij werkstroom 7, 5 november 2026).
    Na elke wijziging rekent Open Planner Studio (OPS) de planning door: de datums en de speling van de opvolgers,
    de projecteinddatum, de fases erboven en `OPS_Analysis.ExactFloat`. Wijzigt ons de duur van taak A en hun die
    van taak B in dezelfde keten, dan zijn die velden aan beide kanten anders: tientallen conflicten, geen
    samenvoeging. Er gaat niets verloren. Een planner verwacht beide wijzigingen. OPS rekent bij openen zelf opnieuw
    door, behalve in de modus "datums zoals opgeslagen"; zie `docs/samenwerken/planning.md`, "Open vraag". Test:
    `andere_duur_voor_twee_taken_in_een_keten`. — B
31. **Twee nieuwe taken in dezelfde fase geven een conflict** (eigen bevinding bij werkstroom 7, 5 november 2026).
    De kinderen van een fase staan in `IfcRelNests.RelatedObjects`, een LIST. Voegen ons en hun elk een taak toe aan
    dezelfde fase, dan wijzigen beide die LIST anders: conflict (ontwerp §5.6). Samenvoegen zou ook twee taken met
    dezelfde WBS-code geven (bijv. twee keer 3.5), want OPS nummert de code uit de boom. Test:
    `nieuwe_taken_in_dezelfde_fase`. — B
32. **Twee nieuwe baselines geven een conflict** (eigen bevinding bij werkstroom 7, 5 november 2026). OPS bewaart
    alle baselines als JSON in één eigenschap (`OPS_Baselines.Baselines`), in een set met een vaste GlobalId. Leggen
    ons en hun elk een baseline vast, dan voegen ze allebei die set toe met een andere inhoud: conflict, geen
    verlies. Test: `allebei_een_baseline`. — B
33. **Dezelfde nieuwe relatie met een breuk in de lag** (eigen bevinding bij werkstroom 7, 5 november 2026). OPS
    schrijft een lag van een breuk van een dag als minuten in `IfcLagTime`, met een record per GlobalId in
    `OPS_Sequences`. Voegen ons en hun dezelfde relatie toe met dezelfde minuten, en heeft alleen hun zo'n record,
    dan houden we de relatie van ons en het record van hun, dat naar een GlobalId wijst die er niet meer is. OPS
    leest de lag dan in minuten in plaats van in dagen; de lengte is gelijk. Niet getest. — S
34. **De boomregel en de plaatsingsregel kijken niet als een kant al een kring had** (eigen bevinding bij werkstroom 7,
    5 november 2026). De regel voor `IfcRelSequence` meldt elke nieuwe kring, ook als ons of hun elders al een kring
    had (`nieuwe_kring_naast_een_oude_kring`). De boomregel en de plaatsingsregel slaan de controle in dat geval nog
    helemaal over. In een model zonder kringen speelt dat niet. — S
35. **Bij een kring die OPS doorrekende, noemt het rapport de afgeleide velden** (eigen bevinding bij werkstroom 7,
    5 november 2026). De kringregel kijkt naar het geschreven resultaat. Hebben ons en hun hun planning doorgerekend,
    dan geven de afgeleide velden (punt 30) eerst een conflict, en komt het rapport niet tot de kring. Het blijft een
    conflict; alleen de reden in het rapport is minder duidelijk. — B
