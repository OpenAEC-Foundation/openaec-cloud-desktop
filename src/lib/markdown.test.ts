// Tests voor de markdown van de assistent. Draaien met `npm test`.

import { describe, it } from "node:test"
import assert from "node:assert/strict"
import { parseInline, parseMarkdown, platteTekst, veiligeLink, type Blok } from "./markdown.ts"

const t = (tekst: string) => ({ soort: "tekst", tekst }) as const

describe("blokken", () => {
  it("kopjes, alinea's met regelbreuk en een lijn", () => {
    assert.deepEqual(parseMarkdown("# Titel\n\nEerste regel\ntweede regel\n\n---\n### Klein ###"), [
      { soort: "kop", niveau: 1, inhoud: [t("Titel")] },
      { soort: "alinea", inhoud: [t("Eerste regel"), { soort: "breuk" }, t("tweede regel")] },
      { soort: "lijn" },
      { soort: "kop", niveau: 3, inhoud: [t("Klein")] },
    ])
  })

  it("codeblok met taal, ook als hij nog niet dicht is", () => {
    assert.deepEqual(parseMarkdown("```python\nprint('<b>')\n\nx = 1\n```\nNa"), [
      { soort: "code", taal: "python", tekst: "print('<b>')\n\nx = 1" },
      { soort: "alinea", inhoud: [t("Na")] },
    ])
    assert.deepEqual(parseMarkdown("Zo:\n```ts\nconst a = 1"), [
      { soort: "alinea", inhoud: [t("Zo:")] },
      { soort: "code", taal: "ts", tekst: "const a = 1" },
    ])
  })

  it("lijsten: ongeordend, geordend met start, genest en met vervolgregels", () => {
    const [lijst] = parseMarkdown("- een\n- twee\n  - genest\n- drie\n  vervolg") as Extract<Blok, { soort: "lijst" }>[]
    assert.equal(lijst.soort, "lijst")
    assert.equal(lijst.geordend, false)
    assert.equal(lijst.items.length, 3)
    assert.deepEqual(lijst.items[0], [{ soort: "alinea", inhoud: [t("een")] }])
    assert.equal(lijst.items[1][1].soort, "lijst")
    assert.deepEqual(lijst.items[2], [{ soort: "alinea", inhoud: [t("drie"), { soort: "breuk" }, t("vervolg")] }])

    const [geordend] = parseMarkdown("3. drie\n4. vier\n\nTekst erna") as Extract<Blok, { soort: "lijst" }>[]
    assert.equal(geordend.geordend, true)
    assert.equal(geordend.start, 3)
    assert.equal(geordend.items.length, 2)
    assert.equal(parseMarkdown("3. drie\n4. vier\n\nTekst erna")[1].soort, "alinea")
    // Een lijst direct na een alinea.
    assert.deepEqual(parseMarkdown("Stappen:\n1. a\n2. b").map((b) => b.soort), ["alinea", "lijst"])
  })

  it("tabel met uitlijning en lege cellen", () => {
    const [tabel] = parseMarkdown("| Post | Bedrag |\n|:--|--:|\n| Fundering | € 1.250 |\n| Dak |") as Extract<Blok, { soort: "tabel" }>[]
    assert.equal(tabel.soort, "tabel")
    assert.deepEqual(tabel.kop, [[t("Post")], [t("Bedrag")]])
    assert.deepEqual(tabel.uitlijning, ["links", "rechts"])
    assert.deepEqual(tabel.rijen, [
      [[t("Fundering")], [t("€ 1.250")]],
      [[t("Dak")], []],
    ])
  })

  it("citaat", () => {
    assert.deepEqual(parseMarkdown("> Let op\n> **vet**"), [
      { soort: "citaat", blokken: [{ soort: "alinea", inhoud: [t("Let op"), { soort: "breuk" }, { soort: "vet", inhoud: [t("vet")] }] }] },
    ])
  })

  it("HTML blijft letterlijk tekst", () => {
    assert.deepEqual(parseMarkdown('<script>alert("x")</script> <img src=x onerror=alert(1)>'), [
      { soort: "alinea", inhoud: [t('<script>alert("x")</script> <img src=x onerror=alert(1)>')] },
    ])
  })
})

describe("inline", () => {
  it("vet, cursief, doorgehaald en code", () => {
    assert.deepEqual(parseInline("**vet** en *cursief*, _ook_ ~~weg~~ `a*b*c`"), [
      { soort: "vet", inhoud: [t("vet")] },
      t(" en "),
      { soort: "cursief", inhoud: [t("cursief")] },
      t(", "),
      { soort: "cursief", inhoud: [t("ook")] },
      t(" "),
      { soort: "doorgehaald", inhoud: [t("weg")] },
      t(" "),
      { soort: "code", tekst: "a*b*c" },
    ])
    assert.deepEqual(parseInline("***beide***"), [{ soort: "vet", inhoud: [{ soort: "cursief", inhoud: [t("beide")] }] }])
    assert.deepEqual(parseInline("**vet met *cursief* erin**"), [
      { soort: "vet", inhoud: [t("vet met "), { soort: "cursief", inhoud: [t("cursief")] }, t(" erin")] },
    ])
  })

  it("losse markeringen en snake_case blijven tekst", () => {
    assert.deepEqual(parseInline("2 * 3 * 4 en een_naam_hier en **open"), [t("2 * 3 * 4 en een_naam_hier en **open")])
    assert.deepEqual(parseInline("\\*geen\\* cursief"), [t("*geen* cursief")])
  })

  it("links: alleen http(s) en mailto", () => {
    assert.deepEqual(parseInline("[OpenAEC](https://open-aec.eu) en [kwaad](javascript:alert(1))"), [
      { soort: "link", href: "https://open-aec.eu", inhoud: [t("OpenAEC")] },
      t(" en "),
      t("kwaad"),
    ])
    assert.deepEqual(parseInline("Zie https://open-aec.eu/docs."), [
      t("Zie "),
      { soort: "link", href: "https://open-aec.eu/docs", inhoud: [t("https://open-aec.eu/docs")] },
      t("."),
    ])
    assert.deepEqual(parseInline("<mailto:info@open-aec.eu>"), [
      { soort: "link", href: "mailto:info@open-aec.eu", inhoud: [t("info@open-aec.eu")] },
    ])
    assert.equal(veiligeLink("data:text/html,<b>"), null)
    assert.equal(veiligeLink(" HTTPS://x.nl "), "HTTPS://x.nl")
  })

  it("platte tekst", () => {
    assert.equal(platteTekst(parseInline("**Fundering** op `staal`\nklaar")), "Fundering op staal\nklaar")
  })
})
