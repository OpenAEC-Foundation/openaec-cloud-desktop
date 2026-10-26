// De assistent: de vorm van gesprekken en bijlagen (zoals de app ze bewaart in %APPDATA%\OpenAEC Storage\assistent) en
// pure hulpfuncties voor het venster: titels, grootte, verbruik, datum en fouten.

export interface Model {
  id: string
  naam?: string | null
  /** "EU" (gegevens blijven in de EU) of "VS". */
  datalabel?: string | null
  standaard: boolean
}

export interface Gebruik {
  prompt_tokens: number
  completion_tokens: number
  total_tokens: number
  cached_tokens: number
}

export type Bijlage = { naam: string; grootte: number } & (
  | { soort: "tekst"; tekst: string }
  | { soort: "afbeelding"; data_url: string; breedte: number; hoogte: number }
)

export interface Bericht {
  rol: "user" | "assistant"
  tekst: string
  bijlagen?: Bijlage[]
  /** Ondoorzichtig, van het model: ongewijzigd terug bij de volgende vraag. */
  denkblokken?: string | null
  model?: string | null
  gebruik?: Gebruik | null
  fout?: string | null
  afgebroken?: boolean
}

export interface Gesprek {
  id: string
  titel: string
  model?: string | null
  aangemaakt: number
  bijgewerkt: number
  berichten: Bericht[]
}

export interface Samenvatting {
  id: string
  titel: string
  bijgewerkt: number
}

/** Wat er tijdens een antwoord binnenkomt. */
export type Gebeurtenis =
  | { soort: "tekst"; tekst: string }
  | { soort: "denkblokken"; denkblokken: string }
  | { soort: "einde"; reden: string }
  | { soort: "gebruik"; gebruik: Gebruik }

/** Een fout van de assistent: de soort bepaalt of er een knop bij hoort (Inloggen, Beheer). */
export interface AiFout {
  soort: string
  melding: string
  retry_na?: number | null
}

export const PRIVACYMELDING =
  "Vragen en bijlagen gaan naar de AI-dienst van OpenAEC in de EU en kosten AI-tegoed van je omgeving."

/** Zoveel tekens tekst uit bijlagen in één gesprek (zoals de app controleert). */
export const MAX_TEKST_TOTAAL = 400_000

export function nieuwId(nu = Date.now(), toeval = Math.random()): string {
  return `${nu.toString(36)}-${Math.floor(toeval * 36 ** 6).toString(36).padStart(6, "0")}`
}

export function nieuwGesprek(model: string | null, nu = Date.now()): Gesprek {
  return { id: nieuwId(nu), titel: "Nieuw gesprek", model, aangemaakt: nu, bijgewerkt: nu, berichten: [] }
}

/** Een titel uit de eerste vraag: de eerste regel, hoogstens 60 tekens (op een woordgrens). */
export function titelVan(vraag: string, bijlagen: Bijlage[] = []): string {
  const regel = vraag.split("\n").map((r) => r.trim()).find(Boolean) ?? ""
  const tekst = regel.replace(/[#*_`>]+/g, "").replace(/\s+/g, " ").trim() || bijlagen[0]?.naam || "Nieuw gesprek"
  if (tekst.length <= 60) return tekst
  const kort = tekst.slice(0, 60)
  const spatie = kort.lastIndexOf(" ")
  return (spatie > 30 ? kort.slice(0, spatie) : kort).replace(/[\s,.;:]+$/, "") + "…"
}

/** De tekens tekst in de bijlagen van een gesprek (plus wat er nog klaarstaat). */
export function tekstInBijlagen(berichten: Bericht[], klaar: Bijlage[] = []): number {
  return [...berichten.flatMap((b) => b.bijlagen ?? []), ...klaar].reduce(
    (n, b) => n + (b.soort === "tekst" ? [...b.tekst].length : 0),
    0,
  )
}

const getal = (n: number) => n.toLocaleString("nl-NL")

/** "1.234 tokens in, 56 uit" (met het deel uit de cache als dat er is). */
export function gebruikTekst(g: Gebruik | null | undefined): string {
  if (!g) return ""
  const cache = g.cached_tokens > 0 ? ` (${getal(g.cached_tokens)} uit de cache)` : ""
  return `${getal(g.prompt_tokens)} tokens in${cache}, ${getal(g.completion_tokens)} uit`
}

/** "vandaag 14:05", "gisteren 09:12" of "12 okt." (met jaar als het een ander jaar is). */
export function datumTekst(ms: number, nu = new Date()): string {
  const d = new Date(ms)
  const tijd = d.toLocaleTimeString("nl-NL", { hour: "2-digit", minute: "2-digit" })
  const dag = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime()
  const verschil = Math.round((dag(nu) - dag(d)) / 86_400_000)
  if (verschil === 0) return `vandaag ${tijd}`
  if (verschil === 1) return `gisteren ${tijd}`
  return d.toLocaleDateString("nl-NL", d.getFullYear() === nu.getFullYear() ? { day: "numeric", month: "short" } : { day: "numeric", month: "short", year: "numeric" })
}

/** Een fout van een opdracht als [AiFout]: een tekst (bijv. van een bijlage) wordt soort "anders". */
export function alsFout(e: unknown): AiFout {
  if (e && typeof e === "object" && "melding" in e && typeof (e as AiFout).melding === "string") {
    const f = e as AiFout
    return { soort: f.soort || "anders", melding: f.melding, retry_na: f.retry_na ?? null }
  }
  return { soort: "anders", melding: String(e) }
}

/** Het label van een model in de keuzelijst: de naam, met het datalabel erachter. */
export function modelLabel(m: Model): string {
  const naam = m.naam || m.id
  return m.datalabel ? `${naam} · ${m.datalabel}` : naam
}

/** Het model voor een nieuw gesprek: het gekozen model als de dienst het nog heeft, anders het standaardmodel. */
export function kiesModel(modellen: Model[], voorkeur: string | null | undefined): string | null {
  if (voorkeur && modellen.some((m) => m.id === voorkeur)) return voorkeur
  return modellen.find((m) => m.standaard)?.id ?? modellen[0]?.id ?? null
}

/** Verwerk een gebeurtenis in het antwoord dat binnenkomt (een nieuw object, voor de weergave). */
export function verwerk(antwoord: Bericht, g: Gebeurtenis): Bericht {
  switch (g.soort) {
    case "tekst":
      return { ...antwoord, tekst: antwoord.tekst + g.tekst }
    case "denkblokken":
      return { ...antwoord, denkblokken: g.denkblokken }
    case "gebruik":
      return { ...antwoord, gebruik: g.gebruik }
    case "einde":
      return g.reden === "length" ? { ...antwoord, fout: "Het antwoord is afgekapt: het werd te lang." } : antwoord
  }
}

/** Bestandstypen voor de knop "Bestand kiezen". */
export const KIESBARE_EXTENSIES = [
  "png", "jpg", "jpeg", "webp", "pdf", "docx", "xlsx", "txt", "md", "csv", "tsv", "json", "xml", "ifc", "py", "ts", "tsx",
  "js", "rs", "toml", "yaml", "yml", "ini", "log", "html", "css", "sql",
]
