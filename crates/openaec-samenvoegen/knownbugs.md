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
3. **Twee gelijke elementen in een SET of BAG krijgen hun anker in tekstvolgorde** (eigen bevinding,
   5 november 2026). In een SET of BAG hangt de positie af van de inhoud van het element. Hebben twee elementen
   precies dezelfde inhoud, dan telt bovendien het hoeveelste gelijke element, en dat volgt de tekst. Een diepere
   verwijzing naar een van de twee kan dan wisselen zonder dat het verschil het ziet, of andersom een wijziging
   tonen. De twee knopen zijn inhoudelijk gelijk. — S
4. **De codetabel van `\P?\` geldt tot het einde van de tekst** (eigen bevinding, 5 november 2026). `src/tekst.rs`
   begint elke tekst met tabel A (ISO 8859-1). Dat de keuze niet over teksten heen doorloopt, is niet nagelezen in
   de tekst van ISO 10303-21 §6.4.3. — S
