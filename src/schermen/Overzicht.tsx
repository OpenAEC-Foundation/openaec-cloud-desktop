import { createSignal, For, onCleanup, onMount, Show } from "solid-js"
import { ask, open } from "@tauri-apps/plugin-dialog"
import { api, type Koppeling, type KoppelOvergeslagen, type MapRapport } from "../lib/api"
import Mappenkiezer from "../Mappenkiezer"
import OvergeslagenLijst from "../OvergeslagenLijst"
import SessieVerlopen from "../SessieVerlopen"
import { rechtenTekst } from "../lib/delen"
import { besparingTekst, grootte, misluktTekst, naamVan, procent, quotumTekst, rondeTekst, snelheid, soort, tijdstip } from "../lib/opmaak"
import {
  basis, bezig, doe, hoofdmap, inHoofdmap, koppelingen, koppelStatus, laadHoofdmap, quotum, rapporten, ruimtes, setHoofdmap,
  setKoppelingen, setMelding, setScherm, stand, verversOverzicht, verwerkSync,
} from "../lib/winkel"

const samenvatting = (m: MapRapport) => {
  if (m.fout) return m.fout
  const r = m.rapport!
  const delen = [
    r.geupload && `${r.geupload} geüpload`,
    r.gedownload && `${r.gedownload} gedownload`,
    r.verwijderd && `${r.verwijderd} verwijderd`,
    r.conflicten && `${r.conflicten} conflict${r.conflicten > 1 ? "en" : ""}`,
    misluktTekst(r.fouten.length + (r.wachtend?.length ?? 0)),
  ].filter(Boolean)
  return delen.length ? delen.join(" · ") : "Niets veranderd"
}

type Kleur = "groen" | "oranje" | "rood" | "grijs"

/** Toestand van één koppeling: label en kleur voor het statuslabel. */
const toestandVan = (k: Koppeling): [string, Kleur] => {
  const st = koppelStatus()[k.lokaal]
  if (st && !st.bestaat) return ["Map ontbreekt", "rood"]
  if (stand()?.gepauzeerd) return ["Gepauzeerd", "grijs"]
  if (stand()?.actief.some((o) => o.lokaal === k.lokaal)) return ["Bezig", "oranje"]
  const r = rapporten()[k.lokaal]
  if (r?.fout || r?.rapport?.fouten.length || r?.rapport?.wachtend?.length) return ["Fout", "rood"]
  if (stand()?.toestand === "bezig") return ["Bezig", "oranje"]
  if (!r && !st?.aantal) return ["Nog niet gesynchroniseerd", "grijs"]
  return ["Bijgewerkt", "groen"]
}

/** De hoofdmap: bij de eerste start een voorstel, daarna waar hij staat en of hij in het navigatiepaneel staat. */
function HoofdmapKaart() {
  const zet = (pad: string) =>
    doe("Hoofdmap instellen", async () => {
      setHoofdmap(await api.zetHoofdmap(pad))
      setMelding(`Hoofdmap ingesteld: ${pad}. Ruimtes die je daarin synchroniseert, staan in de Verkenner onder "OpenAEC".`)
    })
  const kies = async () => {
    const map = await open({ directory: true, multiple: false, title: "Hoofdmap voor OpenAEC" })
    if (typeof map === "string") zet(map)
  }
  return (
    <Show when={hoofdmap()}>
      {(h) => (
        <section class="kaart hoofdmap-kaart">
          <Show
            when={h().hoofdmap}
            fallback={
              <>
                <div>
                  <h2>Eén map voor al je ruimtes</h2>
                  <p class="uitleg">
                    Stel een hoofdmap in: <span class="pad">{h().voorstel}</span>. Elke ruimte krijgt daarin een eigen map
                    <Show when={h().ondersteund}>, de map staat als "OpenAEC" in het navigatiepaneel van de Verkenner en bestanden komen pas op deze computer als je ze opent</Show>.
                  </p>
                </div>
                <div class="knoppen">
                  <button class="primair" disabled={bezig()} onClick={() => zet(h().voorstel)}>
                    Instellen
                  </button>
                  <button disabled={bezig()} onClick={kies}>
                    Andere map…
                  </button>
                </div>
              </>
            }
          >
            <div>
              <h2>Hoofdmap</h2>
              <p class="uitleg">
                <span class="pad">{h().hoofdmap}</span>
                <Show when={h().ondersteund}>
                  {" · "}
                  {h().actief ? "in het navigatiepaneel van de Verkenner" : h().fout ? `niet in de Verkenner: ${h().fout}` : "wordt aangemeld bij de Verkenner…"}
                </Show>
              </p>
            </div>
            <div class="knoppen">
              <button onClick={() => api.openMap(h().hoofdmap!).catch((e) => setMelding(String(e)))}>Openen in Verkenner</button>
              <Show when={h().fout}>
                <button disabled={bezig()} onClick={() => zet(h().hoofdmap!)}>
                  Opnieuw aanmelden
                </button>
              </Show>
            </div>
          </Show>
        </section>
      )}
    </Show>
  )
}

export default function Overzicht() {
  // Mapselectie: voor welke koppeling het venster "Mappen kiezen…" open is.
  const [selectieVoor, setSelectieVoor] = createSignal<Koppeling | null>(null)
  const [overgeslagen, setOvergeslagen] = createSignal<KoppelOvergeslagen[]>([])

  onMount(async () => {
    laadHoofdmap()
    api.overgeslagen().then(setOvergeslagen).catch(() => {})
    const stop = await api.opOvergeslagen(setOvergeslagen)
    onCleanup(stop)
  })

  const syncNu = () =>
    doe("Synchroniseren", async () => {
      verwerkSync(await api.syncNu())
      setMelding("Synchronisatie klaar")
    })

  const ontkoppel = (k: Koppeling) =>
    doe("Ontkoppelen", async () => {
      setKoppelingen(await api.ontkoppel(k.lokaal))
      verversOverzicht()
      setMelding(`${k.ruimte_naam} ontkoppeld; de lokale bestanden blijven staan`)
    })

  const selectieKlaar = (k: Koppeling[] | null, tekst?: string) => {
    setSelectieVoor(null)
    if (!k) return
    setKoppelingen(k)
    if (tekst) setMelding(tekst)
    api.syncNu().then(verwerkSync).catch(() => {})
  }

  /** Overstap van een losse map naar de hoofdmap (na bevestiging; nooit vanzelf). */
  const naarHoofdmap = async (k: Koppeling) => {
    const h = hoofdmap()?.hoofdmap
    if (!h) return
    const ja = await ask(
      `${k.ruimte_naam} verplaatsen van ${k.lokaal} naar de hoofdmap ${h}?\n\n` +
        "De bestanden verhuizen mee (niets wordt opnieuw gedownload). Daarna kun je bestanden ook alleen online laten staan. " +
        "Sluit eerst programma's die bestanden in de map open hebben.",
      { title: "Naar de hoofdmap", kind: "info", okLabel: "Verplaatsen", cancelLabel: "Annuleren" },
    )
    if (!ja) return
    doe("Verplaatsen naar de hoofdmap", async () => {
      setKoppelingen(await api.verplaatsNaarHoofdmap(k.lokaal))
      verversOverzicht()
      setMelding(`${k.ruimte_naam} staat nu in de hoofdmap`)
    })
  }

  // Een gedeelde map: wie hem deelde en wat je kunt, in plaats van de soort ruimte.
  const soortVan = (k: Koppeling) =>
    k.deling
      ? ["gedeeld", k.gedeeld_door ? `door ${k.gedeeld_door}` : "", rechtenTekst("gebruiker", k.rechten ?? "lezen").toLowerCase()]
          .filter(Boolean)
          .join(" · ")
      : soort(ruimtes().find((r) => r.name === k.ruimte)?.soort)

  return (
    <div class="scherm">
      <div class="schermkop">
        <div>
          <h1>Overzicht</h1>
          <p class="uitleg">Gekoppelde mappen op deze computer en hoe het met de synchronisatie staat.</p>
        </div>
        <button class="primair" disabled={bezig() || !koppelingen().length || stand()?.gepauzeerd} onClick={syncNu}>
          Nu synchroniseren
        </button>
      </div>

      <Show when={stand()?.sessie_verlopen}>
        <SessieVerlopen basis={basis()} klaar={(tekst) => setMelding(tekst)} />
      </Show>

      <HoofdmapKaart />

      <Show when={quotum()}>
        {(q) => (
          <section class="kaart opslag">
            <div class="opslag-kop">
              <h2>Opslag</h2>
              <span class="cijfers">{quotumTekst(q().gebruikt_bytes, q().limiet_bytes)}</span>
            </div>
            <Show when={q().limiet_bytes !== null}>
              <span class="balk" classList={{ vol: procent(q().gebruikt_bytes, q().limiet_bytes!) >= 90 }}>
                <span style={{ width: `${procent(q().gebruikt_bytes, q().limiet_bytes!)}%` }} />
              </span>
            </Show>
          </section>
        )}
      </Show>

      <Show when={stand()?.actief.length || stand()?.ronde}>
        <section class="kaart lopend-blok">
          <h2>Nu bezig</h2>
          <Show when={stand()?.ronde}>
            {(r) => (
              <div class="ronde">
                <span class="balk">
                  <span style={{ width: `${procent(r().bytes_gedaan, r().bytes_totaal)}%` }} />
                </span>
                <span class="cijfers">{rondeTekst(r())}</span>
                <Show when={r().bespaard > 0}>
                  <span class="cijfers bespaard">{grootte(r().bespaard)} niet verstuurd dankzij blokken die er al waren</span>
                </Show>
              </div>
            )}
          </Show>
          <ul class="overdrachten">
            <For each={stand()!.actief}>
              {(o) => (
                <li>
                  <span class="richting">{o.richting === "upload" ? "↑" : "↓"}</span>
                  <span class="naam" title={o.pad}>{naamVan(o.pad)}</span>
                  <span class="balk"><span style={{ width: `${procent(o.gedaan, o.totaal)}%` }} /></span>
                  <span class="cijfers">
                    {[`${procent(o.gedaan, o.totaal)}%`, snelheid(o.snelheid), besparingTekst(o.grootte, o.totaal)].filter(Boolean).join(" · ")}
                  </span>
                </li>
              )}
            </For>
          </ul>
        </section>
      </Show>

      <Show
        when={koppelingen().length}
        fallback={
          <section class="leeg">
            <div class="leeg-icoon">⇅</div>
            <h2>Nog geen gekoppelde mappen</h2>
            <p>
              Koppel een ruimte aan een map op deze computer. Daarna blijven de bestanden vanzelf gelijk: wat jij in de map zet, staat op
              de server, en wat collega's toevoegen, komt in je map.
            </p>
            <button class="primair" onClick={() => setScherm("ruimtes")}>
              Naar Ruimtes
            </button>
          </section>
        }
      >
        <div class="koppelingen">
          <For each={koppelingen()}>
            {(k) => {
              const t = () => toestandVan(k)
              const r = () => rapporten()[k.lokaal]
              const st = () => koppelStatus()[k.lokaal]
              return (
                <section class="kaart koppeling">
                  <div class="koppeling-kop">
                    <div>
                      <h2>{k.ruimte_naam}</h2>
                      <Show when={soortVan(k)}>
                        <span class="soort">{soortVan(k)}</span>
                      </Show>
                    </div>
                    <div>
                      <Show when={inHoofdmap(k)}>
                        <span class="label klein blauw">In de hoofdmap</span>
                      </Show>
                      <span class={`label ${t()[1]}`}>{t()[0]}</span>
                    </div>
                  </div>
                  <dl class="gegevens">
                    <div>
                      <dt>Laatste synchronisatie</dt>
                      <dd>{r() ? tijdstip(r()!.tijd) : "nog niet sinds het starten"}</dd>
                    </div>
                    <div>
                      <dt>Bestanden</dt>
                      <dd>{st() ? st()!.aantal.toLocaleString("nl-NL") : "–"}</dd>
                    </div>
                    <div>
                      <dt>Laatste ronde</dt>
                      <dd classList={{ foutief: !!r()?.fout || !!r()?.rapport?.fouten.length || !!r()?.rapport?.wachtend?.length }}>{r() ? samenvatting(r()!) : "–"}</dd>
                    </div>
                    <div class="vol">
                      <dt>Lokale map</dt>
                      <dd class="pad">{k.lokaal}</dd>
                    </div>
                    <Show when={k.uitgesloten?.length}>
                      <div class="vol">
                        <dt>Niet op deze computer</dt>
                        <dd>{k.uitgesloten.join(", ")}</dd>
                      </div>
                    </Show>
                  </dl>
                  <Show when={r()?.rapport?.fouten.length || r()?.rapport?.wachtend?.length}>
                    <details class="foutenlijst">
                      <summary>{misluktTekst(r()!.rapport!.fouten.length + (r()!.rapport!.wachtend?.length ?? 0))}, zie details</summary>
                      <ul>
                        <For each={r()!.rapport!.fouten.slice(0, 20)}>{(f) => <li>{f}</li>}</For>
                        <For each={(r()!.rapport!.wachtend ?? []).slice(0, 20)}>
                          {(w) => <li>{w.pad}: {w.reden} (volgende poging {tijdstip(new Date(w.opnieuw_om * 1000))})</li>}
                        </For>
                      </ul>
                    </details>
                  </Show>
                  <div class="knoppen">
                    <button disabled={st() && !st()!.bestaat} onClick={() => api.openMap(k.lokaal).catch((e) => setMelding(String(e)))}>
                      Openen in Verkenner
                    </button>
                    <button disabled={bezig()} onClick={() => setSelectieVoor(k)}>
                      Mappen kiezen…
                    </button>
                    <Show when={hoofdmap()?.hoofdmap && !inHoofdmap(k)}>
                      <button disabled={bezig()} onClick={() => naarHoofdmap(k)} title="De map verhuist naar een submap van de hoofdmap">
                        Naar de hoofdmap…
                      </button>
                    </Show>
                    <button class="gevaar-licht" disabled={bezig()} onClick={() => ontkoppel(k)}>
                      Ontkoppelen
                    </button>
                  </div>
                </section>
              )
            }}
          </For>
        </div>
        <Show when={overgeslagen().length}>
          <section class="kaart">
            <OvergeslagenLijst lijst={overgeslagen()} klaar={setMelding} />
          </section>
        </Show>
        <p class="hint">Synchroniseert vanzelf zodra er iets verandert, hier of op de server.</p>
      </Show>

      <Show when={selectieVoor()}>{(k) => <Mappenkiezer koppeling={k()} klaar={selectieKlaar} />}</Show>
    </div>
  )
}
