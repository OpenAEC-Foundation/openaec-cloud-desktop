// Statusvenster bij het systeemvak: wat de sync nu doet, wat er net gebeurd is, en snelle knoppen.

import { createSignal, For, onCleanup, onMount, Show } from "solid-js"
import { api, type Gebeurd, type Koppeling, type Stand } from "./lib/api"
import { grootte, lokaalPad, naamVan, procent, snelheid, tijdstip } from "./lib/opmaak"

const actieTekst: Record<Gebeurd["actie"], string> = {
  geupload: "Geüpload",
  gedownload: "Gedownload",
  verwijderd: "Verwijderd",
  conflict: "Conflict",
  fout: "Fout",
}

const actieTeken: Record<Gebeurd["actie"], string> = {
  geupload: "↑",
  gedownload: "↓",
  verwijderd: "×",
  conflict: "!",
  fout: "!",
}

const toestandTeken = { bijgewerkt: "✓", bezig: "⇅", gepauzeerd: "❚❚", fout: "!" }

export default function Status() {
  const [stand, setStand] = createSignal<Stand | null>(null)
  const [koppelingen, setKoppelingen] = createSignal<Koppeling[]>([])
  const [fout, setFout] = createSignal("")

  const laad = async () => {
    const s = await api.status()
    setKoppelingen(s.koppelingen)
    setStand(await api.activiteit())
    setFout("")
  }

  onMount(async () => {
    laad().catch(() => {})
    const stoppers = [await api.opActiviteit(setStand), await api.opVensterOpen(() => laad().catch(() => {}))]
    onCleanup(() => stoppers.forEach((stop) => stop()))
  })

  const ruimteVan = (lokaal: string) => koppelingen().find((k) => k.lokaal === lokaal)?.ruimte_naam ?? naamVan(lokaal)
  const toon = (pad: string) => api.openMap(pad).catch((e) => setFout(String(e)))

  /** Map van de laatste activiteit, anders de eerste koppeling. */
  const mapOpenen = () => {
    const lokaal = stand()?.recent[0]?.lokaal ?? koppelingen()[0]?.lokaal
    if (lokaal) toon(lokaal)
  }

  const toestand = () => stand()?.toestand ?? "bijgewerkt"

  return (
    <div class="status">
      <header class="status-kop">
        <div class="woordmerk">
          Open<span>AEC</span> Desktop App
        </div>
      </header>

      <section class={`status-samenvatting ${toestand()}`}>
        <span class="status-teken">{toestandTeken[toestand()]}</span>
        <div>
          <strong>{koppelingen().length ? stand()?.samenvatting ?? "Alles bijgewerkt" : "Nog geen gekoppelde mappen"}</strong>
          <span>
            {koppelingen().length
              ? `${koppelingen().length} gekoppelde map${koppelingen().length > 1 ? "pen" : ""}`
              : "Open het hoofdvenster om een ruimte te koppelen."}
          </span>
        </div>
      </section>

      <Show when={stand()?.actief.length}>
        <ul class="status-actief">
          <For each={stand()!.actief}>
            {(o) => (
              <li>
                <span class="richting">{o.richting === "upload" ? "↑" : "↓"}</span>
                <div class="regel">
                  <div class="regel-boven">
                    <span class="naam" title={o.pad}>
                      {naamVan(o.pad)}
                    </span>
                    <span class="cijfers">
                      {[o.totaal ? `${procent(o.gedaan, o.totaal)}% van ${grootte(o.totaal)}` : "", snelheid(o.snelheid)].filter(Boolean).join(" · ")}
                    </span>
                  </div>
                  <span class="balk">
                    <span style={{ width: `${procent(o.gedaan, o.totaal)}%` }} />
                  </span>
                </div>
              </li>
            )}
          </For>
        </ul>
      </Show>

      <h3 class="status-tussenkop">Laatste activiteit</h3>
      <ul class="status-recent">
        <For each={stand()?.recent} fallback={<li class="status-leeg">Nog geen activiteit sinds het starten.</li>}>
          {(g) => (
            <li>
              <button class="recent" title={g.fout ?? "Map openen"} onClick={() => toon(lokaalPad(g.lokaal, g.pad))}>
                <span class={`teken ${g.actie}`}>{actieTeken[g.actie]}</span>
                <span class="regel">
                  <span class="naam">{naamVan(g.pad)}</span>
                  <span class="meta">
                    {actieTekst[g.actie]} · {ruimteVan(g.lokaal)}
                    {g.fout ? ` · ${g.fout}` : ""}
                  </span>
                </span>
                <span class="tijd">{tijdstip(new Date(g.tijd))}</span>
              </button>
            </li>
          )}
        </For>
      </ul>

      <Show when={fout()}>
        <p class="status-fout">{fout()}</p>
      </Show>

      <footer class="status-knoppen">
        <button disabled={!koppelingen().length} onClick={mapOpenen}>
          Map openen
        </button>
        <button onClick={() => api.pauzeer(!stand()?.gepauzeerd)}>{stand()?.gepauzeerd ? "Hervatten" : "Pauzeren"}</button>
        <button class="primair" onClick={() => api.toonHoofdvenster()}>
          Hoofdvenster
        </button>
      </footer>
    </div>
  )
}
