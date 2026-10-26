// Statusvenster bij het systeemvak: wat de sync nu doet, wat er net gebeurd is, en snelle knoppen.

import { createSignal, For, onCleanup, onMount, Show } from "solid-js"
import { api, type Gebeurd, type Koppeling, type KoppelOvergeslagen, type Quotum, type Stand } from "./lib/api"
import OvergeslagenLijst from "./OvergeslagenLijst"
import SessieVerlopen from "./SessieVerlopen"
import { besparingTekst, grootte, lokaalPad, misluktTekst, naamVan, procent, quotumTekst, rondeTekst, snelheid, tijdstip } from "./lib/opmaak"

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
  const [quotum, setQuotum] = createSignal<Quotum | null>(null)
  const [overgeslagen, setOvergeslagen] = createSignal<KoppelOvergeslagen[]>([])
  const [basis, setBasis] = createSignal("")

  const laad = async () => {
    const s = await api.status()
    setKoppelingen(s.koppelingen)
    setBasis(s.basis ?? "")
    setStand(await api.activiteit())
    setFout("")
    api.quotum().then(setQuotum).catch(() => {})
    api.overgeslagen().then(setOvergeslagen).catch(() => {})
  }

  onMount(async () => {
    laad().catch(() => {})
    const stoppers = [
      await api.opActiviteit(setStand),
      await api.opVensterOpen(() => laad().catch(() => {})),
      await api.opQuotum(setQuotum),
      await api.opOvergeslagen(setOvergeslagen),
    ]
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

      <Show when={stand()?.sessie_verlopen && basis()}>
        <SessieVerlopen basis={basis()} klaar={setFout} compact />
      </Show>

      <Show when={stand()?.mislukt.length}>
        <details class="status-mislukt">
          <summary>{misluktTekst(stand()!.mislukt.length)}, zie details</summary>
          <ul>
            <For each={stand()!.mislukt}>
              {(m) => (
                <li>
                  <button class="recent" title="Map openen" onClick={() => toon(lokaalPad(m.lokaal, m.pad))}>
                    <span class="regel">
                      <span class="naam">{naamVan(m.pad)}</span>
                      <span class="meta">
                        {ruimteVan(m.lokaal)} · {m.reden}
                        {m.opnieuw_om ? ` · volgende poging ${tijdstip(new Date(m.opnieuw_om))}` : ""}
                      </span>
                    </span>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </details>
      </Show>

      <Show when={stand()?.ronde}>
        {(r) => (
          <div class="status-ronde">
            <span class="balk">
              <span style={{ width: `${procent(r().bytes_gedaan, r().bytes_totaal)}%` }} />
            </span>
            <span class="cijfers">{rondeTekst(r())}</span>
            <Show when={r().bespaard > 0}>
              <span class="cijfers">{grootte(r().bespaard)} bespaard door hergebruik van blokken</span>
            </Show>
          </div>
        )}
      </Show>

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
                  <Show when={besparingTekst(o.grootte, o.totaal)}>
                    <span class="bespaard">{besparingTekst(o.grootte, o.totaal)}</span>
                  </Show>
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
                    {besparingTekst(g.grootte, g.verstuurd) ? ` · ${besparingTekst(g.grootte, g.verstuurd)}` : ""}
                    {g.fout ? ` · ${g.fout}` : ""}
                  </span>
                </span>
                <span class="tijd">{tijdstip(new Date(g.tijd))}</span>
              </button>
            </li>
          )}
        </For>
      </ul>

      <OvergeslagenLijst lijst={overgeslagen()} klaar={setFout} compact />

      <Show when={fout()}>
        <p class="status-fout">{fout()}</p>
      </Show>

      <Show when={quotum()}>
        {(q) => (
          <div class="status-opslag">
            <span>{quotumTekst(q().gebruikt_bytes, q().limiet_bytes)}</span>
            <Show when={q().limiet_bytes !== null}>
              <span class="balk" classList={{ vol: procent(q().gebruikt_bytes, q().limiet_bytes!) >= 90 }}>
                <span style={{ width: `${procent(q().gebruikt_bytes, q().limiet_bytes!)}%` }} />
              </span>
            </Show>
          </div>
        )}
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
