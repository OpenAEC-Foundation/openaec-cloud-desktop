// Tests voor de hulpfuncties van het venster Versturen. Draaien met `npm test`.

import { describe, it } from "node:test"
import assert from "node:assert/strict"
import {
  controleerGeldigTot, controleerGrootte, controleerOntvangers, grootteDecimaal, intrekbaar, laatsteDag, overdrachtMeta,
  opslagTekst, samenvatting, splitsOntvangers, standaardNaam, stapTekst, statusTekst, totaal, voegSamen, type OverdrachtRegel, type VerstuurItem,
} from "./versturen.ts"

const nu = new Date(2026, 9, 26, 14, 30)

const item = (naam: string, grootte: number, map = false): VerstuurItem => ({
  pad: `C:\\Werk\\${naam}`, naam, map, grootte, bestanden: map ? 3 : 1, fout: null,
})

describe("ontvangers", () => {
  it("splitst op komma's, puntkomma's, spaties en regels, zonder dubbelen", () => {
    assert.deepEqual(splitsOntvangers(" a@x.nl, b@x.nl;c@x.nl\nA@X.nl  d@x.nl "), ["a@x.nl", "b@x.nl", "c@x.nl", "d@x.nl"])
    assert.deepEqual(splitsOntvangers("  "), [])
  })
  it("controleert de adressen en het aantal", () => {
    assert.equal(controleerOntvangers(["a@x.nl"], 20), "")
    assert.equal(controleerOntvangers(["a@x"], 20), "Dit is geen geldig e-mailadres: a@x")
    assert.match(controleerOntvangers(["a", "b@"], 20), /geen geldige e-mailadressen: a, b@/)
    assert.equal(controleerOntvangers(["a@x.nl", "b@x.nl", "c@x.nl"], 2), "Hoogstens 2 ontvangers per overdracht.")
  })
})

describe("geldig tot", () => {
  it("mag leeg zijn of van vandaag tot de laatste dag", () => {
    assert.equal(laatsteDag(30, nu), "2026-11-25")
    assert.equal(controleerGeldigTot("", 30, nu), "")
    assert.equal(controleerGeldigTot("2026-10-26", 30, nu), "")
    assert.equal(controleerGeldigTot("2026-11-25", 30, nu), "")
    assert.equal(controleerGeldigTot("2026-10-25", 30, nu), "De vervaldatum ligt in het verleden.")
    assert.equal(controleerGeldigTot("2026-11-26", 30, nu), "Een overdracht kan hoogstens 30 dagen geldig zijn (tot 25 nov 2026).")
    assert.equal(controleerGeldigTot("2026-02-30", 30, nu), "Dit is geen geldige datum.")
  })
})

describe("grootte", () => {
  it("noemt de grenzen zoals de server (GB = 10^9)", () => {
    assert.equal(grootteDecimaal(100_000_000_000), "100 GB")
    assert.equal(grootteDecimaal(1_500_000_000), "1,5 GB")
    assert.equal(grootteDecimaal(999), "999 B")
    assert.equal(grootteDecimaal(2_000_000), "2 MB")
  })
  it("waarschuwt boven de grootste overdracht", () => {
    assert.equal(controleerGrootte(10, 100), "")
    assert.equal(controleerGrootte(101_000_000_000, 100_000_000_000), "Samen 101 GB: een overdracht kan hoogstens 100 GB groot zijn.")
  })
})

describe("selectie", () => {
  it("stelt een naam voor", () => {
    assert.equal(standaardNaam([]), "")
    assert.equal(standaardNaam([item("plan.pdf", 1)]), "plan.pdf")
    assert.equal(standaardNaam([item("plan.pdf", 1), item("Tekeningen", 1, true)]), "plan.pdf en 1 ander")
    assert.equal(standaardNaam([item("a", 1), item("b", 1), item("c", 1)]), "a en 2 andere")
  })
  it("vat bestanden en mappen samen", () => {
    assert.equal(samenvatting([]), "Niets gekozen")
    assert.equal(samenvatting([item("a.pdf", 512), item("b.pdf", 512), item("Tekeningen", 1024, true)]), "2 bestanden en 1 map · 2 kB")
    assert.equal(samenvatting([item("Tekeningen", 0, true)]), "1 map · 0 B")
  })
  it("voegt een selectie uit de Verkenner samen tijdens het samenstellen", () => {
    assert.deepEqual(voegSamen(["a"], ["b", "a"], "samenstellen"), { paden: ["a", "b"], wachtend: [] })
    assert.deepEqual(voegSamen(["a"], ["b"], "klaar"), { paden: ["b"], wachtend: [] })
    assert.deepEqual(voegSamen(["a"], ["b"], "fout"), { paden: ["b"], wachtend: [] })
    assert.deepEqual(voegSamen(["a"], ["b"], "bezig"), { paden: ["a"], wachtend: ["b"] })
  })
})

describe("mijn overdrachten", () => {
  const regel: OverdrachtRegel = {
    overdracht: "OV1", naam: "Tekeningen", status: "Actief", verloopt_op: "2026-11-02 10:00:00", totale_grootte: 2048,
    aantal_bestanden: 2, aantal_downloads: 1, laatst_gedownload: null, ontvangers: ["a@x.nl"], beveiligd: true,
    url: "https://x/overdracht/T", gemaakt: null, voltooid_op: null,
  }
  it("noemt status en details", () => {
    assert.equal(statusTekst("Concept"), "Niet afgemaakt")
    assert.equal(statusTekst("Ingetrokken"), "Ingetrokken")
    assert.equal(overdrachtMeta(regel), "2 bestanden · 2 kB · 1 keer gedownload · geldig tot 2 nov 2026 · met wachtwoord · naar a@x.nl")
    assert.equal(
      overdrachtMeta({ ...regel, status: "Verlopen", aantal_downloads: 0, beveiligd: false, ontvangers: [], aantal_bestanden: 1 }),
      "1 bestand · 2 kB",
    )
  })
  it("trekt alleen actieve en niet afgemaakte in", () => {
    assert.ok(intrekbaar({ status: "Actief" }) && intrekbaar({ status: "Concept" }))
    assert.ok(!intrekbaar({ status: "Verlopen" }) && !intrekbaar({ status: "Ingetrokken" }))
  })
})

describe("voortgang", () => {
  it("telt de stappen samen en noemt elke stap", () => {
    assert.deepEqual(totaal([{ gedaan: 5, totaal: 10 }, { gedaan: 20, totaal: 10 }]), { gedaan: 15, totaal: 20 })
    assert.equal(stapTekst({ status: "bezig", gedaan: 1, totaal: 3, fout: null }), "33%")
    assert.equal(stapTekst({ status: "bezig", gedaan: 0, totaal: 0, fout: null }), "Bezig…")
    assert.equal(stapTekst({ status: "fout", gedaan: 0, totaal: 3, fout: "Geen verbinding" }), "Geen verbinding")
    assert.equal(stapTekst({ status: "inpakken", gedaan: 0, totaal: 0, fout: null }), "Map inpakken…")
    assert.equal(stapTekst({ status: "kopieren", gedaan: 0, totaal: 9, fout: null }), "Kopiëren in OpenAEC…")
  })
  it("zegt wat al in OpenAEC staat en niet opnieuw geüpload wordt", () => {
    const al = { ...item("plan.pdf", 3), in_opslag: true }
    assert.equal(opslagTekst([al, { ...item("Tekeningen", 9, true), in_opslag: true }]), "Staat al in OpenAEC: er wordt niets geüpload.")
    assert.equal(opslagTekst([al, item("nieuw.pdf", 1)]), "1 van 2 staan al in OpenAEC en worden niet opnieuw geüpload; de rest wordt geüpload.")
    assert.equal(opslagTekst([item("nieuw.pdf", 1)]), "")
  })
})
