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

De punten 20 tot en met 29 horen bij de offline wachtrij (werkstroom 6, `docs/samenwerken/offline.md`).

20. **Een conflictkopie die aankwam, komt na een verloren antwoord nog eens** (eigen bevinding, 5 november 2026). De
    herkenning van een eigen upload (`motor::eigen_upload`) kijkt alleen naar het eigen pad. Kwam de upload als
    conflictkopie aan ("naam (conflict …)") en ging het antwoord verloren, dan stuurt de volgende ronde het werk nog eens:
    een tweede conflictkopie met dezelfde inhoud. Geen verlies. Optie B of C uit `offline.md` lost het op. — S
21. **Een eigen upload waarna een ander alweer schreef, wordt niet herkend** (eigen bevinding, 5 november 2026). Alleen
    de huidige versie telt. Kwam de upload aan (versie 2) en schreef een ander versie 3 vóór de volgende ronde, dan
    gaat het werk nog eens: bij IFC een samenvoeging met dezelfde wijzigingen (een versie te veel, of een conflictkopie
    als de ander dezelfde attributen terugdraaide), anders een conflictkopie. Geen verlies. Kan met `versies(bestand)`
    en de blokhashes per versie, of met optie C. — S
22. **Hernoemen wordt alleen herkend als een ronde het zag vóór de volgende wijziging** (eigen bevinding, 5 november
    2026). De herkenning vergelijkt grootte en wijzigingstijd. Draaide de app niet, of hernoemde en wijzigde de gebruiker
    binnen de 2 s van de bestandswachter, dan blijft het zoals vóór deze stap: het nieuwe pad een nieuw bestand, het oude
    naar de prullenbak (of terug als een ander het wijzigde). Ook een lastige naam (een veilige naam, een spatie vooraan)
    valt erbuiten. Geen verlies, wel een nieuw bestand-id en geen samenvoeging. — S
23. **Een hernoeming op de server door een ander wordt niet gevolgd** (eigen bevinding, 5 november 2026). Wijzigde ik
    offline `a.ifc` en hernoemde een ander het naar `b.ifc`, dan komt mijn werk als nieuw bestand op `a.ifc`, en komt
    `b.ifc` erbij. Geen verlies, wel twee bestanden en geen samenvoeging. Kan door in de lijst het bestand-id te volgen
    en het lokale bestand mee te hernoemen. — S
24. **Twee hernoemingen tegelijk: de onze wint** (eigen bevinding, 5 november 2026). `verplaats` heeft geen basis.
    Hernoemde een ander het bestand intussen ook, dan zet onze hernoeming het naar ons pad. Geen verlies van inhoud.
    Optie D uit `offline.md`. — S
25. **Een opslaan-patroon met een tijdelijke hernoeming kan als hernoeming gelden** (eigen bevinding, 5 november 2026).
    Een programma dat bij opslaan `a` hernoemt naar `a.bak`, een nieuwe `a` schrijft en `a.bak` wist: valt een ronde
    precies tussen de eerste twee stappen, dan gaat `a` op de server naar `a.bak`, wordt de nieuwe `a` een nieuw bestand
    en gaat `a.bak` daarna naar de prullenbak. Geen verlies (de geschiedenis staat in de prullenbak), wel een nieuw
    bestand-id. Staat `*.bak` op de negeerlijst, dan speelt het niet. — S
26. **Een afwijkende bron (5a) kan na een crash nog eens naar de server** (eigen bevinding, 5 november 2026).
    `Ronde::lever_in` legt de upload wel vast als onderweg, maar kijkt bij een volgende ronde niet in de toestand: crasht
    het programma na die upload en vóór het afronden van de bron, dan gaat de bron nog eens (een tweede conflictkopie of
    nieuw bestand). Zeldzaam (een programma schreef na een vervanging in de bron) en zonder verlies. — S
27. **Elke upload kost een extra lezing en een fsync** (eigen bevinding, 5 november 2026). Vóór elke upload hasht de
    motor het bestand en schrijft hij een klein bestand met fsync. Bij duizenden kleine bestanden kost dat tijd; niet
    gemeten. — S
28. **Verwijderen op de server is nog niet voorwaardelijk** (ontwerp §5.5, review Sol over het geheel, 5 november
    2026). Een verwijdering gaat alleen als de lijst van deze ronde nog de basis toont. Slaat een ander op tussen die
    lijst en de verwijdering, dan gaat zijn nieuwe versie naar de prullenbak op de server (terug te halen, maar weg uit
    het actieve dossier). Proef `gewist_en_een_ander_slaat_op_vlak_voor_het_verwijderen` staat op `#[ignore]` en faalt
    nu (`cargo test --test offline -- --ignored`: `verwijderd` is 1). Stap 5b bouwt de controle vlak vóór het
    verwijderen; daarna gaat de `#[ignore]` eraf. Dicht pas met optie E uit `offline.md`. — B
