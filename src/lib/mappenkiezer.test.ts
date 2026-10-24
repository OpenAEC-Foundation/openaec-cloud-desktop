// Tests voor "Mappen kiezen…" (selectieve synchronisatie). Draaien met `npm test`.

import { describe, it } from "node:test"
import assert from "node:assert/strict"
import { boom, isUit, nieuwUit, ouderUit, wissel, zichtbaar } from "./mappenkiezer.ts"

const m = (pad: string, bytes = 0) => ({ pad, bytes, bestanden: 0 })

describe("mappenboom", () => {
  it("zet elke map direct na zijn bovenliggende map, per niveau op naam", () => {
    const b = boom([m("Tekeningen/2026"), m("Archief"), m("Tekeningen"), m("tekeningen-oud"), m("Archief/a")])
    assert.deepEqual(b.map((r) => r.pad), ["Archief", "Archief/a", "Tekeningen", "Tekeningen/2026", "tekeningen-oud"])
    assert.deepEqual(b.map((r) => r.diepte), [0, 1, 0, 1, 0])
    assert.deepEqual(b.map((r) => r.kinderen), [true, false, true, false, false])
    assert.equal(b[3].naam, "2026")
  })

  it("klapt submappen in", () => {
    const b = boom([m("A"), m("A/B"), m("A/B/C"), m("D")])
    assert.deepEqual(zichtbaar(b, new Set(["A"])).map((r) => r.pad), ["A", "D"])
    assert.deepEqual(zichtbaar(b, new Set(["A/B"])).map((r) => r.pad), ["A", "A/B", "D"])
  })
})

describe("vinkjes", () => {
  it("een uitgevinkte map neemt zijn submappen mee", () => {
    let uit: string[] = []
    uit = wissel(uit, "A/B")
    assert.deepEqual(uit, ["A/B"])
    uit = wissel(uit, "A")
    assert.deepEqual(uit, ["A"])
    assert.ok(isUit(uit, "A/B/C"))
    assert.ok(ouderUit(uit, "A/B"))
    assert.ok(!ouderUit(uit, "A"))
    assert.ok(!isUit(uit, "AB"))
    uit = wissel(uit, "A")
    assert.deepEqual(uit, [])
  })

  it("weet wat er nieuw uitgevinkt is (wordt hier opgeruimd)", () => {
    assert.deepEqual(nieuwUit(["A"], ["A", "B"]), ["B"])
    assert.deepEqual(nieuwUit(["A"], ["A/B"]), [])
    assert.deepEqual(nieuwUit([], []), [])
  })
})
