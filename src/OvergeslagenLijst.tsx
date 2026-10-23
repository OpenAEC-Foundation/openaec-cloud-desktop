// Overgeslagen bestanden (een naam die niet kan, zoals `nul`), met de knop Opruimen: voor het statusvenster en het
// overzicht in het hoofdvenster.

import { ask } from "@tauri-apps/plugin-dialog"
import { createSignal, For, Show } from "solid-js"
import { api, type KoppelOvergeslagen } from "./lib/api"

/** Zoveel bestanden per map in beeld; de rest als aantal. */
const ZICHTBAAR = 5

export default function OvergeslagenLijst(props: { lijst: KoppelOvergeslagen[]; klaar: (tekst: string) => void; compact?: boolean }) {
  const [bezig, setBezig] = createSignal(false)
  const opruimbaar = () => props.lijst.reduce((n, k) => n + k.bestanden.filter((b) => b.opruimbaar).length, 0)
  const totaal = () => props.lijst.reduce((n, k) => n + k.bestanden.length, 0)

  const ruimOp = async () => {
    const n = opruimbaar()
    const zeker = await ask(
      `${n} bestand${n === 1 ? "" : "en"} met een naam die niet kan (zoals nul) verwijderen? Ze gaan naar de prullenbak waar dat kan. ` +
        "Programma's maken zulke bestanden vaak per ongeluk; ze staan niet op de server.",
      { title: "Overgeslagen bestanden opruimen", kind: "warning", okLabel: "Opruimen", cancelLabel: "Annuleren" },
    ).catch(() => false)
    if (!zeker) return
    setBezig(true)
    try {
      const r = await api.ruimOvergeslagenOp(null)
      props.klaar(
        r.fouten.length
          ? `${r.verwijderd} opgeruimd, ${r.fouten.length} niet: ${r.fouten[0]}`
          : `${r.verwijderd} bestand${r.verwijderd === 1 ? "" : "en"} opgeruimd`,
      )
    } catch (e) {
      props.klaar(`Opruimen mislukt: ${e}`)
    }
    setBezig(false)
  }

  return (
    <Show when={totaal()}>
      <div class={props.compact ? "overgeslagen compact" : "overgeslagen"}>
        <div class="overgeslagen-kop">
          <strong>Overgeslagen bestanden ({totaal()})</strong>
          <Show when={opruimbaar()}>
            <button disabled={bezig()} onClick={ruimOp}>
              Opruimen
            </button>
          </Show>
        </div>
        <For each={props.lijst}>
          {(k) => (
            <ul>
              <For each={k.bestanden.slice(0, ZICHTBAAR)}>
                {(b) => (
                  <li title={`${k.lokaal}: ${b.pad}`}>
                    <span class="naam">{b.pad}</span>
                    <span class="reden">
                      {props.lijst.length > 1 ? `${k.ruimte_naam} · ` : ""}
                      {b.reden}
                    </span>
                  </li>
                )}
              </For>
              <Show when={k.bestanden.length > ZICHTBAAR}>
                <li class="meer">en nog {k.bestanden.length - ZICHTBAAR} in {k.ruimte_naam}</li>
              </Show>
            </ul>
          )}
        </For>
        <Show when={!props.compact}>
          <p class="hint">
            Deze bestanden gaan niet naar de server: hun naam kan niet op Windows (bijv. nul, con.log, of een naam die op een punt
            eindigt). Onder Instellingen kies je of ze worden overgeslagen, vanzelf opgeruimd of onder een veilige naam geüpload.
          </p>
        </Show>
      </div>
    </Show>
  )
}
