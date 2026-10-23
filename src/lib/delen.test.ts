// Tests voor de hulpfuncties van het deelvenster. Draaien met `npm test`.

import { describe, it } from "node:test"
import assert from "node:assert/strict"
import {
  controleerVervaldatum, datumTekst, datumWaarde, geldigEmail, isVerlopen, leesFout, linkRechten, rechtenTekst, rolTekst,
  vervaldatumTekst,
} from "./delen.ts"

const nu = new Date(2026, 9, 23, 14, 30)

describe("rechten", () => {
  it("noemt de rechten per soort deling", () => {
    assert.equal(rechtenTekst("gebruiker", "lezen"), "Kan bekijken")
    assert.equal(rechtenTekst("gebruiker", "schrijven"), "Kan bewerken")
    assert.equal(rechtenTekst("link", "lezen"), "Kan bekijken")
    assert.equal(rechtenTekst("link", "uploaden"), "Bestanden opvragen")
  })
  it("biedt bestanden opvragen alleen bij een map", () => {
    assert.deepEqual(linkRechten(false).map((k) => k.waarde), ["lezen"])
    assert.deepEqual(linkRechten(true).map((k) => k.waarde), ["lezen", "uploaden"])
  })
  it("vertaalt de rol van een lid", () => {
    assert.equal(rolTekst("schrijver"), "Kan bewerken")
    assert.equal(rolTekst("onbekend"), "onbekend")
  })
})

describe("vervaldatum", () => {
  it("toont de datum kort en in het Nederlands", () => {
    assert.equal(datumWaarde(nu), "2026-10-23")
    assert.equal(datumTekst("2026-12-31 23:59:59"), "31 dec 2026")
    assert.equal(datumTekst("2027-03-01"), "1 mrt 2027")
  })
  it("zegt of een deling nog geldig is", () => {
    assert.equal(vervaldatumTekst(null, nu), "Geen vervaldatum")
    assert.equal(vervaldatumTekst("2026-12-31 23:59:59", nu), "Geldig tot 31 dec 2026")
    assert.equal(vervaldatumTekst("2026-10-23 23:59:59", nu), "Verloopt vandaag")
    assert.equal(vervaldatumTekst("2026-10-01 23:59:59.000000", nu), "Verlopen op 1 okt 2026")
    assert.equal(isVerlopen("2026-10-22 23:59:59", nu), true)
    assert.equal(isVerlopen("2026-10-23 23:59:59", nu), false)
    assert.equal(isVerlopen(null, nu), false)
  })
  it("controleert het datumveld", () => {
    assert.equal(controleerVervaldatum("", nu), "")
    assert.equal(controleerVervaldatum("2026-10-23", nu), "")
    assert.equal(controleerVervaldatum("2027-01-15", nu), "")
    assert.equal(controleerVervaldatum("2026-10-22", nu), "De vervaldatum ligt in het verleden.")
    assert.equal(controleerVervaldatum("2026-02-30", nu), "Dit is geen geldige datum.")
    assert.equal(controleerVervaldatum("morgen", nu), "Dit is geen geldige datum.")
  })
})

describe("invoer en fouten", () => {
  it("herkent een geldig e-mailadres", () => {
    assert.equal(geldigEmail(" p.jansen@voorbeeld.nl "), true)
    for (const fout of ["", "jansen", "@voorbeeld.nl", "p@voorbeeld", "p@@voorbeeld.nl", "p jansen@voorbeeld.nl", "p@.nl"]) {
      assert.equal(geldigEmail(fout), false, fout)
    }
  })
  it("leest de fout van een deelcommand en van een gewone tekst", () => {
    assert.deepEqual(leesFout({ code: "ruimte_gearchiveerd", melding: "Gearchiveerd" }), { code: "ruimte_gearchiveerd", melding: "Gearchiveerd" })
    assert.deepEqual(leesFout("niet verbonden"), { code: "fout", melding: "niet verbonden" })
  })
})
