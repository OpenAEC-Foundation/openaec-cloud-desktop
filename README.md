# OpenAEC Desktop App — desktop-app en opdrachtregel

[![Build OpenAEC Desktop App](https://github.com/OpenAEC-Foundation/openaec-cloud-desktop/actions/workflows/build.yml/badge.svg)](https://github.com/OpenAEC-Foundation/openaec-cloud-desktop/actions/workflows/build.yml)

Desktop-app van OpenAEC (tot versie 0.4 heette hij **OpenAEC Storage**). Voor **OpenAEC Storage**: koppel lokale
mappen aan ruimtes in je OpenAEC-omgeving, werk samen zonder elkaars werk te overschrijven, vergrendel bestanden en
deel ze met een link. Daarnaast de **OpenAEC-tools** installeren, bijwerken en starten. Gebouwd met
**Tauri 2 + Rust + SolidJS**.

## Architectuur

- **`crates/openaec-sync`** — de kern, zonder UI (ook bruikbaar voor een WebDAV-gateway of een client
  voor macOS/Linux):
  - `api` — client voor `openaec_storage.api.*` (API-sleutel of bearer-token).
  - `blokken` — BLAKE3-hashes per blok (8 MiB, groter bij zeer grote bestanden).
  - `upload_bestand` / `download_bestand` — delta: bij een wijziging gaan alleen de gewijzigde blokken
    over de lijn; de server kopieert de rest. Downloads worden gecontroleerd en atomair geplaatst.
  - `motor` — één sync-ronde: vergelijkt lokale map, server en de vorige toestand (`.openaec/staat.json`).
    De server beslist over conflicten (basisversie); bij een conflict blijven beide versies bestaan. Uploads en
    downloads lopen begrensd parallel, kleine bestanden gaan in groepen (zie Snelle sync).
  - `opslag` — de motor over de echte API (mappen worden onderweg aangemaakt).
  - `namen` — namen die niet overal kunnen (bijv. `nul`), veilige namen, het voorvoegsel `\\?\` en de negeerlijst
    (zie Lastige namen en negeerlijst).
  - `voortgang` — meldingen van de motor per bestand (start, bytevoortgang, klaar, fout) voor het statusvenster,
    hoogstens vier keer per seconde per bestand; de app kan de sync hiermee ook pauzeren.
  - `overdracht` — client voor `openaec_storage.overdracht.*` (bestanden versturen met een downloadlink, zie
    Versturen met OpenAEC): in delen uploaden, hervatten, en een map als zip (`pak_map`).
- **`src-tauri`** — de app: verbinden (sleutel in de sleutelbos van het besturingssysteem), ruimtes
  koppelen aan mappen, synchroniseren zodra er lokaal iets verandert (bestandswachter) of de wijzigingenlijst
  van de server iets nieuws meldt (controle elke 30 s, eens per 10 minuten een volledige ronde), vergrendelen, deellinks, automatische
  updates, en op Windows via de **Cloud Filter API** statusvinkjes in gekoppelde mappen en cloud-mappen (placeholders die pas bij
  openen worden opgehaald).
- **`src`** — de UI (SolidJS). Het hoofdvenster heeft vijf schermen: **Overzicht** (status per gekoppelde map),
  **Ruimtes** (ruimtes koppelen en de bestanden op de server bekijken), **Delen** (deellinks, vergrendelen, versies),
  **Tools** (zie hieronder) en **Instellingen**; onderin een statusbalk. Een linkerklik op het pictogram in het systeemvak opent het
  **statusvenster** (`status.html`): wat er nu over de lijn gaat, de laatste activiteit en knoppen om een map te
  openen of te pauzeren. Het pictogram zelf toont met een gekleurde stip of alles bijgewerkt is, de sync bezig of
  gepauzeerd is, of dat er een fout is.

## Voortgang en opslag

- **Opslag:** Overzicht en het statusvenster tonen "X GB van Y GB gebruikt (Z GB vrij)", of "X GB gebruikt (geen
  limiet)" (`openaec_storage.api.quotum`); na elke sync-ronde ververst.
- **Ronde:** in het statusvenster, het Overzicht en de statusbalk: "nog N bestanden, A GB van B GB", de snelheid
  (voortschrijdend gemiddelde over 8 seconden) en de geschatte resterende tijd.
- **Blokhergebruik:** de motor meldt per bestand eerst hoeveel er echt over de lijn moet; de app toont wat dat scheelt
  ("alleen 16 MB van 1,2 GB verstuurd"), per bestand en opgeteld per ronde.

## Snelle sync

Een ronde gaat in drie stappen: eerst alle mappen op de server die de uploads nodig hebben, dan alle uploads en
downloads begrensd parallel, en pas daarna het verwijderen (een lokaal verplaatst bestand staat zo eerst op de nieuwe
plek voordat het oude naar de prullenbak gaat). Conflictregels, de toestand en de foutpaden per bestand zijn gelijk
aan die van een ronde na elkaar; een fout bij één bestand houdt de rest niet op. Hashen en het aflopen van de map
gebeuren op een aparte thread. De app gebruikt één HTTP-client met keep-alive (en HTTP/2 als de server het aanbiedt)
en stuurt de stand hoogstens tien keer per seconde naar de vensters.

- **Overdrachten tegelijk** (Instellingen → Synchroniseren; `sync.gelijktijdig` in `instellingen.json`, 1–32,
  standaard 8): zoveel bestanden tegelijk, en de delen van een groot bestand (8 MiB per deel) binnen dezelfde limiet.
- **Kleine bestanden in groepen:** kent de server `openaec_storage.api.mogelijkheden`, dan
  - haalt de app de hele ruimte in één verzoek op met `openaec_storage.api.boom` (in plaats van `bladeren` per map);
  - maakt hij alle ontbrekende mappen van een ronde in één verzoek met `openaec_storage.api.maak_mappen`;
  - gaan bestanden tot de grens van de server (standaard 1 MiB) in groepen van hoogstens 64 bestanden of 16 MiB met
    `openaec_storage.api.upload_klein` (multipart) en `openaec_storage.api.download_klein`. Per bestand komt het
    resultaat of de weigering met foutcode terug; mislukt een verzoek als geheel, dan gaat elk bestand apart.
  Een oudere server kent `mogelijkheden` niet: dan werkt alles zoals voorheen (bladeren per map, `maak_map`,
  `upload_start`/`upload_klaar`, `blokken`/`download`).

Gemeten tegen een lokale bench (zie `scripts/meetbench` in openaec-storage): zie de tabel onder Testen.

## Gedeeld met mij

Een map of bestand die een collega met je deelde, staat in **Ruimtes** onder **Gedeeld met mij**, met wie het deelde,
je rol ("Kan bekijken", "Kan bewerken" of "Kan beheren") en de vervaldatum (server: `openaec_storage.api.gedeeld_met_mij`).

- **Een gedeelde map** bekijk je zoals een ruimte (`bladeren` met `deling`) en koppel je aan een lokale map met
  **Koppelen aan map…**. De koppeling (`instellingen.json`) heeft dan ook `deling`, `wortel` (de gedeelde map),
  `rechten` en `gedeeld_door`; `ruimte_naam` is de naam van de map. De sync gebruikt de gedeelde map als bovenste map
  (`ClientOpslag::gedeeld`): `boom`, `maak_mappen`, `upload_klein` en `wijzigingen` krijgen de deling mee, uploads en
  mappen komen onder de gedeelde map. Een oudere server zonder `deling` in `mogelijkheden`: dan per map bladeren en
  elk bestand apart. Bij elke ronde volgt de app de rechten van de server (bijv. van bewerken naar bekijken).
- **Verkenner:** het rechtermuismenu, de statusvinkjes en **Cloud-map…** (placeholders die bij openen worden opgehaald,
  met de rechten van de deling) werken zoals bij een ruimte. Delen vanuit het menu lukt alleen als je de map mag
  beheren; de gekoppelde map zelf delen deelt de gedeelde map.
- **Kan bekijken (`lezen`):** er gaat niets naar de server. Een lokaal nieuw of gewijzigd bestand blijft staan en komt
  bij de overgeslagen bestanden met de reden "alleen lezen gedeeld" (elke ronde, zonder fout en zonder opnieuw te
  proberen). Een lokaal verwijderd bestand komt terug van de server; nieuwe versies op de server komen binnen, behalve
  over een bestand dat je hier wijzigde (verwijder of herstel dat om de serverversie weer te krijgen).
- **Ingetrokken, verlopen of verwijderd:** geeft de server `deling_ingetrokken`, `deling_verlopen`, `deling_weg` of
  `deling_onbekend` (bij de lijst van een ronde of bij de controle op wijzigingen), dan stopt de koppeling: hij gaat
  uit de instellingen, de statusvinkjes gaan eraf en er komt een melding ("Tekeningen: de deling is ingetrokken. De
  koppeling is gestopt; de bestanden op deze computer blijven staan."). Geen foutlus: de map wordt niet meer
  geprobeerd. Een bestand dat midden in een ronde wordt geweigerd, staat met de reden bij de fouten van die ronde.
- **Een gedeeld bestand** synct niet mee: **Openen in de browser** opent `/opslag?deling=<deling>` op de server, waar
  je het downloadt en (bij bewerken) een nieuwe versie uploadt.

### Delen met een rol

In het deelvenster kies je bij **Met een collega** "Kan bekijken", "Kan bewerken" of "Kan beheren (mag ook delen)"
(`lezen`, `schrijven`, `beheren`). Beheren is bewerken plus verder delen en de toegang op die map regelen. Wat je zelf
mag, vraagt het venster aan de server (`openaec_storage.api.toegang`): je ziet alleen de rollen die je mag geven (nooit
meer dan je zelf hebt), en de knoppen om te delen, een rol te wijzigen (`wijzig_deling`, per collega een keuzelijst bij
**Wie heeft toegang**) en in te trekken staan er alleen als je hier mag delen. Wie via een deling alleen bekijkt of
bewerkt, ziet zijn eigen rol en de uitleg dat verder delen mag wie het beheert. Foutcodes in het venster:
`geen_beheerrecht`, `meer_dan_eigen_recht` en `buiten_deling`, naast de bestaande. Een oudere server zonder
`toegang`: zoals vroeger, delen tot en met bewerken.

## Versturen met OpenAEC

Een of meer bestanden of mappen versturen als **overdracht**: de ontvanger krijgt een downloadlink
(`/overdracht/<token>` op de server), en als je ontvangers invult ook een e-mail. Dat werkt voor elk bestand en elke
map, ook buiten de gekoppelde mappen: de bestanden komen niet in een ruimte. Serverkant: `openaec_storage.overdracht`
(zie de README van openaec-storage, "Overdrachten").

**Het venster Versturen** (`versturen.html`, `src/Verstuurvenster.tsx`, `src-tauri/src/versturen.rs`) opent vanuit het
rechtermuismenu (of via **Instellingen → Venster Versturen openen**):

- de gekozen bestanden en mappen, met **Bestanden toevoegen…**, **Map toevoegen…** en **Weghalen**. Een map gaat met al
  zijn inhoud mee als `<map>.zip` (zonder compressie): een naam op de server kan geen pad bevatten. De app pakt hem in
  de cachemap in en gooit de zip weg als de overdracht klaar is;
- optioneel **ontvangers** (komma's of spaties ertussen), een **bericht**, **Geldig tot** en een **wachtwoord**, binnen
  de grenzen van de server (`grenzen`: grootste overdracht, hoogste aantal ontvangers, langste termijn; zonder datum
  geldt de standaardtermijn). Het wachtwoord gaat niet mee in de mail;
- **Versturen**: de bestanden gaan parallel (vier tegelijk, hun delen samen binnen "Overdrachten tegelijk" uit de
  instellingen), elk in delen met een PUT naar een ondertekende link, altijd met `Content-Length` (ook bij een leeg
  bestand), met opnieuw proberen bij een tijdelijke fout en nieuwe links als een link verlopen is. De voortgang staat
  per bestand in het venster;
- **hervatten**: de stand staat in `versturen.json` (naast `instellingen.json`). Valt de verbinding weg, pauzeer je of
  start je de app opnieuw, dan toont het venster **Onderbroken overdracht** met **Hervatten**: de app legt zijn stand
  naast `info` van de server en stuurt per bestand alleen de delen die nog ontbreken (`bestand_stand`). Is een bestand
  lokaal veranderd, dan gaat het eruit en opnieuw. Een nieuwe overdracht starten trekt de onderbroken in;
- daarna de **link** met **Kopiëren** (hij staat ook al op het klembord, met een melding);
- **Mijn overdrachten** (`mijn`): status, aantal bestanden, grootte, downloads, vervaldatum, wachtwoord en ontvangers,
  met **Kopiëren** en **Intrekken** (twee klikken).

Zonder omgeving of inlog toont het venster een melding met **Inloggen** (opent het hoofdvenster); wie geen recht heeft
om te versturen (de server geeft 403: alleen wie schrijver of beheerder is in een ruimte, of een rol uit de
site_config), krijgt dat te horen.

**Het rechtermuismenu** (`src-tauri/src/rechtermuis.rs`) roept `openaec-cloud --actie versturen <paden…>` aan. De app
zet het bij elke start (het pad naar de app kan na een update veranderd zijn) en het staat aan of uit onder
**Instellingen → Rechtermuismenu**.

- **Windows:** een eigen item **Versturen met OpenAEC** in het hoofdmenu van de Verkenner, op alle bestanden (`*`) en
  mappen (`Directory`), met het app-pictogram (`HKCU\Software\Classes\*\shell\OpenAECVersturen` en
  `Directory\shell\OpenAECVersturen`, zonder beheerdersrechten). Met `MultiSelectModel=Player` werkt het ook bij een
  selectie van meerdere items: de Verkenner start de app dan per item, de single-instance-plugin stuurt de paden door en
  wat binnen 0,4 s binnenkomt, gaat samen naar het venster (en tijdens het samenstellen komt een volgende selectie
  erbij). Een start die binnenkomt terwijl de eerste nog opstart, wacht tot die kan doorsturen. In de gekoppelde mappen
  staat het item ook in het submenu **OpenAEC Desktop App** (waar eerst "Verzenden…" stond). De uninstaller ruimt het
  op (`nsis/hooks.nsh`).
  - **Windows 11** toont zulke items in het nieuwe, korte menu niet; ze staan onder **Meer opties weergeven**
    (Shift+F10), en direct in het menu als het klassieke menu aan staat. Een item in het nieuwe menu vraagt een
    `IExplorerCommand`: een COM-server (DLL) in een pakket met identiteit (een MSIX-*sparse package* met
    `desktop4:FileExplorerContextMenus`), ondertekend met hetzelfde certificaat als de app en bij het installeren
    geregistreerd met `Add-AppxPackage -ExternalLocation`. Dat is een eigen DLL, een tweede ondertekend pakket en een
    stap in de installer (en bij elke update), en het is alleen te testen met een vertrouwd certificaat. Daarom nu de
    registervariant; het nieuwe menu staat bij Nog te doen.
- **Linux:** in `~/.local/share` van de gebruiker die de app start:
  - Nautilus (GNOME): `nautilus/scripts/Versturen met OpenAEC` (onder **Scripts** in het menu), of, als
    nautilus-python geïnstalleerd is, `nautilus-python/extensions/openaec_versturen.py` (direct in het menu; dan geen
    script, anders staat het er twee keer). Nautilus opnieuw starten (`nautilus -q`) laadt een nieuwe extensie;
  - Dolphin (KDE): het service-menu `kio/servicemenus/openaec-versturen.desktop` (uitvoerbaar, zoals KDE 6 vraagt);
  - Nemo (Cinnamon): de actie `nemo/actions/openaec-versturen.nemo_action`.

  Ze roepen de AppImage zelf aan (`$APPIMAGE`) of `/usr/bin/openaec-cloud`. Uitzetten in de instellingen haalt ze weg;
  gebruik je de AppImage, zet het dan uit voordat je hem weggooit. Bij het verwijderen van de `.deb` ruimt
  `src-tauri/linux/postrm` de items op die naar het pakket wijzen (bij elke gebruiker; die van een AppImage blijven
  staan). Een postinst is niet nodig: de app zet de items bij de eerste start zelf, voor de gebruiker die hem start.
- **macOS:** een snelle actie voor de Finder, `~/Library/Services/Versturen met OpenAEC.workflow` (één stap "Voer
  shellscript uit" met de gekozen bestanden als argumenten). Hij staat in het rechtermuismenu onder **Snelle acties**
  (of **Voorzieningen**), zonder extra installatiestap; na het schrijven ververst de app de lijst (`pbs -update`).
  Uitzetten in de instellingen haalt hem weg. (Een `NSServices`-regel in de `Info.plist` van de app zou een eigen
  Objective-C-methode in de app vragen; de workflow werkt met de app zoals Tauri hem bouwt.)

## Lastige namen en negeerlijst

Een naam die op Windows niet kan, is daar met normale paden niet te openen, te verplaatsen of te verwijderen.
Hulpprogramma's maken zulke bestanden vaak per ongeluk (een shell-omleiding als `2>nul` in een Unix-shell op Windows
geeft een bestand dat letterlijk `nul` heet). Herkend worden (`namen::naamprobleem`):

- de apparaatnamen `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM0`–`COM9` en `LPT0`–`LPT9` (ook met de
  superscriptcijfers ¹²³), zonder onderscheid tussen hoofd- en kleine letters en ook met een extensie (`nul.txt`,
  `con.log`);
- namen die op een punt of spatie eindigen;
- de tekens `<>:"/\|?*` en controletekens;
- namen langer dan 255 tekens en paden langer dan 1000 tekens.

De server weigert dezelfde namen (foutcode `ongeldige_naam`), dus zo'n lokaal bestand gaat niet naar de server. Wat de
app ermee doet, staat onder Instellingen → **Bestanden met een gereserveerde naam** (`sync.gereserveerd`):

| Waarde | Wat er gebeurt |
|---|---|
| `overslaan` (standaard) | Niet uploaden; het bestand staat in de lijst **Overgeslagen bestanden** met de reden. Geen fout en geen opnieuw proberen. |
| `verwijderen` | Opruimen als het leeg is of kleiner dan 64 KB: via `\\?\` onder een veilige naam naar `.openaec/opgeruimd` en vandaar naar de prullenbak (lukt dat niet, dan verwijderd). Een groter bestand wordt overgeslagen. |
| `veilige_naam` | Toch uploaden onder een veilige naam (`nul` → `nul_`, `con.log` → `con_.log`, `rapport.` → `rapport_`), met een melding. Lokaal blijft de naam gelijk; wijzigingen gaan beide kanten op. |

Het statusvenster en het Overzicht tonen de overgeslagen bestanden met de knop **Opruimen**: na bevestiging gaan de
opruimbare bestanden veilig weg (via `\\?\`, eerst naar de prullenbak).

Komt zo'n naam van de server (bijv. van een Linux-computer, of van vóór de naamregels), dan zet de app het bestand op
Windows neer onder de veilige naam (`nul_`). De koppeling staat in `.openaec/staat.json` (`lokale_namen`): een
wijziging gaat terug naar het bestand met de oorspronkelijke naam, zonder dubbele uploads. Bestaat de veilige naam hier
al, dan wordt het serverbestand overgeslagen. Op Linux en macOS kan een bestand dat al op de server staat gewoon zijn
naam houden.

**Negeerlijst:** bestanden die niet gesynchroniseerd worden (niet geüpload, niet gedownload, niet verwijderd). Altijd:

    ~$*
    *.tmp
    .DS_Store
    Thumbs.db
    desktop.ini

Eigen patronen komen in `.openaecignore` in de hoofdmap van de gekoppelde map (dat bestand zelf wordt wel
gesynchroniseerd). Eén patroon per regel, `#` voor een opmerking. Zonder `/` geldt een patroon voor elke naam in het
pad (bestand of map); met `/` voor het hele pad vanaf de hoofdmap (een `/` vooraan mag); een `/` aan het eind alleen
voor mappen. `*` staat voor een stuk binnen een naam, `**` ook over mappen heen, `?` voor één teken; hoofdletters maken
niet uit. Voorbeeld:

    *.bak
    build/
    /Export/*.pdf

## Tools

Het tabblad **Tools** toont de OpenAEC-tools als kaarten (naam, omschrijving, afbeelding, categorie, versie) en werkt
ook zonder verbinding met een omgeving. Desktop-apps installeren, bijwerken, starten en verwijderen; webtools openen in
de browser. De logica komt uit de [OpenAEC Installer](https://github.com/OpenAEC-Foundation/openaec-installer) (zie
`NOTICE`): `src-tauri/src/tools` (releases, download, detectie) en `src/lib/catalogus.ts` (de lijst met tools).

- **Releases:** de laatste GitHub-release van elke tool, alleen uit repo's van OpenAEC-Foundation; 6 uur bewaard
  (GitHub staat zonder login 60 aanroepen per uur toe), **Vernieuwen** controleert meteen.
- **Installeren:** alleen release-downloads van OpenAEC-Foundation. Elke download moet overeenkomen met de SHA-256 die
  GitHub voor het bestand publiceert; op Windows start de setup bovendien alleen met een geldige Authenticode-
  handtekening. Daarna loopt de setup van de tool zelf (Windows) of komt de AppImage in de beheerde map (Linux).
- **Detectie:** Windows-register (Uninstall-sleutels) of de AppImage-map `~/.local/share/openaec-installer/apps`.
- **Naast de losse OpenAEC Installer:** beide lezen dezelfde registersleutels en dezelfde AppImage-map en installeren via
  de setup van de tool zelf, dus ze zien dezelfde installaties en schrijven geen eigen, botsende registerwaarden. Dit
  tabblad werkt de installer zelf niet bij.
- Tests van de pure logica: `npm test` (frontend) en `cargo test` in `src-tauri` (assetkeuze, download- en
  digestcontrole, handtekening, uninstall-opdracht).

## Installeren

De installers en het opdrachtregelprogramma komen uit GitHub Actions (workflow **Build OpenAEC Desktop App**; bij een
`v*`-tag als conceptrelease).

- **Windows:** `OpenAEC.Desktop.App_<versie>_x64-setup.exe` (of `.msi`). WebView2 wordt zo nodig meegeïnstalleerd.
- **Linux (desktop):** `.deb` voor Debian/Ubuntu (`sudo apt install ./OpenAEC.Desktop.App_<versie>_amd64.deb`) of de
  `.AppImage` (uitvoerbaar maken en starten). Het pictogram verschijnt in het systeemvak; sleutels gaan naar de
  sleutelbos van de desktop (Secret Service).
- **macOS:** `OpenAEC.Desktop.App_<versie>_universal.dmg` (Apple silicon en Intel), ondertekend en genotariseerd;
  open hem en sleep de app naar Programma's. Op macOS synchroniseert de app gekoppelde mappen, deelt en verstuurt,
  maar zonder statusvinkjes in de Finder, zonder bestanden op aanvraag en zonder hoofdmap in de navigatiekolom (die
  gebruiken de Cloud Filter API van Windows; die code wordt op macOS niet gebouwd).
- **Linux zonder scherm (servers):** het programma `openaec-storage` (de naam blijft: het is de opdrachtregel voor
  OpenAEC Storage, en scripts en `packaging/openaec-storage.service` gebruiken hem):

```bash
install -m 755 openaec-storage ~/.local/bin/
openaec-storage inloggen https://bedrijf.cloud.open-aec.eu          # via de browser
OPENAEC_SLEUTEL="sleutel:geheim" openaec-storage inloggen https://bedrijf.cloud.open-aec.eu --sleutel
openaec-storage ruimtes
openaec-storage koppel "Project A" ~/OpenAEC/Project-A
openaec-storage sync            # één ronde
openaec-storage bewaak          # blijft draaien; zie packaging/openaec-storage.service voor systemd
```

## Testen

```bash
cd crates/openaec-sync && cargo test           # eenheden + motor + API (nagebootst)
cd src-tauri && cargo test --lib               # app (unit-tests)
cd src-tauri && cargo run --example status_smoke    # Windows: statusvinkjes (Cloud Filter API)
cd src-tauri && cargo run --example vinkjes_smoke   # Windows: motor + statusvinkjes samen
cd src-tauri && cargo run --example hoofdmap_smoke  # Windows: hoofdmap, navigatiepaneel, bestanden op aanvraag
cd src-tauri && cargo run --example versturen_smoke # Windows: Versturen met OpenAEC in het rechtermuismenu
npm test                                            # hulpfuncties van de vensters
```

`versturen_smoke` registreert een testwerkwoord onder een testnaam (`OpenAECVersturenRooktest`) dat naar het
rookprogramma zelf wijst, leest het terug, laat de Verkenner (`Shell.Application`) het item noemen bij een bestand en
een map, roept het aan op drie bestanden tegelijk en op een map, controleert dat elke aanroep binnenkomt als
`--actie versturen <pad>` en ruimt alles op. Het echte werkwoord van de app en de instellingen raakt hij niet aan.
De menu's voor Linux en macOS en de overdracht-client tegen een nagebootste server (ook lege bestanden en hervatten)
zitten in de unit-tests; op Linux en macOS draaien de tests van de app in de bouwstraat.

De voorbeeldprogramma's werken op een tijdelijke map onder `%TEMP%` (met een testnaam) en ruimen hun registratie,
het item in het navigatiepaneel en de map altijd op. Met `OPENAEC_ROOKTEST_WACHT=<seconden>` blijft de registratie van
`hoofdmap_smoke` zo lang staan om in de Verkenner te kijken.

Tegen een echte omgeving: `cargo run --example sync -- <basis-url> <ruimte> <map>` met
`OPENAEC_AUTORISATIE="token <sleutel>:<geheim>"`.

**Meting** (`crates/openaec-sync/examples/meting.rs`): `meting maak <map>` zet een vaste set neer (2000 bestanden van
1–20 KB in 40 mappen en 5 bestanden van 50 MB), `meting upload|download <basis> <ruimte> <map>` meet één ronde. Het
script `scripts/meetbench/meet.sh` in openaec-storage zet een lokale bench op (`frappe/erpnext:v16.51.1`, mariadb,
valkey en moto als S3), meet een oudere en de huidige build en ruimt alles weer op.

Meetresultaten (Windows 11, Docker Desktop, gunicorn met 4 werkers van 4 draden, moto als S3; één ronde, standaard 8
overdrachten tegelijk). "Oud" is versie 0.5.0 (elk bestand na elkaar), "nieuw" deze versie:

| Set | Richting | Oud | Nieuw | Sneller |
|---|---|---|---|---|
| 2000 bestanden van 1–20 KB in 40 mappen | upload | 144,8 s | 19,6 s | 7,4× |
| 2000 bestanden van 1–20 KB in 40 mappen | download | 97,0 s | 6,9 s | 14× |
| 5 bestanden van 50 MB | upload | 5,4 s | 3,5 s | 1,5× |
| 5 bestanden van 50 MB | download | 4,2 s | 2,0 s | 2,1× |

## Bouwen / draaien

```bash
npm install
npm run tauri dev      # ontwikkelen
npm run tauri build    # Windows-installer (.msi/.exe)
```

De Windows-installer bundelt de WebView2-bootstrapper mee (`embedBootstrapper`)
+ `WebView2Loader.dll`, zodat de app op een schone Windows-installatie start.
Een GitHub Actions-workflow (`tauri-action`) bouwt de installer reproduceerbaar.

**macOS:** `npm run tauri build -- --target universal-apple-darwin` (eerst `rustup target add aarch64-apple-darwin
x86_64-apple-darwin`) maakt de `.app`, de `.dmg` en de updater-bundel (`.app.tar.gz` met `.sig`). In de bouwstraat
ondertekent tauri-action de app met het Developer ID-certificaat (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
`APPLE_SIGNING_IDENTITY`) en notariseert hij met een App Store Connect API-sleutel (`APPLE_API_ISSUER`, `APPLE_API_KEY`
en de sleutel zelf uit `APPLE_API_KEY_CONTENT`, die als `.p8` op de runner komt; `APPLE_TEAM_ID`). Een stap daarna
controleert de app uit de `.dmg`: handtekening, Gatekeeper, het notarisatieticket en beide architecturen. In
`latest.json` staat de universele bundel voor `darwin-aarch64` en `darwin-x86_64`. De instellingen voor macOS staan in
`src-tauri/tauri.macos.conf.json`.

`npm run tauri build` maakt ook de updatebestanden (`.sig`) en heeft daarvoor de geheime updater-sleutel nodig:
zet `TAURI_SIGNING_PRIVATE_KEY` op de sleutel of het pad ernaar (en `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` als hij een
wachtwoord heeft). De sleutel staat nooit in de repo; in de bouwstraat komt hij uit de GitHub-secrets met dezelfde naam.

## De Verkenner (Windows)

### De hoofdmap en het navigatiepaneel

Bij de eerste start stelt de app een **hoofdmap** voor: `%USERPROFILE%\OpenAEC` (Overzicht → **Instellen**, of
**Andere map…**). Elke ruimte die je daarin synchroniseert (Ruimtes → **In de hoofdmap synchroniseren**) krijgt een
eigen submap met de naam van de ruimte (tekens die in een mapnaam niet kunnen worden `_`; bestaat de naam al, dan
`Naam (2)`). De hoofdmap staat in `instellingen.json` als `hoofdmap`.

Een map die met mij gedeeld is (Ruimtes → Gedeeld met mij → **In de hoofdmap synchroniseren**) komt onder
`<hoofdmap>\Gedeeld met mij\<naam>`, met dezelfde statusiconen en bestanden op aanvraag. Is de map alleen te lezen
gedeeld, dan gaat er ook vanuit de hoofdmap niets naar de server.

De hoofdmap is één sync-root bij Windows en staat daardoor als eigen item **OpenAEC**, met het pictogram van de app,
in het navigatiepaneel van de Verkenner. Wat Windows daarvoor nodig heeft (`cloudfilter::registreer_hoofdmap`):

- registratie via de shell (`StorageProviderSyncRootManager::Register`, niet alleen `CfRegisterSyncRoot`) met een id
  `OpenAECStorage!<SID van de gebruiker>!<sleutel van de map>`, een `DisplayNameResource` ("OpenAEC") en een
  `IconResource` (`<pad naar de exe>,0`);
- `HydrationPolicy` **Full** met **AutoDehydrationAllowed** (bestanden mogen alleen online staan, ook opslagbeheer van
  Windows mag ze vrijmaken), `PopulationPolicy` **AlwaysFull** (de app zet zelf alle placeholders neer),
  `InSyncPolicy` **TrackAll** en `AllowPinning`;
- twee eigen kolommen (`StorageProviderItemPropertyDefinitions`): "Status van OpenAEC" en "Vergrendeld door".

Windows maakt dan zelf het naamruimte-item aan: een CLSID onder `HKCU\Software\Classes\CLSID\{…}` met
`System.IsPinnedToNamespaceTree = 1` en `TargetFolderPath` = de hoofdmap, een sleutel onder
`HKCU\…\Explorer\Desktop\NameSpace`, en `NamespaceCLSID` in de registratie onder
`HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager\<id>`. Bij afmelden ruimt Windows dat weer
op. De app registreert en verbindt bij elke start opnieuw (herhaalbaar).

**Losse gekoppelde mappen** (Ruimtes → **Koppelen aan map…**, en alles van vóór de hoofdmap) blijven werken zoals ze
werkten: elke map is zijn eigen sync-root met volledige bestanden. In het Overzicht staat bij zo'n map **Naar de
hoofdmap…**: na bevestiging gaan de statusvinkjes eraf, verhuist de map met zijn toestand (`.openaec`) naar een submap
van de hoofdmap (dus zonder opnieuw te downloaden) en krijgt hij daar zijn status terug. Dat kan alleen binnen hetzelfde
station; de app doet het nooit vanzelf.

Een ruimte in de hoofdmap ontkoppelen haalt de bestanden die alleen online stonden weg (ze staan op de server); wat
lokaal staat, blijft staan.

Een ruimte waarvan de map ontbreekt (verwijderd, of een station dat niet is aangesloten) wordt niet gesynchroniseerd:
de ronde geeft "de map ontbreekt op deze computer" in plaats van alles als lokaal verwijderd te zien.

### Statusiconen

Elk bestand en elke map krijgt de standaardstatus van de Cloud Files API (de app zet de in-sync-status met
`CfSetInSyncState`; het wolkje en de vinkjes tekent Windows uit de placeholder- en pin-status). De afleiding staat los
van Windows in `src-tauri/src/itemstatus.rs` (met unit-tests):

| Icoon | Betekenis |
|---|---|
| wolkje | **alleen online**: staat op de server en wordt opgehaald zodra je het opent (alleen in de hoofdmap) |
| groen vinkje (open) | **lokaal beschikbaar**: op deze computer, "Ruimte vrijmaken" kan |
| groen vinkje (dicht) | **altijd op dit apparaat** (vastgezet); in een losse gekoppelde map is dat altijd zo |
| sync-pijlen | **bezig**: nieuw of gewijzigd en nog niet op de server, of nu onderweg (ook tijdens de parallelle sync: een bestand krijgt de pijlen als het begint en zijn vinkje zodra het klaar is) |
| sync-pijlen en een kolom **Status van OpenAEC** | **fout** ("Fout: …") of **overgeslagen** ("Overgeslagen: …", bijv. een naam die niet kan) |
| kolom **Vergrendeld door** met een slotje | vergrendeld (Opslagvergrendeling): wie, en tot wanneer |

Mappen tonen het geheel: sync-pijlen als er iets in bezig is of fout gaat, anders een vinkje (een overgeslagen bestand
maakt de map niet "bezig"). De eigen kolommen zijn Verkenner-kolommen (rechtsklik op de kolomkoppen → Meer…) en
staan ook in Eigenschappen; ze worden na elke ronde bijgewerkt en in `.openaec/verkenner.json` onthouden.
Vergrendelingen komen uit de wijzigingenlijst van de server ("vergrendeld"/"ontgrendeld"; bij het starten de hele
lijst), met `api.info` voor wie; elke 10 minuten kijkt de app na of ze nog gelden (een slot verloopt ook zonder
melding).

### Bestanden op aanvraag (hoofdmap)

- **Nieuwe bestanden van de server** komen als placeholder die alleen online staat (wolkje): de grootte en de naam zijn
  er, de inhoud niet. Onder een map met "Altijd op dit apparaat behouden" worden ze meteen helemaal gedownload.
- **Openen** van een bestand dat alleen online staat, haalt het op (FETCH_DATA-callback): de app vraagt een downloadlink
  voor precies de versie uit de toestand (`api.download` met `versie`) en geeft de inhoud in stukken van 1 MB door aan
  Windows, dat daarmee de voortgang toont. Draait de app niet, dan kan Windows het bestand niet openen.
- **Rechtermuismenu**: "Altijd op dit apparaat behouden" en "Ruimte vrijmaken" zijn de ingebouwde opties van Windows;
  ze zetten alleen de pin-status. De app volgt die via de bestandswachter: vastgezet en alleen online → ophalen
  (`CfHydratePlaceholder`), vrijgemaakt en lokaal → `CfDehydratePlaceholder`. Een bestand dat niet in sync is (lokaal
  werk dat nog niet op de server staat) weigert Windows vrij te maken.
- **Een nieuwe versie op de server** van een bestand dat alleen online staat, past alleen de placeholder aan (grootte,
  tijd; `CfUpdatePlaceholder`), er wordt niets gedownload. Een lokaal bestand wordt gewoon bijgewerkt.
- Vrijmaken en ophalen veranderen grootte en wijzigingstijd niet: de motor ziet geen wijziging en uploadt niets.

In een losse gekoppelde map staan bestanden altijd volledig op deze computer (`HydrationPolicy` AlwaysFull); daar
kan "Ruimte vrijmaken" niet.

### Selectieve synchronisatie

Per gekoppelde map (ook een ruimte in de hoofdmap) opent Overzicht → **Mappen kiezen…** een boom van de mappen op de
server, met per map een vinkje en de grootte en het aantal bestanden eronder (uit `api.boom` als de server die kent,
anders map voor map met `api.bladeren`). Een map uitvinken neemt zijn submappen mee.

- Een uitgevinkte map wordt niet lokaal opgeslagen of gesynchroniseerd. Staan er al bestanden van op deze computer,
  dan vraagt de app eerst bevestiging ("N bestanden (X MB) worden hier verwijderd; op de server blijven ze staan");
  wat lokaal gewijzigd is, gaat eerst nog naar de server.
- **Nieuwe mappen op de server**: **Automatisch meenemen** (standaard) of **Alleen wat ik hier gekozen heb**. Bij het
  tweede onthoudt de app welke mappen er bij het kiezen waren; een map die daarna op de server verschijnt (ook een
  submap van een gekozen map) blijft weg tot je hem aanvinkt. Een map die hier al bestaat, telt nooit als nieuw.
- De keuze staat per koppeling in `instellingen.json` (`uitgesloten`, `nieuwe_mappen`, `bekende_mappen`). De regels
  staan in één module zonder IO met unit-tests (`crates/openaec-sync/src/selectie.rs`); de motor slaat uitgesloten
  mappen daarmee over bij uploaden en downloaden.

## Automatische updates

De app kijkt kort na het starten en daarna elke 6 uur of er een nieuwere versie is (`latest.json` bij de laatste
GitHub-release). Een update wordt op de achtergrond gedownload en gecontroleerd tegen de publieke sleutel in
`tauri.conf.json`; dan verschijnt een melding en in het systeemvakmenu **Update installeren (vX.Y.Z)**. Pas na die klik
wordt hij geïnstalleerd, nooit midden in een sync-ronde. Op Windows sluit de installer de app en start hem daarna
opnieuw. Een conceptrelease telt pas mee als hij gepubliceerd is.

## Overstap van OpenAEC Storage (0.4 en ouder)

De nieuwe naam komt met de gewone automatische update binnen. Wat er gebeurt:

- **Windows (NSIS):** de installer leidt map, uninstall-sleutel en snelkoppelingen af van de productnaam. De hooks in
  `src-tauri/nsis/hooks.nsh` zorgen dat een bestaande installatie onder de oude naam wordt bijgewerkt in de map waar
  hij staat (`%LOCALAPPDATA%\OpenAEC Storage`): het pad van `openaec-cloud.exe` blijft gelijk, dus het
  rechtermuismenu, de pictogrammen van gekoppelde mappen, vastgemaakte snelkoppelingen en de autostart blijven werken.
  Daarna verdwijnt de oude regel uit "Apps en onderdelen" (alleen de nieuwe blijft) en krijgen de snelkoppelingen in
  het Startmenu en op het bureaublad de nieuwe naam. Een nieuwe installatie komt in
  `%LOCALAPPDATA%\OpenAEC Desktop App`.
- **Blijft gelijk:** de `identifier` (`com.openaec.cloud`, dus ook de WebView-gegevens en de updater-sleutel), de
  instellingenmap en de sleutelbos-dienst (`OpenAEC Storage`), de naam van de autostart-regel (`OpenAEC Storage`, zie
  `AUTOSTART_NAAM` in `lib.rs`), de registersleutel van het rechtermuismenu en de sync-root-registraties.
- **MSI:** de upgrade-code staat vast in `tauri.conf.json` (de waarde die bij de oude naam hoorde), zodat een nieuwe
  `.msi` de oude vervangt in plaats van ernaast te komen.
- **Linux (.deb):** het pakket heet nu `open-aec-desktop-app`; het vervangt `open-aec-storage`
  (`Conflicts`/`Replaces`/`Provides` in `tauri.linux.conf.json`). De AppImage wordt zoals altijd ter plekke vervangen.

Handmatig testen (op een testmachine of VM, niet op een werkplek met een echte installatie):

1. Installeer `OpenAEC.Storage_0.4.0_x64-setup.exe`, koppel een map, zet "Starten bij inloggen" aan en maak de app vast
   aan de taakbalk.
2. Laat de app updaten naar de nieuwe versie (of start de nieuwe setup met `/P /UPDATE`).
3. Controleer: één regel "OpenAEC Desktop App" in Apps en onderdelen, geen regel "OpenAEC Storage"; de map
   `%LOCALAPPDATA%\OpenAEC Storage` bevat de nieuwe exe; geen map `OpenAEC Desktop App`; de Startmenu-
   snelkoppeling heet nu "OpenAEC Desktop App"; de autostart staat nog aan (Instellingen); het
   rechtermuismenu en de statusvinkjes werken.
4. Verwijder de app via Apps en onderdelen: de map, de autostart-regel en het rechtermuismenu zijn weg.

## Nog te doen

- Bij het verwijderen van de app de registratie van gekoppelde mappen opruimen (nu: eerst ontkoppelen).
- Statusvinkjes in de bestandsbeheerder op Linux en macOS (op macOS ook bestanden op aanvraag: File Provider).
- "Versturen met OpenAEC" in het nieuwe, korte rechtermuismenu van Windows 11 (`IExplorerCommand` in een ondertekend
  pakket met identiteit; nu onder "Meer opties weergeven").
- De macOS-build en de menu's voor Linux en macOS zijn nog niet op een echte machine nagelopen (alleen de bouwstraat en
  de unit-tests).
- Bestanden die alleen online staan, zijn niet te openen zolang de app niet draait.
- Gedeelde mappen in het opdrachtregelprogramma `openaec-storage` (nu alleen ruimtes).
- Een gedeeld bestand openen of een nieuwe versie uploaden vanuit de app zelf (nu via de browser).
