// Opmaak van getallen, tijden en paden, gedeeld door het hoofdvenster en het statusvenster.

export const grootte = (b: number) => {
  if (b < 1024) return `${b} B`
  if (b < 1048576) return `${Math.round(b / 1024)} kB`
  if (b < 1073741824) return `${(b / 1048576).toFixed(1).replace(".", ",")} MB`
  return `${(b / 1073741824).toFixed(2).replace(".", ",")} GB`
}

export const snelheid = (bps: number) => (bps > 0 ? `${grootte(Math.round(bps))}/s` : "")

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
