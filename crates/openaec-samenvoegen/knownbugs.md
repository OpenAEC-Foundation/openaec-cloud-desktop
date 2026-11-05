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
