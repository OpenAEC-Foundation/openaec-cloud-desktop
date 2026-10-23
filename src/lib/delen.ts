// Hulpfuncties voor het deelvenster: rechten en vervaldatum leesbaar maken, invoer controleren en fouten van de
// deelcommands uitlezen. Zonder Tauri, zodat `npm test` ze kan testen.

export type Soort = "link" | "gebruiker"
export type Rechten = "lezen" | "schrijven" | "uploaden"

export interface Keuze {
  waarde: Rechten
  tekst: string
}

/** Rechten van een deling zoals het venster ze noemt. */
export const rechtenTekst = (soort: string, rechten: string) => {
  if (soort === "link") return rechten === "uploaden" ? "Bestanden opvragen" : "Kan bekijken"
  return rechten === "schrijven" ? "Kan bewerken" : "Kan bekijken"
}

/** Keuzes bij delen met een collega. */
export const collegaRechten: Keuze[] = [
  { waarde: "lezen", tekst: "Kan bekijken" },
  { waarde: "schrijven", tekst: "Kan bewerken" },
]

/** Keuzes voor een link: bij een map ook bestanden opvragen (uploadlink). */
export const linkRechten = (isMap: boolean): Keuze[] => [
  { waarde: "lezen", tekst: "Kan bekijken" },
  ...(isMap ? [{ waarde: "uploaden" as const, tekst: "Bestanden opvragen" }] : []),
]

/** Wat een lid van de ruimte al mag, volgens zijn rol. */
export const rolTekst = (rol: string) =>
  ({ beheerder: "Beheerder", schrijver: "Kan bewerken", lezer: "Kan bekijken", gast: "Gast (kan bekijken)" })[rol] ?? rol

const twee = (n: number) => String(n).padStart(2, "0")

/** Datum als `JJJJ-MM-DD` in lokale tijd (de waarde van een datumveld). */
export const datumWaarde = (d: Date) => `${d.getFullYear()}-${twee(d.getMonth() + 1)}-${twee(d.getDate())}`

const MAANDEN = ["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"]

/** `JJJJ-MM-DD` (eventueel met tijd erachter, zoals de server hem geeft) als "31 dec 2026". */
export const datumTekst = (waarde: string) => {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(waarde)
  if (!m) return waarde
  return `${Number(m[3])} ${MAANDEN[Number(m[2]) - 1] ?? m[2]} ${m[1]}`
}

/** Is een deling met deze vervaldatum (einde van die dag) verlopen? */
export const isVerlopen = (verloopt: string | null | undefined, nu = new Date()) =>
  !!verloopt && verloopt.slice(0, 10) < datumWaarde(nu)

/** Is deze deling verlopen? Het oordeel van de server als die het geeft, anders zelf gerekend. */
export const delingVerlopen = (d: { verlopen?: boolean | null; verloopt: string | null }, nu = new Date()) =>
  typeof d.verlopen === "boolean" ? d.verlopen : isVerlopen(d.verloopt, nu)

/** Extra regel bij een deling: met wachtwoord en wie hem maakte (alleen wat de server weet). */
export const delingDetails = (d: { beveiligd?: boolean | null; gemaakt_door?: string | null; gemaakt_door_naam?: string | null }) =>
  [d.beveiligd ? "met wachtwoord" : "", d.gemaakt_door_naam || d.gemaakt_door ? `door ${d.gemaakt_door_naam || d.gemaakt_door}` : ""]
    .filter(Boolean)
    .join(" · ")

/** Tekst bij een suggestie: naam en, voor een lid, zijn rol in de ruimte. */
export const collegaTekst = (c: { gebruiker: string; naam: string; lid_als: string | null }) =>
  [c.naam !== c.gebruiker ? c.naam : "", c.lid_als ? `lid: ${rolTekst(c.lid_als).toLowerCase()}` : ""].filter(Boolean).join(" · ")

/** Vervaldatum van een deling: "Geen vervaldatum", "Geldig tot 31 dec 2026", "Verloopt vandaag" of "Verlopen op …". */
export const vervaldatumTekst = (verloopt: string | null | undefined, nu = new Date()) => {
  if (!verloopt) return "Geen vervaldatum"
  const dag = verloopt.slice(0, 10)
  if (dag < datumWaarde(nu)) return `Verlopen op ${datumTekst(dag)}`
  if (dag === datumWaarde(nu)) return "Verloopt vandaag"
  return `Geldig tot ${datumTekst(dag)}`
}

/** Controle van het datumveld: leeg mag (geen vervaldatum), anders vandaag of later. Geeft een melding of "". */
export const controleerVervaldatum = (waarde: string, nu = new Date()) => {
  if (!waarde) return ""
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(waarde)
  const d = m && new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
  if (!d || datumWaarde(d) !== waarde) return "Dit is geen geldige datum."
  if (waarde < datumWaarde(nu)) return "De vervaldatum ligt in het verleden."
  return ""
}

/** E-mailadres van een collega: één '@', geen spaties, en een punt in het domein (zelfde regel als de Rust-kant). */
export const geldigEmail = (email: string) => {
  const e = email.trim()
  const [wie, domein, ...rest] = e.split("@")
  return (
    !rest.length && !!wie && !!domein && domein.includes(".") && !domein.startsWith(".") && !domein.endsWith(".") && !/\s/.test(e)
  )
}

export interface Deelfout {
  code: string
  melding: string
}

/** Fout van een deelcommand (`{ code, melding }`), of van een ander command (tekst). */
export const leesFout = (e: unknown): Deelfout => {
  if (e && typeof e === "object" && "melding" in e) {
    const f = e as { code?: unknown; melding: unknown }
    return { code: String(f.code ?? "fout"), melding: String(f.melding) }
  }
  return { code: "fout", melding: String(e) }
}
