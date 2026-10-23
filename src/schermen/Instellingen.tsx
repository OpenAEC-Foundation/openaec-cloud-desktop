import { createSignal, onMount, Show } from "solid-js"
import { api, type SyncInstellingen } from "../lib/api"
import {
  basis, bezig, doe, koppelingen, setMelding, setRuimtes, setRuimtesOpgehaald, setScherm, setVerbonden, stand,
} from "../lib/winkel"

export default function Instellingen() {
  const [autostart, setAutostart] = createSignal(false)
  const [versie, setVersie] = createSignal("")
  const [update, setUpdate] = createSignal<"" | "zoeken" | "actueel" | string>("")
  const [updateFout, setUpdateFout] = createSignal("")
  const [sync, setSync] = createSignal<SyncInstellingen | null>(null)

  onMount(() => {
    api.autostart().then(setAutostart).catch(() => {})
    api.versie().then(setVersie).catch(() => {})
    api.syncInstellingen().then(setSync).catch(() => {})
  })

  const zetSync = (wijziging: Partial<SyncInstellingen>) => {
    const huidig = sync()
    if (!huidig) return
    api
      .zetSyncInstellingen({ ...huidig, ...wijziging })
      .then(setSync)
      .catch((f) => setMelding("Instellingen opslaan: " + f))
  }

  const controleer = async () => {
    setUpdate("zoeken")
    setUpdateFout("")
    try {
      setUpdate((await api.controleerUpdates()) ?? "actueel")
    } catch (e) {
      setUpdate("")
      setUpdateFout(String(e))
    }
  }

  const verbreek = () =>
    doe("Verbreken", async () => {
      await api.verbreek()
      setVerbonden(false)
      setRuimtes([])
      setRuimtesOpgehaald(false)
      setScherm("overzicht")
      setMelding("Verbinding verbroken; je gekoppelde mappen en bestanden blijven staan")
    })

  const nieuweVersie = () => !["", "zoeken", "actueel"].includes(update())

  return (
    <div class="scherm">
      <div class="schermkop">
        <div>
          <h1>Instellingen</h1>
        </div>
      </div>

      <section class="kaart">
        <h2>Account en server</h2>
        <dl class="gegevens">
          <div class="vol">
            <dt>Omgeving</dt>
            <dd class="pad">{basis()}</dd>
          </div>
          <div>
            <dt>Gekoppelde mappen</dt>
            <dd>{koppelingen().length}</dd>
          </div>
          <div>
            <dt>Synchronisatie</dt>
            <dd>{stand()?.gepauzeerd ? "gepauzeerd" : "actief"}</dd>
          </div>
        </dl>
        <div class="knoppen">
          <button onClick={() => api.pauzeer(!stand()?.gepauzeerd)}>{stand()?.gepauzeerd ? "Hervatten" : "Pauzeren"}</button>
          <button class="gevaar-licht" disabled={bezig()} onClick={verbreek}>
            Verbinding verbreken
          </button>
        </div>
        <p class="hint">Na verbreken blijven je gekoppelde mappen en bestanden op deze computer staan; ze synchroniseren weer als je opnieuw inlogt.</p>
      </section>

      <Show when={sync()}>
        {(s) => (
          <section class="kaart">
            <h2>Synchroniseren</h2>
            <label class="smal">
              Overdrachten tegelijk
              <input
                type="number"
                min="1"
                max="32"
                value={s().gelijktijdig}
                onChange={(e) => zetSync({ gelijktijdig: Math.min(32, Math.max(1, Math.round(Number(e.currentTarget.value) || 8))) })}
              />
            </label>
            <p class="hint">
              Zoveel bestanden (of delen van een groot bestand) gaan tegelijk over de lijn; standaard 8. Hoger helpt bij veel kleine
              bestanden op een snelle verbinding, lager bij een trage of gedeelde verbinding. Geldt vanaf de volgende ronde.
            </p>
          </section>
        )}
      </Show>

      <section class="kaart">
        <h2>Opstarten</h2>
        <label class="vinkje">
          <input
            type="checkbox"
            checked={autostart()}
            onChange={(e) => api.zetAutostart(e.currentTarget.checked).then(setAutostart).catch((f) => setMelding("Starten bij inloggen: " + f))}
          />
          Starten bij inloggen (op de achtergrond, in het systeemvak)
        </label>
        <p class="hint">
          Sluit je dit venster, dan blijft OpenAEC Desktop App synchroniseren. Klik op het pictogram in het systeemvak voor het statusvenster;
          afsluiten doe je via de rechtermuisknop op dat pictogram.
        </p>
      </section>

      <section class="kaart">
        <h2>Over OpenAEC Desktop App</h2>
        <dl class="gegevens">
          <div>
            <dt>Versie</dt>
            <dd>{versie() || "–"}</dd>
          </div>
        </dl>
        <div class="knoppen">
          <button disabled={update() === "zoeken"} onClick={controleer}>
            {update() === "zoeken" ? "Controleren…" : "Controleren op updates"}
          </button>
          <Show when={nieuweVersie()}>
            <button class="primair" onClick={() => api.installeerUpdate()}>
              Versie {update()} installeren
            </button>
          </Show>
        </div>
        <Show when={update() === "actueel"}>
          <p class="succesvlak">Je hebt de nieuwste versie.</p>
        </Show>
        <Show when={nieuweVersie()}>
          <p class="hint">Versie {update()} is gedownload. Na installeren start de app opnieuw; een lopende synchronisatie wordt eerst afgemaakt.</p>
        </Show>
        <Show when={updateFout()}>
          <p class="foutvlak">Controleren mislukt: {updateFout()}</p>
        </Show>
      </section>
    </div>
  )
}
