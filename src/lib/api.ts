import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"

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

// Bindings naar de Rust-commands (src-tauri/src/lib.rs).
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
  versies: (bestand: string) => invoke<Versie[]>("versies", { bestand }),
  openVersie: (bestand: string, versie: number) => invoke<void>("open_versie", { bestand, versie }),
  opActie: (f: (a: { actie: string; pad: string }) => void) =>
    listen<{ actie: string; pad: string }>("actie", (e) => f(e.payload)),
  maakCloudmap: (ruimte: string, lokaal: string) => invoke<number>("maak_cloudmap", { ruimte, lokaal }),
  autostart: () => invoke<boolean>("autostart"),
  zetAutostart: (aan: boolean) => invoke<boolean>("zet_autostart", { aan }),
  opSync: (f: (r: MapRapport[]) => void) => listen<MapRapport[]>("sync", (e) => f(e.payload)),
}
