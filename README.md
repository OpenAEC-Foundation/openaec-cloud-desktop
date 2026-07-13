# OpenAEC Cloud — desktop sync-client

[![Build OpenAEC Cloud (Windows)](https://github.com/OpenAEC-Foundation/openaec-cloud-desktop/actions/workflows/build.yml/badge.svg)](https://github.com/OpenAEC-Foundation/openaec-cloud-desktop/actions/workflows/build.yml)

Dropbox-achtige desktop-app voor de OpenAEC SuperCloud: synchroniseer lokale
mappen met je eigen Nextcloud (`cloud.openaec.lan`), deel hele mappen en stuur
bestanden naar mensen — via je eigen server. Gebouwd met **Tauri 2 + Rust +
SolidJS**, met dezelfde UI-stijl als Open Speech Studio.

## Architectuur

- **App-shell** — Tauri 2 + SolidJS (OpenAEC-ribbon-look, `src/`).
- **Sync-backend (Rust, `src-tauri/src/webdav.rs`)** — user-scoped WebDAV +
  OCS-Share-client (`reqwest` + `quick-xml`). Praat als de ingelogde gebruiker
  (app-wachtwoord/bearer), niet als service-account (i.t.t. de server-side
  `openaec-cloud`-lib, die group-folders + volume-mounts doet).
- **Sync-engine (`src-tauri/src/sync.rs`)** — lokale map ↔ Nextcloud-map:
  bestandslijst-diff (mtime/grootte/etag) → up/download. Fase 1 = handmatige/
  interval-sync; Fase 2 = live filewatcher (`notify`).
- **Delen** — Nextcloud OCS Share-API → deellinks + hele mappen delen.

## Fasering

1. **MVP (nu)** — inloggen op `cloud.openaec.lan`, map koppelen, tweerichtings-
   sync, deellink maken. Werkt als Dropbox qua sync + delen.
2. **Verkenner-vinkjes** — Windows **Cloud Filter API** (`cldapi`, via de
   `windows`-crate): sync-root + files-on-demand + status-overlays (vinkje/cloud/
   sync) in Verkenner, net als OneDrive. Het lastigste stuk, dus apart.
3. **Polish** — tray, autostart, conflict-scherm, meerdere mappen, auto-update.

## Bouwen / draaien

```bash
npm install
npm run tauri dev      # ontwikkelen
npm run tauri build    # Windows-installer (.msi/.exe)
```

De Windows-installer bundelt de WebView2-bootstrapper mee (`embedBootstrapper`)
+ `WebView2Loader.dll`, zodat de app op een schone Windows-installatie start.
Een GitHub Actions-workflow (`tauri-action`) bouwt de installer reproduceerbaar.

## Status

Fase 1 in opbouw — zie `src-tauri/src/` (backend) en `src/` (UI).
