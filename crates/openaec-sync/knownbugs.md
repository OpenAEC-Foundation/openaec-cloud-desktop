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

Bewijs 2 (simulatie, chaosproef en lastproef, `scripts/simulatie/`): nummers 60 tot en met 69. Een deel gaat over de
server (openaec-storage, `integratie-4-4b`). Die ligt stil tot de kern gelezen is; daarom staan ze hier, met een
reproductie.

60. **Onafgemaakte multipart-uploads blijven in S3 staan** (eigen bevinding, chaosproef c1, 5 november 2026). Een
    upload die na `upload_start` stopt (verbinding weg, client gestopt, een deel naar S3 mislukt), laat een
    multipart-upload in de bucket achter. Bij een gewone upload breekt de server die nooit af: `s3.afbreken` komt
    alleen voor bij een overdracht (`overdracht.py` r. 415), en een lifecycle-regel op de bucket
    (`AbortIncompleteMultipartUpload`) staat nergens beschreven. Na de chaosruns stonden er 16 tot 65 (c1: 29, c1b: 16,
    c2: 57, c2b: 65); zonder chaos 0. Er gaat niets verloren, maar bij AWS kosten de delen opslag tot iemand ze
    afbreekt. Reproductie: `upload_start` zonder `upload_klaar`, dan
    `list_multipart_uploads` op de bucket (`simulatie.py` telt ze als `onafgemaakte_multipart`). — B
61. **Na kill -9 midden in een ronde worden opgehaalde bestanden overbodige conflictkopieën** (eigen bevinding,
    chaosproef c2, 5 november 2026). De motor schrijft zijn toestand pas aan het eind van een ronde weg (`motor.rs`,
    `sync_met_opties`, de `bewaar` na de overdrachten). Stopt de ronde hard (kill -9, een crash, de stroom valt weg)
    nadat hij bestanden ophaalde, dan kent de volgende ronde die bestanden niet. Een bestand dat dan hier en op de
    server staat zonder bekende basis, gaat zonder vergelijking van de inhoud als upload zonder basis naar de server
    (`Taak::Upload { basis: None }`), en de server maakt er een conflictkopie van, met dezelfde inhoud. Er gaat niets
    verloren, maar na één crash krijgt de gebruiker een reeks kopieën die niets nieuws bevatten: in c2 waren 110 van de
    127 conflictkopieën overbodig, ook kopieën van kopieën. Reproductie: `draai.sh repro-61-gestopte-ronde
    repro_gestopt.py` (A zet vijf notities op de server; B stopt met kill -9 zodra de eerste in zijn map staat; de
    volgende ronde van B maakt vijf conflictkopieën met dezelfde inhoud, log van 5 november). Een mogelijke richting:
    vóór zo'n upload de blokhashes van hier en van de server vergelijken (zoals `neem_regel_over` en `vastgelegd` al
    doen), of de toestand na elke overdracht bewaren. Niet gerepareerd: dit is de motor van 5b. — B
62. **De veger nummert een achterstand in één keer, en zolang wacht elke lezing van de wijzigingenlijst op de hele
    site** (eigen bevinding, lastproef d, 5 november 2026). `commitnummer.veeg()` nummert alle vastgelegde regels met
    nummer 0 in één transactie, één `UPDATE` per regel, onder het slot op de teller. Elke andere lezing van
    `wijzigingen()` op de site wacht op dat slot, ook voor een andere ruimte. Gemeten op de proefbench (4 workers),
    in twee runs: een achterstand van ongeveer 10 000 regels kost 2,5 tot 2,7 s, 40 000 regels 9,0 s, 150 000 regels
    31,6 tot 32,9 s (ongeveer 0,21 ms per regel). Twintig clients die intussen een stille ruimte lazen, wachtten even
    lang (31,1 en 32,3 s bij 150 000). Afgeleid en niet gemeten: haalt de veger de time-out van de worker (120 s op de
    proefbench, bij deze snelheid rond 550 000 regels; op een drukke machine eerder), dan draait zijn transactie
    terug, en begint de volgende lezing weer bij nul; de lijst komt dan niet meer vrij. Zo'n achterstand ontstaat als
    er veel wordt vastgelegd terwijl niemand leest, bijvoorbeeld een grote import. De gewone lezing is wel snel: de
    eerste pagina 0,06 tot 0,07 s, een lege pagina 0,02 s, een stille ruimte vanaf 0 0,02 s, de hele lijst van
    200 000 regels vanaf 0 in 7,1 tot 7,5 s (201 pagina's). Reproductie: `last.py --lijst 10000,50000,200000` (regels
    met nummer 0 direct in de tabel, dan één lezing; log `2026-11-05-d-last-4werkers.log`). Een mogelijke richting: in
    stukken nummeren (een vast aantal per transactie), of met één `UPDATE` in volgorde van `name`. Dit is servercode;
    niet gerepareerd. — B
63. **Veel schrijvers op één bestand: bijna niemand slaat op, en de laatste winnaar wint weer** (eigen bevinding,
    lastproef d, 5 november 2026). Een grens, geen fout in de code. In de lastproef schrijven 100, 200 of 300 lichte
    clients ongeveer 46 s lang hetzelfde bestand via vergelijk-en-schrijf (`bij_conflict` "weiger", zoals de
    opslaan-lus), met hoogstens 1 s pauze na een opslag. Bij 100 clients lukten 20 opslagen (0,43 per s), en 96
    clients kregen geen enkele opslag; één client had er 7. Bij 200: 11 opslagen, 199 clients zonder opslag, en één
    client had alle 11. Bij 300: 7 opslagen, 299 zonder, en één client had alle 7. Bijna elke poging trof een nieuwere
    versie: bij 100 clients weigerde de server 1 535 van de 1 855 pogingen al bij `upload_start`, en 300 bij
    `upload_klaar`. Er zijn twee oorzaken. Ten eerste groeit de tijd tussen lezen en vastleggen met de last: de
    mediaan van `upload_klaar` ging van 1,1 s (100 clients) naar 5,7 s (300). Ten tweede kent de winnaar zijn nieuwe
    versie al en schrijft hij meteen weer, terwijl de anderen eerst `versies` en de inhoud moeten lezen. De server
    kent geen volgorde of beurt. De echte opslaan-lus stopt na 5 pogingen (`LusOpties`) en maakt dan een
    conflictkopie; bij deze last wordt dus bijna elke opslag een conflictkopie. Er gaat niets verloren. Een echte
    gebruiker slaat niet elke seconde op; de proef is een bovengrens van de druk. Bij 10 echte clients (b2) gaf de
    lus 2 keer op in 239 rondes. Een mogelijke richting voor een bestand met veel schrijvers: het slot
    (`bestand_vergrendeld` bestaat al), samenvoegen op de server, of een beurt per schrijver. Reproductie:
    `last.py --fasen schrijvers --aantallen 100,200,300` (log `2026-11-05-d-last-4werkers.log`). — B
