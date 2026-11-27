// "Delen met klant" in het venster Versturen: de typen van de server (openaec_werkplek.datadrop) en de teksten. Zie
// src-tauri/src/klant.rs.

export interface KlantProject {
  project: string
  naam: string
  modellen: unknown[]
}

export interface KlantProjecten {
  /** Staat Speckle aan op deze omgeving (anders alleen bestanden). */
  speckle: boolean
  melding: string | null
  projecten: KlantProject[]
}

/** Een eigen levering (datadrop). */
export interface KlantLevering {
  datadrop: string
  titel: string
  status: "Concept" | "Actief" | "Verlopen" | "Ingetrokken"
  versie: number
  ontvangers: string[]
  url: string | null
  project_naam: string | null
  verloopt_op: string | null
  opmerkingen: number
}

export interface KlantGemaakt {
  datadrop: string
  titel: string
  status: string
  versie: number
  url: string
  verloopt_op: string | null
}

export interface KlantGedeeld {
  status: string
  url: string
  ontvangers: string[]
  mail: string[]
}

/** Waar een nieuwe levering naartoe gaat: een nieuwe, of een nieuwe versie van een bestaande. */
export const NIEUW = ""

/** De leveringen die nog een versie kunnen krijgen (niet ingetrokken of verlopen). */
export function voortzetbaar(lijst: KlantLevering[]): KlantLevering[] {
  return lijst.filter((l) => l.status === "Concept" || l.status === "Actief")
}

/** De regel onder een levering in de lijst. */
export function leveringMeta(l: KlantLevering): string {
  const delen = [
    l.status === "Concept" ? "concept, nog niet gedeeld" : l.status.toLowerCase(),
    l.versie > 1 ? `versie ${l.versie}` : "",
    l.project_naam ? `model ${l.project_naam}` : "zonder model",
    l.ontvangers.length ? `gedeeld met ${l.ontvangers.join(", ")}` : "",
    l.opmerkingen ? `${l.opmerkingen} ${l.opmerkingen === 1 ? "opmerking" : "opmerkingen"}` : "",
  ]
  return delen.filter(Boolean).join(" · ")
}

/** Wat er na het delen in de statusregel komt. */
export function gedeeldTekst(g: KlantGedeeld): string {
  return g.mail.length ? `Gedeeld: een mail met de link ging naar ${g.mail.join(", ")}.` : "Gedeeld. Kopieer de link om hem zelf te sturen."
}
