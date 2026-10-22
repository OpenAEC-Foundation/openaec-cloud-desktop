# OpenAEC SuperCloud Desktop — desktop-app en opdrachtregel

[![Build OpenAEC SuperCloud Desktop](https://github.com/OpenAEC-Foundation/openaec-cloud-desktop/actions/workflows/build.yml/badge.svg)](https://github.com/OpenAEC-Foundation/openaec-cloud-desktop/actions/workflows/build.yml)

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
    De server beslist over conflicten (basisversie); bij een conflict blijven beide versies bestaan.
  - `opslag` — de motor over de echte API (mappen worden onderweg aangemaakt).
  - `voortgang` — meldingen van de motor per bestand (start, bytevoortgang, klaar, fout) voor het statusvenster,
    hoogstens vier keer per seconde per bestand; de app kan de sync hiermee ook pauzeren.
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

De installers en het opdrachtregelprogramma komen uit GitHub Actions (workflow **Build OpenAEC SuperCloud Desktop**; bij een
`v*`-tag als conceptrelease).

- **Windows:** `OpenAEC.SuperCloud.Desktop_<versie>_x64-setup.exe` (of `.msi`). WebView2 wordt zo nodig meegeïnstalleerd.
- **Linux (desktop):** `.deb` voor Debian/Ubuntu (`sudo apt install ./OpenAEC.SuperCloud.Desktop_<versie>_amd64.deb`) of de
  `.AppImage` (uitvoerbaar maken en starten). Het pictogram verschijnt in het systeemvak; sleutels gaan naar de
  sleutelbos van de desktop (Secret Service).
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
```

De twee voorbeeldprogramma's werken op een tijdelijke map onder `%TEMP%` en ruimen hun registratie en map altijd op.

Tegen een echte omgeving: `cargo run --example sync -- <basis-url> <ruimte> <map>` met
`OPENAEC_AUTORISATIE="token <sleutel>:<geheim>"`.

## Bouwen / draaien

```bash
npm install
npm run tauri dev      # ontwikkelen
npm run tauri build    # Windows-installer (.msi/.exe)
```

De Windows-installer bundelt de WebView2-bootstrapper mee (`embedBootstrapper`)
+ `WebView2Loader.dll`, zodat de app op een schone Windows-installatie start.
Een GitHub Actions-workflow (`tauri-action`) bouwt de installer reproduceerbaar.

`npm run tauri build` maakt ook de updatebestanden (`.sig`) en heeft daarvoor de geheime updater-sleutel nodig:
zet `TAURI_SIGNING_PRIVATE_KEY` op de sleutel of het pad ernaar (en `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` als hij een
wachtwoord heeft). De sleutel staat nooit in de repo; in de bouwstraat komt hij uit de GitHub-secrets met dezelfde naam.

## Statusvinkjes in de Verkenner (Windows)

Een gekoppelde map wordt bij het koppelen (en bij elke start) een sync-root voor de Verkenner. Per bestand en map:

- **groen vinkje** — gesynchroniseerd en altijd op deze computer (de bestanden blijven gewone, volledige bestanden;
  "ruimte vrijmaken" kan niet);
- **sync-pijlen** — gewijzigd of nieuw en nog niet geüpload, of er ging iets mis (dan ook de mappen erboven).

De toestandsmap `.openaec` staat als "uitgesloten". Bij ontkoppelen gaan de vinkjes eraf en wordt de map afgemeld; de
bestanden blijven staan. Valt een map al onder een andere sync-provider of staat hij niet op NTFS, dan zijn er geen
vinkjes, maar synchroniseren werkt gewoon.

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
  `%LOCALAPPDATA%\OpenAEC SuperCloud Desktop`.
- **Blijft gelijk:** de `identifier` (`com.openaec.cloud`, dus ook de WebView-gegevens en de updater-sleutel), de
  instellingenmap en de sleutelbos-dienst (`OpenAEC Storage`), de naam van de autostart-regel (`OpenAEC Storage`, zie
  `AUTOSTART_NAAM` in `lib.rs`), de registersleutel van het rechtermuismenu en de sync-root-registraties.
- **MSI:** de upgrade-code staat vast in `tauri.conf.json` (de waarde die bij de oude naam hoorde), zodat een nieuwe
  `.msi` de oude vervangt in plaats van ernaast te komen.
- **Linux (.deb):** het pakket heet nu `open-aec-super-cloud-desktop`; het vervangt `open-aec-storage`
  (`Conflicts`/`Replaces`/`Provides` in `tauri.linux.conf.json`). De AppImage wordt zoals altijd ter plekke vervangen.

Handmatig testen (op een testmachine of VM, niet op een werkplek met een echte installatie):

1. Installeer `OpenAEC.Storage_0.4.0_x64-setup.exe`, koppel een map, zet "Starten bij inloggen" aan en maak de app vast
   aan de taakbalk.
2. Laat de app updaten naar de nieuwe versie (of start de nieuwe setup met `/P /UPDATE`).
3. Controleer: één regel "OpenAEC SuperCloud Desktop" in Apps en onderdelen, geen regel "OpenAEC Storage"; de map
   `%LOCALAPPDATA%\OpenAEC Storage` bevat de nieuwe exe; geen map `OpenAEC SuperCloud Desktop`; de Startmenu-
   snelkoppeling heet nu "OpenAEC SuperCloud Desktop"; de autostart staat nog aan (Instellingen); het
   rechtermuismenu en de statusvinkjes werken.
4. Verwijder de app via Apps en onderdelen: de map, de autostart-regel en het rechtermuismenu zijn weg.

## Nog te doen

- Bij het verwijderen van de app de registratie van gekoppelde mappen opruimen (nu: eerst ontkoppelen).
- Statusvinkjes in de bestandsbeheerder op Linux.
