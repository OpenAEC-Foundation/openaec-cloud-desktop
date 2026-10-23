import { createSignal, For, Match, onCleanup, onMount, Show, Switch } from "solid-js"
import { api } from "./lib/api"
import {
  basis, bezig, doe, koppelingen, melding, ruimtes, scherm, setBasis, setBestand, setKoppelingen, setMelding,
  setQuotum, setRuimtes, setRuimtesOpgehaald, setScherm, setStand, setVerbonden, stand, verbonden, verwerkSync, ververs,
  type Scherm,
} from "./lib/winkel"
import { naamVan, procent, rondeTekst } from "./lib/opmaak"
import Overzicht from "./schermen/Overzicht"
import Ruimtes from "./schermen/Ruimtes"
import Delen, { toonVersies, wisDeelresultaat } from "./schermen/Delen"
import Instellingen from "./schermen/Instellingen"
import Tools, { aantalToolUpdates } from "./schermen/Tools"

const schermen: { id: Scherm; titel: string; icoon: string }[] = [
  { id: "overzicht", titel: "Overzicht", icoon: "M3 12h4l3-8 4 16 3-8h4" },
  { id: "ruimtes", titel: "Ruimtes", icoon: "M3 6h6l2 2h10v11H3z" },
  { id: "delen", titel: "Delen", icoon: "M18 8a3 3 0 1 0-3-3M6 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 22a3 3 0 1 0-3-3M8.6 13.5l6.8 4M15.4 6.5l-6.8 4" },
  { id: "tools", titel: "Tools", icoon: "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM17 14v6M14 17h6" },
  { id: "instellingen", titel: "Instellingen", icoon: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19 12h2M3 12h2M12 3v2M12 19v2M17 7l1.4-1.4M5.6 18.4 7 17M17 17l1.4 1.4M5.6 5.6 7 7" },
]

function Inloggen() {
  const [sleutel, setSleutel] = createSignal("")

  const klaar = (r: Awaited<ReturnType<typeof api.ruimtes>>, tekst: string) => {
    setRuimtes(r)
    setRuimtesOpgehaald(true)
    setVerbonden(true)
    setMelding(tekst)
  }

  return (
    <div class="inloggen">
      <section class="kaart">
        <h2>Verbinden met OpenAEC Storage</h2>
        <p class="uitleg">Koppel mappen op deze computer aan de ruimtes van je omgeving. Bestanden blijven daarna vanzelf gelijk.</p>
        <label>
          Adres van je omgeving
          <input value={basis()} placeholder="https://jouwbedrijf.cloud.open-aec.eu" onInput={(e) => setBasis(e.currentTarget.value)} />
        </label>
        <button
          class="primair breed"
          disabled={bezig() || !basis()}
          onClick={() => doe("Inloggen (rond het af in je browser)", async () => klaar(await api.logIn(basis()), `Ingelogd bij ${basis()}`))}
        >
          Inloggen met OpenAEC-account
        </button>
        <details class="alternatief">
          <summary>Of verbind met een API-sleutel</summary>
          <label>
            Sleutel
            <input type="password" value={sleutel()} placeholder="token sleutel:geheim" onInput={(e) => setSleutel(e.currentTarget.value)} />
          </label>
          <p class="hint">Voor servers en scripts: maak een API-sleutel in je omgeving (Mijn instellingen → API-toegang).</p>
          <button
            disabled={bezig() || !basis() || !sleutel()}
            onClick={() =>
              doe("Verbinden", async () => {
                klaar(await api.verbind(basis(), sleutel()), `Verbonden met ${basis()}`)
                setSleutel("")
              })
            }
          >
            Verbinden met sleutel
          </button>
        </details>
      </section>
    </div>
  )
}

/** Smalle balk onderin: wat de sync doet, en de laatste melding van een actie. */
function Statusbalk() {
  const eerste = () => stand()?.actief[0]
  return (
    <footer class="statusbalk">
      <span class={`stip ${verbonden() ? stand()?.toestand ?? "bijgewerkt" : "gepauzeerd"}`} />
      <span class="samenvatting">{verbonden() ? stand()?.samenvatting ?? "Alles bijgewerkt" : "Niet verbonden"}</span>
      <Show when={stand()?.ronde}>
        {(r) => <span class="lopend">{rondeTekst(r())}</span>}
      </Show>
      <Show when={!stand()?.ronde && eerste()}>
        {(o) => (
          <span class="lopend">
            {o().richting === "upload" ? "↑" : "↓"} {naamVan(o().pad)} · {procent(o().gedaan, o().totaal)}%
          </span>
        )}
      </Show>
      <span class="melding" title={melding()}>
        {melding()}
      </span>
    </footer>
  )
}

export default function App() {
  onMount(async () => {
    const s = await api.status()
    setBasis(s.basis ?? "")
    setKoppelingen(s.koppelingen)
    if (s.verbonden) {
      setVerbonden(true)
      ververs()
    }
    api.activiteit().then(setStand).catch(() => {})

    // Rechtermuismenu in de Verkenner: "Verzenden…" en "Versies bekijken" komen in het deelscherm binnen.
    const stoppers = [
      await api.opActie(({ actie, pad }) => {
        setBestand(pad)
        wisDeelresultaat()
        setScherm("delen")
        if (actie === "versies") toonVersies()
      }),
      await api.opSync(verwerkSync),
      await api.opActiviteit(setStand),
      await api.opQuotum(setQuotum),
      // Het venster komt (weer) in beeld vanuit het systeemvak: ruimtes en overzicht verversen.
      await api.opVensterOpen(() => verbonden() && ververs()),
    ]
    const bijFocus = () => verbonden() && ververs()
    window.addEventListener("focus", bijFocus)
    onCleanup(() => {
      stoppers.forEach((stop) => stop())
      window.removeEventListener("focus", bijFocus)
    })
  })

  const teller = (id: Scherm) => {
    if (id === "overzicht") return koppelingen().length || ""
    if (id === "ruimtes") return ruimtes().length || ""
    if (id === "tools") return aantalToolUpdates() || ""
    return ""
  }

  return (
    <div class="app">
      <header class="kopbalk" data-tauri-drag-region>
        <div class="woordmerk" data-tauri-drag-region>
          Open<span>AEC</span> Desktop <em>App</em>
        </div>
        <Show when={verbonden()}>
          <div class="omgeving" title="Verbonden omgeving">
            <span class="stip bijgewerkt" />
            {basis().replace(/^https?:\/\//, "")}
          </div>
        </Show>
      </header>

      {/* De zijbalk staat er altijd: het tabblad Tools werkt ook zonder verbinding met een omgeving. */}
      <div class="werkvlak">
        <nav class="zijbalk">
          <For each={schermen}>
            {(s) => (
              <button class="navknop" classList={{ actief: scherm() === s.id }} onClick={() => setScherm(s.id)}>
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d={s.icoon} />
                </svg>
                <span>{s.titel}</span>
                <Show when={teller(s.id)}>
                  <span class="teller">{teller(s.id)}</span>
                </Show>
              </button>
            )}
          </For>
          <Show when={verbonden()}>
            <div class="zijbalk-voet">
              <button class="tekstknop" onClick={() => api.pauzeer(!stand()?.gepauzeerd)}>
                {stand()?.gepauzeerd ? "Synchroniseren hervatten" : "Synchroniseren pauzeren"}
              </button>
            </div>
          </Show>
        </nav>
        <main class="inhoud">
          <Switch>
            <Match when={scherm() === "tools"}>
              <Tools />
            </Match>
            <Match when={scherm() === "instellingen" && verbonden()}>
              <Instellingen />
            </Match>
            <Match when={!verbonden()}>
              <Inloggen />
            </Match>
            <Match when={scherm() === "overzicht"}>
              <Overzicht />
            </Match>
            <Match when={scherm() === "ruimtes"}>
              <Ruimtes />
            </Match>
            <Match when={scherm() === "delen"}>
              <Delen />
            </Match>
          </Switch>
        </main>
      </div>

      <Statusbalk />
    </div>
  )
}
