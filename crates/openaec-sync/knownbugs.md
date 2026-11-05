# Bekende punten

Punten uit reviews die geen nieuwe fixronde vragen (VERVOLG): zeldzaam, alleen tekst, een ontbrekende test of
opruimen. Elk punt heeft een vast nummer; nummers worden nooit hergebruikt. De punten 1 tot en met 9 horen bij
PR #8 (veilig vervangen, stap 5a), 10 tot en met 19 bij stap 5b (samenvoegen in de sync). Zekerheid: **B** = bevestigd (met een test, een script of door de reviewer in de
code), **S** = afgeleid, niet getoetst.

1. **macOS: nul gevonden processen telt als "vrij"** (review Sol op PR #8, punt 6, 5 november 2026).
   `vervang::in_gebruik` geeft `Vrij` als `proc_listpidspath` 0 teruggeeft. Die functie slaat processen over die ze
   niet kan bekijken (bijv. van een andere gebruiker). Onvolledige inzage hoort `Onbekend` te zijn, zoals op Linux
   de lease dat afdekt. Er is geen proef op een echte Mac met een proces van een andere gebruiker. — S
2. **Windows: een bron is niet aantoonbaar beschermd tegen ontwateren** (review Sol op PR #8, punt 7, 5 november
   2026). In een gekoppelde map met bestanden op aanvraag is een bron na het verplaatsen naar `.openaec/bron/` nog
   een placeholder. Of Windows (Opslagsensor, "ruimte vrijmaken") die kan ontwateren, is niet nagegaan.
   `CfRevertPlaceholder` bij het verplaatsen (als de inhoud volledig lokaal is) is een concrete mogelijkheid. Alleen
   op een echte Windows-machine te bewijzen. — S
3. **Een bron die niet af te ronden is, krijgt alleen een logregel** (review Sol op PR #8, punt 8, 5 november 2026).
   Bijvoorbeeld op een bestandssysteem zonder leases (Linux) blijft een bron bewaakt staan, zonder melding in de app.
   Niets gaat verloren (bron en journaal blijven), maar de bron neemt ruimte in. Een melding vraagt nieuwe tekst in
   de app (schermen komen later, ontwerp §8). — B
4. **Windows: `werk_alleen_online_bij` ontwatert zonder exclusieve handle** (eigen bevinding, 5 november 2026).
   Microsoft schrijft bij `CF_UPDATE_FLAG_DEHYDRATE` dat de aanroeper een exclusieve handle moet hebben.
   `cloudfilter::open` opent met `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`. Sinds fixronde 1 staat er
   `CF_UPDATE_FLAG_VERIFY_IN_SYNC` bij: een bestand dat na het ophalen gewijzigd is, wordt dan geweigerd. Een
   programma dat tegelijk via een open handle schrijft, is daarmee niet uitgesloten. Niet getest op Windows. — S
5. **Windows: het kenmerk van een placeholder is niet op Windows getoetst** (eigen bevinding bij fixronde 2, 5 november
   2026). Sinds fixronde 2 onthoudt de motor bij een placeholder het kenmerk dat de schil zelf zette: de grootte van de
   versie en de wijzigingstijd die `CfCreatePlaceholders` of `CfUpdatePlaceholder` meekreeg. Of NTFS daarna precies
   die waarden teruggeeft (ook na `CF_UPDATE_FLAG_DEHYDRATE`), is niet nagegaan. Wijkt het af, dan ziet de volgende
   ronde een lokale wijziging: het bestand wordt opgehaald en als nieuwe versie met dezelfde inhoud geüpload. Er gaat
   niets verloren. Alleen op een echte Windows-machine te bewijzen. — S
10. **Een samenvoeging hier gaat zonder `samengevoegd_uit` naar de server** (eigen bevinding bij stap 5b, 5 november
    2026). Slaat de gebruiker op terwijl de sync een samenvoeging neerzet (ontwerp §5.6, "Terugzetten in de lokale
    map", punt 2), dan voegt de volgende ronde eerst hier samen (momentopname, eigen werk, huidige versie) en schrijft
    dat met de huidige versie als basis. Die upload zegt niet dat hij een samenvoeging is: de geschiedenis op de server
    toont hem als gewone versie. Er gaat niets verloren. — B
11. **Een afwijkende bron van een IFC-bestand wordt een conflictkopie, geen samenvoeging** (eigen bevinding bij stap
    5b, 5 november 2026). Slaat de gebruiker op precies tussen de controle en het verplaatsen van de veilige routine,
    dan levert de motor die bron in langs de gewone uploadweg (`Ronde::lever_in`), met de oude basis. Bij een IFC-bestand
    wordt dat een conflictkopie, ook als samenvoegen had gekund. Er gaat niets verloren; het venster is klein. — S
12. **Een verloren antwoord bij een conflictkopie geeft nog een tweede kopie** (eigen bevinding bij stap 5b, 5 november
    2026). Na een fout bij het afronden kijkt de client of zijn inhoud op de plek staat (Sol, terloops bij review #9).
    Werd de upload een conflictkopie, dan staat zijn inhoud niet op de plek maar in de kopie: de volgende ronde
    uploadt opnieuw, en dat wordt een tweede conflictkopie met dezelfde inhoud. Er gaat niets verloren. — S
13. **`.openaec/eigen/` ruimt niet op bij een verwijderd bestand** (eigen bevinding bij stap 5b, 5 november 2026). Per
    bestand blijft de eigen invoer van de laatste drie samenvoegingen bewaard. Wordt het bestand verwijderd of
    hernoemd, dan blijven die kopieën staan tot de map ontkoppeld wordt. — B
14. **Samenvoegen met bestanden op aanvraag is niet op Windows getoetst** (eigen bevinding bij stap 5b, 5 november
    2026). Het resultaat van een samenvoeging komt via de veilige routine op het pad, ook in een map met placeholders.
    De proeven draaien op Linux, zonder Cloud Filter API. — S
