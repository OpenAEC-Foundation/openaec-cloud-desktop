// Deelvenster: een bestand of map delen met een collega of met een link, en zien wie er toegang heeft. Opent vanuit
// "Delen…" in het rechtermuismenu van de Verkenner (met een lokaal pad) of vanuit de bestandsweergave van Ruimtes.

import { createEffect, createSignal, For, on, onCleanup, onMount, Show } from "solid-js"
import { api, type Deeldoel, type Deelverzoek, type DelingRegel, type Ruimtelid } from "./lib/api"
import {
  collegaRechten, controleerVervaldatum, datumWaarde, geldigEmail, isVerlopen, leesFout, linkRechten, rechtenTekst, rolTekst,
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
  const [lijstFout, setLijstFout] = createSignal<Deelfout | null>(null)
  const [intrekken, setIntrekken] = createSignal("")

  const [bezig, setBezig] = createSignal("")
  const [melding, setMelding] = createSignal("")

  const isMap = () => doel()?.doctype === "Opslagmap"
  const lid = () => leden().find((l) => l.gebruiker === collega().trim().toLowerCase())
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
      setLijstFout(leesFout(e))
    }
  }

  // Ander doel: delingen en leden van de ruimte ophalen. Lukt het ophalen van de leden niet (bijv. alleen toegang
  // via een deling), dan blijft de lijst leeg en typ je het e-mailadres zelf.
  createEffect(
    on(doel, (d) => {
      if (!d) return
      laadDelingen()
      api.ruimteleden(d.ruimte).then(setLeden).catch(() => setLeden([]))
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

        <Show when={doel()}>
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
                    setExtern("")
                    setCollegaFout(null)
                  }}
                  onKeyDown={(e) => e.key === "Enter" && collega() && !bezig() && delen()}
                />
              </label>
              <label>
                Rechten
                <select value={collegaRecht()} onChange={(e) => setCollegaRecht(e.currentTarget.value)}>
                  <For each={collegaRechten}>{(k) => <option value={k.waarde}>{k.tekst}</option>}</For>
                </select>
              </label>
              <button class="primair" disabled={!!bezig() || !collega().trim()} onClick={delen}>
                {bezig() === "collega" ? "Bezig…" : "Delen"}
              </button>
            </div>
            <datalist id="ruimteleden">
              <For each={leden()}>{(l) => <option value={l.gebruiker}>{rolTekst(l.rol)}</option>}</For>
            </datalist>
            <Show when={lid()}>
              {(l) => (
                <p class="hint">
                  Al lid van {doel()!.ruimte_naam} ({rolTekst(l().rol).toLowerCase()}).
                  {collegaRecht() === "schrijven" && !["beheerder", "schrijver"].includes(l().rol)
                    ? " Met Kan bewerken mag deze collega hier ook wijzigen."
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
                <p>Wil je het nu al delen? Maak hieronder een link, eventueel met een wachtwoord en een vervaldatum, en stuur die zelf door.</p>
              </div>
            </Show>
          </section>

          <section class="deel-sectie">
            <h2>Met een link</h2>
            <p class="uitleg">
              {linkRecht() === "uploaden"
                ? "Wie de link opent, kan bestanden in deze map zetten zonder account, maar ziet de inhoud niet."
                : `Iedereen met de link kan ${isMap() ? "de map" : "het bestand"} bekijken en downloaden, zonder account.`}
            </p>
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
                      <li classList={{ verlopen: isVerlopen(d.verloopt) }}>
                        <span class={`avatar ${d.soort}`} aria-hidden="true">
                          {d.soort === "link" ? "🔗" : (d.gebruiker ?? "?").slice(0, 1).toUpperCase()}
                        </span>
                        <div class="toegang-regel">
                          <span class="naam">
                            {d.soort === "link" ? (d.rechten === "uploaden" ? "Uploadlink" : "Iedereen met de link") : d.gebruiker}
                          </span>
                          <span class="meta">
                            {[rechtenTekst(d.soort, d.rechten), vervaldatumTekst(d.verloopt), d.gemaakt ? `gemaakt ${servertijd(d.gemaakt)}` : ""]
                              .filter(Boolean)
                              .join(" · ")}
                          </span>
                        </div>
                        <div class="knoppen">
                          <Show when={d.url}>
                            <button class="klein" onClick={() => kopieer(d.url!)}>
                              Kopiëren
                            </button>
                          </Show>
                          <button class="klein gevaar-licht" disabled={!!bezig()} onClick={() => trekIn(d)}>
                            {intrekken() === d.deling ? "Zeker intrekken?" : "Intrekken"}
                          </button>
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
