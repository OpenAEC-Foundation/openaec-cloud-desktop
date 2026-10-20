import { createSignal, For, onCleanup, onMount, Show } from "solid-js"
import { open } from "@tauri-apps/plugin-dialog"
import { api, type Koppeling, type MapRapport, type Ruimte } from "./lib/api"

const samenvatting = (m: MapRapport) => {
  if (m.fout) return `fout: ${m.fout}`
  const r = m.rapport!
  const delen = [
    r.geupload && `↑ ${r.geupload}`,
    r.gedownload && `↓ ${r.gedownload}`,
    r.verwijderd && `🗑 ${r.verwijderd}`,
    r.conflicten && `⚠ ${r.conflicten} conflict`,
  ].filter(Boolean)
  return delen.length ? delen.join(" · ") : "bijgewerkt"
}

export default function App() {
  const [basis, setBasis] = createSignal("")
  const [sleutel, setSleutel] = createSignal("")
  const [verbonden, setVerbonden] = createSignal(false)
  const [bezig, setBezig] = createSignal(false)
  const [melding, setMelding] = createSignal("Niet verbonden")
  const [ruimtes, setRuimtes] = createSignal<Ruimte[]>([])
  const [koppelingen, setKoppelingen] = createSignal<Koppeling[]>([])
  const [rapporten, setRapporten] = createSignal<Record<string, MapRapport>>({})
  const [laatste, setLaatste] = createSignal("")
  const [bestand, setBestand] = createSignal("")
  const [wachtwoord, setWachtwoord] = createSignal("")
  const [link, setLink] = createSignal("")

  const verwerk = (r: MapRapport[]) => {
    setRapporten(Object.fromEntries(r.map((m) => [m.lokaal, m])))
    setLaatste(new Date().toLocaleTimeString("nl-NL"))
  }

  const doe = async (wat: string, f: () => Promise<unknown>) => {
    setBezig(true)
    setMelding(wat + "…")
    try {
      await f()
    } catch (e) {
      setMelding(`${wat} mislukt: ${e}`)
    }
    setBezig(false)
  }

  onMount(async () => {
    const s = await api.status()
    setBasis(s.basis ?? "")
    setKoppelingen(s.koppelingen)
    if (s.verbonden) {
      setVerbonden(true)
      setMelding(`Verbonden met ${s.basis}`)
      api.ruimtes().then(setRuimtes).catch((e) => setMelding("Ruimtes ophalen mislukt: " + e))
    }
    const stop = await api.opSync(verwerk)
    onCleanup(stop)
  })

  const verbind = () =>
    doe("Verbinden", async () => {
      setRuimtes(await api.verbind(basis(), sleutel()))
      setSleutel("")
      setVerbonden(true)
      setMelding(`Verbonden met ${basis()}`)
    })

  const logIn = () =>
    doe("Inloggen (rond het af in je browser)", async () => {
      setRuimtes(await api.logIn(basis()))
      setVerbonden(true)
      setMelding(`Ingelogd bij ${basis()}`)
    })

  const verbreek = () =>
    doe("Verbreken", async () => {
      await api.verbreek()
      setVerbonden(false)
      setMelding("Niet verbonden")
    })

  const koppel = (r: Ruimte) =>
    doe("Koppelen", async () => {
      const map = await open({ directory: true, multiple: false, title: `Lokale map voor ${r.naam}` })
      if (typeof map !== "string") return setMelding("Geen map gekozen")
      setKoppelingen(await api.koppel(r.name, r.naam, map))
      verwerk(await api.syncNu())
      setMelding(`${r.naam} gekoppeld aan ${map}`)
    })

  const ontkoppel = (k: Koppeling) =>
    doe("Ontkoppelen", async () => {
      setKoppelingen(await api.ontkoppel(k.lokaal))
      setMelding(`${k.ruimte_naam} ontkoppeld; de lokale bestanden blijven staan`)
    })

  const syncNu = () =>
    doe("Synchroniseren", async () => {
      verwerk(await api.syncNu())
      setMelding("Synchronisatie klaar")
    })

  const kiesBestand = async () => {
    const f = await open({ directory: false, multiple: false })
    if (typeof f === "string") {
      setBestand(f)
      setLink("")
    }
  }

  const cloudmap = (r: Ruimte) =>
    doe("Cloud-map maken", async () => {
      const map = await open({ directory: true, multiple: false, title: `Lege map voor cloud-map ${r.naam}` })
      if (typeof map !== "string") return setMelding("Geen map gekozen")
      const n = await api.maakCloudmap(r.name, map)
      setMelding(`${n} bestand(en) als placeholder geplaatst in ${map}`)
    })

  const gekoppeld = (r: Ruimte) => koppelingen().some((k) => k.ruimte === r.name)

  return (
    <div class="app">
      <header class="titlebar" data-tauri-drag-region>
        <div class="wordmark">
          Open<span>AEC</span> Cloud
        </div>
        <div class="dot" classList={{ on: verbonden() }} title={verbonden() ? "verbonden" : "niet verbonden"} />
      </header>

      <main class="body">
        <Show
          when={verbonden()}
          fallback={
            <section class="card">
              <h2>Verbinden met OpenAEC Storage</h2>
              <label>
                Adres van je omgeving
                <input value={basis()} placeholder="https://jouwbedrijf.cloud.open-aec.com" onInput={(e) => setBasis(e.currentTarget.value)} />
              </label>
              <button class="primary" disabled={bezig() || !basis()} onClick={logIn}>
                Inloggen met OpenAEC-account
              </button>
              <details class="alternatief">
                <summary>Of verbind met een API-sleutel</summary>
                <label>
                  Sleutel
                  <input type="password" value={sleutel()} placeholder="token sleutel:geheim" onInput={(e) => setSleutel(e.currentTarget.value)} />
                </label>
                <p class="hint">Voor servers en scripts: maak een API-sleutel in je omgeving (Mijn instellingen → API-toegang).</p>
                <button disabled={bezig() || !basis() || !sleutel()} onClick={verbind}>
                  Verbinden met sleutel
                </button>
              </details>
            </section>
          }
        >
          <section class="card">
            <div class="kop">
              <h2>Gekoppelde mappen</h2>
              <button class="primary" disabled={bezig() || !koppelingen().length} onClick={syncNu}>
                Nu synchroniseren
              </button>
            </div>
            <Show when={koppelingen().length} fallback={<p class="hint">Nog geen mappen gekoppeld. Kies hieronder een ruimte.</p>}>
              <ul class="lijst">
                <For each={koppelingen()}>
                  {(k) => (
                    <li>
                      <div>
                        <strong>{k.ruimte_naam}</strong>
                        <span class="path">{k.lokaal}</span>
                        <Show when={rapporten()[k.lokaal]}>
                          {(m) => (
                            <span class="status" classList={{ fout: !!m().fout || !!m().rapport?.fouten.length }}>
                              {samenvatting(m())}
                            </span>
                          )}
                        </Show>
                      </div>
                      <button disabled={bezig()} onClick={() => ontkoppel(k)}>
                        Ontkoppelen
                      </button>
                    </li>
                  )}
                </For>
              </ul>
              <p class="hint">Synchroniseert automatisch elke 30 seconden{laatste() && ` · laatst ${laatste()}`}.</p>
            </Show>
          </section>

          <section class="card">
            <h2>Ruimtes</h2>
            <ul class="lijst">
              <For each={ruimtes()}>
                {(r) => (
                  <li>
                    <div>
                      <strong>{r.naam}</strong>
                      <span class="path">{r.rol}</span>
                    </div>
                    <div class="row">
                      <button disabled={bezig() || gekoppeld(r)} onClick={() => koppel(r)}>
                        {gekoppeld(r) ? "Gekoppeld" : "Koppel aan map…"}
                      </button>
                      <button disabled={bezig()} onClick={() => cloudmap(r)} title="Alleen Windows: bestanden worden opgehaald zodra je ze opent">
                        Cloud-map…
                      </button>
                    </div>
                  </li>
                )}
              </For>
            </ul>
          </section>

          <section class="card">
            <h2>Bestand vergrendelen of delen</h2>
            <div class="row">
              <button onClick={kiesBestand}>Bestand kiezen…</button>
              <span class="path">{bestand() || "(uit een gekoppelde map)"}</span>
            </div>
            <div class="row">
              <button disabled={bezig() || !bestand()} onClick={() => doe("Vergrendelen", async () => { await api.vergrendel(bestand()); setMelding("Vergrendeld: collega's zien dat jij eraan werkt") })}>
                Vergrendelen
              </button>
              <button disabled={bezig() || !bestand()} onClick={() => doe("Ontgrendelen", async () => { await api.ontgrendel(bestand()); setMelding("Ontgrendeld") })}>
                Ontgrendelen
              </button>
            </div>
            <label>
              Wachtwoord voor de link (optioneel)
              <input type="password" value={wachtwoord()} onInput={(e) => setWachtwoord(e.currentTarget.value)} />
            </label>
            <button disabled={bezig() || !bestand()} onClick={() => doe("Delen", async () => { setLink(await api.deel(bestand(), wachtwoord())); setMelding("Deellink gemaakt") })}>
              Deellink maken
            </button>
            <Show when={link()}>
              <div class="sharelink">
                <a href={link()} target="_blank" rel="noreferrer">{link()}</a>
              </div>
            </Show>
          </section>

          <button class="link" disabled={bezig()} onClick={verbreek}>
            Verbinding verbreken
          </button>
        </Show>
      </main>

      <footer class="statusbar">{melding()}</footer>
    </div>
  )
}
