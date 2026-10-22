import { createSignal, For, Show } from "solid-js"
import { open } from "@tauri-apps/plugin-dialog"
import { api, type Versie } from "../lib/api"
import { grootte, naamVan, servertijd } from "../lib/opmaak"
import { bestand, bezig, doe, koppelingen, setBestand, setMelding } from "../lib/winkel"

// Op moduleniveau: het rechtermuismenu in de Verkenner zet ze ook als het scherm nog niet open is.
const [link, setLink] = createSignal("")
const [versielijst, setVersielijst] = createSignal<Versie[] | null>(null)

export const wisDeelresultaat = () => {
  setLink("")
  setVersielijst(null)
}

export const toonVersies = () =>
  doe("Versies ophalen", async () => {
    setVersielijst(await api.versies(bestand()))
    setMelding("")
  })

export default function Delen() {
  const [wachtwoord, setWachtwoord] = createSignal("")
  const [verloopt, setVerloopt] = createSignal("")

  const kies = async (map: boolean) => {
    const f = await open({ directory: map, multiple: false, defaultPath: koppelingen()[0]?.lokaal })
    if (typeof f === "string") {
      setBestand(f)
      wisDeelresultaat()
    }
  }

  const deel = () =>
    doe("Delen", async () => {
      setLink(await api.deel(bestand(), wachtwoord(), verloopt()))
      setMelding("Deellink gemaakt")
    })

  return (
    <div class="scherm">
      <div class="schermkop">
        <div>
          <h1>Delen</h1>
          <p class="uitleg">
            Deel een bestand of map met een link, vergrendel een bestand terwijl je eraan werkt, of haal een oudere versie terug. Dit kan ook
            via de rechtermuisknop in de Verkenner.
          </p>
        </div>
      </div>

      <section class="kaart">
        <h2>1. Kies een bestand of map</h2>
        <div class="keuze" classList={{ gekozen: !!bestand() }}>
          <Show when={bestand()} fallback={<span class="hint">Nog niets gekozen. Het moet in een gekoppelde map staan.</span>}>
            <div>
              <strong>{naamVan(bestand())}</strong>
              <span class="pad">{bestand()}</span>
            </div>
          </Show>
          <div class="knoppen">
            <button onClick={() => kies(false)}>Bestand kiezen…</button>
            <button onClick={() => kies(true)}>Map kiezen…</button>
          </div>
        </div>
      </section>

      <div class="twee-kolommen">
        <section class="kaart">
          <h2>2. Deellink maken</h2>
          <p class="uitleg">Iedereen met de link kan het bestand of de map bekijken en downloaden, zonder account.</p>
          <div class="velden">
            <label>
              Wachtwoord (optioneel)
              <input type="password" value={wachtwoord()} onInput={(e) => setWachtwoord(e.currentTarget.value)} />
            </label>
            <label>
              Geldig tot (optioneel)
              <input type="date" value={verloopt()} onInput={(e) => setVerloopt(e.currentTarget.value)} />
            </label>
          </div>
          <button class="primair" disabled={bezig() || !bestand()} onClick={deel}>
            Deellink maken
          </button>
          <Show when={link()}>
            <div class="deellink">
              <input readOnly value={link()} onFocus={(e) => e.currentTarget.select()} />
              <button onClick={() => navigator.clipboard.writeText(link()).then(() => setMelding("Link gekopieerd"))}>Kopiëren</button>
            </div>
          </Show>
        </section>

        <section class="kaart">
          <h2>Vergrendelen en versies</h2>
          <p class="uitleg">Vergrendel een bestand terwijl je eraan werkt: collega's zien dan dat jij ermee bezig bent.</p>
          <div class="knoppen">
            <button
              disabled={bezig() || !bestand()}
              onClick={() => doe("Vergrendelen", async () => { await api.vergrendel(bestand()); setMelding("Vergrendeld: collega's zien dat jij eraan werkt") })}
            >
              Vergrendelen
            </button>
            <button
              disabled={bezig() || !bestand()}
              onClick={() => doe("Ontgrendelen", async () => { await api.ontgrendel(bestand()); setMelding("Ontgrendeld") })}
            >
              Ontgrendelen
            </button>
            <button disabled={bezig() || !bestand()} onClick={toonVersies}>
              Versies bekijken
            </button>
          </div>
          <Show when={versielijst()}>
            {(lijst) => (
              <Show when={lijst().length} fallback={<p class="hint">Geen versies gevonden.</p>}>
                <ul class="versies">
                  <For each={lijst()}>
                    {(v, i) => (
                      <li>
                        <div>
                          <strong>
                            Versie {v.volgnummer}
                            {i() === 0 && <span class="label groen klein">huidig</span>}
                          </strong>
                          <span class="pad">{[servertijd(v.creation), v.maker, grootte(v.grootte)].filter(Boolean).join(" · ")}</span>
                        </div>
                        <button
                          class="klein"
                          disabled={bezig()}
                          onClick={() => doe("Downloaden", async () => { await api.openVersie(bestand(), v.volgnummer); setMelding(`Versie ${v.volgnummer} wordt gedownload`) })}
                        >
                          Downloaden
                        </button>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            )}
          </Show>
        </section>
      </div>
    </div>
  )
}
