import { createEffect, createSignal, For, on, Show } from "solid-js"
import { open } from "@tauri-apps/plugin-dialog"
import { api, type Deeldoel, type Gedeeld, type MapInhoud, type Ruimte } from "../lib/api"
import { datumTekst, gedeeldMeta, rechtenTekst } from "../lib/delen"
import { grootte, servertijd, soort } from "../lib/opmaak"
import {
  bezig, doe, gedeeld, hoofdmap, inHoofdmap, koppelingen, laadRuimtes, ruimteFout, ruimtes, ruimtesLaden, ruimtesOpgehaald,
  setKoppelingen, setMelding, setScherm, verversOverzicht, verwerkSync,
} from "../lib/winkel"

const rolNaam: Record<string, string> = { beheerder: "beheerder", schrijver: "schrijver", lezer: "lezer", gast: "gast" }
const rol = (r: string) => rolNaam[r.toLowerCase()] ?? r

/** Eén map in het kruimelpad: id (null = hoofdniveau) en naam. */
type Stap = { id: string | null; naam: string }

/** Waar de bestandsweergave kijkt: een ruimte, of een map die met mij gedeeld is (dan is die map het hoofdniveau). */
type Plek = { ruimte: string; naam: string; ruimte_naam: string; deling?: string; magDelen: boolean }

function Bestandsweergave(props: { plek: Plek }) {
  const [pad, setPad] = createSignal<Stap[]>([])
  const [inhoud, setInhoud] = createSignal<MapInhoud | null>(null)
  const [laden, setLaden] = createSignal(false)
  const [fout, setFout] = createSignal("")

  const hier = () => pad()[pad().length - 1]?.id ?? null

  const laad = async () => {
    setLaden(true)
    setFout("")
    try {
      setInhoud(await api.bladeren(props.plek.ruimte, hier(), props.plek.deling))
    } catch (e) {
      setInhoud(null)
      setFout(String(e))
    }
    setLaden(false)
  }

  // Andere ruimte of gedeelde map gekozen: terug naar het hoofdniveau.
  createEffect(
    on(
      () => props.plek.deling ?? props.plek.ruimte,
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
    const plek = [props.plek.naam, ...pad().map((s) => s.naam), naam].join(" / ")
    api
      .openDelen({ doel: { doctype, id, naam, pad: plek, ruimte: props.plek.ruimte, ruimte_naam: props.plek.ruimte_naam } })
      .catch((fout) => setMelding(String(fout)))
  }

  return (
    <div class="bestanden">
      <div class="bestanden-kop">
        <nav class="kruimels">
          <button class="kruimel" onClick={() => naarMap([])}>
            {props.plek.naam}
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
                when={pad().length || props.plek.deling}
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
                      <Show when={props.plek.magDelen}>
                        <button class="klein" title={`${m.naam} delen`} onClick={(e) => deel(e, "Opslagmap", m.id, m.naam)}>
                          Delen…
                        </button>
                      </Show>
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
                      <Show when={props.plek.magDelen}>
                        <button class="klein" title={`${b.naam} delen`} onClick={(e) => deel(e, "Opslagbestand", b.id, b.naam)}>
                          Delen…
                        </button>
                      </Show>
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

/** Wat er gekozen is in de lijst: een ruimte (`r:<id>`) of iets dat met mij gedeeld is (`d:<deling>`). */
type Keuze = string | null

export default function Ruimtes() {
  const [gekozen, setGekozen] = createSignal<Keuze>(null)
  const ruimte = () => {
    const k = gekozen()
    if (k?.startsWith("d:")) return undefined
    return ruimtes().find((r) => `r:${r.name}` === k) ?? (k ? undefined : ruimtes()[0])
  }
  const gedeeldItem = () => {
    const k = gekozen()
    if (k?.startsWith("d:")) return gedeeld().find((g) => `d:${g.deling}` === k)
    return ruimtes().length || k ? undefined : gedeeld()[0]
  }
  const gekoppeldAan = (ruimte: string, deling?: string) =>
    koppelingen().filter((k) => k.ruimte === ruimte && (k.deling ?? undefined) === deling)

  const koppel = (naam: string, f: (map: string) => Promise<unknown>) =>
    doe("Koppelen", async () => {
      const map = await open({ directory: true, multiple: false, title: `Lokale map voor ${naam}` })
      if (typeof map !== "string") return setMelding("Geen map gekozen")
      await f(map)
      verversOverzicht()
      setMelding(`${naam} gekoppeld aan ${map}; de eerste synchronisatie loopt`)
      api.syncNu().then(verwerkSync).catch(() => {})
    })

  /** In de hoofdmap: een submap met de naam van de ruimte, als "OpenAEC" in het navigatiepaneel. */
  const inDeHoofdmap = (r: Ruimte) =>
    doe("Synchroniseren in de hoofdmap", async () => {
      setKoppelingen(await api.koppelInHoofdmap(r.name, r.naam))
      verversOverzicht()
      setMelding(`${r.naam} staat nu in de hoofdmap; de eerste synchronisatie loopt`)
      api.syncNu().then(verwerkSync).catch(() => {})
    })
  const alInHoofdmap = (r: Ruimte) => koppelingen().some((k) => k.ruimte === r.name && !k.deling && inHoofdmap(k))

  const cloudmap = (naam: string, f: (map: string) => Promise<number>) =>
    doe("Cloud-map maken", async () => {
      const map = await open({ directory: true, multiple: false, title: `Lege map voor cloud-map ${naam}` })
      if (typeof map !== "string") return setMelding("Geen map gekozen")
      const n = await f(map)
      setMelding(`${n} bestand(en) als placeholder geplaatst in ${map}`)
    })

  const openInBrowser = (g: Gedeeld) => api.openGedeeld(g.deling).catch((e) => setMelding(String(e)))
  const beheer = () => api.openBeheer().catch((e) => setMelding(String(e)))

  /** Gekoppeld aan …: de lokale mappen, met een klik naar het overzicht. */
  const Gekoppeld = (props: { ruimte: string; deling?: string }) => (
    <Show when={gekoppeldAan(props.ruimte, props.deling).length}>
      <p class="gekoppeld">
        Gekoppeld aan{" "}
        <For each={gekoppeldAan(props.ruimte, props.deling)}>
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
  )

  return (
    <div class="scherm">
      <div class="schermkop">
        <div>
          <h1>Ruimtes</h1>
          <p class="uitleg">
            De ruimtes waar je lid van bent, en wat anderen met je deelden. Koppel een ruimte of gedeelde map aan een map om de
            bestanden op deze computer te hebben.
          </p>
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
        when={ruimtes().length || gedeeld().length}
        fallback={
          <Show when={ruimtesOpgehaald()} fallback={<p class="hint">Ruimtes ophalen…</p>}>
            <section class="leeg">
              <div class="leeg-icoon">▢</div>
              <h2>Nog geen ruimte</h2>
              <p>
                Je hebt nog geen ruimte, en er is nog niets met je gedeeld. Een beheerder maakt ruimtes aan in Beheer → Opslag van je
                omgeving.
              </p>
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
                  <button class="ruimte" classList={{ actief: ruimte()?.name === r.name }} onClick={() => setGekozen(`r:${r.name}`)}>
                    <span class="ruimte-naam">{r.naam}</span>
                    <span class="ruimte-meta">{[soort(r.soort), rol(r.rol)].filter(Boolean).join(" · ")}</span>
                    <Show when={gekoppeldAan(r.name).length}>
                      <span class="label groen klein">Gekoppeld</span>
                    </Show>
                  </button>
                </li>
              )}
            </For>
            <Show when={gedeeld().length}>
              <li class="lijstkop">Gedeeld met mij</li>
              <For each={gedeeld()}>
                {(g) => (
                  <li>
                    <button
                      class="ruimte"
                      classList={{ actief: gedeeldItem()?.deling === g.deling }}
                      onClick={() => setGekozen(`d:${g.deling}`)}
                    >
                      <span class="ruimte-naam">
                        <span class={`icoon ${g.doel_doctype === "Opslagmap" ? "map-icoon" : "bestand-icoon"}`} />
                        {g.naam}
                      </span>
                      <span class="ruimte-meta">{gedeeldMeta(g)}</span>
                      <Show when={gekoppeldAan(g.ruimte, g.deling).length}>
                        <span class="label groen klein">Gekoppeld</span>
                      </Show>
                    </button>
                  </li>
                )}
              </For>
            </Show>
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
                    <Show when={hoofdmap()?.hoofdmap && !alInHoofdmap(r())}>
                      <button class="primair" disabled={bezig()} onClick={() => inDeHoofdmap(r())} title={`In ${hoofdmap()?.hoofdmap}`}>
                        In de hoofdmap synchroniseren
                      </button>
                    </Show>
                                        <button
                      classList={{ primair: !hoofdmap()?.hoofdmap }}
                      disabled={bezig()}
                      onClick={() => koppel(r().naam, async (map) => setKoppelingen(await api.koppel(r().name, r().naam, map)))}
                    >
                      Koppelen aan map…
                    </button>
                    <button
                      disabled={bezig()}
                      onClick={() => cloudmap(r().naam, (map) => api.maakCloudmap(r().name, map))}
                      title="Alleen Windows: bestanden worden opgehaald zodra je ze opent"
                    >
                      Cloud-map…
                    </button>
                  </div>
                </div>
                <Gekoppeld ruimte={r().name} />
                <Bestandsweergave plek={{ ruimte: r().name, naam: r().naam, ruimte_naam: r().naam, magDelen: true }} />
              </section>
            )}
          </Show>

          <Show when={gedeeldItem()}>
            {(g) => (
              <section class="kaart ruimte-detail">
                <div class="koppeling-kop">
                  <div>
                    <h2>{g().naam}</h2>
                    <span class="soort">
                      {[g().doel_doctype === "Opslagmap" ? "gedeelde map" : "gedeeld bestand", g().ruimte_naam ? `in ${g().ruimte_naam}` : ""]
                        .filter(Boolean)
                        .join(" ")}
                    </span>
                  </div>
                  <div class="knoppen">
                    <Show
                      when={g().doel_doctype === "Opslagmap"}
                      fallback={
                        <button class="primair" onClick={() => openInBrowser(g())}>
                          Openen in de browser
                        </button>
                      }
                    >
                      <Show when={hoofdmap()?.hoofdmap && !koppelingen().some((k) => k.deling === g().deling && inHoofdmap(k))}>
                        <button
                          class="primair"
                          disabled={bezig()}
                          title={`In ${hoofdmap()?.hoofdmap} → Gedeeld met mij`}
                          onClick={() =>
                            doe("Synchroniseren in de hoofdmap", async () => {
                              setKoppelingen(await api.koppelGedeeldInHoofdmap(g()))
                              verversOverzicht()
                              setMelding(`${g().naam} staat nu in de hoofdmap onder Gedeeld met mij; de eerste synchronisatie loopt`)
                              api.syncNu().then(verwerkSync).catch(() => {})
                            })
                          }
                        >
                          In de hoofdmap synchroniseren
                        </button>
                      </Show>
                      <button
                        classList={{ primair: !hoofdmap()?.hoofdmap }}
                        disabled={bezig()}
                        onClick={() => koppel(g().naam, async (map) => setKoppelingen(await api.koppelGedeeld(g(), map)))}
                      >
                        Koppelen aan map…
                      </button>
                      <button
                        disabled={bezig()}
                        onClick={() => cloudmap(g().naam, (map) => api.maakCloudmap(g().ruimte, map, g()))}
                        title="Alleen Windows: bestanden worden opgehaald zodra je ze opent"
                      >
                        Cloud-map…
                      </button>
                    </Show>
                  </div>
                </div>
                <dl class="gegevens gedeeld-gegevens">
                  <div>
                    <dt>Gedeeld door</dt>
                    <dd>{g().gedeeld_door_naam || g().gedeeld_door || "onbekend"}</dd>
                  </div>
                  <div>
                    <dt>Jouw rol</dt>
                    <dd>{rechtenTekst("gebruiker", g().rechten)}</dd>
                  </div>
                  <div>
                    <dt>Geldig tot</dt>
                    <dd>{g().verloopt ? datumTekst(g().verloopt!) : "geen vervaldatum"}</dd>
                  </div>
                </dl>
                <Show when={g().rechten === "lezen" && g().doel_doctype === "Opslagmap"}>
                  <p class="hint">
                    Alleen bekijken: wat je op deze computer wijzigt, gaat niet naar de server en staat bij de overgeslagen bestanden
                    ("alleen lezen gedeeld").
                  </p>
                </Show>
                <Show when={g().al_lid && g().rol}>
                  <p class="hint">
                    Je bent ook {rol(g().rol!)} in {g().ruimte_naam}; deze deling geeft je hier{" "}
                    {rechtenTekst("gebruiker", g().rechten).toLowerCase()}.
                  </p>
                </Show>
                <Gekoppeld ruimte={g().ruimte} deling={g().deling} />
                <Show
                  when={g().doel_doctype === "Opslagmap"}
                  fallback={
                    <p class="hint">
                      {g().grootte != null ? `${grootte(g().grootte!)} · ` : ""}Een gedeeld bestand synct niet mee: open het in de
                      browser om het te downloaden{g().rechten !== "lezen" ? " of een nieuwe versie te uploaden" : ""}.
                    </p>
                  }
                >
                  <Bestandsweergave
                    plek={{
                      ruimte: g().ruimte,
                      naam: g().naam,
                      ruimte_naam: g().ruimte_naam ?? g().naam,
                      deling: g().deling,
                      magDelen: g().rechten === "beheren",
                    }}
                  />
                </Show>
              </section>
            )}
          </Show>
        </div>
      </Show>
    </div>
  )
}
