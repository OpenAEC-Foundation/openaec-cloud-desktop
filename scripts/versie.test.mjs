import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { dirname, join } from "node:path"
import { test } from "node:test"
import { fileURLToPath } from "node:url"

import {
  bestanden,
  controleer,
  isoWeek,
  ontleed,
  volgendeVersie,
  wekenInJaar,
  wixVersie,
  zetInCargoLock,
  zetInCargoToml,
  zetInPackageJson,
  zetInPackageLock,
  zetInTauriConf,
} from "./versie.mjs"

const wortel = join(dirname(fileURLToPath(import.meta.url)), "..")
const dag = (j, m, d) => new Date(j, m - 1, d)

test("ISO-week, ook rond de jaarwisseling", () => {
  assert.deepEqual(isoWeek(dag(2026, 10, 26)), { jaar: 2026, week: 44 }) // maandag
  assert.deepEqual(isoWeek(dag(2026, 11, 1)), { jaar: 2026, week: 44 }) // zondag, zelfde week
  assert.deepEqual(isoWeek(dag(2026, 11, 2)), { jaar: 2026, week: 45 })
  assert.deepEqual(isoWeek(dag(2026, 1, 1)), { jaar: 2026, week: 1 }) // donderdag
  assert.deepEqual(isoWeek(dag(2026, 12, 31)), { jaar: 2026, week: 53 })
  assert.deepEqual(isoWeek(dag(2027, 1, 3)), { jaar: 2026, week: 53 }) // hoort nog bij ISO-jaar 2026
  assert.deepEqual(isoWeek(dag(2027, 1, 4)), { jaar: 2027, week: 1 })
  assert.deepEqual(isoWeek(dag(2024, 12, 30)), { jaar: 2025, week: 1 }) // hoort al bij ISO-jaar 2025
  assert.equal(wekenInJaar(2026), 53)
  assert.equal(wekenInJaar(2027), 52)
})

test("volgnummer per week, beginnend bij 0", () => {
  const nu = dag(2026, 10, 26)
  assert.equal(volgendeVersie(nu, []), "2026.44.0")
  assert.equal(volgendeVersie(nu, ["v0.8.0", "v0.8.1"]), "2026.44.0")
  assert.equal(volgendeVersie(nu, ["v2026.44.0"]), "2026.44.1")
  assert.equal(volgendeVersie(nu, ["v2026.44.0", "v2026.44.2", "v2026.44.1"]), "2026.44.3")
  assert.equal(volgendeVersie(nu, ["v2026.43.5", "v2026.4.0", "v2026.440.0"]), "2026.44.0")
  assert.equal(volgendeVersie(nu, ["v2026.44.01", "v2026.44.x"]), "2026.44.0")
  assert.equal(volgendeVersie(dag(2026, 3, 2), []), "2026.10.0")
  assert.equal(volgendeVersie(dag(2026, 1, 5), []), "2026.2.0") // geen voorloopnul
})

test("alleen geldige versies", () => {
  assert.deepEqual(ontleed("v2026.44.0"), { jaar: 2026, week: 44, volgnummer: 0, tekst: "2026.44.0" })
  for (const fout of ["0.8.1", "2026.044.0", "2026.03.0", "2026.44.01", "2026-44", "2026.44", "2026.0.0", "2027.53.0", "2026.54.0"]) {
    assert.throws(() => ontleed(fout), undefined, fout)
  }
  assert.equal(ontleed("2026.53.0").week, 53)
})

test("MSI-versie: JJ.W.N, groter dan de oude 0.x", () => {
  assert.equal(wixVersie("2026.44.0"), "26.44.0")
  assert.equal(wixVersie("v2026.53.12"), "26.53.12")
  assert.equal(wixVersie("2100.1.65535"), "100.1.65535")
  assert.throws(() => wixVersie("2256.1.0"))
  assert.throws(() => wixVersie("2026.1.65536"))
  const groter = (a, b) => a.split(".").map(Number).reduce((r, x, i) => r || x - b.split(".").map(Number)[i], 0) > 0
  assert.ok(groter(wixVersie("2026.44.0"), "0.8.1"))
  assert.ok(groter(wixVersie("2027.1.0"), wixVersie("2026.53.9")))
})

test("bestanden bijwerken zonder de opmaak te veranderen", () => {
  for (const b of bestanden(wortel)) {
    const [[, huidig]] = b.lees(b.tekst)
    if (b.pad.endsWith("tauri.conf.json")) continue // krijgt er bundle.windows.wix.version bij
    assert.equal(b.zet(b.tekst, huidig), b.tekst, `${b.pad} blijft gelijk bij dezelfde versie`)
  }
  // tauri.conf.json: alleen de twee versieregels veranderen.
  const conf = bestanden(wortel).find((b) => b.pad === "src-tauri/tauri.conf.json")
  // Een andere versie dan die in de repo, zodat de test niet afhangt van de huidige stand (en of de wix-versie er al staat).
  const oud = conf.tekst.split(/\r?\n/)
  const nieuw = conf.zet(conf.tekst, "2031.7.3").split(/\r?\n/)
  const hadWix = JSON.parse(conf.tekst).bundle?.windows?.wix?.version !== undefined
  assert.equal(nieuw.length, oud.length + (hadWix ? 0 : 1))
  assert.deepEqual(nieuw.filter((r) => !oud.includes(r)).map((r) => r.trim()), ['"version": "2031.7.3",', '"version": "31.7.3",'])
})

test("alle velden krijgen de nieuwe versie", () => {
  const nieuw = bestanden(wortel).map((b) => ({ ...b, tekst: b.zet(b.tekst, "2026.44.0") }))
  assert.deepEqual(controleer(nieuw, "v2026.44.0"), { versie: "2026.44.0", fouten: [] })
  assert.equal(controleer(nieuw, "v2026.44.1").fouten.length > 0, true)

  const conf = JSON.parse(nieuw.find((b) => b.pad === "src-tauri/tauri.conf.json").tekst)
  assert.equal(conf.version, "2026.44.0")
  assert.equal(conf.bundle.windows.wix.version, "26.44.0")
  assert.equal(conf.bundle.windows.wix.upgradeCode, "05e45fa2-f31f-52e2-b4e8-f0b880688a57")

  // Pakketten van buiten de repo met toevallig hetzelfde nummer blijven staan.
  const lock = nieuw.find((b) => b.pad === "src-tauri/Cargo.lock").tekst
  const oud = readFileSync(join(wortel, "src-tauri/Cargo.lock"), "utf8")
  assert.equal(lock.split("\n").length, oud.split("\n").length)
  assert.match(lock, /name = "openaec-cloud"\r?\nversion = "2026\.44\.0"/)
  assert.match(lock, /name = "openaec-sync"\r?\nversion = "2026\.44\.0"/)
})

test("losse bewerkingen", () => {
  assert.equal(zetInPackageJson('{\n  "name": "x",\n  "version": "0.1.0"\n}\n', "2026.44.0"), '{\n  "name": "x",\n  "version": "2026.44.0"\n}\n')
  assert.equal(
    zetInPackageLock('{\r\n  "version": "0.1.0",\r\n  "packages": {\r\n    "": {\r\n      "version": "0.1.0"\r\n    }\r\n  }\r\n}\r\n', "2026.44.0"),
    '{\r\n  "version": "2026.44.0",\r\n  "packages": {\r\n    "": {\r\n      "version": "2026.44.0"\r\n    }\r\n  }\r\n}\r\n',
  )
  const toml = '[package]\nname = "a"\nversion = "0.1.0"\n\n[dependencies]\nb = { version = "1" }\n'
  assert.equal(zetInCargoToml(toml, "2026.44.0"), toml.replace('"0.1.0"', '"2026.44.0"'))
  const lock = '[[package]]\nname = "a"\nversion = "0.1.0"\n\n[[package]]\nname = "ab"\nversion = "0.1.0"\n'
  assert.equal(zetInCargoLock(lock, ["a"], "2026.44.0"), lock.replace('"0.1.0"', '"2026.44.0"'))
  const conf = JSON.parse(zetInTauriConf('{"version":"0.1.0","bundle":{"windows":{"wix":{"version":"0.1.0","upgradeCode":"x"}}}}', "2026.44.3"))
  assert.deepEqual(conf.bundle.windows.wix, { version: "26.44.3", upgradeCode: "x" })
})
