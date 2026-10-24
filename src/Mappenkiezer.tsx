import { createSignal, For, onMount, Show } from "solid-js"
import { api, type Koppeling, type NieuweMappen, type Omvang } from "./lib/api"
import { boom, isUit, nieuwUit, ouderUit, wissel, zichtbaar, type Boomregel } from "./lib/mappenkiezer"
import { grootte } from "./lib/opmaak"

/** "Mappen kiezen…" voor één koppeling: een boom van de mappen op de server met een vinkje per map, de grootte per
 *  map, en wat er met nieuwe mappen gebeurt. Uitvinken ruimt de lokale kopie op na een bevestiging. */
export default function Mappenkiezer(props: { koppeling: Koppeling; klaar: (k: Koppeling[] | null, tekst?: string) => void }) {
  const k = props.koppeling
  const [regels, setRegels] = createSignal<Boomregel[]>([])
  const [laden, setLaden] = createSignal(true)
  const [fout, setFout] = createSignal("")
  const [uit, setUit] = createSignal<string[]>(k.uitgesloten ?? [])
  const [nieuwe, setNieuwe] = createSignal<NieuweMappen>(k.nieuwe_mappen ?? "automatisch")
  const [ingeklapt, setIngeklapt] = createSignal<Set<string>>(new Set())
  const [bevestig, setBevestig] = createSignal<{ mappen: string[]; omvang: Omvang } | null>(null)
  const [bezig, setBezig] = createSignal(false)

  onMount(async () => {
    try {
      const b = boom(await api.mappenVan(k.ruimte, k.lokaal))
      setRegels(b)
      // Diepere niveaus eerst ingeklapt bij een grote ruimte.
      if (b.length > 40) setIngeklapt(new Set(b.filter((r) => r.kinderen && r.diepte >= 0).map((r) => r.pad)))
    } catch (e) {
      setFout(String(e))
    }
    setLaden(false)
  })

  const klap = (pad: string) => {
    const s = new Set(ingeklapt())
    s.has(pad) ? s.delete(pad) : s.add(pad)
    setIngeklapt(s)
  }

  const bewaar = async () => {
    setBezig(true)
    try {
      const k2 = await api.zetSelectie(k.lokaal, uit(), nieuwe(), regels().map((r) => r.pad))
      const n = uit().length
      props.klaar(k2, n ? `${n} map(pen) niet op deze computer; ze blijven op de server` : "Alle mappen staan op deze computer")
    } catch (e) {
      setFout(String(e))
      setBezig(false)
    }
  }

  const opslaan = async () => {
    const nieuw = nieuwUit(k.uitgesloten ?? [], uit())
    if (!nieuw.length) return bewaar()
    setBezig(true)
    try {
      const omvang = await api.lokaleOmvang(k.lokaal, nieuw)
      setBezig(false)
      if (!omvang.bestanden) return bewaar()
      setBevestig({ mappen: nieuw, omvang })
    } catch (e) {
      setFout(String(e))
      setBezig(false)
    }
  }

  return (
    <div class="laag" onClick={(e) => e.target === e.currentTarget && !bezig() && props.klaar(null)}>
      <section class="kaart dialoog">
        <h2>Mappen kiezen: {k.ruimte_naam}</h2>
        <p class="uitleg">
          Wat aangevinkt is, staat op deze computer. Uitgevinkte mappen blijven op de server en voor collega's beschikbaar.
        </p>
        <Show when={!fout()} fallback={<p class="foutvlak">{fout()}</p>}>
          <Show when={!laden()} fallback={<p class="hint">Mappen ophalen van de server…</p>}>
            <Show when={regels().length} fallback={<p class="hint">Deze ruimte heeft nog geen mappen.</p>}>
              <ul class="mapboom">
                <For each={zichtbaar(regels(), ingeklapt())}>
                  {(r) => (
                    <li style={{ "padding-left": `${r.diepte * 18}px` }}>
                      <span class="klapper">
                        <Show when={r.kinderen}>
                          <button class="tekstknop" aria-label={ingeklapt().has(r.pad) ? "Uitklappen" : "Inklappen"} onClick={() => klap(r.pad)}>
                            {ingeklapt().has(r.pad) ? "▸" : "▾"}
                          </button>
                        </Show>
                      </span>
                      <label class="vinkje">
                        <input type="checkbox" checked={!isUit(uit(), r.pad)} disabled={ouderUit(uit(), r.pad)} onChange={() => setUit(wissel(uit(), r.pad))} />
                        {r.naam}
                      </label>
                      <Show when={r.bestanden}>
                        <span class="cijfers">
                          {grootte(r.bytes)} · {r.bestanden.toLocaleString("nl-NL")} bestand{r.bestanden === 1 ? "" : "en"}
                        </span>
                      </Show>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </Show>
        </Show>
        <fieldset class="keuze">
          <legend>Nieuwe mappen op de server</legend>
          <label class="vinkje">
            <input type="radio" name="nieuwe" checked={nieuwe() === "automatisch"} onChange={() => setNieuwe("automatisch")} />
            Automatisch meenemen
          </label>
          <label class="vinkje">
            <input type="radio" name="nieuwe" checked={nieuwe() === "alleen_gekozen"} onChange={() => setNieuwe("alleen_gekozen")} />
            Alleen wat ik hier gekozen heb
          </label>
        </fieldset>
        <Show when={bevestig()}>
          {(b) => (
            <p class="foutvlak">
              In {b().mappen.length === 1 ? `de map ${b().mappen[0]}` : `${b().mappen.length} mappen`} staan{" "}
              {b().omvang.bestanden.toLocaleString("nl-NL")} bestand{b().omvang.bestanden === 1 ? "" : "en"} ({grootte(b().omvang.bytes)}) op deze
              computer. Die worden hier verwijderd; op de server blijven ze staan. Wat je hier gewijzigd hebt, gaat eerst nog naar de server.
            </p>
          )}
        </Show>
        <div class="knoppen rechts">
          <button disabled={bezig()} onClick={() => props.klaar(null)}>
            Annuleren
          </button>
          <Show
            when={bevestig()}
            fallback={
              <button class="primair" disabled={bezig() || laden() || !!fout()} onClick={opslaan}>
                Opslaan
              </button>
            }
          >
            <button class="gevaar-licht" disabled={bezig()} onClick={bewaar}>
              Opruimen en opslaan
            </button>
          </Show>
        </div>
      </section>
    </div>
  )
}
