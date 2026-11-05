# Offline werken: de wachtrij en het afspelen

Stand: 5 november 2026. Hoort bij het ontwerp `docs/samenwerken/ontwerp.md` in openaec-storage (§5.5, §5.6 vanaf "De
opslaan-lus", §5.13). Code: `crates/openaec-sync/src/motor.rs` en `onderweg.rs`. Proeven: `tests/offline.rs`.

## Wat het moet doen

Iemand werkt uren of dagen offline. Hij slaat vaak op, maakt bestanden, hernoemt, verplaatst en wist. Intussen werken
anderen op de server door. Bij weer online komt alles netjes aan: een IFC-bestand via de opslaan-lus (samenvoegen), de
rest als gewone upload of als conflictkopie. De volgorde klopt: een map vóór het bestand erin, een hernoeming vóór de
wijziging van het hernoemde bestand. Er gaat niets verloren, ook niet na een herstart of een crash midden in het
afspelen. En niets gebeurt twee keer.

## Wat de wachtrij bewaart

Er komt geen lijst met elke handeling. De sync werkt op toestand: elke ronde vergelijkt de map, de server en de toestand
van de vorige ronde. Wat offline gebeurde, is dus het verschil tussen de map en de toestand. Tien keer opslaan is één
verschil, en wordt één upload. De wachtrij bestaat uit drie delen.

1. **De toestand** (`.openaec/staat.json`, al uit 5a en 5b; geschreven met fsync). Per bestand de basis: het bestand-id,
   het volgnummer en de `versie_id` waarop het lokale werk rust, en het kenmerk (grootte, wijzigingstijd) dat bij die
   versie hoort. Offline verandert de basis niet, want de toestand gaat pas verder na een upload of download die de
   server bevestigde. Bij een samenvoeging die nog niet lokaal staat, ook "afgeleid van S" (5b). Een bestand dat hier
   offline gewist is, blijft in de toestand staan tot de verwijdering op de server gedaan is: die regel is de basis van
   de verwijdering (ontwerp §5.5, "Ook verwijderen is voorwaardelijk").
2. **Hernoemingen** (`verplaatst` in de toestand): nieuw pad → pad op de server. Een ronde ziet een hernoeming aan een
   bekend bestand dat van zijn pad verdween, terwijl precies één nieuw bestand dezelfde grootte en wijzigingstijd heeft
   (hernoemen laat die staan). De ronde verplaatst de basis mee naar het nieuwe pad en legt het oude pad op de server
   vast. Dat gebeurt vóór het eerste verzoek aan de server, dus ook offline: de app start 2 s na een lokale wijziging
   een ronde, en die komt tot de server.
3. **Uploads onderweg** (`.openaec/onderweg/<id>.json`, nieuw; zelfde stijl als het journaal van 5a). Vóór elke upload:
   het pad, de grootte en de blokhashes van de verstuurde inhoud, en het kenmerk van de momentopname S van het lokale
   bestand. Is de verstuurde inhoud een samenvoeging van S, dan ook waar S staat (`.openaec/eigen/`, zoals in 5b). Het
   bestand gaat pas weg als de toestand met de uitkomst op schijf staat.

Waarom geen tweede toestand of journaal: de basis en S staan al in de toestand en in `.openaec/eigen/`. Het journaal van
5a gaat over het lokaal vervangen van een bestand, met een bron en stappen; een upload past daar niet in. De toestand
zelf kan niet vóór elke upload opnieuw op schijf: bij duizenden bestanden is dat te zwaar. Een klein bestand per upload
wel.

## Hoe een ronde afspeelt

1. **Herstel** (5a): onafgemaakte lokale vervangingen afmaken of terugzetten.
2. **Hernoemingen zoeken** en vastleggen in de toestand (fsync). Ook offline.
3. **Hernoemingen op de server** (`Opslag::verplaats`), vóór de lijst. De opslag maakt een ontbrekende map eerst aan.
   Zo ziet de vergelijking het bestand op zijn nieuwe pad, met zijn basis. Een wijziging daarna is een wijziging van
   hetzelfde bestand, en de opslaan-lus voegt haar samen met wat een ander intussen deed.
4. **De lijst** van de server. Offline stopt de ronde hier, en blijft alles staan.
5. **Uploads onderweg terugvinden.** Per bestand in `.openaec/onderweg/`: staat precies die inhoud (grootte en
   blokhashes) nu als huidige versie op dat pad, en kent de toestand die versie nog niet? Dan was het de eigen upload.
   De toestand neemt die versie over, met het kenmerk van S (en "afgeleid" bij een samenvoeging).
6. **Vergelijken en overdragen** (zoals in 5b): eerst de mappen voor de uploads, dan alle uploads en downloads (een
   gewijzigd IFC-bestand door de opslaan-lus), dan pas de verwijderingen. Een verwijdering op de server gaat alleen als
   de versie in de lijst nog de basis is: hetzelfde bestand-id, volgnummer en `versie_id`. Anders werkte een ander
   verder, en komt die versie terug. `Opslag::verwijder` krijgt het bestand uit de lijst, dus met de `versie_id` van de
   basis.
7. **De toestand op schijf**, daarna pas de afgehandelde bestanden in `.openaec/onderweg/` weg.

Voorbeeld: Anna hernoemt offline `planning.ifc` naar `fase 2/planning-v2.ifc` en wijzigt taak B. Piet wijzigt op de
server taak A. Bij weer online: eerst `verplaats` (de map `fase 2` komt er eerst), dan leest de lus de basis (versie 1)
en die van Piet (versie 2), voegt samen en schrijft versie 3 op het nieuwe pad. Het is hetzelfde bestand, met zijn
geschiedenis. Proef: `offline_hernoemen_en_wijzigen_terwijl_een_ander_wijzigt`.

## Herhalen zonder dubbel werk

Een fout na het versturen laat open of de server de upload vastlegde. Voorbeeld: `upload_klaar` legt versie 2 vast,
maar de verbinding valt weg vóór het antwoord. Of de client stuurt `upload_klaar` nog eens, en storage-PR #9 geeft dan
403 "Upload onbekend of verlopen". Zonder herkenning zou de volgende ronde de inhoud nog eens versturen. Bij een gewoon
bestand wordt dat een conflictkopie van het eigen werk, bij een nieuw bestand ook, en bij IFC een tweede versie.

De motor kijkt niet naar de soort fout. Na elke fout blijft het bestand in `.openaec/onderweg/` staan, en stap 5 van
de volgende ronde beslist op inhoud. Sloeg de gebruiker intussen opnieuw op, dan is dat een gewone wijziging op de
versie die al aankwam (geen conflictkopie). Kwam de upload niet aan, dan gaat hij gewoon opnieuw.

Dezelfde inhoud is genoeg om de versie over te nemen. Ook als een ander toevallig precies dit opsloeg, is het lokale
bestand gelijk aan die versie. Er gaat dus niets verloren.

`verplaats` herhalen is veilig: staat het bestand al op het doel, dan geeft de server gewoon succes (`api.py`,
`verplaats`, `doel.name == bron.name`).

## Herstel na een crash

| Moment van de crash | Wat de volgende start ziet | Uitkomst |
|---|---|---|
| Vóór het bestand onderweg | Lokale wijziging tegen de oude basis | Gewone upload |
| Na het bestand onderweg, vóór de upload | Idem; het bestand onderweg past niet bij de server | Gewone upload; het bestand gaat weg |
| Na de upload, vóór de toestand | Het bestand onderweg past bij de huidige versie | Versie overgenomen; niets opnieuw (proef `onderbreking_na_de_upload_voor_de_toestand`) |
| Idem, en de gebruiker sloeg daarna opnieuw op | Idem, plus een lokale wijziging | Eén nieuwe versie op de overgenomen versie (`onderbreking_en_daarna_opnieuw_opgeslagen`) |
| Na een samengevoegde upload, vóór de toestand | Overgenomen als "afgeleid van S" | Het resultaat komt lokaal, of S, lokaal en de server worden samengevoegd (`onderbreking_na_een_samenvoeging`) |
| Na de toestand, vóór het opruimen | De toestand kent de versie al | Het bestand onderweg gaat weg |
| Na een hernoeming hier, vóór de server | `verplaatst` in de toestand | De hernoeming gaat bij de volgende online ronde |

## Wat als de server intussen iets deed

| Op de server | Hier offline | Uitkomst |
|---|---|---|
| Een ander wijzigde het bestand | Gewijzigd | IFC: samengevoegd, of een conflictkopie met rapport. Anders: een conflictkopie |
| Een ander wiste het bestand | Gewijzigd | Het werk komt terug als nieuw bestand (proef `de_server_wiste_het_bestand_intussen`) |
| Een ander sloeg een nieuwe versie op | Gewist | De verwijdering gaat niet; de nieuwe versie komt terug (`offline_gewist_terwijl_een_ander_opsloeg`; ook bij hetzelfde volgnummer met een andere versieregel) |
| Een ander sloeg op tussen onze lijst en onze verwijdering | Gewist | Nog een gat: de versie van de ander gaat naar de prullenbak (knownbugs 28). Stap 5b haalt vlak vóór het verwijderen de huidige versie op; dicht pas met optie E |
| Een ander wiste het en maakte een nieuw met dezelfde naam | Gewijzigd | `onbekende_basis`: een conflictkopie met rapport; hier het nieuwe bestand van de ander (`dezelfde_naam_opnieuw_gemaakt_op_de_server`) |
| Een ander hernoemde het bestand | Gewijzigd | Het werk komt als nieuw bestand op het oude pad, het hernoemde bestand komt erbij. Geen verlies, wel twee bestanden (knownbugs 23) |
| Op het nieuwe pad staat al een ander bestand | Hernoemd | De server weigert `verplaats`: zoals vóór deze stap een nieuw bestand (conflictkopie) en het oude pad naar de prullenbak (`hernoemen_naar_een_bezet_pad_wordt_een_nieuw_bestand`) |
| Een ander wiste het bestand | Hernoemd | De server weigert `verplaats`: het hernoemde bestand komt terug als nieuw bestand |
| Een ander hernoemde het ook | Hernoemd | Onze hernoeming wint; `verplaats` heeft geen basis (knownbugs 24) |
| Een ander vergrendelde het | Gewijzigd of hernoemd | Zoals altijd: een conflictkopie, of `verplaats` geweigerd en dan een nieuw bestand |

## Keuzes die van de server afhangen (opties)

De server ligt stil tot de kern gelezen is, en de API van storage-PR #9 is een voorstel. De wachtrij gebruikt daarom
alleen wat de trait `Opslag` al vraagt, en legt geen vorm vast die alleen bij PR #9 past.

- **A. Herkennen op inhoud (gekozen).** Nodig: de lijst geeft per bestand de huidige versie (`versie_id`), en de opslag
  geeft de blokhashes van een versie (`Opslag::blokhashes`). Beide bestaan al. Werkt na elke soort fout. Grens: alleen
  het eigen pad en alleen de huidige versie (knownbugs 20 en 21).
- **B. Een herhaalbare afronding op de server.** `upload_klaar` met hetzelfde token geeft een tijd lang hetzelfde
  antwoord, in plaats van 403. Dan kan de client na een netwerkfout `upload_klaar` gewoon herhalen, zonder de delen
  opnieuw te sturen. Lost ook knownbugs 20 op. Vraagt een wijziging op de server.
- **C. Een sleutel per upload van de client.** De client geeft bij `upload_start` een eigen id mee; de server geeft bij
  een tweede upload met dezelfde sleutel de uitkomst van de eerste. Werkt ook over een herstart heen, en ook voor een
  conflictkopie. Het id staat dan in het bestand onderweg. Vraagt een wijziging op de server.
- **D. Verplaatsen met een basis.** `verplaats` alleen als het bestand nog op het oude pad staat. Dan wint een
  hernoeming door een ander, en wordt de onze een nieuw bestand of een keuze (knownbugs 24). Vraagt een wijziging op de
  server.

- **E. Verwijderen met een basis.** `verwijder(bestand, basis_versie_id)` (ontwerp §5.5): de server wist alleen als de
  basis nog de huidige versie is. De client heeft de basis al bij de hand (zie stap 6); alleen de aanroep verandert.
  Vraagt een wijziging op de server.

A werkt nu al, ook tegen de server van `main` (zonder vergelijk-en-schrijf schrijft de motor geen bestanden onderweg;
zie hieronder). B of C maakt het afspelen ook voor conflictkopieën herhaalbaar.

## Grenzen

- Bestanden onderweg schrijft de motor alleen bij een opslag met vergelijk-en-schrijf (`Opslag::vergelijk_en_schrijf`):
  alleen dan geeft de lijst een `versie_id` om de eigen upload aan terug te vinden. Bij een oudere server blijft alles
  zoals het was.
- Een hernoeming herkent de motor alleen als een ronde haar zag vóór het bestand daarna veranderde. Zie knownbugs 22.
- Aanwezigheid en meldingen zijn er offline niet (§5.13). Uitgecheckte objecten offline bewaren hoort bij §5.7 en is hier
  niet gebouwd.
- De punten die bewust open blijven, staan in `crates/openaec-sync/knownbugs.md` (20 en verder).
