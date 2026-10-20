# OpenAEC Cloud — desktop sync-client

[![Build OpenAEC Cloud (Windows)](https://github.com/OpenAEC-Foundation/openaec-cloud-desktop/actions/workflows/build.yml/badge.svg)](https://github.com/OpenAEC-Foundation/openaec-cloud-desktop/actions/workflows/build.yml)

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
  koppelen aan mappen, elke 30 s een sync-ronde, vergrendelen, deellinks, en op Windows cloud-mappen
  via de **Cloud Filter API** (placeholders die pas bij openen worden opgehaald).
- **`src`** — de UI (SolidJS).

## Testen

```bash
cd crates/openaec-sync && cargo test           # eenheden + motor + API (nagebootst)
```

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

## Nog te doen

- Inloggen met het centrale OpenAEC-account (OIDC met PKCE) in plaats van een API-sleutel.
- Live bewaking van de map (direct syncen bij een wijziging) naast de ronde van 30 s.
- Status-vinkjes per bestand in Verkenner en vergrendelen/delen vanuit het contextmenu.
- Tray, autostart en automatische updates.
