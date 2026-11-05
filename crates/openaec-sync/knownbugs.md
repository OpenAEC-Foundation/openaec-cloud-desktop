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

De punten 20 tot en met 29 horen bij de offline wachtrij (werkstroom 6, `docs/samenwerken/offline.md`).

20. **Een eigen upload waarna een ander alweer schreef, wordt niet herkend** (eigen bevinding, 5 november 2026). Alleen
    de huidige versie telt. Kwam de upload aan (versie 2) en schreef een ander versie 3 vóór de volgende ronde, dan
    gaat het werk nog eens: bij IFC een samenvoeging met dezelfde wijzigingen (een versie te veel, of een conflictkopie
    als de ander dezelfde attributen terugdraaide), anders een conflictkopie. Geen verlies. Kan met `versies(bestand)`
    en de blokhashes per versie, of met optie C. — S
21. **Hernoemen wordt alleen herkend als een ronde het zag vóór de volgende wijziging** (eigen bevinding, 5 november
    2026). Grootte en wijzigingstijd kiezen de kandidaat; de vingerafdruk van de inhoud in de toestand bewijst het
    (review Sol op PR #11, punt 1). Draaide de app niet, of hernoemde en wijzigde de gebruiker binnen de 2 s van de
    bestandswachter, dan blijft het zoals vóór deze stap: het nieuwe pad een nieuw bestand, het oude naar de prullenbak
    (of terug als een ander het wijzigde). Zo ook zonder vingerafdruk: een placeholder die alleen online staat, en een
    bestand uit een toestand van vóór deze stap (tot de volgende overdracht). Een lastige naam (een veilige naam, een
    spatie vooraan) valt er ook buiten. Geen verlies, wel een nieuw bestand-id en geen samenvoeging. — S
22. **Een hernoeming op de server door een ander wordt niet gevolgd** (eigen bevinding, 5 november 2026). Wijzigde ik
    offline `a.ifc` en hernoemde een ander het naar `b.ifc`, dan komt mijn werk als nieuw bestand op `a.ifc`, en komt
    `b.ifc` erbij. Geen verlies, wel twee bestanden en geen samenvoeging. Kan door in de lijst het bestand-id te volgen
    en het lokale bestand mee te hernoemen. — S
23. **Twee hernoemingen tegelijk: de onze wint** (eigen bevinding, 5 november 2026). `verplaats` heeft geen basis.
    Hernoemde een ander het bestand intussen ook, dan zet onze hernoeming het naar ons pad. Geen verlies van inhoud.
    Optie D uit `offline.md`. — S
24. **Een opslaan-patroon met een tijdelijke hernoeming kan als hernoeming gelden** (eigen bevinding, 5 november 2026).
    Een programma dat bij opslaan `a` hernoemt naar `a.bak`, een nieuwe `a` schrijft en `a.bak` wist: valt een ronde
    precies tussen de eerste twee stappen, dan gaat `a` op de server naar `a.bak`, wordt de nieuwe `a` een nieuw bestand
    en gaat `a.bak` daarna naar de prullenbak. Geen verlies (de geschiedenis staat in de prullenbak), wel een nieuw
    bestand-id. Staat `*.bak` op de negeerlijst, dan speelt het niet. — S
25. **Een afwijkende bron (5a) kan na een crash nog eens naar de server** (eigen bevinding, 5 november 2026).
    `Ronde::lever_in` legt de upload wel vast als onderweg, maar kijkt bij een volgende ronde niet in de toestand: crasht
    het programma na die upload en vóór het afronden van de bron, dan gaat de bron nog eens (een tweede conflictkopie of
    nieuw bestand). Zeldzaam (een programma schreef na een vervanging in de bron) en zonder verlies. — S
26. **Elke overdracht kost een extra lezing, en een upload ook een fsync** (eigen bevinding, 5 november 2026; review
    Sol op PR #11, "dubbele volledige hashlezing"). Vóór elke upload hasht de motor het bestand (`Ronde::noteer_bestand`)
    en schrijft hij een klein bestand met fsync; de client hasht het daarna nog eens voor de upload zelf
    (`upload_poging` in `lib.rs`). Na elke download leest de motor het bestand nog eens voor de vingerafdruk
    (`Ronde::inhoud`). Bij duizenden kleine bestanden of een heel groot bestand kost dat tijd; niet gemeten. Kan door de
    hashes van de motor aan de client mee te geven, en bij een download de hashes te gebruiken die de client al
    controleerde. — B
27. **Verwijderen op de server: verkleind, niet gesloten** (ontwerp §5.5, review Sol over het geheel en op PR #11,
    punt 3). Een verwijdering gaat alleen met een zekere basis (de versieregel in de toestand), als de lijst die basis
    toont, en als het bestand vlak vóór het verwijderen opnieuw opgevraagd nog op die versie staat (`Opslag::huidig`).
    De race tussen dat opvragen en het verwijderen sluit pas als de server `verwijder(bestand, basis_versie_id)` kent
    (ontwerp §5.5, optie E in `offline.md`). Slaat een ander precies in dat gat op, dan gaat zijn nieuwe versie naar de
    prullenbak op de server (terug te halen, maar weg uit het actieve dossier). Proef
    `gewist_en_een_ander_slaat_op_vlak_voor_het_verwijderen` dekt het gat tussen de lijst en het opvragen. Dit punt is
    geen vervolgpunt: het blokkeert tot de server met een basis verwijdert. — B
28. **Het bewijs van een eigen upload hangt aan het volgnummer** (review Sol op PR #11, punt 5). `eigen_upload`
    vergelijkt de blokhashes van de huidige versie, maar `ClientOpslag::blokhashes` vraagt ze op met het volgnummer
    (`api.blokken`), niet met de versieregel. Komt hetzelfde nummer met een andere regel terug tussen de lijst en die
    vraag, dan kan de toestand een regel krijgen waar de hashes niet bij horen. Zeldzaam (een herstel of verplaatsing
    met overschrijven precies dan); de volgende ronde ziet dan een andere regel en haalt op of voegt samen. Kan met
    `blokken` per versieregel op de server, of door na de vergelijking met `Opslag::huidig` na te gaan dat dezelfde regel
    er nog staat (een extra verzoek), of met `lees_versie` als terugval (een volledige download). — B
