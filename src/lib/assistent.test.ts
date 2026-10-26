// Tests voor de hulpfuncties van de assistent. Draaien met `npm test`.

import { describe, it } from "node:test"
import assert from "node:assert/strict"
import {
  alsFout, datumTekst, gebruikTekst, kiesModel, modelLabel, nieuwGesprek, nieuwId, tekstInBijlagen, titelVan, verwerk,
  type Bericht, type Bijlage,
} from "./assistent.ts"

describe("gesprekken", () => {
  it("een titel uit de eerste vraag", () => {
    assert.equal(titelVan("  \n# Wat is een **paalfundering**?\nMeer"), "Wat is een paalfundering?")
    assert.equal(titelVan("", [{ naam: "rapport.pdf", grootte: 1, soort: "tekst", tekst: "x" }]), "rapport.pdf")
    assert.equal(titelVan(""), "Nieuw gesprek")
    const lang = titelVan("Bereken de doorbuiging van een stalen ligger van zes meter met een gelijkmatig verdeelde belasting")
    assert.ok(lang.endsWith("…") && lang.length <= 61, lang)
    assert.ok(!lang.includes(" …"))
  })

  it("een id en een nieuw gesprek", () => {
    assert.match(nieuwId(1_700_000_000_000, 0.5), /^[a-z0-9]+-[a-z0-9]{6}$/)
    assert.notEqual(nieuwId(1, 0.1), nieuwId(1, 0.2))
    const g = nieuwGesprek("claude-haiku-5-5", 42)
    assert.deepEqual(g, { id: g.id, titel: "Nieuw gesprek", model: "claude-haiku-5-5", aangemaakt: 42, bijgewerkt: 42, berichten: [] })
  })

  it("telt de tekst in bijlagen, niet de afbeeldingen", () => {
    const tekst: Bijlage = { naam: "a.txt", grootte: 3, soort: "tekst", tekst: "één" }
    const beeld: Bijlage = { naam: "a.png", grootte: 9, soort: "afbeelding", data_url: "data:", breedte: 1, hoogte: 1 }
    const berichten: Bericht[] = [{ rol: "user", tekst: "x", bijlagen: [tekst, beeld] }, { rol: "assistant", tekst: "y" }]
    assert.equal(tekstInBijlagen(berichten, [tekst]), 6)
  })
})

describe("opmaak", () => {
  it("verbruik in tokens", () => {
    assert.equal(gebruikTekst({ prompt_tokens: 1234, completion_tokens: 56, total_tokens: 1290, cached_tokens: 0 }), "1.234 tokens in, 56 uit")
    assert.equal(
      gebruikTekst({ prompt_tokens: 2000, completion_tokens: 5, total_tokens: 2005, cached_tokens: 1500 }),
      "2.000 tokens in (1.500 uit de cache), 5 uit",
    )
    assert.equal(gebruikTekst(null), "")
  })

  it("datum van een gesprek", () => {
    const nu = new Date(2026, 9, 26, 15, 0)
    assert.equal(datumTekst(new Date(2026, 9, 26, 14, 5).getTime(), nu), "vandaag 14:05")
    assert.equal(datumTekst(new Date(2026, 9, 25, 9, 12).getTime(), nu), "gisteren 09:12")
    assert.match(datumTekst(new Date(2026, 9, 12).getTime(), nu), /^12 okt/)
    assert.match(datumTekst(new Date(2025, 0, 3).getTime(), nu), /2025/)
  })

  it("modellen: label met EU en de standaardkeuze van de server", () => {
    const modellen = [
      { id: "claude-sonnet-5-5", naam: "Claude Sonnet 5.5", datalabel: "EU", standaard: false },
      { id: "claude-haiku-5-5", naam: "Claude Haiku 5.5", datalabel: "EU", standaard: true },
    ]
    assert.equal(modelLabel(modellen[0]), "Claude Sonnet 5.5 · EU")
    assert.equal(modelLabel({ id: "x", standaard: false }), "x")
    assert.equal(kiesModel(modellen, null), "claude-haiku-5-5")
    assert.equal(kiesModel(modellen, "claude-sonnet-5-5"), "claude-sonnet-5-5")
    assert.equal(kiesModel(modellen, "bestaat-niet"), "claude-haiku-5-5")
    assert.equal(kiesModel([], "x"), null)
  })

  it("fouten van een opdracht", () => {
    assert.deepEqual(alsFout({ soort: "plafond", melding: "Plafond", retry_na: 60 }), { soort: "plafond", melding: "Plafond", retry_na: 60 })
    assert.deepEqual(alsFout("a.pdf: te groot"), { soort: "anders", melding: "a.pdf: te groot" })
  })

  it("een antwoord stukje voor stukje", () => {
    let a: Bericht = { rol: "assistant", tekst: "" }
    a = verwerk(a, { soort: "tekst", tekst: "Hal" })
    a = verwerk(a, { soort: "tekst", tekst: "lo" })
    a = verwerk(a, { soort: "denkblokken", denkblokken: "b1.x" })
    a = verwerk(a, { soort: "einde", reden: "stop" })
    a = verwerk(a, { soort: "gebruik", gebruik: { prompt_tokens: 1, completion_tokens: 2, total_tokens: 3, cached_tokens: 0 } })
    assert.deepEqual(a, {
      rol: "assistant",
      tekst: "Hallo",
      denkblokken: "b1.x",
      gebruik: { prompt_tokens: 1, completion_tokens: 2, total_tokens: 3, cached_tokens: 0 },
    })
    assert.match(verwerk(a, { soort: "einde", reden: "length" }).fout ?? "", /afgekapt/)
  })
})
