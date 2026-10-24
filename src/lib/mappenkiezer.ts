// Logica van "Mappen kiezen…" (selectieve synchronisatie), los van het scherm: een boom uit de mappen van de server,
// vinkjes die met hun bovenliggende map meegaan, in- en uitklappen, en wat er bij opslaan nieuw uitgevinkt is.

export interface Mapregel {
  pad: string
  bytes: number
  bestanden: number
}

export interface Boomregel extends Mapregel {
  naam: string
  diepte: number
  /** Heeft deze map submappen (dan is hij in te klappen)? */
  kinderen: boolean
}

/** Ligt `map` in `ouder` (of is het dezelfde map)? Hele mapnamen. */
export const onder = (map: string, ouder: string) => map === ouder || map.startsWith(ouder + "/")

/** De mappen als boom, op volgorde: elke map direct na zijn bovenliggende map, per niveau op naam. */
export function boom(mappen: Mapregel[]): Boomregel[] {
  const paden = new Set(mappen.map((m) => m.pad))
  const gesorteerd = [...mappen].sort((a, b) => {
    const x = a.pad.split("/")
    const y = b.pad.split("/")
    for (let i = 0; i < Math.min(x.length, y.length); i++) {
      if (x[i] !== y[i]) return x[i].localeCompare(y[i], "nl", { sensitivity: "base" })
    }
    return x.length - y.length
  })
  return gesorteerd.map((m) => ({
    ...m,
    naam: m.pad.split("/").pop() ?? m.pad,
    diepte: m.pad.split("/").length - 1,
    kinderen: [...paden].some((p) => p !== m.pad && onder(p, m.pad)),
  }))
}

/** Is de map uitgevinkt (zelf of via een bovenliggende map)? */
export const isUit = (uit: string[], map: string) => uit.some((u) => onder(map, u))

/** Is een bovenliggende map uitgevinkt (dan is het vinkje van deze map niet te wijzigen)? */
export const ouderUit = (uit: string[], map: string) => uit.some((u) => u !== map && onder(map, u))

/** Vinkje omzetten: aan haalt de map uit de lijst; uit zet hem erin en haalt zijn submappen eruit. */
export const wissel = (uit: string[], map: string) =>
  isUit(uit, map) ? uit.filter((u) => u !== map) : [...uit.filter((u) => !onder(u, map)), map]

/** Zichtbare regels: niet onder een ingeklapte map. */
export const zichtbaar = (regels: Boomregel[], ingeklapt: Set<string>) =>
  regels.filter((r) => ![...ingeklapt].some((k) => k !== r.pad && onder(r.pad, k)))

/** Mappen die bij opslaan nieuw uitgevinkt zijn (nu uit, eerst niet): die worden hier opgeruimd. */
export const nieuwUit = (oud: string[], nieuw: string[]) => nieuw.filter((m) => !isUit(oud, m))
