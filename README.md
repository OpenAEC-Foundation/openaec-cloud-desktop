# OpenAEC Storage — desktop-app en opdrachtregel

[![Build OpenAEC Storage](https://github.com/OpenAEC-Foundation/openaec-cloud-desktop/actions/workflows/build.yml/badge.svg)](https://github.com/OpenAEC-Foundation/openaec-cloud-desktop/actions/workflows/build.yml)

Desktop-app voor **OpenAEC Storage**: koppel lokale mappen aan ruimtes in je OpenAEC-omgeving, werk samen
zonder elkaars werk te overschrijven, vergrendel bestanden en deel ze met een link. Gebouwd met
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
- **`src-tauri`** — de app: verbinden (sleutel in de sleutelbos van het besturingssysteem), ruimtes
  koppelen aan mappen, synchroniseren zodra er lokaal iets verandert (bestandswachter) of de wijzigingenlijst
  van de server iets nieuws meldt (controle elke 30 s, eens per 10 minuten een volledige ronde), vergrendelen, deellinks, automatische
  updates, en op Windows via de **Cloud Filter API** statusvinkjes in gekoppelde mappen en cloud-mappen (placeholders die pas bij
  openen worden opgehaald).
- **`src`** — de UI (SolidJS).

## Installeren

De installers en het opdrachtregelprogramma komen uit GitHub Actions (workflow **Build OpenAEC Storage**; bij een
`v*`-tag als conceptrelease).

- **Windows:** `OpenAEC Storage_<versie>_x64-setup.exe` (of `.msi`). WebView2 wordt zo nodig meegeïnstalleerd.
- **Linux (desktop):** `.deb` voor Debian/Ubuntu (`sudo apt install ./openaec-storage_<versie>_amd64.deb`) of de
  `.AppImage` (uitvoerbaar maken en starten). Het pictogram verschijnt in het systeemvak; sleutels gaan naar de
  sleutelbos van de desktop (Secret Service).
- **Linux zonder scherm (servers):** het programma `openaec-storage`:

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

## Nog te doen

- Bij het verwijderen van de app de registratie van gekoppelde mappen opruimen (nu: eerst ontkoppelen).
- Statusvinkjes in de bestandsbeheerder op Linux.
