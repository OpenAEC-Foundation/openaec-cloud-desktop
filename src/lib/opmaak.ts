// Opmaak van getallen, tijden en paden, gedeeld door het hoofdvenster en het statusvenster.

export const grootte = (b: number) => {
  if (b < 1024) return `${b} B`
  if (b < 1048576) return `${Math.round(b / 1024)} kB`
  if (b < 1073741824) return `${(b / 1048576).toFixed(1).replace(".", ",")} MB`
  return `${(b / 1073741824).toFixed(2).replace(".", ",")} GB`
}

export const snelheid = (bps: number) => (bps > 0 ? `${grootte(Math.round(bps))}/s` : "")

const GB = 1073741824

/** Altijd in GB, voor opslag en rondes ("0,25 GB", "12 GB", "1,2 GB"). */
export const gb = (b: number) => {
  const g = b / GB
  const decimalen = g >= 100 ? 0 : g >= 10 ? 1 : 2
  return `${Number(g.toFixed(decimalen)).toString().replace(".", ",")} GB`
}

/** Resterende tijd: "45 s", "3 min", "1 u 5 min". */
export const duur = (sec: number | null | undefined) => {
  if (sec === null || sec === undefined || !Number.isFinite(sec)) return ""
  const s = Math.max(0, Math.round(sec))
  if (s < 60) return `${s} s`
  const min = Math.round(s / 60)
  if (min < 60) return `${min} min`
  const u = Math.floor(min / 60)
  const rest = min % 60
  return rest ? `${u} u ${rest} min` : `${u} u`
}

/** Opslaggebruik: "X GB van Y GB gebruikt (Z GB vrij)" of "X GB gebruikt (geen limiet)". */
export const quotumTekst = (gebruikt: number, limiet: number | null) =>
  limiet === null
    ? `${gb(gebruikt)} gebruikt (geen limiet)`
    : `${gb(gebruikt)} van ${gb(limiet)} gebruikt (${gb(Math.max(0, limiet - gebruikt))} vrij)`

/** Lopende ronde: "nog 3 bestanden, 1,2 GB van 4 GB · 12,5 MB/s · nog 4 min". */
export const rondeTekst = (r: { nog: number; bytes_gedaan: number; bytes_totaal: number; snelheid: number; resterend: number | null }) =>
  [
    `nog ${r.nog} bestand${r.nog === 1 ? "" : "en"}, ${gb(r.bytes_gedaan)} van ${gb(r.bytes_totaal)}`,
    snelheid(r.snelheid),
    r.resterend !== null && r.snelheid > 0 ? `nog ${duur(r.resterend)}` : "",
  ]
    .filter(Boolean)
    .join(" · ")

/** Besparing door blokhergebruik: "alleen 16,0 MB van 1,20 GB verstuurd", of leeg als alles over moest. */
export const besparingTekst = (bestand: number | null | undefined, verstuurd: number | null | undefined) =>
  bestand && verstuurd !== null && verstuurd !== undefined && verstuurd < bestand
    ? `alleen ${grootte(verstuurd)} van ${grootte(bestand)} verstuurd`
    : ""

export const procent = (gedaan: number, totaal: number) => (totaal > 0 ? Math.min(100, Math.round((gedaan / totaal) * 100)) : 0)

/** Tijd van vandaag als uu:mm, anders dag en maand erbij. */
export const tijdstip = (d: Date) => {
  const vandaag = new Date().toDateString() === d.toDateString()
  return vandaag
    ? d.toLocaleTimeString("nl-NL", { hour: "2-digit", minute: "2-digit" })
    : d.toLocaleString("nl-NL", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" })
}

/** Tijdstempel van de server (`JJJJ-MM-DD uu:mm:ss.ffffff`) leesbaar maken. */
export const servertijd = (t: string | null) => {
  if (!t) return ""
  const d = new Date(t.replace(" ", "T").slice(0, 19))
  return isNaN(d.getTime()) ? t.slice(0, 16) : tijdstip(d)
}

/** Laatste deel van een pad (met '/' of '\'). */
export const naamVan = (pad: string) => pad.split(/[\\/]/).filter(Boolean).pop() ?? pad

/** Pad in een gekoppelde map (met '/') aan de lokale map plakken, met het scheidingsteken van die map. */
export const lokaalPad = (lokaal: string, pad: string) => {
  const sep = lokaal.includes("\\") ? "\\" : "/"
  return lokaal.replace(/[\\/]+$/, "") + sep + pad.split("/").join(sep)
}

const soortNaam: Record<string, string> = { team: "Teamruimte", project: "Projectruimte", persoonlijk: "Persoonlijke ruimte" }

/** Leesbare soort van een ruimte. */
export const soort = (s: string | null | undefined) => (s ? soortNaam[s] ?? s : null)
