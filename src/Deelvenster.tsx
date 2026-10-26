// Deelvenster: een bestand of map delen met een collega of met een link, en zien wie er toegang heeft. Opent vanuit
// "Delen…" in het rechtermuismenu van de Verkenner (met een lokaal pad) of vanuit de bestandsweergave van Ruimtes.
// Met een collega delen kan als "Kan bekijken", "Kan bewerken" of "Kan beheren" (mag ook delen). Wat je zelf mag, zegt de
// server (`toegang`): je geeft nooit meer dan je zelf hebt, en de knoppen om te delen, de rol te wijzigen en in te trekken
// staan er alleen als je hier mag delen.

import { createEffect, createSignal, For, on, onCleanup, onMount, Show } from "solid-js"
import { api, type Collega, type Deeldoel, type Deelverzoek, type DelingRegel, type Ruimtelid, type Toegang } from "./lib/api"
import {
  collegaTekst, controleerVervaldatum, datumWaarde, delingDetails, delingVerlopen, geldigEmail, leesFout,
  linkRechten, linkUitleg, rang, rechtenTekst, rolTekst, toegestaneRechten,
  vervaldatumTekst, type Deelfout,
} from "./lib/delen"
import { naamVan, servertijd } from "./lib/opmaak"

/** Melding bij een fout (geen verbinding, geen rechten, gearchiveerde ruimte, …). */
function Foutmelding(props: { fout: Deelfout | null }) {
  return (
    <Show when={props.fout}>
      {(f) => <p class="foutvlak">{f().melding}</p>}
    </Show>
  )
}

export default function Deelvenster() {
  const [pad, setPad] = createSignal("")
  const [doel, setDoel] = createSignal<Deeldoel | null>(null)
  const [opzoekFout, setOpzoekFout] = createSignal<Deelfout | null>(null)

  // Met een collega
  const [collega, setCollega] = createSignal("")
  const [collegaRecht, setCollegaRecht] = createSignal("lezen")
  const [collegaFout, setCollegaFout] = createSignal<Deelfout | null>(null)
  const [extern, setExtern] = createSignal("")
  const [gedeeld, setGedeeld] = createSignal("")

  // Met een link
  const [linkRecht, setLinkRecht] = createSignal("lezen")
  const [wachtwoord, setWachtwoord] = createSignal("")
  const [verloopt, setVerloopt] = createSignal("")
  const [link, setLink] = createSignal("")
  const [linkFout, setLinkFout] = createSignal<Deelfout | null>(null)

  // Wie heeft toegang
  const [delingen, setDelingen] = createSignal<DelingRegel[] | null>(null)
  const [leden, setLeden] = createSignal<Ruimtelid[]>([])
  // Suggesties bij het e-mailadres: collega's van de omgeving (of, op een oudere server, de leden van de ruimte).
  const [suggesties, setSuggesties] = createSignal<Collega[]>([])
  const [lijstFout, setLijstFout] = createSignal<Deelfout | null>(null)
  const [intrekken, setIntrekken] = createSignal("")

  const [bezig, setBezig] = createSignal("")
  const [melding, setMelding] = createSignal("")

  // Wat ik hier mag (null: nog niet bekend). Tot het antwoord er is, gelden de oude regels: delen tot en met bewerken.
  const [toegang, setToegang] = createSignal<Toegang | null>(null)
  const [lijstVerborgen, setLijstVerborgen] = createSignal(false)
  const magDelen = () => toegang()?.mag_delen ?? true
  const keuzes = () => toegestaneRechten(toegang()?.max_rechten ?? "schrijven")
  /** Mag ik de rol van deze collega wijzigen? Alleen als ik hier deel en hij niet meer heeft dan ik mag geven. */
  const magWijzigen = (d: DelingRegel) =>
    magDelen() && d.soort === "gebruiker" && rang(d.rechten) <= rang(toegang()?.max_rechten ?? "schrijven")

  const isMap = () => doel()?.doctype === "Opslagmap"
  const lid = () => {
    const wie = collega().trim().toLowerCase()
    const uitLeden = leden().find((l) => l.gebruiker === wie)
    if (uitLeden) return uitLeden
    const s = suggesties().find((c) => c.gebruiker.toLowerCase() === wie && c.lid_als)
    return s ? { gebruiker: s.gebruiker, rol: s.lid_als! } : undefined
  }

  // Zoeken terwijl je typt, met een korte pauze; het laatste antwoord telt.
  let zoekTimer: ReturnType<typeof setTimeout> | undefined
  let zoekNummer = 0
  const zoekCollegas = (tekst: string) => {
    clearTimeout(zoekTimer)
    const d = doel()
    if (!d) return
    zoekTimer = setTimeout(() => {
      const nummer = ++zoekNummer
      api
        .collegas(d.ruimte, tekst.trim())
        .then((lijst) => nummer === zoekNummer && setSuggesties(lijst))
        .catch(() => nummer === zoekNummer && setSuggesties([]))
    }, 200)
  }
  const datumFout = () => controleerVervaldatum(verloopt())

  const opzoeken = async (p: string) => {
    setOpzoekFout(null)
    try {
      setDoel(await api.deeldoel(p))
    } catch (e) {
      setOpzoekFout(leesFout(e))
    }
  }

  /** Nieuw verzoek: alles terug naar het begin. */
  const verwerk = (v: Deelverzoek | null) => {
    if (!v) return
    setDoel(null)
    setOpzoekFout(null)
    setCollega("")
    setCollegaRecht("lezen")
    setCollegaFout(null)
    setExtern("")
    setGedeeld("")
    setLinkRecht("lezen")
    setWachtwoord("")
    setVerloopt("")
    setLink("")
    setLinkFout(null)
    setDelingen(null)
    setLeden([])
    setLijstFout(null)
    setIntrekken("")
    setMelding("")
    setToegang(null)
    setLijstVerborgen(false)
    if (v.doel) {
      setPad(v.doel.pad)
      setDoel(v.doel)
    } else if (v.pad) {
      setPad(v.pad)
      opzoeken(v.pad)
    }
  }

  const laadDelingen = async () => {
    const d = doel()
    if (!d) return
    try {
      setDelingen(await api.delingenVan(d))
      setLijstFout(null)
    } catch (e) {
      // Wie via een deling alleen bekijkt of bewerkt, ziet niet wie er nog meer toegang heeft.
      if (leesFout(e).code === "geen_beheerrecht") setDelingen(null)
      setLijstFout(leesFout(e).code === "geen_beheerrecht" ? null : leesFout(e))
      setLijstVerborgen(leesFout(e).code === "geen_beheerrecht")
    }
  }

  // Ander doel: delingen en leden van de ruimte ophalen. Lukt het ophalen van de leden niet (bijv. alleen toegang
  // via een deling), dan blijft de lijst leeg en typ je het e-mailadres zelf.
  createEffect(
    on(doel, async (d) => {
      if (!d) return
      api.ruimteleden(d.ruimte).then(setLeden).catch(() => setLeden([]))
      const t = await api.toegangVan(d).catch(() => null)
      if (doel() !== d) return
      setToegang(t)
      if (!keuzes().some((k) => k.waarde === collegaRecht())) setCollegaRecht("lezen")
      laadDelingen()
      if (magDelen()) zoekCollegas("")
    }),
  )

  onMount(async () => {
    verwerk(await api.delenVerzoek().catch(() => null))
    const stop = await api.opDelenVerzoek(verwerk)
    onCleanup(stop)
  })

  const doe = async (wat: string, f: () => Promise<unknown>) => {
    setBezig(wat)
    try {
      await f()
    } finally {
      setBezig("")
    }
  }

  const delen = () =>
    doe("collega", async () => {
      const wie = collega().trim().toLowerCase()
      setCollegaFout(null)
      setExtern("")
      setGedeeld("")
      if (!geldigEmail(wie)) return setCollegaFout({ code: "ongeldig_email", melding: "Vul een geldig e-mailadres in." })
      try {
        await api.deelMetCollega(doel()!, wie, collegaRecht(), "")
        setGedeeld(`Gedeeld met ${wie}: ${rechtenTekst("gebruiker", collegaRecht()).toLowerCase()}.`)
        setCollega("")
        laadDelingen()
      } catch (e) {
        const f = leesFout(e)
        if (f.code === "onbekende_gebruiker") setExtern(wie)
        else setCollegaFout(f)
      }
    })

  const maakLink = () =>
    doe("link", async () => {
      setLinkFout(null)
      setLink("")
      if (datumFout()) return setLinkFout({ code: "ongeldige_datum", melding: datumFout() })
      try {
        setLink(await api.maakLink(doel()!, linkRecht(), wachtwoord(), verloopt()))
        setMelding("Link staat op het klembord")
        setWachtwoord("")
        laadDelingen()
      } catch (e) {
        setLinkFout(leesFout(e))
      }
    })

  const kopieer = (tekst: string) =>
    api
      .kopieer(tekst)
      .then(() => setMelding("Link staat op het klembord"))
      .catch((e) => setMelding(leesFout(e).melding))

  /** De rol van een collega wijzigen (bekijken, bewerken of beheren). */
  const wijzigRol = (d: DelingRegel, rechten: string) => {
    if (rechten === d.rechten) return
    doe("wijzigen", async () => {
      try {
        await api.wijzigDeling(d.deling, rechten)
        setMelding(`${d.gebruiker}: ${rechtenTekst("gebruiker", rechten).toLowerCase()}`)
      } catch (e) {
        setLijstFout(leesFout(e))
      }
      await laadDelingen()
    })
  }

  /** Intrekken in twee klikken: de eerste vraagt om bevestiging. */
  const trekIn = (d: DelingRegel) => {
    if (intrekken() !== d.deling) return setIntrekken(d.deling)
    doe("intrekken", async () => {
      try {
        await api.trekDelingIn(d.deling)
        setIntrekken("")
        setMelding(d.soort === "link" ? "Link ingetrokken: hij werkt niet meer" : `Toegang van ${d.gebruiker} ingetrokken`)
        await laadDelingen()
      } catch (e) {
        setLijstFout(leesFout(e))
      }
    })
  }

  return (
    <div class="deel">
      <header class="deel-kop">
        <span class={`deel-icoon ${isMap() ? "map" : "bestand"}`} aria-hidden="true" />
        <div>
          <h1>{doel()?.naam ?? (naamVan(pad()) || "Delen")}</h1>
          <span class="pad" title={pad()}>
            {pad()}
          </span>
          <Show when={doel()}>
            {(d) => (
              <span class="deel-ruimte">
                {isMap() ? "Map" : "Bestand"} in {d().ruimte_naam}
              </span>
            )}
          </Show>
        </div>
      </header>

      <div class="deel-inhoud">
        <Show when={!doel()}>
          <Show when={opzoekFout()} fallback={<p class="hint">Opzoeken op de server…</p>}>
            <Foutmelding fout={opzoekFout()} />
            <div class="knoppen">
              <button onClick={() => opzoeken(pad())}>Opnieuw proberen</button>
              <button onClick={() => api.toonHoofdvenster()}>Hoofdvenster openen</button>
            </div>
          </Show>
        </Show>

        <Show when={doel() && !magDelen()}>
          <section class="deel-sectie">
            <h2>Je toegang</h2>
            <p class="uitleg">
              Jouw rol hier: {rechtenTekst("gebruiker", toegang()?.rechten ?? "lezen").toLowerCase()}. Verder delen mag wie{" "}
              {isMap() ? "deze map" : "dit bestand"} beheert; vraag het aan een beheerder of aan wie het met je deelde.
            </p>
          </section>
        </Show>

        <Show when={doel() && magDelen()}>
          <section class="deel-sectie snelle-link">
            <h2>Link voor externen (zonder account)</h2>
            <p class="uitleg">{linkUitleg(isMap(), linkRecht())}</p>
            <p class="hint">Staat al in OpenAEC: er wordt niets geüpload.</p>
            <Show when={isMap()}>
              <div class="keuzeknoppen" role="radiogroup" aria-label="Rechten van de link">
                <For each={linkRechten(true)}>
                  {(k) => (
                    <button
                      role="radio"
                      aria-checked={linkRecht() === k.waarde}
                      classList={{ gekozen: linkRecht() === k.waarde }}
                      onClick={() => setLinkRecht(k.waarde)}
                    >
                      {k.tekst}
                    </button>
                  )}
                </For>
              </div>
            </Show>
            <div class="velden">
              <label>
                Wachtwoord (optioneel)
                <input type="password" autocomplete="new-password" value={wachtwoord()} onInput={(e) => setWachtwoord(e.currentTarget.value)} />
              </label>
              <label>
                Geldig tot (optioneel)
                <input type="date" min={datumWaarde(new Date())} value={verloopt()} onInput={(e) => setVerloopt(e.currentTarget.value)} />
              </label>
            </div>
            <Show when={datumFout()}>
              <p class="hint fout">{datumFout()}</p>
            </Show>
            <button class="primair" disabled={!!bezig() || !!datumFout()} onClick={maakLink}>
              {bezig() === "link" ? "Bezig…" : "Link maken en kopiëren"}
            </button>
            <Foutmelding fout={linkFout()} />
            <Show when={link()}>
              <div class="deellink">
                <input readOnly value={link()} onFocus={(e) => e.currentTarget.select()} />
                <button onClick={() => kopieer(link())}>Kopiëren</button>
              </div>
            </Show>
          </section>

          <section class="deel-sectie">
            <h2>Met een collega</h2>
            <p class="uitleg">
              Iemand met een account in deze omgeving krijgt toegang tot {isMap() ? "deze map en alles erin" : "dit bestand"}.
            </p>
            <div class="deel-rij">
              <label class="groei">
                E-mailadres
                <input
                  type="email"
                  list="ruimteleden"
                  placeholder="naam@bedrijf.nl"
                  value={collega()}
                  onInput={(e) => {
                    setCollega(e.currentTarget.value)
                    zoekCollegas(e.currentTarget.value)
                    setExtern("")
                    setCollegaFout(null)
                  }}
                  onKeyDown={(e) => e.key === "Enter" && collega() && !bezig() && delen()}
                />
              </label>
              <label>
                Rechten
                <select value={collegaRecht()} onChange={(e) => setCollegaRecht(e.currentTarget.value)}>
                  <For each={keuzes()}>{(k) => <option value={k.waarde}>{k.tekst}</option>}</For>
                </select>
              </label>
              <button class="primair" disabled={!!bezig() || !collega().trim()} onClick={delen}>
                {bezig() === "collega" ? "Bezig…" : "Delen"}
              </button>
            </div>
            <datalist id="ruimteleden">
              <For each={suggesties()}>{(c) => <option value={c.gebruiker}>{collegaTekst(c)}</option>}</For>
            </datalist>
            <Show when={lid()}>
              {(l) => (
                <p class="hint">
                  Al lid van {doel()!.ruimte_naam} ({rolTekst(l().rol).toLowerCase()}).
                  {collegaRecht() === "schrijven" && !["beheerder", "schrijver"].includes(l().rol)
                    ? " Met Kan bewerken mag deze collega hier ook wijzigen."
                    : collegaRecht() === "beheren" && l().rol !== "beheerder"
                      ? " Met Kan beheren mag deze collega hier ook verder delen."
                      : ""}
                </p>
              )}
            </Show>
            <Foutmelding fout={collegaFout()} />
            <Show when={gedeeld()}>
              <p class="succesvlak">{gedeeld()}</p>
            </Show>
            <Show when={extern()}>
              <div class="extern">
                <h3>Extern iemand uitnodigen</h3>
                <p>
                  <strong>{extern()}</strong> heeft geen account in deze omgeving. Mensen van buiten je organisatie uitnodigen, met een
                  extra licentie, is nog in aanbouw.
                </p>
                <p>Wil je het nu al delen? Maak hierboven een link voor externen, eventueel met een wachtwoord en een vervaldatum, en stuur die zelf door.</p>
              </div>
            </Show>
          </section>

          <section class="deel-sectie" hidden={lijstVerborgen()}>
            <div class="deel-sectiekop">
              <h2>Wie heeft toegang</h2>
              <button class="tekstknop" onClick={laadDelingen}>
                Vernieuwen
              </button>
            </div>
            <Foutmelding fout={lijstFout()} />
            <Show
              when={delingen()}
              fallback={
                <Show when={!lijstFout()}>
                  <p class="hint">Ophalen…</p>
                </Show>
              }
            >
              {(lijst) => (
                <ul class="toegang">
                  <For each={lijst()} fallback={<li class="toegang-leeg">Nog niet gedeeld: alleen de leden van de ruimte hebben toegang.</li>}>
                    {(d) => (
                      <li classList={{ verlopen: delingVerlopen(d) }}>
                        <span class={`avatar ${d.soort}`} aria-hidden="true">
                          {d.soort === "link" ? "🔗" : (d.gebruiker ?? "?").slice(0, 1).toUpperCase()}
                        </span>
                        <div class="toegang-regel">
                          <span class="naam">
                            {d.soort === "link" ? (d.rechten === "uploaden" ? "Uploadlink" : "Iedereen met de link") : d.gebruiker}
                          </span>
                          <span class="meta">
                            {[
                              magWijzigen(d) ? "" : rechtenTekst(d.soort, d.rechten),
                              d.verlopen === true && !d.verloopt ? "Verlopen" : vervaldatumTekst(d.verloopt),
                              d.gemaakt ? `gemaakt ${servertijd(d.gemaakt)}` : "",
                              delingDetails(d),
                            ]
                              .filter(Boolean)
                              .join(" · ")}
                          </span>
                        </div>
                        <div class="knoppen">
                          <Show when={magWijzigen(d)}>
                            <select
                              class="klein"
                              aria-label={`Rol van ${d.gebruiker}`}
                              value={d.rechten}
                              disabled={!!bezig()}
                              onChange={(e) => wijzigRol(d, e.currentTarget.value)}
                            >
                              <For each={keuzes()}>{(k) => <option value={k.waarde}>{k.tekst}</option>}</For>
                            </select>
                          </Show>
                          <Show when={d.url}>
                            <button class="klein" onClick={() => kopieer(d.url!)}>
                              Kopiëren
                            </button>
                          </Show>
                          <Show when={magDelen()}>
                            <button class="klein gevaar-licht" disabled={!!bezig()} onClick={() => trekIn(d)}>
                              {intrekken() === d.deling ? "Zeker intrekken?" : "Intrekken"}
                            </button>
                          </Show>
                        </div>
                      </li>
                    )}
                  </For>
                </ul>
              )}
            </Show>
            <Show when={leden().length}>
              <details class="leden">
                <summary>
                  {leden().length === 1 ? "1 lid" : `${leden().length} leden`} van {doel()!.ruimte_naam}{" "}
                  {leden().length === 1 ? "heeft" : "hebben"} toegang via de ruimte
                </summary>
                <ul>
                  <For each={leden()}>
                    {(l) => (
                      <li>
                        <span>{l.gebruiker}</span>
                        <span class="meta">{rolTekst(l.rol)}</span>
                      </li>
                    )}
                  </For>
                </ul>
              </details>
            </Show>
          </section>
        </Show>
      </div>

      <footer class="deel-voet">
        <span class="melding">{melding()}</span>
        <button onClick={() => api.sluitDelen()}>Sluiten</button>
      </footer>
    </div>
  )
}
