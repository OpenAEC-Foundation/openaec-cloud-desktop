// Hulpfuncties voor het venster Versturen: de invoer controleren binnen de grenzen van de server, de selectie
// samenvoegen en de overdrachten leesbaar maken. Zonder Tauri, zodat `npm test` ze kan testen.

import { datumTekst, datumWaarde, geldigEmail } from "./delen.ts"
import { grootte } from "./opmaak.ts"

/** Een gekozen bestand of map (`versturen_items`). */
export interface VerstuurItem {
  pad: string
  naam: string
  map: boolean
  grootte: number
  bestanden: number
  fout: string | null
  /** In een gekoppelde map en helemaal gesynchroniseerd: staat al in OpenAEC, er hoeft niets geüpload te worden. */
  in_opslag?: boolean
}

/** De grenzen van de server voor een overdracht (`grenzen`); GB = 10^9. */
export interface Overdrachtgrenzen {
  max_bytes: number
  standaard_dagen: number
  max_dagen: number
  max_ontvangers: number
  concept_uren: number
  zip_max_bytes: number
  quotum?: { gebruikt_bytes: number; limiet_bytes: number | null; overdrachten_bytes?: number } | null
}

/** Eén bestand van een lopende overdracht, zoals de app hem bijhoudt. */
export interface Onderdeel {
  pad: string
  naam: string
  map: boolean
  grootte: number
  bestand: string | null
  klaar: boolean
  /** Staat al in OpenAEC: de server kopieert het (geen upload). */
  bron?: { toegevoegd: boolean } | null
}

/** Een overdracht die bezig of onderbroken is. */
export interface Lopend {
  overdracht: string
  naam: string
  basis: string
  onderdelen: Onderdeel[]
  concept_geldig_tot: string | null
}

export type Stapstatus = "wachten" | "inpakken" | "kopieren" | "bezig" | "klaar" | "fout"

/** Voortgang van één onderdeel (event `versturen-stap`). */
export interface VerstuurStap {
  overdracht: string
  index: number
  status: Stapstatus
  gedaan: number
  totaal: number
  fout: string | null
}

/** Antwoord van `voltooi`: de link en naar wie de mail ging. */
export interface Voltooid {
  overdracht: string
  url: string
  verloopt_op: string | null
  aantal_bestanden: number
  totale_grootte: number
  mail: string[]
}

/** Een eigen overdracht in "Mijn overdrachten" (`mijn`). */
export interface OverdrachtRegel {
  overdracht: string
  naam: string
  status: "Concept" | "Actief" | "Verlopen" | "Ingetrokken" | string
  verloopt_op: string | null
  totale_grootte: number
  aantal_bestanden: number
  aantal_downloads: number
  laatst_gedownload: string | null
  ontvangers: string[]
  beveiligd: boolean
  url: string | null
  gemaakt: string | null
  voltooid_op: string | null
}

/** Grootte zoals de server zijn grenzen noemt (GB = 10^9): "100 GB", "2 MB", "1,5 GB". */
export const grootteDecimaal = (b: number) => {
  const eenheden = ["B", "kB", "MB", "GB", "TB"]
  let waarde = b
  let i = 0
  while (waarde >= 1000 && i < eenheden.length - 1) {
    waarde /= 1000
    i++
  }
  const tekst = i === 0 ? String(Math.round(waarde)) : String(Number(waarde.toFixed(1))).replace(".", ",")
  return `${tekst} ${eenheden[i]}`
}

/** Ontvangers uit een invoerveld (komma's, puntkomma's, spaties of regels), zonder dubbelen. */
export const splitsOntvangers = (tekst: string) => {
  const gezien = new Set<string>()
  const uit: string[] = []
  for (const deel of tekst.split(/[,;\s]+/)) {
    const adres = deel.trim()
    if (!adres || gezien.has(adres.toLowerCase())) continue
    gezien.add(adres.toLowerCase())
    uit.push(adres)
  }
  return uit
}

/** Controle van de ontvangers; geeft een melding of "". */
export const controleerOntvangers = (lijst: string[], max: number) => {
  const fout = lijst.filter((a) => !geldigEmail(a))
  if (fout.length === 1) return `Dit is geen geldig e-mailadres: ${fout[0]}`
  if (fout.length > 1) return `Dit zijn geen geldige e-mailadressen: ${fout.join(", ")}`
  if (lijst.length > max) return `Hoogstens ${max} ontvangers per overdracht.`
  return ""
}

/** De laatste dag waarop een overdracht nog geldig mag zijn (`max_dagen` vanaf vandaag), als `JJJJ-MM-DD`. */
export const laatsteDag = (maxDagen: number, nu = new Date()) => {
  const d = new Date(nu.getFullYear(), nu.getMonth(), nu.getDate() + maxDagen)
  return datumWaarde(d)
}

/** Controle van "Geldig tot": leeg mag (de standaardtermijn), anders van vandaag tot de laatste dag. */
export const controleerGeldigTot = (waarde: string, maxDagen: number, nu = new Date()) => {
  if (!waarde) return ""
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(waarde)
  const d = m && new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
  if (!d || datumWaarde(d) !== waarde) return "Dit is geen geldige datum."
  if (waarde < datumWaarde(nu)) return "De vervaldatum ligt in het verleden."
  if (waarde > laatsteDag(maxDagen, nu)) return `Een overdracht kan hoogstens ${maxDagen} dagen geldig zijn (tot ${datumTekst(laatsteDag(maxDagen, nu))}).`
  return ""
}

/** Past alles samen binnen de grootste overdracht? Geeft een melding of "". */
export const controleerGrootte = (totaal: number, max: number) =>
  max > 0 && totaal > max ? `Samen ${grootteDecimaal(totaal)}: een overdracht kan hoogstens ${grootteDecimaal(max)} groot zijn.` : ""

/** Voorstel voor de naam van de overdracht: de naam van het enige item, of het eerste met het aantal andere. */
export const standaardNaam = (items: { naam: string }[]) => {
  if (!items.length) return ""
  if (items.length === 1) return items[0].naam
  const rest = items.length - 1
  return `${items[0].naam} en ${rest} ${rest === 1 ? "ander" : "andere"}`
}

/** "2 bestanden en 1 map · 12,3 MB" (een map telt als één map, met al zijn bestanden in de grootte). */
export const samenvatting = (items: VerstuurItem[]) => {
  const mappen = items.filter((i) => i.map).length
  const bestanden = items.length - mappen
  const delen = [
    bestanden ? `${bestanden} ${bestanden === 1 ? "bestand" : "bestanden"}` : "",
    mappen ? `${mappen} ${mappen === 1 ? "map" : "mappen"}` : "",
  ].filter(Boolean)
  if (!delen.length) return "Niets gekozen"
  return `${delen.join(" en ")} · ${grootte(items.reduce((s, i) => s + i.grootte, 0))}`
}

export type Fase = "samenstellen" | "bezig" | "klaar" | "fout"

/** Een nieuwe selectie uit het rechtermuismenu: tijdens het samenstellen komt hij erbij (een selectie van meerdere
 * items komt in de Verkenner per item binnen), na een verstuurde of mislukte overdracht vervangt hij de oude, en
 * tijdens het versturen wacht hij (`wachtend`). */
export const voegSamen = (huidig: string[], nieuw: string[], fase: Fase) => {
  if (fase === "bezig") return { paden: huidig, wachtend: nieuw }
  const basis = fase === "samenstellen" ? huidig : []
  const paden = [...basis]
  for (const p of nieuw) if (!paden.includes(p)) paden.push(p)
  return { paden, wachtend: [] as string[] }
}

/** Regel onder de lijst: wat er al in OpenAEC staat en dus niet geüpload hoeft te worden ("" als niets). */
export const opslagTekst = (items: VerstuurItem[]) => {
  const al = items.filter((i) => i.in_opslag && !i.fout).length
  if (!al) return ""
  if (al === items.length) return "Staat al in OpenAEC: er wordt niets geüpload."
  return `${al} van ${items.length} staan al in OpenAEC en worden niet opnieuw geüpload; de rest wordt geüpload.`
}

/** Status van een overdracht zoals het venster hem noemt. */
export const statusTekst = (status: string) =>
  ({ Concept: "Niet afgemaakt", Actief: "Actief", Verlopen: "Verlopen", Ingetrokken: "Ingetrokken" })[status] ?? status

/** Regel onder een overdracht in "Mijn overdrachten". */
export const overdrachtMeta = (r: OverdrachtRegel) =>
  [
    `${r.aantal_bestanden} ${r.aantal_bestanden === 1 ? "bestand" : "bestanden"}`,
    grootte(r.totale_grootte),
    r.status === "Actief" || r.aantal_downloads
      ? r.aantal_downloads === 1
        ? "1 keer gedownload"
        : `${r.aantal_downloads} keer gedownload`
      : "",
    r.status === "Actief" && r.verloopt_op ? `geldig tot ${datumTekst(r.verloopt_op)}` : "",
    r.beveiligd ? "met wachtwoord" : "",
    r.ontvangers.length ? `naar ${r.ontvangers.join(", ")}` : "",
  ]
    .filter(Boolean)
    .join(" · ")

/** Kan deze overdracht nog ingetrokken worden? */
export const intrekbaar = (r: { status: string }) => r.status === "Actief" || r.status === "Concept"

/** Alle stappen samen: bytes gedaan van het totaal. */
export const totaal = (stappen: { gedaan: number; totaal: number }[]) =>
  stappen.reduce((s, x) => ({ gedaan: s.gedaan + Math.min(x.gedaan, x.totaal), totaal: s.totaal + x.totaal }), { gedaan: 0, totaal: 0 })

/** Tekst bij een onderdeel tijdens het versturen. */
export const stapTekst = (s: { status: Stapstatus; gedaan: number; totaal: number; fout: string | null }) => {
  switch (s.status) {
    case "wachten":
      return "Wacht"
    case "inpakken":
      return "Map inpakken…"
    case "kopieren":
      return "Kopiëren in OpenAEC…"
    case "bezig":
      return s.totaal > 0 ? `${Math.min(100, Math.floor((s.gedaan / s.totaal) * 100))}%` : "Bezig…"
    case "klaar":
      return "Verstuurd"
    case "fout":
      return s.fout ?? "Mislukt"
  }
}
