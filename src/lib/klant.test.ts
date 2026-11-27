// Tests voor Delen met klant in het venster Versturen. Draaien met `npm test`.

import { describe, it } from "node:test"
import assert from "node:assert/strict"
import { gedeeldTekst, leveringMeta, voortzetbaar, type KlantLevering } from "./klant.ts"

const levering = (extra: Partial<KlantLevering> = {}): KlantLevering => ({
  datadrop: "d1",
  titel: "DO",
  status: "Actief",
  versie: 1,
  ontvangers: [],
  url: "https://klant.test/datadrop/abc",
  project_naam: null,
  verloopt_op: null,
  opmerkingen: 0,
  ...extra,
})

describe("Delen met klant", () => {
  it("alleen een concept of actieve levering kan een nieuwe versie krijgen", () => {
    const lijst = [levering(), levering({ datadrop: "d2", status: "Concept" }), levering({ datadrop: "d3", status: "Ingetrokken" }), levering({ datadrop: "d4", status: "Verlopen" })]
    assert.deepEqual(voortzetbaar(lijst).map((l) => l.datadrop), ["d1", "d2"])
  })

  it("vat een levering samen", () => {
    assert.equal(leveringMeta(levering({ status: "Concept" })), "concept, nog niet gedeeld · zonder model")
    assert.equal(
      leveringMeta(levering({ versie: 3, project_naam: "Lindelaan", ontvangers: ["jan@klant.nl"], opmerkingen: 1 })),
      "actief · versie 3 · model Lindelaan · gedeeld met jan@klant.nl · 1 opmerking",
    )
  })

  it("zegt waar de mail heen ging", () => {
    assert.equal(gedeeldTekst({ status: "Actief", url: "x", ontvangers: ["a@b.nl"], mail: ["a@b.nl"] }), "Gedeeld: een mail met de link ging naar a@b.nl.")
    assert.equal(gedeeldTekst({ status: "Actief", url: "x", ontvangers: [], mail: [] }), "Gedeeld. Kopieer de link om hem zelf te sturen.")
  })
})
