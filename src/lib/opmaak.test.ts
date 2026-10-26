// Tests voor de opmaak van opslag, rondevoortgang en tijden. Draaien met `npm test`.

import { describe, it } from "node:test"
import assert from "node:assert/strict"
import { besparingTekst, duur, gb, misluktTekst, quotumTekst, resterendTekst, rondeTekst } from "./opmaak.ts"

const MB = 1048576
const GB = 1073741824

describe("GB en tijd", () => {
  it("toont GB met hooguit twee decimalen en een komma", () => {
    assert.equal(gb(0), "0 GB")
    assert.equal(gb(GB / 4), "0,25 GB")
    assert.equal(gb(1.2 * GB), "1,2 GB")
    assert.equal(gb(12.34 * GB), "12,3 GB")
    assert.equal(gb(250.4 * GB), "250 GB")
  })
  it("toont resterende tijd in seconden, minuten en uren", () => {
    assert.equal(duur(45), "45 s")
    assert.equal(duur(180), "3 min")
    assert.equal(duur(3900), "1 u 5 min")
    assert.equal(duur(7200), "2 u")
    assert.equal(duur(null), "")
  })
})

describe("opslag", () => {
  it("met en zonder limiet", () => {
    assert.equal(quotumTekst(3 * GB, 10 * GB), "3 GB van 10 GB gebruikt (7 GB vrij)")
    assert.equal(quotumTekst(12 * GB, 10 * GB), "12 GB van 10 GB gebruikt (0 GB vrij)")
    assert.equal(quotumTekst(1.5 * GB, null), "1,5 GB gebruikt (geen limiet)")
  })
})

describe("ronde en besparing", () => {
  it("vat de ronde samen met snelheid en resterende tijd", () => {
    const r = { nog: 3, bytes_gedaan: 1.2 * GB, bytes_totaal: 4 * GB, snelheid: 12.5 * MB, resterend: 240 }
    assert.equal(rondeTekst(r), "nog 3 bestanden, 1,2 GB van 4 GB · 12,5 MB/s · nog ongeveer 4 minuten")
    assert.equal(rondeTekst({ ...r, nog: 1, snelheid: 0, resterend: null }), "nog 1 bestand, 1,2 GB van 4 GB · bezig met voorbereiden…")
  })
  it("rondt de resterende tijd grof af", () => {
    assert.equal(resterendTekst(null), "bezig met voorbereiden…")
    assert.equal(resterendTekst(0), "nog minder dan een minuut")
    assert.equal(resterendTekst(59), "nog minder dan een minuut")
    assert.equal(resterendTekst(60), "nog ongeveer 1 minuut")
    assert.equal(resterendTekst(185), "nog ongeveer 3 minuten")
    assert.equal(resterendTekst(9 * 60 + 20), "nog ongeveer 9 minuten")
    assert.equal(resterendTekst(23 * 60), "nog ongeveer 25 minuten")
    assert.equal(resterendTekst(41 * 60), "nog ongeveer 40 minuten")
    assert.equal(resterendTekst(50 * 60), "nog ongeveer een uur")
    assert.equal(resterendTekst(80 * 60), "nog ruim een uur")
    assert.equal(resterendTekst(150 * 60), "nog ongeveer 3 uur")
    assert.equal(resterendTekst(30 * 3600), "nog meer dan een dag")
  })
  it("verandert niet bij kleine verschuivingen", () => {
    // Binnen dezelfde stap blijft de tekst gelijk, ook als de schatting een halve minuut verschuift.
    assert.equal(resterendTekst(23 * 60), resterendTekst(26 * 60))
    assert.equal(resterendTekst(80 * 60), resterendTekst(110 * 60))
  })
  it("meldt alleen een besparing als er minder verstuurd is dan het bestand groot is", () => {
    assert.equal(besparingTekst(1.2 * GB, 16 * MB), "alleen 16,0 MB van 1,20 GB verstuurd")
    assert.equal(besparingTekst(10 * MB, 10 * MB), "")
    assert.equal(besparingTekst(null, 5), "")
  })
})

describe("bestanden die niet lukken", () => {
  it("vat samen in één regel", () => {
    assert.equal(misluktTekst(0), "")
    assert.equal(misluktTekst(1), "1 bestand lukt niet")
    assert.equal(misluktTekst(3), "3 bestanden lukken niet")
  })
})
