import { createSignal, For, onCleanup, onMount, Show } from "solid-js"
import { api, type Koppeling, type KoppelOvergeslagen, type MapRapport } from "../lib/api"
import OvergeslagenLijst from "../OvergeslagenLijst"
import { besparingTekst, grootte, naamVan, procent, quotumTekst, rondeTekst, snelheid, soort, tijdstip } from "../lib/opmaak"
import {
  bezig, doe, koppelingen, koppelStatus, quotum, rapporten, ruimtes, setKoppelingen, setMelding, setScherm, stand,
  verversOverzicht, verwerkSync,
} from "../lib/winkel"

const samenvatting = (m: MapRapport) => {
  if (m.fout) return m.fout
  const r = m.rapport!
  const delen = [
    r.geupload && `${r.geupload} geüpload`,
    r.gedownload && `${r.gedownload} gedownload`,
    r.verwijderd && `${r.verwijderd} verwijderd`,
    r.conflicten && `${r.conflicten} conflict${r.conflicten > 1 ? "en" : ""}`,
    r.fouten.length && `${r.fouten.length} fout${r.fouten.length > 1 ? "en" : ""}`,
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
  if (r?.fout || r?.rapport?.fouten.length) return ["Fout", "rood"]
  if (stand()?.toestand === "bezig") return ["Bezig", "oranje"]
  if (!r && !st?.aantal) return ["Nog niet gesynchroniseerd", "grijs"]
  return ["Bijgewerkt", "groen"]
}

export default function Overzicht() {
  // Mapselectie: welke koppeling, alle mappen van de ruimte en de uitgevinkte mappen.
  const [selectieVoor, setSelectieVoor] = createSignal<Koppeling | null>(null)
  const [alleMappen, setAlleMappen] = createSignal<string[]>([])
  const [uit, setUit] = createSignal<string[]>([])
  const [overgeslagen, setOvergeslagen] = createSignal<KoppelOvergeslagen[]>([])

  onMount(async () => {
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

  const kiesMappen = (k: Koppeling) =>
    doe("Mappen ophalen", async () => {
      setAlleMappen(await api.mappenVan(k.ruimte))
      setUit(k.uitgesloten ?? [])
      setSelectieVoor(k)
      setMelding("")
    })

  // Een map is uitgesloten als hij zelf of een bovenliggende map uitgevinkt is.
  const onder = (map: string, ouder: string) => map === ouder || map.startsWith(ouder + "/")
  const isUit = (map: string) => uit().some((u) => onder(map, u))
  const ouderUit = (map: string) => uit().some((u) => u !== map && onder(map, u))
  const wissel = (map: string) =>
    setUit(isUit(map) ? uit().filter((u) => u !== map) : [...uit().filter((u) => !onder(u, map)), map])

  const bewaarSelectie = () =>
    doe("Mapselectie opslaan", async () => {
      const k = selectieVoor()!
      setKoppelingen(await api.zetSelectie(k.lokaal, uit()))
      setSelectieVoor(null)
      api.syncNu().then(verwerkSync).catch(() => {})
      setMelding(uit().length ? `${uit().length} map(pen) niet meer op deze computer; ze blijven op de server` : "Alle mappen staan op deze computer")
    })

  const soortVan = (k: Koppeling) => soort(ruimtes().find((r) => r.name === k.ruimte)?.soort)

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
                    <span class={`label ${t()[1]}`}>{t()[0]}</span>
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
                      <dd classList={{ foutief: !!r()?.fout || !!r()?.rapport?.fouten.length }}>{r() ? samenvatting(r()!) : "–"}</dd>
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
                  <Show when={r()?.rapport?.fouten.length}>
                    <ul class="foutenlijst">
                      <For each={r()!.rapport!.fouten.slice(0, 5)}>{(f) => <li>{f}</li>}</For>
                    </ul>
                  </Show>
                  <div class="knoppen">
                    <button disabled={st() && !st()!.bestaat} onClick={() => api.openMap(k.lokaal).catch((e) => setMelding(String(e)))}>
                      Openen in Verkenner
                    </button>
                    <button disabled={bezig()} onClick={() => kiesMappen(k)}>
                      Mappen kiezen…
                    </button>
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

      <Show when={selectieVoor()}>
        {(k) => (
          <div class="laag" onClick={(e) => e.target === e.currentTarget && setSelectieVoor(null)}>
            <section class="kaart dialoog">
              <h2>Mappen op deze computer: {k().ruimte_naam}</h2>
              <p class="uitleg">Uitgevinkte mappen blijven op de server en voor collega's beschikbaar, maar komen niet (meer) op deze computer.</p>
              <Show when={alleMappen().length} fallback={<p class="hint">Deze ruimte heeft nog geen mappen.</p>}>
                <ul class="mapboom">
                  <For each={alleMappen()}>
                    {(m) => (
                      <li style={{ "padding-left": `${(m.split("/").length - 1) * 18}px` }}>
                        <label class="vinkje">
                          <input type="checkbox" checked={!isUit(m)} disabled={ouderUit(m)} onChange={() => wissel(m)} />
                          {m.split("/").pop()}
                        </label>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
              <div class="knoppen rechts">
                <button disabled={bezig()} onClick={() => setSelectieVoor(null)}>
                  Annuleren
                </button>
                <button class="primair" disabled={bezig()} onClick={bewaarSelectie}>
                  Opslaan
                </button>
              </div>
            </section>
          </div>
        )}
      </Show>
    </div>
  )
}
