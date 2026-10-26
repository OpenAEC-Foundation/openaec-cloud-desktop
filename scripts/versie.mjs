#!/usr/bin/env node
// Versienummer van OpenAEC Desktop App: jaar.week, `JJJJ.W.N` (ISO-jaar, ISO-week zonder voorloopnul, volgnummer
// binnen de week dat bij 0 begint). De Git-tag is `vJJJJ.W.N`.
//
//   node scripts/versie.mjs                 volgende versie uit de datum en de bestaande tags, en die overal zetten
//   node scripts/versie.mjs 2026.44.1       deze versie overal zetten (een "v" ervoor mag)
//   node scripts/versie.mjs --volgende      alleen de volgende versie tonen
//   node scripts/versie.mjs --controleer [vJJJJ.W.N]
//                                           controleren dat alle bestanden hetzelfde nummer dragen (en gelijk zijn
//                                           aan de tag); faalt met code 1 bij een verschil
//   --droog                                 bij zetten: alleen tonen wat er zou veranderen
//
// De MSI krijgt een eigen versie in `bundle.windows.wix.version`: een MSI-hoofdversie is hoogstens 255, dus
// 2026.44.0 wordt 26.44.0. Dat is groter dan de oude 0.x-versies, dus een upgrade vanaf een oudere MSI blijft werken.

import { execFileSync } from "node:child_process"
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs"
import { dirname, join, relative } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"

/** `JJJJ.W.N`: geen voorloopnullen in week en volgnummer. */
export const VORM = /^(\d{4})\.([1-9]\d?)\.(0|[1-9]\d*)$/

/** ISO-jaar en ISO-week van een datum (de kalenderdag in de lokale tijd). */
export function isoWeek(datum) {
  const d = new Date(Date.UTC(datum.getFullYear(), datum.getMonth(), datum.getDate()))
  const dag = d.getUTCDay() || 7 // maandag 1 … zondag 7
  d.setUTCDate(d.getUTCDate() + 4 - dag) // de donderdag van deze week bepaalt het jaar
  const jaar = d.getUTCFullYear()
  const week = Math.ceil(((d.getTime() - Date.UTC(jaar, 0, 1)) / 86_400_000 + 1) / 7)
  return { jaar, week }
}

/** Aantal ISO-weken in een jaar (52 of 53): 28 december valt altijd in de laatste week. */
export function wekenInJaar(jaar) {
  return isoWeek(new Date(jaar, 11, 28)).week
}

/** Ontleed `JJJJ.W.N` (met of zonder "v"); gooit bij een ongeldige versie. */
export function ontleed(versie) {
  const tekst = String(versie).trim().replace(/^v/, "")
  const m = VORM.exec(tekst)
  if (!m) throw new Error(`"${versie}" is geen versie in de vorm JJJJ.W.N (bijvoorbeeld 2026.44.0)`)
  const [jaar, week, volgnummer] = m.slice(1).map(Number)
  if (week > wekenInJaar(jaar)) throw new Error(`${jaar} heeft geen week ${week}`)
  return { jaar, week, volgnummer, tekst }
}

/** De volgende versie voor deze datum: volgnummer 0, of één hoger dan de hoogste tag van deze week. */
export function volgendeVersie(datum, tags) {
  const { jaar, week } = isoWeek(datum)
  const voor = `${jaar}.${week}.`
  let hoogste = -1
  for (const tag of tags) {
    const t = String(tag).trim().replace(/^v/, "")
    if (!t.startsWith(voor)) continue
    const n = t.slice(voor.length)
    if (/^(0|[1-9]\d*)$/.test(n)) hoogste = Math.max(hoogste, Number(n))
  }
  return `${voor}${hoogste + 1}`
}

/** MSI-versie bij `JJJJ.W.N`: `JJ.W.N` (hoofdversie en minor hoogstens 255, build hoogstens 65535). */
export function wixVersie(versie) {
  const { jaar, week, volgnummer } = ontleed(versie)
  if (jaar < 2000 || jaar > 2255) throw new Error(`jaar ${jaar} past niet in een MSI-versie`)
  if (volgnummer > 65535) throw new Error(`volgnummer ${volgnummer} past niet in een MSI-versie`)
  return `${jaar - 2000}.${week}.${volgnummer}`
}

// ---------- bestanden ----------

const lf = (tekst) => tekst.replace(/\r\n/g, "\n")
/** Zelfde regeleinden terugschrijven als er stonden. */
const zoalsOrigineel = (origineel, nieuw) => (origineel.includes("\r\n") ? nieuw.replace(/\n/g, "\r\n") : nieuw)
/** JSON met twee spaties inspringing, en een slotregel alleen als die er al was. */
const alsJson = (origineel, data) => JSON.stringify(data, null, 2) + (/\n$/.test(origineel) ? "\n" : "")

export function zetInPackageJson(tekst, versie) {
  const data = JSON.parse(tekst)
  data.version = versie
  return zoalsOrigineel(tekst, alsJson(tekst, data))
}

export function zetInPackageLock(tekst, versie) {
  const data = JSON.parse(tekst)
  data.version = versie
  if (data.packages?.[""]) data.packages[""].version = versie
  return zoalsOrigineel(tekst, alsJson(tekst, data))
}

export function zetInTauriConf(tekst, versie) {
  const data = JSON.parse(tekst)
  data.version = versie
  const windows = ((data.bundle ??= {}).windows ??= {})
  const { version: _oud, ...wix } = windows.wix ?? {}
  windows.wix = { version: wixVersie(versie), ...wix }
  return zoalsOrigineel(tekst, alsJson(tekst, data))
}

const PACKAGE_VERSIE = /^(\[package\]\n(?:(?!\[)[^\n]*\n)*?version\s*=\s*")([^"]*)(")/m

export function zetInCargoToml(tekst, versie) {
  const t = lf(tekst)
  if (!PACKAGE_VERSIE.test(t)) throw new Error("geen version in [package]")
  return zoalsOrigineel(tekst, t.replace(PACKAGE_VERSIE, `$1${versie}$3`))
}

const lockRegel = (naam) =>
  new RegExp(`(\\[\\[package\\]\\]\\nname = "${naam.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"\\nversion = ")([^"]*)(")`)

/** Zet de versie van de eigen pakketten in een Cargo.lock; pakketten van buiten blijven ongemoeid. */
export function zetInCargoLock(tekst, namen, versie) {
  let t = lf(tekst)
  for (const naam of namen) t = t.replace(lockRegel(naam), `$1${versie}$3`)
  return zoalsOrigineel(tekst, t)
}

export function cargoNaam(tekst) {
  return /^\[package\]\n(?:(?!\[)[^\n]*\n)*?name\s*=\s*"([^"]*)"/m.exec(lf(tekst))?.[1]
}
export function cargoVersie(tekst) {
  return PACKAGE_VERSIE.exec(lf(tekst))?.[2]
}
export function lockVersie(tekst, naam) {
  return lockRegel(naam).exec(lf(tekst))?.[2]
}

/** Alle bestanden met een versienummer, met hun huidige waarde en hoe je hem zet. */
export function bestanden(wortel) {
  const lees = (pad) => readFileSync(join(wortel, pad), "utf8")
  const crates = ["src-tauri/Cargo.toml"]
  if (existsSync(join(wortel, "crates"))) {
    for (const map of readdirSync(join(wortel, "crates")).sort()) {
      if (existsSync(join(wortel, "crates", map, "Cargo.toml"))) crates.push(`crates/${map}/Cargo.toml`)
    }
  }
  const namen = crates.map((pad) => cargoNaam(lees(pad))).filter(Boolean)
  const locks = crates.map((pad) => pad.replace(/Cargo\.toml$/, "Cargo.lock")).filter((pad) => existsSync(join(wortel, pad)))

  const lijst = [
    { pad: "package.json", lees: (t) => [["version", JSON.parse(t).version]], zet: zetInPackageJson },
    {
      pad: "package-lock.json",
      lees: (t) => {
        const d = JSON.parse(t)
        return [["version", d.version], ['packages[""].version', d.packages?.[""]?.version]]
      },
      zet: zetInPackageLock,
    },
    {
      pad: "src-tauri/tauri.conf.json",
      lees: (t) => {
        const d = JSON.parse(t)
        return [["version", d.version], ["bundle.windows.wix.version", d.bundle?.windows?.wix?.version, "wix"]]
      },
      zet: zetInTauriConf,
    },
    ...crates.map((pad) => ({ pad, lees: (t) => [["[package] version", cargoVersie(t)]], zet: zetInCargoToml })),
    ...locks.map((pad) => ({
      pad,
      lees: (t) => namen.filter((n) => lockVersie(t, n) !== undefined).map((n) => [n, lockVersie(t, n)]),
      zet: (t, v) => zetInCargoLock(t, namen, v),
    })),
  ]
  return lijst.map((b) => ({ ...b, tekst: lees(b.pad) }))
}

/** Verschillen tussen de bestanden en de verwachte versie (of de eerste gevonden versie). */
export function controleer(lijst, verwacht) {
  const fouten = []
  let versie = verwacht ? ontleed(verwacht).tekst : undefined
  for (const b of lijst) {
    for (const [veld, waarde, soort] of b.lees(b.tekst)) {
      if (soort === "wix") continue
      versie ??= waarde
      if (waarde !== versie) fouten.push(`${b.pad} (${veld}): ${waarde ?? "ontbreekt"}, verwacht ${versie}`)
    }
  }
  if (versie !== undefined) {
    try {
      const wix = wixVersie(versie)
      for (const b of lijst) {
        for (const [veld, waarde, soort] of b.lees(b.tekst)) {
          if (soort === "wix" && waarde !== wix) fouten.push(`${b.pad} (${veld}): ${waarde ?? "ontbreekt"}, verwacht ${wix}`)
        }
      }
    } catch (e) {
      fouten.push(e.message)
    }
  }
  return { versie, fouten }
}

function tags(wortel) {
  try {
    return execFileSync("git", ["tag", "--list", "v*"], { cwd: wortel, encoding: "utf8" }).split(/\r?\n/).filter(Boolean)
  } catch {
    return []
  }
}

function hoofd(argv) {
  const wortel = join(dirname(fileURLToPath(import.meta.url)), "..")
  const vlaggen = new Set(argv.filter((a) => a.startsWith("--")))
  const rest = argv.filter((a) => !a.startsWith("--"))

  if (vlaggen.has("--controleer")) {
    const { versie, fouten } = controleer(bestanden(wortel), rest[0])
    if (fouten.length) {
      console.error(`Versienummers kloppen niet${rest[0] ? ` met tag ${rest[0]}` : ""}:`)
      for (const f of fouten) console.error(`  - ${f}`)
      return 1
    }
    console.log(`Alle bestanden staan op ${versie} (MSI ${wixVersie(versie)}).`)
    return 0
  }

  const versie = rest[0] ? ontleed(rest[0]).tekst : volgendeVersie(new Date(), tags(wortel))
  if (vlaggen.has("--volgende")) {
    console.log(versie)
    return 0
  }
  ontleed(versie)
  const droog = vlaggen.has("--droog")
  for (const b of bestanden(wortel)) {
    const nieuw = b.zet(b.tekst, versie)
    if (nieuw === b.tekst) continue
    console.log(`${droog ? "zou bijwerken" : "bijgewerkt"}: ${relative(wortel, join(wortel, b.pad)).replace(/\\/g, "/")}`)
    if (!droog) writeFileSync(join(wortel, b.pad), nieuw)
  }
  console.log(`Versie ${versie} (MSI ${wixVersie(versie)}), tag v${versie}.`)
  return 0
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    process.exitCode = hoofd(process.argv.slice(2))
  } catch (e) {
    console.error(e.message)
    process.exitCode = 1
  }
}
