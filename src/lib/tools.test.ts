// Tests voor de pure logica van het tabblad Tools. Draaien met `npm test` (node --test, zonder extra pakketten).

import { describe, it } from "node:test"
import assert from "node:assert/strict"
import { ALLE_TOOLS, DESKTOP_TOOLS, TOOLS, WEB_TOOLS, type Tool } from "./catalogus.ts"
import { eigenRepo, kanInstalleren, statusVan, vergelijkVersies, voegSamen, zoekTreffer, type ReleaseInfo } from "./tools.ts"

const release = (id: string, versie: string | null, ok = true): ReleaseInfo => ({
  id,
  ok,
  error: ok ? null : "rate_limited",
  tag: versie && `v${versie}`,
  version: versie,
  assetName: versie && `Tool_${versie}_x64-setup.exe`,
  assetUrl: versie && `https://github.com/OpenAEC-Foundation/tool/releases/download/v${versie}/Tool_${versie}_x64-setup.exe`,
  assetSize: 1,
  assetDigest: "sha256:00",
  pageUrl: null,
  publishedAt: null,
})

const desktop: Tool = { id: "t", naam: "Tool", soort: "desktop", categorie: "cad", repo: "OpenAEC-Foundation/tool", omschrijving: "" }
const web: Tool = { id: "w", naam: "Web", soort: "web", categorie: "bim", webUrl: "https://example.org/", omschrijving: "" }
const geinstalleerd = (versie: string | null) => ({ id: "t", displayName: "Tool", version: versie, exePath: "C:\\t.exe", uninstall: null })

describe("versies vergelijken", () => {
  it("vergelijkt numeriek per segment", () => {
    assert.equal(vergelijkVersies("0.10.0", "0.9.0"), 1)
    assert.equal(vergelijkVersies("2026.7.10", "2026.7.9"), 1)
    assert.equal(vergelijkVersies("1.2.0", "1.2.1"), -1)
  })
  it("negeert een v-voorvoegsel en ontbrekende segmenten", () => {
    assert.equal(vergelijkVersies("v1.2", "1.2.0"), 0)
    assert.equal(vergelijkVersies("1.0", "1.0.1"), -1)
  })
})

describe("status per tool", () => {
  it("webtools zijn altijd 'web'", () => {
    assert.equal(statusVan(web, undefined, undefined), "web")
  })
  it("niet geïnstalleerd, actueel, update en onbekend", () => {
    assert.equal(statusVan(desktop, undefined, release("t", "1.0.0")), "niet_geinstalleerd")
    assert.equal(statusVan(desktop, geinstalleerd("1.0.0"), release("t", "1.0.0")), "actueel")
    assert.equal(statusVan(desktop, geinstalleerd("1.1.0"), release("t", "1.0.0")), "actueel")
    assert.equal(statusVan(desktop, geinstalleerd("0.9.0"), release("t", "1.0.0")), "update")
    assert.equal(statusVan(desktop, geinstalleerd("0.9.0"), release("t", null, false)), "geinstalleerd_onbekend")
    assert.equal(statusVan(desktop, geinstalleerd(null), release("t", "1.0.0")), "geinstalleerd_onbekend")
  })
})

describe("installeren", () => {
  it("alleen desktop-tools uit de eigen organisatie, met een asset, als er iets te doen is", () => {
    assert.equal(kanInstalleren(desktop, "niet_geinstalleerd", release("t", "1.0.0")), true)
    assert.equal(kanInstalleren(desktop, "update", release("t", "1.0.0")), true)
    assert.equal(kanInstalleren(desktop, "actueel", release("t", "1.0.0")), false)
    assert.equal(kanInstalleren(desktop, "niet_geinstalleerd", release("t", null, false)), false)
    const community = { ...desktop, repo: "iemand/tool", community: true }
    assert.equal(eigenRepo(community), false)
    assert.equal(kanInstalleren(community, "niet_geinstalleerd", release("t", "1.0.0")), false)
  })
})

describe("release-informatie samenvoegen", () => {
  it("een mislukte ronde wist eerder opgehaalde versies niet", () => {
    const bekend = { t: release("t", "1.0.0") }
    const na = voegSamen(bekend, [release("t", null, false), release("u", null, false)])
    assert.equal(na.t.version, "1.0.0")
    assert.equal(na.u.ok, false)
    assert.equal(voegSamen(bekend, [release("t", "1.1.0")]).t.version, "1.1.0")
  })
})

describe("catalogus", () => {
  it("verborgen tools staan nergens in de zichtbare lijsten", () => {
    const verborgen = ALLE_TOOLS.filter((t) => t.verborgen).map((t) => t.id)
    for (const t of [...TOOLS, ...DESKTOP_TOOLS, ...WEB_TOOLS]) assert.ok(!verborgen.includes(t.id))
  })
  it("ids zijn uniek en desktop-tools hebben een repo en registernaam", () => {
    const ids = ALLE_TOOLS.map((t) => t.id)
    assert.equal(new Set(ids).size, ids.length)
    for (const t of DESKTOP_TOOLS) assert.ok(t.repo && t.registerNaam, t.id)
    for (const t of WEB_TOOLS) assert.ok(t.webUrl, t.id)
  })
  it("zoeken op naam, omschrijving en categorie", () => {
    const pdf = ALLE_TOOLS.find((t) => t.id === "open-pdf-studio")!
    assert.ok(zoekTreffer(pdf, "pdf", "Documenten"))
    assert.ok(zoekTreffer(pdf, "annoteren", "Documenten"))
    assert.ok(zoekTreffer(pdf, "docu", "Documenten"))
    assert.ok(!zoekTreffer(pdf, "sondering", "Documenten"))
  })
})
