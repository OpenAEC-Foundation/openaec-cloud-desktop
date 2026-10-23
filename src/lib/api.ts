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
  /** Niet gesynchroniseerd, met de reden (bijv. een gereserveerde naam als `nul`); geen fout. */
  overgeslagen: Overgeslagen[]
  /** Wat de sync zelf deed met een lastige naam (opgeruimd, onder een veilige naam). */
  meldingen: string[]
}

export interface Overgeslagen {
  pad: string
  reden: string
  /** Een lokaal bestand met een lastige naam: kan met "Opruimen" veilig weg. */
  opruimbaar: boolean
}

export interface KoppelOvergeslagen {
  lokaal: string
  ruimte_naam: string
  bestanden: Overgeslagen[]
}

export interface Opgeruimd {
  verwijderd: number
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

/** Wat de sync doet met een bestand met een gereserveerde of ongeldige naam. */
export type Gereserveerd = "overslaan" | "verwijderen" | "veilige_naam"

/** Instellingen van de sync, voor alle gekoppelde mappen. */
export interface SyncInstellingen {
  /** Overdrachten tegelijk (1–32). */
  gelijktijdig: number
  gereserveerd: Gereserveerd
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

/** Wat er gedeeld wordt: een bestand of map op de server. */
export interface Deeldoel {
  doctype: "Opslagbestand" | "Opslagmap"
  id: string
  naam: string
  /** Lokaal pad, of de plek in de ruimte ("Ruimte / Map / bestand.pdf"). */
  pad: string
  ruimte: string
  ruimte_naam: string
}

/** Opdracht aan het deelvenster: een lokaal pad (nog op te zoeken) of een bekend doel. */
export interface Deelverzoek {
  pad?: string | null
  doel?: Deeldoel | null
}

export interface DelingRegel {
  deling: string
  soort: "link" | "gebruiker"
  gebruiker: string | null
  rechten: "lezen" | "schrijven" | "uploaden"
  verloopt: string | null
  gemaakt: string | null
  url: string | null
}

export interface Ruimtelid {
  gebruiker: string
  rol: string
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
  syncInstellingen: () => invoke<SyncInstellingen>("sync_instellingen"),
  zetSyncInstellingen: (sync: SyncInstellingen) => invoke<SyncInstellingen>("zet_sync_instellingen", { sync }),
  overgeslagen: () => invoke<KoppelOvergeslagen[]>("overgeslagen"),
  ruimOvergeslagenOp: (lokaal: string | null) => invoke<Opgeruimd>("ruim_overgeslagen_op", { lokaal }),
  opOvergeslagen: (f: (l: KoppelOvergeslagen[]) => void) => listen<KoppelOvergeslagen[]>("overgeslagen", (e) => f(e.payload)),
  // Deelvenster (src-tauri/src/delen.rs); fouten komen als { code, melding } (zie leesFout in lib/delen.ts).
  openDelen: (verzoek: Deelverzoek) => invoke<void>("open_delen", { verzoek }),
  sluitDelen: () => invoke<void>("sluit_delen"),
  delenVerzoek: () => invoke<Deelverzoek | null>("delen_verzoek"),
  opDelenVerzoek: (f: (v: Deelverzoek) => void) => listen<Deelverzoek>("delen-verzoek", (e) => f(e.payload)),
  deeldoel: (pad: string) => invoke<Deeldoel>("deeldoel", { pad }),
  deelMetCollega: (doel: Deeldoel, gebruiker: string, rechten: string, verloopt: string) =>
    invoke<void>("deel_met_collega", { doctype: doel.doctype, id: doel.id, gebruiker, rechten, verloopt }),
  maakLink: (doel: Deeldoel, rechten: string, wachtwoord: string, verloopt: string) =>
    invoke<string>("maak_link", { doctype: doel.doctype, id: doel.id, rechten, wachtwoord, verloopt }),
  delingenVan: (doel: Deeldoel) => invoke<DelingRegel[]>("delingen_van", { doctype: doel.doctype, id: doel.id }),
  trekDelingIn: (deling: string) => invoke<void>("trek_deling_in", { deling }),
  ruimteleden: (ruimte: string) => invoke<Ruimtelid[]>("ruimteleden", { ruimte }),
  kopieer: (tekst: string) => invoke<void>("kopieer", { tekst }),
  toolsGeinstalleerd: (queries: ToolVraag[]) => invoke<GeinstalleerdeTool[]>("tools_geinstalleerd", { queries }),
  toolsReleases: (repos: { id: string; owner: string; repo: string }[]) => invoke<ReleaseInfo[]>("tools_releases", { repos }),
  toolInstalleren: (id: string, url: string, bestandsnaam: string, digest: string | null, naam: string) =>
    invoke<InstallUitkomst>("tool_installeren", { id, url, bestandsnaam, digest, naam }),
  toolStarten: (exePad: string) => invoke<void>("tool_starten", { exePad }),
  toolVerwijderen: (query: ToolVraag) => invoke<"ranUninstaller" | "removed">("tool_verwijderen", { query }),
  opToolDownload: (f: (v: DownloadVoortgang) => void) => listen<DownloadVoortgang>("tool-download", (e) => f(e.payload)),
}
