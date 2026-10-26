// Tests voor de hulpfuncties van het deelvenster. Draaien met `npm test`.

import { describe, it } from "node:test"
import assert from "node:assert/strict"
import {
  collegaTekst, controleerVervaldatum, datumTekst, datumWaarde, delingDetails, delingVerlopen, geldigEmail, isVerlopen, leesFout,
  gedeeldMeta, linkRechten, linkUitleg, rang, rechtenTekst, rolTekst, toegestaneRechten, vervaldatumTekst,
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
  it("legt de link voor externen uit: zonder account en zonder de rest van de ruimte", () => {
    assert.match(linkUitleg(true, "lezen"), /deze map en alles erin.*zonder account en zonder toegang tot de rest van de ruimte/)
    assert.match(linkUitleg(false, "lezen"), /dit bestand/)
    assert.match(linkUitleg(true, "uploaden"), /bestanden in deze map zetten/)
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

describe("velden van de server", () => {
  it("gebruikt verlopen van de server, en rekent zelf zonder", () => {
    assert.equal(delingVerlopen({ verlopen: true, verloopt: null }, nu), true)
    assert.equal(delingVerlopen({ verlopen: false, verloopt: "2026-01-01" }, nu), false)
    assert.equal(delingVerlopen({ verloopt: "2026-10-22 23:59:59" }, nu), true)
    assert.equal(delingVerlopen({ verlopen: null, verloopt: "2026-10-23" }, nu), false)
  })
  it("noemt wachtwoord en maker als de server ze geeft", () => {
    assert.equal(delingDetails({ beveiligd: true, gemaakt_door: "a@x.nl", gemaakt_door_naam: "Anna" }), "met wachtwoord · door Anna")
    assert.equal(delingDetails({ beveiligd: false, gemaakt_door: "a@x.nl" }), "door a@x.nl")
    assert.equal(delingDetails({}), "")
  })
  it("beschrijft een suggestie met naam en rol", () => {
    assert.equal(collegaTekst({ gebruiker: "p@x.nl", naam: "Piet", lid_als: "lezer" }), "Piet · lid: kan bekijken")
    assert.equal(collegaTekst({ gebruiker: "p@x.nl", naam: "p@x.nl", lid_als: null }), "")
  })
})

describe("beheren en gedeeld met mij", () => {
  it("noemt beheren en houdt de volgorde lezen < schrijven < beheren", () => {
    assert.equal(rechtenTekst("gebruiker", "beheren"), "Kan beheren")
    assert.ok(rang("lezen") < rang("schrijven") && rang("schrijven") < rang("beheren"))
    assert.equal(rang(null), 0)
  })
  it("biedt nooit meer dan je zelf mag geven", () => {
    assert.deepEqual(toegestaneRechten("schrijven").map((k) => k.waarde), ["lezen", "schrijven"])
    assert.deepEqual(toegestaneRechten("beheren").map((k) => k.waarde), ["lezen", "schrijven", "beheren"])
    assert.deepEqual(toegestaneRechten("lezen").map((k) => k.waarde), ["lezen"])
    assert.equal(toegestaneRechten("beheren")[2].tekst, "Kan beheren (mag ook delen)")
  })
  it("vat een gedeeld item samen: wie, rol, vervaldatum en lidmaatschap", () => {
    assert.equal(
      gedeeldMeta({ gedeeld_door: "anna@x.nl", gedeeld_door_naam: "Anna Bakker", rechten: "schrijven", verloopt: "2026-11-22 23:59:59" }, nu),
      "door Anna Bakker · Kan bewerken · Geldig tot 22 nov 2026",
    )
    assert.equal(gedeeldMeta({ gedeeld_door: "anna@x.nl", rechten: "beheren", al_lid: true, rol: "lezer" }, nu), "door anna@x.nl · Kan beheren · ook lid: kan bekijken")
    assert.equal(gedeeldMeta({ rechten: "lezen" }, nu), "Kan bekijken")
  })
})
