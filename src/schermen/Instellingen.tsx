import { createSignal, For, onMount, Show } from "solid-js"
import { api, type Gereserveerd, type SyncInstellingen } from "../lib/api"
import { useAppVersie, versieLabel } from "../lib/appversie"
import {
  basis, bezig, doe, kiesOmgeving, kortAdres, neemStatus, omgevingen, setGedeeld, setMelding, setRuimtes, setRuimtesOpgehaald,
  setScherm, setToevoegen, stand, ververs,
} from "../lib/winkel"

export default function Instellingen() {
  const [autostart, setAutostart] = createSignal(false)
  const versie = useAppVersie()
  const [update, setUpdate] = createSignal<"" | "zoeken" | "actueel" | string>("")
  const [updateFout, setUpdateFout] = createSignal("")
  const [sync, setSync] = createSignal<SyncInstellingen | null>(null)
  const [menuVersturen, setMenuVersturen] = createSignal(true)

  onMount(() => {
    api.autostart().then(setAutostart).catch(() => {})
    api.syncInstellingen().then(setSync).catch(() => {})
    api.menuVersturen().then(setMenuVersturen).catch(() => {})
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

  /** Uitloggen bij één omgeving; de andere blijven verbonden. */
  const verbreek = (b: string) =>
    doe(`Uitloggen bij ${kortAdres(b)}`, async () => {
      const gekozen = b === basis()
      const s = await api.verbreek(b)
      neemStatus(s)
      if (gekozen) {
        setRuimtes([])
        setGedeeld([])
        setRuimtesOpgehaald(false)
      }
      if (!s.verbonden) setScherm("overzicht")
      else if (gekozen) ververs()
      setMelding(`Uitgelogd bij ${kortAdres(b)}; de gekoppelde mappen en bestanden blijven staan`)
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
        <h2>Omgevingen</h2>
        <ul class="omgevingen">
          <For each={omgevingen()}>
            {(o) => (
              <li classList={{ gekozen: o.basis === basis() }}>
                <div class="omgeving-tekst">
                  <span class="pad">{kortAdres(o.basis)}</span>
                  <span class="hint">
                    {o.basis === basis() ? "gekozen · " : ""}
                    {o.koppelingen === 1 ? "1 gekoppelde map" : `${o.koppelingen} gekoppelde mappen`}
                    {o.verlopen ? " · opnieuw inloggen nodig" : !o.verbonden ? " · niet ingelogd" : ""}
                  </span>
                </div>
                <div class="knoppen">
                  <Show when={o.basis !== basis()}>
                    <button disabled={bezig()} onClick={() => kiesOmgeving(o.basis)}>
                      Kiezen
                    </button>
                  </Show>
                  <button class="gevaar-licht" disabled={bezig()} onClick={() => verbreek(o.basis)}>
                    Uitloggen
                  </button>
                </div>
              </li>
            )}
          </For>
        </ul>
        <div class="knoppen">
          <button disabled={bezig()} onClick={() => setToevoegen(true)}>
            Omgeving toevoegen
          </button>
          <button onClick={() => api.pauzeer(!stand()?.gepauzeerd)}>{stand()?.gepauzeerd ? "Synchroniseren hervatten" : "Synchroniseren pauzeren"}</button>
        </div>
        <p class="hint">
          Gekoppelde mappen synchroniseren altijd met hun eigen omgeving. Ruimtes, Versturen en de assistent werken met de
          gekozen omgeving. Na uitloggen blijven de gekoppelde mappen en bestanden op deze computer staan; ze synchroniseren
          weer als je opnieuw inlogt.
        </p>
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
            <label class="smal">
              Bestanden met een gereserveerde naam
              <select value={s().gereserveerd} onChange={(e) => zetSync({ gereserveerd: e.currentTarget.value as Gereserveerd })}>
                <option value="overslaan">Overslaan</option>
                <option value="verwijderen">Automatisch verwijderen als ze klein zijn</option>
                <option value="veilige_naam">Toch uploaden onder een veilige naam</option>
              </select>
            </label>
            <p class="hint">
              Een naam als nul, con.log of een naam die op een punt eindigt, kan niet op Windows. Programma's maken zulke bestanden vaak
              per ongeluk. Overslaan (standaard) zet ze in de lijst Overgeslagen bestanden, waar je ze kunt opruimen. Automatisch
              verwijderen ruimt ze op als ze leeg zijn of kleiner dan 64 KB (eerst naar de prullenbak). Een veilige naam uploadt ze
              als bijv. nul_; hier blijft de naam gelijk.
            </p>
          </section>
        )}
      </Show>

      <section class="kaart">
        <h2>Rechtermuismenu</h2>
        <label class="vinkje">
          <input
            type="checkbox"
            checked={menuVersturen()}
            onChange={(e) =>
              api
                .zetMenuVersturen(e.currentTarget.checked)
                .then(setMenuVersturen)
                .catch((f) => setMelding("Rechtermuismenu: " + f))
            }
          />
          "Versturen met OpenAEC" in het rechtermuismenu van de bestandsbeheerder
        </label>
        <p class="hint">
          Op elk bestand en elke map, ook buiten de gekoppelde mappen: verstuur ze met een downloadlink, eventueel per e-mail. Op
          Windows staat het in het menu van de Verkenner (in Windows 11 onder "Meer opties weergeven"), op Linux in Nautilus, Dolphin
          en Nemo, op macOS bij de snelle acties van de Finder. Gebruik je de AppImage, zet het dan hier uit voordat je de app
          weggooit.
        </p>
        <div class="knoppen">
          <button onClick={() => api.openVersturen()}>Venster Versturen openen</button>
        </div>
      </section>

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
            <dd>{versieLabel(versie()) || "–"}</dd>
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
