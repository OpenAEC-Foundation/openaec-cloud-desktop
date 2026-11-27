// Venster Versturen: een of meer bestanden of mappen versturen met een downloadlink, eventueel met een e-mail aan de
// ontvangers, een bericht, een vervaldatum en een wachtwoord (binnen de grenzen van de server). Opent vanuit "Versturen
// met OpenAEC" in het rechtermuismenu van de bestandsbeheerder, op elk bestand en elke map. Een map gaat met al zijn
// inhoud mee als zip. Onderaan "Mijn overdrachten", met de status en Intrekken.
//
// Of: "Delen met klant". Dezelfde upload, maar daarna wordt het een levering (datadrop) met het 3D-model uit Speckle
// achter één vaste link, eerst als concept: het voorbeeld opent in een eigen venster, daarna delen met e-mailadressen.
// Een volgende keer kan het een nieuwe versie van dezelfde levering worden (dezelfde link). Zie src/lib/klant.ts.

import { createSignal, For, onCleanup, onMount, Show } from "solid-js"
import { open } from "@tauri-apps/plugin-dialog"
import { api } from "./lib/api"
import { datumTekst, datumWaarde, leesFout, type Deelfout } from "./lib/delen"
import { grootte, naamVan } from "./lib/opmaak"
import { Symbool } from "./Titelbalk"
import { gedeeldTekst, leveringMeta, NIEUW, voortzetbaar, type KlantGemaakt, type KlantLevering, type KlantProjecten } from "./lib/klant"
import {
  controleerGeldigTot, controleerGrootte, controleerOntvangers, grootteDecimaal, intrekbaar, laatsteDag, opnieuwNa, overdrachtMeta,
  opslagTekst, samenvatting, splitsOntvangers, standaardNaam, stapTekst, statusTekst, totaal, voegSamen,
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
  const [verbinden, setVerbinden] = createSignal(false)

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

  // Delen met klant
  const [soort, setSoort] = createSignal<"versturen" | "klant">("versturen")
  const [projecten, setProjecten] = createSignal<KlantProjecten | null>(null)
  const [project, setProject] = createSignal("")
  const [leveringen, setLeveringen] = createSignal<KlantLevering[]>([])
  const [vervolg, setVervolg] = createSignal(NIEUW)
  const [notitie, setNotitie] = createSignal("")
  const [levering, setLevering] = createSignal<KlantGemaakt | null>(null)
  const [klantFout, setKlantFout] = createSignal<Deelfout | null>(null)
  const klant = () => soort() === "klant"

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
    !!grenzen() &&
    items().length > 0 &&
    !ontbrekend().length &&
    !grootteFout() &&
    !bezig() &&
    (klant() ? !klantFout() || klantFout()?.code === "fout" : !ontvangerFout() && !datumFout())
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
    setLevering(null)
    setNotitie("")
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

  /** De projecten in Speckle en de eigen leveringen, de eerste keer dat Delen met klant gekozen wordt. */
  const laadKlant = async () => {
    setKlantFout(null)
    try {
      const [p, l] = await Promise.all([api.klantProjecten(), api.klantMijn()])
      setProjecten(p)
      setLeveringen(l)
    } catch (e) {
      setKlantFout(leesFout(e))
    }
  }

  const kiesSoort = (nieuw: "versturen" | "klant") => {
    setSoort(nieuw)
    if (nieuw === "klant" && !projecten()) laadKlant()
  }

  const laadMijn = async () => {
    try {
      setMijn(await api.mijnOverdrachten())
      setMijnFout(null)
    } catch (e) {
      setMijnFout(leesFout(e))
    }
  }

  /**
   * Verbinden: de grenzen van de server (meteen de test of je ingelogd bent en mag versturen). Eén poging tegelijk; een
   * tijdelijke fout (vlak na het opstarten is de sessie of het netwerk er soms nog niet) gaat eerst stil opnieuw, pas
   * daarna verschijnt de melding.
   */
  let bezigMetVerbinden: Promise<void> | null = null
  const laadGrenzen = () => {
    bezigMetVerbinden ??= (async () => {
      setVerbinden(true)
      setAanmeldFout(null)
      for (let poging = 0; ; poging++) {
        try {
          setGrenzen(await api.versturenGrenzen())
          setAanmeldFout(null)
          laadMijn()
          if (fase() === "samenstellen") setOnderbroken(await api.onderbrokenVersturen().catch(() => null))
          break
        } catch (e) {
          const f = leesFout(e)
          const wacht = opnieuwNa(f.code, poging)
          if (wacht === null) {
            setGrenzen(null)
            setAanmeldFout(f)
            break
          }
          await new Promise((klaar) => setTimeout(klaar, wacht))
        }
      }
    })().finally(() => {
      setVerbinden(false)
      bezigMetVerbinden = null
    })
    return bezigMetVerbinden
  }

  /**
   * De paden uit het rechtermuismenu ophalen (de app houdt ze vast tot het venster ze neemt, en herinnert het venster
   * tot dan). Kon het venster eerder geen verbinding maken of was je niet ingelogd, dan meteen opnieuw proberen: dat is
   * een oude melding van toen het venster op de achtergrond laadde, niet van nu.
   */
  const haalVerzoek = async () => {
    const nieuw = await api.versturenVerzoek().catch(() => [] as string[])
    if (!nieuw.length) return
    ontvang(nieuw)
    if (!grenzen()) laadGrenzen()
    else laadMijn()
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
    // Al klaar voordat het venster de overdracht kende (de server kopieerde alles meteen).
    if (vroeg?.overdracht === l.overdracht) klaar(vroeg)
    vroeg = null
  }

  /** Een melding "klaar" die binnenkwam voordat het venster de overdracht kende. */
  let vroeg: Voltooid | null = null

  /** Was de lopende upload voor de klant: dan wordt het daarna een levering. */
  let voorKlant = false

  const klaar = (v: Voltooid) => {
    setStappen((oud) => oud.map((s) => ({ ...s, status: "klaar", gedaan: s.totaal })))
    setFase("klaar")
    if (voorKlant) return void maakLevering(v.overdracht)
    setUitkomst(v)
    setMelding("Link staat op het klembord")
    laadMijn()
  }

  /** Van de verstuurde bestanden een levering maken (of een nieuwe versie), en meteen het voorbeeld tonen. */
  const maakLevering = async (overdracht: string) => {
    setBezig("levering")
    setKlantFout(null)
    try {
      const gemaakt = await api.klantVanOverdracht(overdracht, {
        titel: naam().trim() || standaardNaam(items()),
        project: vervolg() === NIEUW ? project() || null : null,
        bericht: vervolg() === NIEUW ? bericht().trim() || null : null,
        datadrop: vervolg() === NIEUW ? null : vervolg(),
        notitie: vervolg() === NIEUW ? null : notitie().trim() || null,
      })
      setLevering(gemaakt)
      setMelding(gemaakt.versie > 1 ? `Versie ${gemaakt.versie} staat klaar onder dezelfde link` : "Het concept staat klaar")
      setLeveringen(await api.klantMijn().catch(() => leveringen()))
      api.klantVoorbeeld(gemaakt.url).catch((e) => setKlantFout(leesFout(e)))
    } catch (e) {
      setKlantFout(leesFout(e))
    } finally {
      setBezig("")
    }
  }

  const deelLevering = async (l: KlantGemaakt) => {
    setBezig("delen")
    setKlantFout(null)
    try {
      const g = await api.klantDeel(l.datadrop, lijst(), bericht().trim() || null)
      setLevering({ ...l, status: g.status })
      setOntvangers("")
      setMelding(gedeeldTekst(g))
      setLeveringen(await api.klantMijn().catch(() => leveringen()))
    } catch (e) {
      setKlantFout(leesFout(e))
    } finally {
      setBezig("")
    }
  }

  const versturen = async () => {
    if (!kanVersturen()) return
    setBezig("versturen")
    setFout(null)
    voorKlant = klant()
    try {
      // Voor de klant zonder ontvangers, datum en wachtwoord: de levering zelf regelt de link, de termijn en de mail.
      volg(
        await api.startVersturen({
          paden: paden(),
          naam: naam().trim() || standaardNaam(items()),
          bericht: voorKlant ? null : bericht().trim() || null,
          verloopt_op: voorKlant ? null : geldigTot() || null,
          wachtwoord: voorKlant ? null : wachtwoord() || null,
          ontvangers: voorKlant ? [] : lijst(),
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
      await api.opVersturenVerzoek(haalVerzoek),
      await api.opVersturenStap((s) => {
        if (s.overdracht !== lopend()?.overdracht) return
        setStappen((oud) => oud.map((x, i) => (i === s.index ? s : x)))
      }),
      await api.opVersturenKlaar((v) => {
        if (v.overdracht !== lopend()?.overdracht) {
          vroeg = v
          return
        }
        klaar(v)
      }),
      await api.opVersturenFout(({ overdracht, fout }) => {
        if (overdracht !== lopend()?.overdracht) return
        setFase("fout")
        setFout(fout)
        api.onderbrokenVersturen().then(setOnderbroken).catch(() => setOnderbroken(null))
        laadMijn()
      }),
    ]
    // Na inloggen in het hoofdvenster: opnieuw kijken zodra dit venster weer de focus krijgt of zichtbaar wordt; dan
    // ook de paden ophalen die er misschien nog staan.
    const bijFocus = () => {
      haalVerzoek()
      if (!grenzen()) laadGrenzen()
    }
    const bijZichtbaar = () => document.visibilityState === "visible" && bijFocus()
    window.addEventListener("focus", bijFocus)
    document.addEventListener("visibilitychange", bijZichtbaar)
    onCleanup(() => {
      stoppers.forEach((s) => s())
      window.removeEventListener("focus", bijFocus)
      document.removeEventListener("visibilitychange", bijZichtbaar)
    })
    // Tegelijk: de paden meteen tonen, ook als de verbinding nog even duurt.
    haalVerzoek()
    laadGrenzen()
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
        <span class="verstuur-logo" aria-hidden="true">
          <Symbool class="verstuur-symbool" />
          <span class="verstuur-envelop" />
        </span>
        <div>
          <h1>{titel()}</h1>
          <span class="pad">{ondertitel()}</span>
          <span class="deel-ruimte">{klant() ? "Delen met klant" : "Versturen met OpenAEC"}</span>
        </div>
      </header>

      <div class="deel-inhoud">
        <Show when={verbinden() && !grenzen() && !aanmeldFout()}>
          <p class="hint">Verbinden met OpenAEC…</p>
        </Show>
        <Show when={aanmeldFout()}>
          {(f) => (
            <section class="deel-sectie">
              <h2>
                {f().code === "geen_recht"
                  ? "Geen recht om te versturen"
                  : f().code === "niet_ingelogd"
                    ? "Niet ingelogd"
                    : f().code === "niet_op_server"
                      ? "Nog niet beschikbaar op deze omgeving"
                      : "Niet beschikbaar"}
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
                verder waar je was, of gooi hem weg. Verstuur je iets nieuws, dan vervalt deze.
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
          <div class="soortkeuze" role="radiogroup" aria-label="Wat wil je doen">
            <button role="radio" aria-checked={!klant()} classList={{ actief: !klant() }} onClick={() => kiesSoort("versturen")}>
              Versturen
              <span>downloadlink</span>
            </button>
            <button role="radio" aria-checked={klant()} classList={{ actief: klant() }} onClick={() => kiesSoort("klant")}>
              Delen met klant
              <span>3D-model en tekeningen, met voorbeeld</span>
            </button>
          </div>
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
                            ? `map, ${i.bestanden} ${i.bestanden === 1 ? "bestand" : "bestanden"} · ${grootte(i.grootte)} · ${i.in_opslag ? "staat al in OpenAEC" : "gaat mee als zip"}`
                            : `${grootte(i.grootte)}${i.in_opslag ? " · staat al in OpenAEC" : ""}`)}
                      </span>
                    </div>
                    <button class="klein" aria-label={`${i.naam} weghalen`} onClick={() => haalWeg(i.pad)}>
                      Weghalen
                    </button>
                  </li>
                )}
              </For>
            </ul>
            <Show when={opslagTekst(items())}>
              <p class="hint">{opslagTekst(items())}</p>
            </Show>
            <Show when={grenzen()}>
              {(g) => (
                <p class="hint" classList={{ fout: !!grootteFout() }}>
                  {grootteFout() || `${grootteDecimaal(totaalBytes())} van hoogstens ${grootteDecimaal(g().max_bytes)} per overdracht`}
                </p>
              )}
            </Show>
          </section>

          <Show when={klant()}>
            <section class="deel-sectie">
              <h2>Delen met klant</h2>
              <p class="uitleg">
                De klant krijgt één link naar het 3D-model en deze bestanden, kan ze bekijken, downloaden en er opmerkingen bij zetten. Eerst
                zie je een voorbeeld; daarna deel je het.
              </p>
              <Foutmelding fout={klantFout()} />
              <Show when={leveringen().length > 0 && voortzetbaar(leveringen()).length > 0}>
                <label>
                  Levering
                  <select value={vervolg()} onChange={(e) => setVervolg(e.currentTarget.value)}>
                    <option value={NIEUW}>Nieuwe levering</option>
                    <For each={voortzetbaar(leveringen())}>
                      {(l) => <option value={l.datadrop}>{`Nieuwe versie van ${l.titel} (nu versie ${l.versie})`}</option>}
                    </For>
                  </select>
                </label>
              </Show>
              <Show
                when={vervolg() === NIEUW}
                fallback={
                  <label>
                    Wat er veranderd is (optioneel)
                    <textarea rows="2" maxLength={500} value={notitie()} onInput={(e) => setNotitie(e.currentTarget.value)} />
                  </label>
                }
              >
                <label>
                  Titel
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
                  3D-model uit Speckle
                  <select value={project()} disabled={!projecten()?.speckle} onChange={(e) => setProject(e.currentTarget.value)}>
                    <option value="">Geen model, alleen bestanden</option>
                    <For each={(projecten()?.projecten ?? []).filter((p) => p.modellen.length > 0)}>
                      {(p) => <option value={p.project}>{p.naam}</option>}
                    </For>
                  </select>
                </label>
                <Show when={projecten() && !projecten()!.speckle}>
                  <p class="hint">{projecten()!.melding ?? "Speckle staat op deze omgeving niet aan."} De levering krijgt dan alleen bestanden.</p>
                </Show>
                <label>
                  Bericht voor de klant (optioneel)
                  <textarea rows="2" maxLength={2000} value={bericht()} onInput={(e) => setBericht(e.currentTarget.value)} />
                </label>
              </Show>
              <Foutmelding fout={fout()} />
              <button class="primair" disabled={!kanVersturen()} onClick={versturen}>
                {bezig() === "versturen" ? "Bezig…" : vervolg() === NIEUW ? "Uploaden en voorbeeld tonen" : "Uploaden als nieuwe versie"}
              </button>
            </section>
          </Show>

          <Show when={!klant()}>
          <section class="deel-sectie">
            <h2>Versturen</h2>
            <p class="uitleg">Je krijgt een downloadlink; ontvangers krijgen hem ook per e-mail.</p>
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
              <textarea rows="2" maxLength={5000} value={bericht()} onInput={(e) => setBericht(e.currentTarget.value)} />
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
                            {/* Inpakken en kopiëren op de server duren onbepaald lang: dan de heen-en-weer-balk. */}
                            <Show
                              when={stap()?.status === "inpakken" || stap()?.status === "kopieren"}
                              fallback={<span style={{ width: `${breedte()}%` }} />}
                            >
                              <span class="onbepaald" />
                            </Show>
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

        <Show when={fase() === "klaar" && voorKlant}>
          <section class="deel-sectie">
            <Show
              when={levering()}
              fallback={
                <>
                  <p class="hint">{bezig() === "levering" ? "De levering wordt gemaakt…" : ""}</p>
                  <Foutmelding fout={klantFout()} />
                </>
              }
            >
              {(l) => (
                <>
                  <h2>{l().status === "Concept" ? "Concept klaar" : l().versie > 1 ? `Versie ${l().versie} klaar` : "Gedeeld"}</h2>
                  <p class="uitleg">
                    {l().status === "Concept"
                      ? "Het voorbeeld staat in een eigen venster: zo ziet de klant het. Deel het als het goed is."
                      : "De klant ziet voortaan deze versie onder dezelfde link. Stuur hem een bericht als je wilt."}
                  </p>
                  <div class="knoppen">
                    <button onClick={() => api.klantVoorbeeld(l().url).catch((e) => setKlantFout(leesFout(e)))}>Voorbeeld tonen</button>
                  </div>
                  <label>
                    E-mailadressen van de klant
                    <input
                      type="text"
                      placeholder="jan@klant.nl, piet@aannemer.nl"
                      value={ontvangers()}
                      onInput={(e) => setOntvangers(e.currentTarget.value)}
                    />
                  </label>
                  <Show when={ontvangerFout()}>
                    <p class="hint fout">{ontvangerFout()}</p>
                  </Show>
                  <label>
                    Bericht in de mail (optioneel)
                    <textarea rows="2" maxLength={2000} value={bericht()} onInput={(e) => setBericht(e.currentTarget.value)} />
                  </label>
                  <Foutmelding fout={klantFout()} />
                  <div class="knoppen">
                    <button
                      class="primair"
                      disabled={!!bezig() || !!ontvangerFout() || (l().status !== "Concept" && !lijst().length)}
                      onClick={() => deelLevering(l())}
                    >
                      {bezig() === "delen" ? "Bezig…" : l().status === "Concept" ? "Delen" : "Mail sturen"}
                    </button>
                  </div>
                  <div class="deellink">
                    <input readOnly value={l().url} onFocus={(e) => e.currentTarget.select()} />
                    <button onClick={() => kopieer(l().url)}>Kopiëren</button>
                  </div>
                  <div class="knoppen">
                    <button onClick={() => beginOpnieuw(wachtend())}>Nieuwe levering of versie</button>
                  </div>
                </>
              )}
            </Show>
          </section>
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

        <Show when={klant() && leveringen().length > 0}>
          <section class="deel-sectie">
            <div class="deel-sectiekop">
              <h2>Mijn leveringen</h2>
              <button class="tekstknop" onClick={laadKlant}>
                Vernieuwen
              </button>
            </div>
            <ul class="toegang">
              <For each={leveringen()}>
                {(l) => (
                  <li classList={{ verlopen: l.status === "Verlopen" || l.status === "Ingetrokken" }}>
                    <span class={`statuslabel ${l.status.toLowerCase()}`}>{l.status === "Concept" ? "Concept" : l.status}</span>
                    <div class="toegang-regel">
                      <span class="naam">{l.titel}</span>
                      <span class="meta">{leveringMeta(l)}</span>
                    </div>
                    <div class="knoppen">
                      <Show when={l.url}>
                        <button class="klein" onClick={() => api.klantVoorbeeld(l.url!).catch((e) => setKlantFout(leesFout(e)))}>
                          Bekijken
                        </button>
                        <button class="klein" onClick={() => kopieer(l.url!)}>
                          Kopiëren
                        </button>
                      </Show>
                    </div>
                  </li>
                )}
              </For>
            </ul>
          </section>
        </Show>

        <Show when={grenzen() && !klant()}>
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
