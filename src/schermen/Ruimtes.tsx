import { createEffect, createSignal, For, on, Show } from "solid-js"
import { open } from "@tauri-apps/plugin-dialog"
import { api, type Deeldoel, type MapInhoud, type Ruimte } from "../lib/api"
import { grootte, servertijd, soort } from "../lib/opmaak"
import {
  bezig, doe, koppelingen, laadRuimtes, ruimteFout, ruimtes, ruimtesLaden, ruimtesOpgehaald, setKoppelingen,
  setMelding, setScherm, verversOverzicht, verwerkSync,
} from "../lib/winkel"

const rolNaam: Record<string, string> = { beheerder: "beheerder", schrijver: "schrijver", lezer: "lezer", gast: "gast" }
const rol = (r: string) => rolNaam[r.toLowerCase()] ?? r


/** Eén map in het kruimelpad: id (null = hoofdniveau) en naam. */
type Stap = { id: string | null; naam: string }

function Bestandsweergave(props: { ruimte: Ruimte }) {
  const [pad, setPad] = createSignal<Stap[]>([])
  const [inhoud, setInhoud] = createSignal<MapInhoud | null>(null)
  const [laden, setLaden] = createSignal(false)
  const [fout, setFout] = createSignal("")

  const hier = () => pad()[pad().length - 1]?.id ?? null

  const laad = async () => {
    setLaden(true)
    setFout("")
    try {
      setInhoud(await api.bladeren(props.ruimte.name, hier()))
    } catch (e) {
      setInhoud(null)
      setFout(String(e))
    }
    setLaden(false)
  }

  // Andere ruimte gekozen: terug naar het hoofdniveau.
  createEffect(
    on(
      () => props.ruimte.name,
      () => {
        setPad([])
        laad()
      },
    ),
  )

  const naarMap = (stappen: Stap[]) => {
    setPad(stappen)
    laad()
  }

  const leeg = () => inhoud() && !inhoud()!.mappen.length && !inhoud()!.bestanden.length

  /** Het deelvenster openen voor een map of bestand op deze plek (de klik opent de map niet). */
  const deel = (e: MouseEvent, doctype: Deeldoel["doctype"], id: string, naam: string) => {
    e.stopPropagation()
    const plek = [props.ruimte.naam, ...pad().map((s) => s.naam), naam].join(" / ")
    api
      .openDelen({ doel: { doctype, id, naam, pad: plek, ruimte: props.ruimte.name, ruimte_naam: props.ruimte.naam } })
      .catch((fout) => setMelding(String(fout)))
  }

  return (
    <div class="bestanden">
      <div class="bestanden-kop">
        <nav class="kruimels">
          <button class="kruimel" onClick={() => naarMap([])}>
            {props.ruimte.naam}
          </button>
          <For each={pad()}>
            {(s, i) => (
              <>
                <span class="scheiding">/</span>
                <button class="kruimel" onClick={() => naarMap(pad().slice(0, i() + 1))}>
                  {s.naam}
                </button>
              </>
            )}
          </For>
        </nav>
        <button class="klein" disabled={laden()} onClick={laad} title="Opnieuw ophalen van de server">
          {laden() ? "Bezig…" : "Vernieuwen"}
        </button>
      </div>

      <Show when={!fout()} fallback={<p class="foutvlak">Ophalen mislukt: {fout()}</p>}>
        <Show
          when={!leeg()}
          fallback={
            <div class="leeg klein-leeg">
              <Show
                when={pad().length}
                fallback={
                  <>
                    <h2>Deze ruimte is nog leeg</h2>
                    <p>Deze ruimte is nog leeg. Koppel hem aan een map en zet er bestanden in.</p>
                  </>
                }
              >
                <p>Deze map is leeg.</p>
              </Show>
            </div>
          }
        >
          <table class="tabel">
            <thead>
              <tr>
                <th>Naam</th>
                <th class="getal">Grootte</th>
                <th>Gewijzigd</th>
                <th>Vergrendeld door</th>
                <th class="acties" aria-label="Acties" />
              </tr>
            </thead>
            <tbody classList={{ vaag: laden() }}>
              <Show when={pad().length}>
                <tr class="map" onClick={() => naarMap(pad().slice(0, -1))}>
                  <td colSpan={5}>
                    <span class="icoon">↰</span> Omhoog
                  </td>
                </tr>
              </Show>
              <For each={inhoud()?.mappen}>
                {(m) => (
                  <tr class="map" onClick={() => naarMap([...pad(), { id: m.id, naam: m.naam }])}>
                    <td>
                      <span class="icoon map-icoon" />
                      {m.naam}
                    </td>
                    <td class="getal">–</td>
                    <td>{servertijd(m.gewijzigd)}</td>
                    <td />
                    <td class="acties">
                      <button class="klein" title={`${m.naam} delen`} onClick={(e) => deel(e, "Opslagmap", m.id, m.naam)}>
                        Delen…
                      </button>
                    </td>
                  </tr>
                )}
              </For>
              <For each={inhoud()?.bestanden}>
                {(b) => (
                  <tr>
                    <td>
                      <span class="icoon bestand-icoon" />
                      {b.naam}
                    </td>
                    <td class="getal">{grootte(b.grootte)}</td>
                    <td>{servertijd(b.gewijzigd)}</td>
                    <td>
                      <Show when={b.vergrendeld_door}>
                        <span
                          class="slot"
                          title={`${b.vergrendeld_door}${b.vergrendeld_tot ? ` (tot ${servertijd(b.vergrendeld_tot)})` : ""}`}
                        >
                          {b.vergrendeld_door!.split("@")[0]}
                        </span>
                      </Show>
                    </td>
                    <td class="acties">
                      <button class="klein" title={`${b.naam} delen`} onClick={(e) => deel(e, "Opslagbestand", b.id, b.naam)}>
                        Delen…
                      </button>
                    </td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </Show>
      </Show>
    </div>
  )
}

export default function Ruimtes() {
  const [gekozen, setGekozen] = createSignal<string | null>(null)
  const ruimte = () => ruimtes().find((r) => r.name === gekozen()) ?? ruimtes()[0]
  const gekoppeldAan = (r: Ruimte) => koppelingen().filter((k) => k.ruimte === r.name)

  const koppel = (r: Ruimte) =>
    doe("Koppelen", async () => {
      const map = await open({ directory: true, multiple: false, title: `Lokale map voor ${r.naam}` })
      if (typeof map !== "string") return setMelding("Geen map gekozen")
      setKoppelingen(await api.koppel(r.name, r.naam, map))
      verversOverzicht()
      setMelding(`${r.naam} gekoppeld aan ${map}; de eerste synchronisatie loopt`)
      api.syncNu().then(verwerkSync).catch(() => {})
    })

  const cloudmap = (r: Ruimte) =>
    doe("Cloud-map maken", async () => {
      const map = await open({ directory: true, multiple: false, title: `Lege map voor cloud-map ${r.naam}` })
      if (typeof map !== "string") return setMelding("Geen map gekozen")
      const n = await api.maakCloudmap(r.name, map)
      setMelding(`${n} bestand(en) als placeholder geplaatst in ${map}`)
    })

  const beheer = () => api.openBeheer().catch((e) => setMelding(String(e)))

  return (
    <div class="scherm">
      <div class="schermkop">
        <div>
          <h1>Ruimtes</h1>
          <p class="uitleg">De ruimtes waar je lid van bent. Koppel een ruimte aan een map om de bestanden op deze computer te hebben.</p>
        </div>
        <button disabled={ruimtesLaden()} onClick={() => laadRuimtes(true)}>
          {ruimtesLaden() ? "Ophalen…" : "Vernieuwen"}
        </button>
      </div>

      <Show when={ruimteFout()}>
        <p class="foutvlak">
          Ruimtes ophalen mislukt: {ruimteFout()}{" "}
          <button class="tekstknop" onClick={() => laadRuimtes(true)}>
            Opnieuw proberen
          </button>
        </p>
      </Show>

      <Show
        when={ruimtes().length}
        fallback={
          <Show when={ruimtesOpgehaald()} fallback={<p class="hint">Ruimtes ophalen…</p>}>
            <section class="leeg">
              <div class="leeg-icoon">▢</div>
              <h2>Nog geen ruimte</h2>
              <p>Je hebt nog geen ruimte. Een beheerder maakt ruimtes aan in Beheer → Opslag van je omgeving.</p>
              <div class="knoppen midden">
                <button class="primair" onClick={beheer}>
                  Beheer → Opslag openen
                </button>
                <button onClick={() => laadRuimtes(true)}>Opnieuw kijken</button>
              </div>
            </section>
          </Show>
        }
      >
        <div class="ruimtes">
          <ul class="ruimtelijst">
            <For each={ruimtes()}>
              {(r) => (
                <li>
                  <button class="ruimte" classList={{ actief: ruimte()?.name === r.name }} onClick={() => setGekozen(r.name)}>
                    <span class="ruimte-naam">{r.naam}</span>
                    <span class="ruimte-meta">
                      {[soort(r.soort), rol(r.rol)].filter(Boolean).join(" · ")}
                    </span>
                    <Show when={gekoppeldAan(r).length}>
                      <span class="label groen klein">Gekoppeld</span>
                    </Show>
                  </button>
                </li>
              )}
            </For>
          </ul>

          <Show when={ruimte()}>
            {(r) => (
              <section class="kaart ruimte-detail">
                <div class="koppeling-kop">
                  <div>
                    <h2>{r().naam}</h2>
                    <span class="soort">{[soort(r().soort), `jouw rol: ${rol(r().rol)}`].filter(Boolean).join(" · ")}</span>
                  </div>
                  <div class="knoppen">
                    <button class="primair" disabled={bezig()} onClick={() => koppel(r())}>
                      Koppelen aan map…
                    </button>
                    <button disabled={bezig()} onClick={() => cloudmap(r())} title="Alleen Windows: bestanden worden opgehaald zodra je ze opent">
                      Cloud-map…
                    </button>
                  </div>
                </div>
                <Show when={gekoppeldAan(r()).length}>
                  <p class="gekoppeld">
                    Gekoppeld aan{" "}
                    <For each={gekoppeldAan(r())}>
                      {(k, i) => (
                        <>
                          {i() > 0 && ", "}
                          <button class="tekstknop" onClick={() => setScherm("overzicht")}>
                            {k.lokaal}
                          </button>
                        </>
                      )}
                    </For>
                  </p>
                </Show>
                <Bestandsweergave ruimte={r()} />
              </section>
            )}
          </Show>
        </div>
      </Show>
    </div>
  )
}
