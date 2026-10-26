// Markdown van de assistent als boom van blokken en inline-delen. Het venster bouwt daar gewone elementen met tekst van
// (zie Markdown.tsx), nooit HTML: een `<script>` in een antwoord blijft letterlijk tekst. Genoeg voor wat een model
// schrijft: kopjes, alinea's, lijsten (ook genest), codeblokken, tabellen, citaten, een lijn, vet, cursief,
// doorgehaald, code en links (alleen http, https en mailto). Een codeblok dat nog niet dicht is (tijdens het streamen)
// is gewoon een codeblok tot het eind.

export type Inline =
  | { soort: "tekst"; tekst: string }
  | { soort: "code"; tekst: string }
  | { soort: "breuk" }
  | { soort: "vet" | "cursief" | "doorgehaald"; inhoud: Inline[] }
  | { soort: "link"; href: string; inhoud: Inline[] }

export type Uitlijning = "links" | "midden" | "rechts" | null

export type Blok =
  | { soort: "alinea"; inhoud: Inline[] }
  | { soort: "kop"; niveau: number; inhoud: Inline[] }
  | { soort: "code"; taal: string; tekst: string }
  | { soort: "lijst"; geordend: boolean; start: number; items: Blok[][] }
  | { soort: "citaat"; blokken: Blok[] }
  | { soort: "tabel"; kop: Inline[][]; uitlijning: Uitlijning[]; rijen: Inline[][][] }
  | { soort: "lijn" }

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^`\s]*)[^`]*$/
const KOP = /^ {0,3}(#{1,6})(?:[ \t]+(.*?))?[ \t#]*$/
const LIJN = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/
const LIJSTITEM = /^( {0,12})([-*+]|\d{1,9}[.)])(?:[ \t]+(.*))?$/
const CITAAT = /^ {0,3}> ?(.*)$/
const SCHEIDING = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/

/** Alleen veilige adressen worden een link; de rest blijft tekst. */
export function veiligeLink(href: string): string | null {
  const h = href.trim()
  return /^(https?:\/\/|mailto:)/i.test(h) && !/[\s<>"]/.test(h) ? h : null
}

function cellen(regel: string): string[] {
  let r = regel.trim()
  if (r.startsWith("|")) r = r.slice(1)
  if (r.endsWith("|") && !r.endsWith("\\|")) r = r.slice(0, -1)
  const uit: string[] = []
  let huidig = ""
  for (let i = 0; i < r.length; i++) {
    if (r[i] === "\\" && r[i + 1] === "|") {
      huidig += "|"
      i++
    } else if (r[i] === "|") {
      uit.push(huidig.trim())
      huidig = ""
    } else huidig += r[i]
  }
  uit.push(huidig.trim())
  return uit
}

function uitlijning(cel: string): Uitlijning {
  const c = cel.trim()
  const links = c.startsWith(":")
  const rechts = c.endsWith(":")
  return links && rechts ? "midden" : rechts ? "rechts" : links ? "links" : null
}

/** Begint op deze regel een ander soort blok (dan stopt een alinea)? */
function blokBegin(regel: string, volgende: string | undefined): boolean {
  return (
    FENCE.test(regel) ||
    KOP.test(regel) ||
    LIJN.test(regel) ||
    CITAAT.test(regel) ||
    /^ {0,3}([-*+]|\d{1,9}[.)])[ \t]+\S/.test(regel) ||
    (regel.includes("|") && volgende !== undefined && SCHEIDING.test(volgende) && volgende.includes("-"))
  )
}

export function parseMarkdown(bron: string): Blok[] {
  return blokken(bron.replace(/\r\n?/g, "\n").split("\n"))
}

function blokken(regels: string[]): Blok[] {
  const uit: Blok[] = []
  let i = 0
  while (i < regels.length) {
    const regel = regels[i]
    if (!regel.trim()) {
      i++
      continue
    }
    // Codeblok
    const fence = FENCE.exec(regel)
    if (fence) {
      const teken = fence[1][0]
      const lengte = fence[1].length
      const inspringing = regel.length - regel.trimStart().length
      const code: string[] = []
      i++
      while (i < regels.length) {
        const r = regels[i]
        const t = r.trim()
        if (t.length >= lengte && [...t].every((c) => c === teken)) {
          i++
          break
        }
        code.push(r.slice(Math.min(inspringing, r.length - r.trimStart().length)))
        i++
      }
      uit.push({ soort: "code", taal: fence[2] ?? "", tekst: code.join("\n") })
      continue
    }
    const kop = KOP.exec(regel)
    if (kop) {
      uit.push({ soort: "kop", niveau: kop[1].length, inhoud: parseInline(kop[2] ?? "") })
      i++
      continue
    }
    if (LIJN.test(regel)) {
      uit.push({ soort: "lijn" })
      i++
      continue
    }
    if (CITAAT.test(regel)) {
      const binnen: string[] = []
      while (i < regels.length && regels[i].trim()) {
        const c = CITAAT.exec(regels[i])
        binnen.push(c ? c[1] : regels[i])
        i++
      }
      uit.push({ soort: "citaat", blokken: blokken(binnen) })
      continue
    }
    // Tabel: een kopregel met | en daaronder de scheiding.
    if (regel.includes("|") && i + 1 < regels.length && SCHEIDING.test(regels[i + 1]) && regels[i + 1].includes("-")) {
      const kopcellen = cellen(regel)
      const lijnen = cellen(regels[i + 1]).map(uitlijning)
      const breedte = kopcellen.length
      i += 2
      const rijen: Inline[][][] = []
      while (i < regels.length && regels[i].trim() && regels[i].includes("|")) {
        const c = cellen(regels[i])
        rijen.push(Array.from({ length: breedte }, (_, k) => parseInline(c[k] ?? "")))
        i++
      }
      uit.push({
        soort: "tabel",
        kop: kopcellen.map(parseInline),
        uitlijning: Array.from({ length: breedte }, (_, k) => lijnen[k] ?? null),
        rijen,
      })
      continue
    }
    const item = LIJSTITEM.exec(regel)
    if (item && item[1].length <= 3 && (item[3] ?? "").trim() !== "" ) {
      const [lijst, na] = lijstVanaf(regels, i)
      uit.push(lijst)
      i = na
      continue
    }
    // Alinea: tot een lege regel of een ander blok.
    const alinea: string[] = [regel.trim()]
    i++
    while (i < regels.length && regels[i].trim() && !blokBegin(regels[i], regels[i + 1])) {
      alinea.push(regels[i].trim())
      i++
    }
    uit.push({ soort: "alinea", inhoud: parseInline(alinea.join("\n")) })
  }
  return uit
}

/** Een lijst vanaf regel `i`: items met dezelfde soort markering; ingesprongen regels horen bij het item ervoor. */
function lijstVanaf(regels: string[], i: number): [Blok, number] {
  const eerste = LIJSTITEM.exec(regels[i])!
  const geordend = /\d/.test(eerste[2])
  const basis = eerste[1].length
  const items: Blok[][] = []
  let huidig: string[] | null = null
  let inhoudBegin = 0
  const sluit = () => {
    if (huidig) items.push(blokken(huidig))
  }
  while (i < regels.length) {
    const regel = regels[i]
    const m = LIJSTITEM.exec(regel)
    if (m && m[1].length <= basis + 1 && /\d/.test(m[2]) === geordend) {
      sluit()
      huidig = [m[3] ?? ""]
      inhoudBegin = m[1].length + m[2].length + 1
      i++
      continue
    }
    if (!regel.trim()) {
      // Een lege regel: de lijst gaat door als er een ingesprongen regel of een nieuw item volgt.
      const volgende = regels[i + 1]
      if (volgende !== undefined && (volgende.startsWith(" ".repeat(Math.max(2, inhoudBegin))) || LIJSTITEM.exec(volgende)?.[1].length === basis)) {
        huidig?.push("")
        i++
        continue
      }
      break
    }
    const inspringing = regel.length - regel.trimStart().length
    if (inspringing >= Math.min(inhoudBegin, basis + 2)) {
      // Bij het item (ook een geneste lijst): zoveel inspringing eraf als de inhoud van het item begint.
      huidig?.push(regel.slice(Math.min(inspringing, inhoudBegin)))
      i++
      continue
    }
    if (blokBegin(regel, regels[i + 1])) break
    // Een lui vervolg van de laatste alinea in het item.
    huidig?.push(regel.trim())
    i++
  }
  sluit()
  const start = geordend ? parseInt(eerste[2], 10) : 1
  return [{ soort: "lijst", geordend, start, items }, i]
}

const LEESTEKENS = "\\`*_{}[]()#+-.!|~<>\"'"

/** Inline-opmaak. Een openende markering zonder sluitende blijft gewoon tekst. */
export function parseInline(tekst: string): Inline[] {
  const uit: Inline[] = []
  let buffer = ""
  const leeg = () => {
    if (buffer) uit.push({ soort: "tekst", tekst: buffer })
    buffer = ""
  }
  let i = 0
  while (i < tekst.length) {
    const c = tekst[i]
    // Escape
    if (c === "\\" && i + 1 < tekst.length && LEESTEKENS.includes(tekst[i + 1])) {
      buffer += tekst[i + 1]
      i += 2
      continue
    }
    if (c === "\n") {
      leeg()
      uit.push({ soort: "breuk" })
      i++
      continue
    }
    // Code: een rij backticks tot dezelfde rij.
    if (c === "`") {
      let n = 1
      while (tekst[i + n] === "`") n++
      const rij = "`".repeat(n)
      const eind = tekst.indexOf(rij, i + n)
      if (eind > -1 && tekst[eind + n] !== "`") {
        leeg()
        let code = tekst.slice(i + n, eind).replace(/\n/g, " ")
        if (code.startsWith(" ") && code.endsWith(" ") && code.trim()) code = code.slice(1, -1)
        uit.push({ soort: "code", tekst: code })
        i = eind + n
        continue
      }
      buffer += rij
      i += n
      continue
    }
    // Link [tekst](adres)
    if (c === "[") {
      const sluit = zoekSluitend(tekst, i)
      if (sluit > -1 && tekst[sluit + 1] === "(") {
        // Het adres tot de bijbehorende `)` (haakjes in een adres mogen, als ze in paren staan).
        let eind = -1
        for (let k = sluit + 2, diepte = 0; k < tekst.length && tekst[k] !== "\n"; k++) {
          if (tekst[k] === "(") diepte++
          else if (tekst[k] === ")" && diepte-- === 0) {
            eind = k
            break
          }
        }
        if (eind > -1) {
          const adres = tekst.slice(sluit + 2, eind).trim().split(/\s+/)[0] ?? ""
          const href = veiligeLink(adres.replace(/^<|>$/g, ""))
          const inhoud = parseInline(tekst.slice(i + 1, sluit))
          leeg()
          uit.push(href ? { soort: "link", href, inhoud } : { soort: "tekst", tekst: tekst.slice(i + 1, sluit) })
          i = eind + 1
          continue
        }
      }
    }
    // Autolink <https://…> en een kaal adres
    if (c === "<") {
      const eind = tekst.indexOf(">", i)
      const href = eind > -1 ? veiligeLink(tekst.slice(i + 1, eind)) : null
      if (href) {
        leeg()
        uit.push({ soort: "link", href, inhoud: [{ soort: "tekst", tekst: href.replace(/^mailto:/i, "") }] })
        i = eind + 1
        continue
      }
    }
    if ((c === "h" || c === "H") && /^https?:\/\//i.test(tekst.slice(i, i + 8)) && !/[\w/]/.test(tekst[i - 1] ?? "")) {
      const m = /^https?:\/\/[^\s<>"]+/i.exec(tekst.slice(i))!
      // Een leesteken aan het eind hoort bij de zin, niet bij het adres.
      const adres = m[0].replace(/[.,;:!?)\]]+$/, "")
      leeg()
      uit.push({ soort: "link", href: adres, inhoud: [{ soort: "tekst", tekst: adres }] })
      i += adres.length
      continue
    }
    // Vet, cursief, doorgehaald
    if (c === "*" || c === "_" || c === "~") {
      let n = 1
      while (tekst[i + n] === c) n++
      const vorige = tekst[i - 1] ?? " "
      const volgende = tekst[i + n] ?? " "
      // `_` midden in een woord (snake_case) en een markering voor witruimte openen niets.
      const kanOpenen = !/\s/.test(volgende) && !(c === "_" && /[\p{L}\p{N}]/u.test(vorige))
      const soort = c === "~" ? (n === 2 ? "doorgehaald" : null) : n >= 2 ? "vet" : "cursief"
      if (kanOpenen && soort && n === 3 && c !== "~") {
        // ***vet en cursief***
        const eind = zoekMarkering(tekst, i + 3, c.repeat(3))
        if (eind > -1) {
          leeg()
          uit.push({ soort: "vet", inhoud: [{ soort: "cursief", inhoud: parseInline(tekst.slice(i + 3, eind)) }] })
          i = eind + 3
          continue
        }
      }
      if (kanOpenen && soort) {
        const rij = c.repeat(c === "~" ? 2 : Math.min(n, 2))
        const eind = zoekMarkering(tekst, i + rij.length, rij)
        if (eind > -1) {
          leeg()
          uit.push({ soort, inhoud: parseInline(tekst.slice(i + rij.length, eind)) })
          i = eind + rij.length
          continue
        }
      }
      buffer += c.repeat(n)
      i += n
      continue
    }
    buffer += c
    i++
  }
  leeg()
  return uit
}

/** De `]` bij de `[` op `begin` (met geneste haken). */
function zoekSluitend(tekst: string, begin: number): number {
  let diepte = 0
  for (let i = begin; i < tekst.length; i++) {
    if (tekst[i] === "\\") i++
    else if (tekst[i] === "[") diepte++
    else if (tekst[i] === "]" && --diepte === 0) return i
    else if (tekst[i] === "\n" && tekst[i + 1] === "\n") return -1
  }
  return -1
}

/** De sluitende markering: niet direct na witruimte, en niet in code. */
function zoekMarkering(tekst: string, vanaf: number, rij: string): number {
  for (let i = vanaf; i < tekst.length; i++) {
    if (tekst[i] === "\\") {
      i++
      continue
    }
    if (tekst[i] === "`") {
      const eind = tekst.indexOf("`", i + 1)
      if (eind > -1) i = eind
      continue
    }
    if (tekst.startsWith(rij, i) && i > vanaf && !/\s/.test(tekst[i - 1])) {
      // Bij `*`: niet de eerste helft van `**` (die hoort bij een binnenste vet).
      if (rij.length === 1 && tekst[i + 1] === rij && tekst[i + 2] !== rij) {
        const binnen = zoekMarkering(tekst, i + 2, rij + rij)
        if (binnen > -1) {
          i = binnen + 1
          continue
        }
      }
      if (rij[0] === "_" && /[\p{L}\p{N}]/u.test(tekst[i + rij.length] ?? "")) continue
      return i
    }
  }
  return -1
}

/** De platte tekst van inline-delen (voor een titel of om te kopiëren). */
export function platteTekst(delen: Inline[]): string {
  return delen
    .map((d) => (d.soort === "tekst" || d.soort === "code" ? d.tekst : d.soort === "breuk" ? "\n" : platteTekst(d.inhoud)))
    .join("")
}
