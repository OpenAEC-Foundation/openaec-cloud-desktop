import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import type { GeinstalleerdeTool, ReleaseInfo } from "./tools.ts"

export interface Ruimte {
  name: string
  naam: string
  soort: string | null
  rol: string
}

export interface Koppeling {
  ruimte: string
  ruimte_naam: string
  lokaal: string
  uitgesloten: string[]
}

export interface Status {
  basis: string | null
  verbonden: boolean
  koppelingen: Koppeling[]
}

export interface Rapport {
  geupload: number
  gedownload: number
  verwijderd: number
  conflicten: number
  fouten: string[]
}

export interface Versie {
  volgnummer: number
  grootte: number
  maker: string | null
  creation: string | null
}

export interface MapRapport {
  lokaal: string
  ruimte_naam: string
  rapport: Rapport | null
  fout: string | null
}

export interface MapRegel {
  id: string
  naam: string
  gewijzigd: string | null
}

export interface BestandRegel {
  id: string
  naam: string
  grootte: number
  versie: number
  gewijzigd: string | null
  vergrendeld_door: string | null
  vergrendeld_tot: string | null
}

export interface MapInhoud {
  mappen: MapRegel[]
  bestanden: BestandRegel[]
}

export interface KoppelStatus {
  lokaal: string
  aantal: number
  bestaat: boolean
}

export interface Overzicht {
  bezig: boolean
  koppelingen: KoppelStatus[]
}

export type Toestand = "bijgewerkt" | "bezig" | "gepauzeerd" | "fout"

export interface Overdracht {
  lokaal: string
  pad: string
  richting: "upload" | "download"
  /** Grootte van het bestand; `totaal` is wat er echt over de lijn gaat. */
  grootte: number
  gedaan: number
  totaal: number
  snelheid: number
}

export interface Gebeurd {
  lokaal: string
  pad: string
  actie: "geupload" | "gedownload" | "verwijderd" | "conflict" | "fout"
  tijd: number
  fout: string | null
  /** Bij een upload of download: grootte van het bestand en wat er echt verstuurd is. */
  grootte: number | null
  verstuurd: number | null
}

/** Voortgang van de lopende ronde, over alle gekoppelde mappen samen. */
export interface RondeStand {
  nog: number
  bytes_gedaan: number
  bytes_totaal: number
  /** Bytes per seconde, voortschrijdend gemiddelde. */
  snelheid: number
  /** Geschatte resterende tijd in seconden. */
  resterend: number | null
  /** Bytes die door blokhergebruik niet over de lijn hoefden. */
  bespaard: number
}

export interface Quotum {
  gebruikt_bytes: number
  limiet_bytes: number | null
}

export interface Stand {
  toestand: Toestand
  samenvatting: string
  gepauzeerd: boolean
  uploaden: number
  downloaden: number
  actief: Overdracht[]
  recent: Gebeurd[]
  ronde: RondeStand | null
}

/** Uitkomst van installeren: setup gestart (Windows) of direct geïnstalleerd (Linux). */
export interface InstallUitkomst {
  path: string
  mode: "ranInstaller" | "installed"
}

export interface DownloadVoortgang {
  id: string
  downloaded: number
  total: number | null
  done: boolean
}

export interface ToolVraag {
  id: string
  displayName: string
  exeName: string | null
}

// Bindings naar de Rust-commands (src-tauri/src/lib.rs en src-tauri/src/tools).
export const api = {
  status: () => invoke<Status>("status"),
  verbind: (basis: string, autorisatie: string) => invoke<Ruimte[]>("verbind", { basis, autorisatie }),
  logIn: (basis: string) => invoke<Ruimte[]>("log_in", { basis }),
  verbreek: () => invoke<void>("verbreek"),
  ruimtes: () => invoke<Ruimte[]>("ruimtes"),
  koppel: (ruimte: string, ruimteNaam: string, lokaal: string) =>
    invoke<Koppeling[]>("koppel", { ruimte, ruimteNaam, lokaal }),
  ontkoppel: (lokaal: string) => invoke<Koppeling[]>("ontkoppel", { lokaal }),
  syncNu: () => invoke<MapRapport[]>("sync_nu"),
  vergrendel: (bestand: string) => invoke<void>("vergrendel", { bestand }),
  ontgrendel: (bestand: string) => invoke<void>("ontgrendel", { bestand }),
  deel: (bestand: string, wachtwoord: string, verloopt: string) => invoke<string>("deel", { bestand, wachtwoord, verloopt }),
  mappenVan: (ruimte: string) => invoke<string[]>("mappen_van", { ruimte }),
  zetSelectie: (lokaal: string, uitgesloten: string[]) => invoke<Koppeling[]>("zet_selectie", { lokaal, uitgesloten }),
  versies: (bestand: string) => invoke<Versie[]>("versies", { bestand }),
  openVersie: (bestand: string, versie: number) => invoke<void>("open_versie", { bestand, versie }),
  opActie: (f: (a: { actie: string; pad: string }) => void) =>
    listen<{ actie: string; pad: string }>("actie", (e) => f(e.payload)),
  maakCloudmap: (ruimte: string, lokaal: string) => invoke<number>("maak_cloudmap", { ruimte, lokaal }),
  autostart: () => invoke<boolean>("autostart"),
  zetAutostart: (aan: boolean) => invoke<boolean>("zet_autostart", { aan }),
  opSync: (f: (r: MapRapport[]) => void) => listen<MapRapport[]>("sync", (e) => f(e.payload)),
  bladeren: (ruimte: string, map: string | null) => invoke<MapInhoud>("bladeren", { ruimte, map }),
  overzicht: () => invoke<Overzicht>("overzicht"),
  openMap: (pad: string) => invoke<void>("open_map", { pad }),
  openBeheer: () => invoke<void>("open_beheer"),
  versie: () => invoke<string>("versie"),
  controleerUpdates: () => invoke<string | null>("controleer_updates"),
  installeerUpdate: () => invoke<void>("installeer_update"),
  activiteit: () => invoke<Stand>("activiteit_stand"),
  pauzeer: (aan: boolean) => invoke<void>("pauzeer", { aan }),
  toonHoofdvenster: () => invoke<void>("toon_hoofdvenster"),
  opActiviteit: (f: (s: Stand) => void) => listen<Stand>("activiteit", (e) => f(e.payload)),
  opVensterOpen: (f: () => void) => listen("venster-open", () => f()),
  quotum: () => invoke<Quotum>("quotum"),
  opQuotum: (f: (q: Quotum) => void) => listen<Quotum>("quotum", (e) => f(e.payload)),
  toolsGeinstalleerd: (queries: ToolVraag[]) => invoke<GeinstalleerdeTool[]>("tools_geinstalleerd", { queries }),
  toolsReleases: (repos: { id: string; owner: string; repo: string }[]) => invoke<ReleaseInfo[]>("tools_releases", { repos }),
  toolInstalleren: (id: string, url: string, bestandsnaam: string, digest: string | null, naam: string) =>
    invoke<InstallUitkomst>("tool_installeren", { id, url, bestandsnaam, digest, naam }),
  toolStarten: (exePad: string) => invoke<void>("tool_starten", { exePad }),
  toolVerwijderen: (query: ToolVraag) => invoke<"ranUninstaller" | "removed">("tool_verwijderen", { query }),
  opToolDownload: (f: (v: DownloadVoortgang) => void) => listen<DownloadVoortgang>("tool-download", (e) => f(e.payload)),
}
