# Bekende punten

Punten uit reviews die geen nieuwe fixronde vragen (VERVOLG): zeldzaam, alleen tekst, een ontbrekende test of
opruimen. Elk punt heeft een vast nummer; nummers worden nooit hergebruikt. De punten 1 tot en met 9 horen bij
PR #8 (veilig vervangen, stap 5a). Zekerheid: **B** = bevestigd (met een test, een script of door de reviewer in de
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
6. **Windows: de versie in de identiteit van een placeholder is niet op Windows getoetst** (eigen bevinding bij
   fixronde 3, 5 november 2026). Sinds fixronde 3 krijgt een placeholder als identiteit `<bestand>@<versie>`, en de
   ophaler vraagt die versie. Na het verwerken van een opgehaald en gewijzigd bestand zet `cloudfilter::markeer` de
   identiteit op het merkteken `lokaal` (`CfUpdatePlaceholder` met `CF_UPDATE_FLAG_MARK_IN_SYNC`), zodat de ophaler na
   "ruimte vrijmaken" de versie uit de toestand neemt. De regels zijn getest met de nagebootste Cloud Filter API; de
   echte aanroepen zijn alleen gecompileerd. Bewaart Windows de nieuwe identiteit anders dan de documentatie van
   Microsoft zegt, dan kan de ophaler na "ruimte vrijmaken" een oudere versie ophalen. Alleen op een echte
   Windows-machine te bewijzen. — S
