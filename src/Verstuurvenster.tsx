// Venster Versturen: een of meer bestanden of mappen versturen met een downloadlink, eventueel met een e-mail aan de
// ontvangers, een bericht, een vervaldatum en een wachtwoord (binnen de grenzen van de server). Opent vanuit "Versturen
// met OpenAEC" in het rechtermuismenu van de bestandsbeheerder, op elk bestand en elke map. Een map gaat met al zijn
// inhoud mee als zip. Onderaan "Mijn overdrachten", met de status en Intrekken.

import { createSignal, For, onCleanup, onMount, Show } from "solid-js"
import { open } from "@tauri-apps/plugin-dialog"
import { api } from "./lib/api"
import { datumTekst, datumWaarde, leesFout, type Deelfout } from "./lib/delen"
import { grootte, naamVan } from "./lib/opmaak"
import {
  controleerGeldigTot, controleerGrootte, controleerOntvangers, grootteDecimaal, intrekbaar, laatsteDag, overdrachtMeta,
  samenvatting, splitsOntvangers, standaardNaam, stapTekst, statusTekst, totaal, voegSamen,
  type Fase, type Lopend, type OverdrachtRegel, type Overdrachtgrenzen, type VerstuurItem, type VerstuurStap, type Voltooid,
} from "./lib/versturen"

/** Melding bij een fout (geen verbinding, geen recht, grens overschreden, …). */
function Foutmelding(props: { fout: Deelfout | null }) {
  return (
    <Show when={props.fout}>
      {(f) => <p class="foutvlak">{f().melding}</p>}
    </Show>
  )
}

export default function Verstuurvenster() {
  const [paden, setPaden] = createSignal<string[]>([])
  const [items, setItems] = createSignal<VerstuurItem[]>([])
  const [wachtend, setWachtend] = createSignal<string[]>([])

  // Omgeving en grenzen: null = nog niet bekend.
  const [grenzen, setGrenzen] = createSignal<Overdrachtgrenzen | null>(null)
  const [aanmeldFout, setAanmeldFout] = createSignal<Deelfout | null>(null)

  const [naam, setNaam] = createSignal("")
  const [naamAangepast, setNaamAangepast] = createSignal(false)
  const [ontvangers, setOntvangers] = createSignal("")
  const [bericht, setBericht] = createSignal("")
  const [geldigTot, setGeldigTot] = createSignal("")
  const [wachtwoord, setWachtwoord] = createSignal("")

  const [fase, setFase] = createSignal<Fase>("samenstellen")
  const [lopend, setLopend] = createSignal<Lopend | null>(null)
  const [stappen, setStappen] = createSignal<VerstuurStap[]>([])
  const [uitkomst, setUitkomst] = createSignal<Voltooid | null>(null)
  const [fout, setFout] = createSignal<Deelfout | null>(null)
  const [onderbroken, setOnderbroken] = createSignal<Lopend | null>(null)

  const [mijn, setMijn] = createSignal<OverdrachtRegel[] | null>(null)
  const [mijnFout, setMijnFout] = createSignal<Deelfout | null>(null)
  const [intrekken, setIntrekken] = createSignal("")
  const [bezig, setBezig] = createSignal("")
  const [melding, setMelding] = createSignal("")

  const lijst = () => splitsOntvangers(ontvangers())
  const totaalBytes = () => items().reduce((s, i) => s + i.grootte, 0)
  const ontvangerFout = () => controleerOntvangers(lijst(), grenzen()?.max_ontvangers ?? 20)
  const datumFout = () => controleerGeldigTot(geldigTot(), grenzen()?.max_dagen ?? 30)
  const grootteFout = () => controleerGrootte(totaalBytes(), grenzen()?.max_bytes ?? 0)
  const ontbrekend = () => items().filter((i) => i.fout)
  const kanVersturen = () =>
    !!grenzen() && items().length > 0 && !ontbrekend().length && !ontvangerFout() && !datumFout() && !grootteFout() && !bezig()
  const voortgang = () => totaal(stappen())

  /** Wat er in de gekozen paden zit (en de naam, zolang de gebruiker hem niet zelf veranderde). */
  const werkItemsBij = async (nieuw: string[]) => {
    setPaden(nieuw)
    const gevonden = nieuw.length ? await api.versturenItems(nieuw).catch(() => [] as VerstuurItem[]) : []
    if (paden() !== nieuw) return
    setItems(gevonden)
    if (!naamAangepast()) setNaam(standaardNaam(gevonden))
  }

  const beginOpnieuw = (nieuw: string[]) => {
    setFase("samenstellen")
    setLopend(null)
    setStappen([])
    setUitkomst(null)
    setFout(null)
    setNaamAangepast(false)
    setOntvangers("")
    setBericht("")
    setGeldigTot("")
    setWachtwoord("")
    setWachtend([])
    werkItemsBij(nieuw)
  }

  /** Paden uit het rechtermuismenu (of uit de knoppen hieronder). */
  const ontvang = (nieuw: string[]) => {
    if (!nieuw.length) return
    const r = voegSamen(paden(), nieuw, fase())
    if (fase() === "bezig") return setWachtend((w) => [...w, ...r.wachtend.filter((p) => !w.includes(p))])
    if (fase() === "klaar" || fase() === "fout") return beginOpnieuw(r.paden)
    werkItemsBij(r.paden)
  }

  const laadMijn = async () => {
    try {
      setMijn(await api.mijnOverdrachten())
      setMijnFout(null)
    } catch (e) {
      setMijnFout(leesFout(e))
    }
  }

  const laadGrenzen = async () => {
    try {
      setGrenzen(await api.versturenGrenzen())
      setAanmeldFout(null)
      laadMijn()
      if (fase() === "samenstellen") setOnderbroken(await api.onderbrokenVersturen().catch(() => null))
    } catch (e) {
      setGrenzen(null)
      setAanmeldFout(leesFout(e))
    }
  }

  const volg = (l: Lopend) => {
    setLopend(l)
    setStappen(
      l.onderdelen.map((o, index) => ({
        overdracht: l.overdracht,
        index,
        status: o.klaar ? "klaar" : "wachten",
        gedaan: o.klaar ? o.grootte : 0,
        totaal: o.grootte,
        fout: null,
      })),
    )
    setFase("bezig")
    setFout(null)
    setOnderbroken(null)
  }

  const versturen = async () => {
    if (!kanVersturen()) return
    setBezig("versturen")
    setFout(null)
    try {
      volg(
        await api.startVersturen({
          paden: paden(),
          naam: naam().trim() || standaardNaam(items()),
          bericht: bericht().trim() || null,
          verloopt_op: geldigTot() || null,
          wachtwoord: wachtwoord() || null,
          ontvangers: lijst(),
        }),
      )
      setWachtwoord("")
    } catch (e) {
      setFout(leesFout(e))
      if (leesFout(e).code === "niet_ingelogd") laadGrenzen()
    } finally {
      setBezig("")
    }
  }

  const hervat = async () => {
    setFout(null)
    try {
      volg(await api.hervatVersturen())
    } catch (e) {
      setFout(leesFout(e))
    }
  }

  /** Stoppen: de overdracht blijft staan om later verder te gaan, of (met `intrekken`) gaat helemaal weg. */
  const stop = async (intrekken: boolean) => {
    setBezig("stoppen")
    try {
      await api.stopVersturen(intrekken)
      if (intrekken) {
        setOnderbroken(null)
        beginOpnieuw(paden())
        setMelding("Overdracht gestopt en ingetrokken")
      } else {
        setFase("fout")
        setFout({ code: "gestopt", melding: "Gepauzeerd. Je kunt later verder waar je was." })
        setOnderbroken(await api.onderbrokenVersturen().catch(() => null))
      }
      laadMijn()
    } catch (e) {
      setFout(leesFout(e))
    } finally {
      setBezig("")
    }
  }

  const kopieer = (tekst: string) =>
    api
      .kopieer(tekst)
      .then(() => setMelding("Link staat op het klembord"))
      .catch((e) => setMelding(leesFout(e).melding))

  /** Intrekken in twee klikken: de eerste vraagt om bevestiging. */
  const trekIn = async (r: OverdrachtRegel) => {
    if (intrekken() !== r.overdracht) return setIntrekken(r.overdracht)
    setBezig("intrekken")
    try {
      await api.trekOverdrachtIn(r.overdracht)
      setIntrekken("")
      setMelding(`${r.naam} ingetrokken: de link werkt niet meer`)
      if (onderbroken()?.overdracht === r.overdracht) setOnderbroken(null)
      if (lopend()?.overdracht === r.overdracht && fase() !== "klaar") beginOpnieuw(paden())
      await laadMijn()
    } catch (e) {
      setMijnFout(leesFout(e))
    } finally {
      setBezig("")
    }
  }

  const voegToe = async (map: boolean) => {
    const keuze = await open({ multiple: true, directory: map, title: map ? "Map toevoegen" : "Bestanden toevoegen" }).catch(() => null)
    if (!keuze) return
    ontvang(Array.isArray(keuze) ? keuze : [keuze])
  }

  const haalWeg = (pad: string) => werkItemsBij(paden().filter((p) => p !== pad))

  onMount(async () => {
    const stoppers = [
      await api.opVersturenVerzoek(ontvang),
      await api.opVersturenStap((s) => {
        if (s.overdracht !== lopend()?.overdracht) return
        setStappen((oud) => oud.map((x, i) => (i === s.index ? s : x)))
      }),
      await api.opVersturenKlaar((v) => {
        if (v.overdracht !== lopend()?.overdracht) return
        setUitkomst(v)
        setFase("klaar")
        setMelding("Link staat op het klembord")
        laadMijn()
      }),
      await api.opVersturenFout(({ overdracht, fout }) => {
        if (overdracht !== lopend()?.overdracht) return
        setFase("fout")
        setFout(fout)
        api.onderbrokenVersturen().then(setOnderbroken).catch(() => setOnderbroken(null))
        laadMijn()
      }),
    ]
    // Na inloggen in het hoofdvenster: opnieuw kijken zodra dit venster weer de focus krijgt.
    const bijFocus = () => !grenzen() && laadGrenzen()
    window.addEventListener("focus", bijFocus)
    onCleanup(() => {
      stoppers.forEach((s) => s())
      window.removeEventListener("focus", bijFocus)
    })
    await laadGrenzen()
    ontvang(await api.versturenVerzoek().catch(() => []))
  })

  const titel = () => (fase() === "samenstellen" ? naam() || "Versturen" : (lopend()?.naam ?? naam()))
  const ondertitel = () => {
    if (fase() === "samenstellen") return samenvatting(items())
    const n = lopend()?.onderdelen.length ?? 0
    return `${n} ${n === 1 ? "onderdeel" : "onderdelen"} · ${grootte(voortgang().totaal)}`
  }

  return (
    <div class="deel versturen">
      <header class="deel-kop">
        <span class="deel-icoon versturen" aria-hidden="true" />
        <div>
          <h1>{titel()}</h1>
          <span class="pad">{ondertitel()}</span>
          <span class="deel-ruimte">Versturen met OpenAEC</span>
        </div>
      </header>

      <div class="deel-inhoud">
        <Show when={aanmeldFout()}>
          {(f) => (
            <section class="deel-sectie">
              <h2>
                {f().code === "geen_recht" ? "Geen recht om te versturen" : f().code === "niet_ingelogd" ? "Niet ingelogd" : "Niet beschikbaar"}
              </h2>
              <p class="uitleg">{f().melding}</p>
              <div class="knoppen">
                <Show when={f().code === "niet_ingelogd"}>
                  <button class="primair" onClick={() => api.toonHoofdvenster()}>
                    Inloggen
                  </button>
                </Show>
                <button onClick={laadGrenzen}>Opnieuw proberen</button>
              </div>
            </section>
          )}
        </Show>

        <Show when={fase() !== "bezig" && onderbroken()}>
          {(o) => (
            <section class="deel-sectie onderbroken">
              <h2>Onderbroken overdracht</h2>
              <p class="uitleg">
                <strong>{o().naam}</strong>: {o().onderdelen.filter((d) => d.klaar).length} van {o().onderdelen.length} verstuurd. Ga
                verder waar je was, of gooi hem weg.
              </p>
              <div class="knoppen">
                <button class="primair" disabled={!!bezig()} onClick={hervat}>
                  Hervatten
                </button>
                <button class="gevaar-licht" disabled={!!bezig()} onClick={() => stop(true)}>
                  Weggooien
                </button>
              </div>
            </section>
          )}
        </Show>

        <Show when={fase() === "samenstellen"}>
          <section class="deel-sectie">
            <div class="deel-sectiekop">
              <h2>Bestanden en mappen</h2>
              <span class="knoppen">
                <button class="tekstknop" onClick={() => voegToe(false)}>
                  Bestanden toevoegen…
                </button>
                <button class="tekstknop" onClick={() => voegToe(true)}>
                  Map toevoegen…
                </button>
              </span>
            </div>
            <ul class="verstuurlijst">
              <For
                each={items()}
                fallback={<li class="toegang-leeg">Kies bestanden of mappen, of gebruik "Versturen met OpenAEC" in het rechtermuismenu.</li>}
              >
                {(i) => (
                  <li classList={{ fout: !!i.fout }}>
                    <span class={`klein-icoon ${i.map ? "map" : "bestand"}`} aria-hidden="true" />
                    <div class="toegang-regel">
                      <span class="naam" title={i.pad}>
                        {i.naam}
                      </span>
                      <span class="meta">
                        {i.fout ??
                          (i.map
                            ? `map, ${i.bestanden} ${i.bestanden === 1 ? "bestand" : "bestanden"} · ${grootte(i.grootte)} · gaat mee als zip`
                            : grootte(i.grootte))}
                      </span>
                    </div>
                    <button class="klein" aria-label={`${i.naam} weghalen`} onClick={() => haalWeg(i.pad)}>
                      Weghalen
                    </button>
                  </li>
                )}
              </For>
            </ul>
            <Show when={grenzen()}>
              {(g) => (
                <p class="hint" classList={{ fout: !!grootteFout() }}>
                  {grootteFout() || `${grootteDecimaal(totaalBytes())} van hoogstens ${grootteDecimaal(g().max_bytes)} per overdracht`}
                </p>
              )}
            </Show>
          </section>

          <section class="deel-sectie">
            <h2>Versturen</h2>
            <p class="uitleg">Je krijgt een downloadlink om zelf te delen. Vul je ontvangers in, dan krijgen zij de link ook per e-mail.</p>
            <label>
              Naam
              <input
                value={naam()}
                maxLength={140}
                placeholder={standaardNaam(items())}
                onInput={(e) => {
                  setNaam(e.currentTarget.value)
                  setNaamAangepast(true)
                }}
              />
            </label>
            <label>
              Ontvangers (optioneel)
              <input
                type="text"
                placeholder="naam@bedrijf.nl, collega@bedrijf.nl"
                value={ontvangers()}
                onInput={(e) => setOntvangers(e.currentTarget.value)}
              />
            </label>
            <Show when={ontvangerFout()}>
              <p class="hint fout">{ontvangerFout()}</p>
            </Show>
            <label>
              Bericht (optioneel)
              <textarea rows="3" maxLength={5000} value={bericht()} onInput={(e) => setBericht(e.currentTarget.value)} />
            </label>
            <div class="velden">
              <label>
                Geldig tot (optioneel)
                <input
                  type="date"
                  min={datumWaarde(new Date())}
                  max={laatsteDag(grenzen()?.max_dagen ?? 30)}
                  value={geldigTot()}
                  onInput={(e) => setGeldigTot(e.currentTarget.value)}
                />
              </label>
              <label>
                Wachtwoord (optioneel)
                <input type="password" autocomplete="new-password" value={wachtwoord()} onInput={(e) => setWachtwoord(e.currentTarget.value)} />
              </label>
            </div>
            <p class="hint" classList={{ fout: !!datumFout() }}>
              {datumFout() ||
                (geldigTot() ? "" : `Zonder datum ${grenzen()?.standaard_dagen ?? 7} dagen geldig, hoogstens ${grenzen()?.max_dagen ?? 30} dagen.`)}
            </p>
            <Show when={wachtwoord()}>
              <p class="hint">Het wachtwoord gaat niet mee in de e-mail; geef het zelf door.</p>
            </Show>
            <Foutmelding fout={fout()} />
            <button class="primair" disabled={!kanVersturen()} onClick={versturen}>
              {bezig() === "versturen" ? "Bezig…" : "Versturen"}
            </button>
          </section>
        </Show>

        <Show when={fase() !== "samenstellen" ? lopend() : null}>
          {(l) => (
            <section class="deel-sectie">
              <div class="deel-sectiekop">
                <h2>{fase() === "klaar" ? "Verstuurd" : fase() === "bezig" ? "Bezig met versturen" : "Niet verstuurd"}</h2>
                <span class="meta">
                  {grootte(voortgang().gedaan)} van {grootte(voortgang().totaal)}
                </span>
              </div>
              <ul class="verstuurlijst">
                <For each={l().onderdelen}>
                  {(o, i) => {
                    const stap = () => stappen()[i()]
                    const breedte = () => {
                      const s = stap()
                      if (!s) return 0
                      if (s.status === "klaar") return 100
                      return s.totaal ? Math.min(100, (s.gedaan / s.totaal) * 100) : 0
                    }
                    return (
                      <li classList={{ fout: stap()?.status === "fout", klaar: stap()?.status === "klaar" }}>
                        <span class={`klein-icoon ${o.map ? "map" : "bestand"}`} aria-hidden="true" />
                        <div class="toegang-regel">
                          <span class="naam" title={o.pad}>
                            {o.naam}
                          </span>
                          <span class="balk">
                            <span classList={{ onbepaald: stap()?.status === "inpakken" }} style={{ width: `${breedte()}%` }} />
                          </span>
                        </div>
                        <span class="meta stap">{stap() ? stapTekst(stap()!) : ""}</span>
                      </li>
                    )
                  }}
                </For>
              </ul>
              <Foutmelding fout={fase() === "fout" ? fout() : null} />
              <Show when={fase() === "bezig"}>
                <div class="knoppen">
                  <button disabled={!!bezig()} onClick={() => stop(false)}>
                    Pauzeren
                  </button>
                  <button class="gevaar-licht" disabled={!!bezig()} onClick={() => stop(true)}>
                    Stoppen en intrekken
                  </button>
                </div>
              </Show>
              <Show when={fase() === "fout"}>
                <div class="knoppen">
                  <Show when={onderbroken()}>
                    <button class="primair" onClick={hervat}>
                      Hervatten
                    </button>
                  </Show>
                  <button onClick={() => beginOpnieuw(paden())}>Opnieuw samenstellen</button>
                </div>
              </Show>
            </section>
          )}
        </Show>

        <Show when={uitkomst()}>
          {(v) => (
            <section class="deel-sectie">
              <h2>Downloadlink</h2>
              <p class="uitleg">
                {v().mail.length ? `Gemaild naar ${v().mail.join(", ")}. ` : ""}
                Wie de link heeft, kan de bestanden downloaden{v().verloopt_op ? ` tot ${datumTekst(v().verloopt_op!)}` : ""}.
              </p>
              <div class="deellink">
                <input readOnly value={v().url} onFocus={(e) => e.currentTarget.select()} />
                <button onClick={() => kopieer(v().url)}>Kopiëren</button>
              </div>
              <div class="knoppen">
                <button onClick={() => beginOpnieuw(wachtend())}>
                  {wachtend().length ? `Volgende versturen (${wachtend().length})` : "Nieuwe overdracht"}
                </button>
              </div>
            </section>
          )}
        </Show>

        <Show when={fase() === "bezig" && wachtend().length}>
          <p class="hint">
            {wachtend().length} {wachtend().length === 1 ? "item wacht" : "items wachten"} op de volgende overdracht (
            {wachtend().map(naamVan).join(", ")}).
          </p>
        </Show>

        <Show when={grenzen()}>
          <section class="deel-sectie">
            <div class="deel-sectiekop">
              <h2>Mijn overdrachten</h2>
              <button class="tekstknop" onClick={laadMijn}>
                Vernieuwen
              </button>
            </div>
            <Foutmelding fout={mijnFout()} />
            <Show
              when={mijn()}
              fallback={
                <Show when={!mijnFout()}>
                  <p class="hint">Ophalen…</p>
                </Show>
              }
            >
              {(regels) => (
                <ul class="toegang">
                  <For each={regels()} fallback={<li class="toegang-leeg">Nog niets verstuurd.</li>}>
                    {(r) => (
                      <li classList={{ verlopen: !intrekbaar(r) }}>
                        <span class={`statuslabel ${r.status.toLowerCase()}`}>{statusTekst(r.status)}</span>
                        <div class="toegang-regel">
                          <span class="naam">{r.naam}</span>
                          <span class="meta">{overdrachtMeta(r)}</span>
                        </div>
                        <div class="knoppen">
                          <Show when={r.url}>
                            <button class="klein" onClick={() => kopieer(r.url!)}>
                              Kopiëren
                            </button>
                          </Show>
                          <Show when={intrekbaar(r)}>
                            <button class="klein gevaar-licht" disabled={!!bezig()} onClick={() => trekIn(r)}>
                              {intrekken() === r.overdracht ? "Zeker intrekken?" : "Intrekken"}
                            </button>
                          </Show>
                        </div>
                      </li>
                    )}
                  </For>
                </ul>
              )}
            </Show>
          </section>
        </Show>
      </div>

      <footer class="deel-voet">
        <span class="melding">{melding()}</span>
        <button onClick={() => api.sluitVersturen()}>Sluiten</button>
      </footer>
    </div>
  )
}
