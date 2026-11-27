// De sessie is verlopen en vernieuwen lukt niet meer: één duidelijke vraag om opnieuw in te loggen, met de knop
// Inloggen, per omgeving waar het speelt. Voor het overzicht in het hoofdvenster en het statusvenster; de sync van die
// omgeving gaat verder zodra het gelukt is (andere omgevingen synchroniseren gewoon door).

import { createResource, createSignal, For } from "solid-js"
import { api } from "./lib/api"

export const SESSIE_VERLOPEN = "Je sessie is verlopen. Log opnieuw in."

export default function SessieVerlopen(props: { basis: string; klaar: (tekst: string) => void; compact?: boolean }) {
  const [bezig, setBezig] = createSignal("")
  // Welke omgevingen wachten; weet de app dat (nog) niet, dan de omgeving die meegegeven is.
  const [status, { refetch }] = createResource(() => api.status().catch(() => null))
  const verlopen = () => {
    const lijst = status()?.omgevingen.filter((o) => o.verlopen).map((o) => o.basis) ?? []
    return lijst.length ? lijst : props.basis ? [props.basis] : []
  }
  const meer = () => verlopen().length > 1 || (status()?.omgevingen.length ?? 0) > 1
  const kort = (b: string) => b.replace(/^https?:\/\//, "")

  const inloggen = async (b: string) => {
    setBezig(b)
    try {
      await api.logIn(b)
      props.klaar("Je bent weer ingelogd; de synchronisatie gaat verder.")
      refetch()
    } catch (e) {
      props.klaar(`Inloggen mislukt: ${e}`)
    }
    setBezig("")
  }

  return (
    <For each={verlopen()}>
      {(b) => (
        <div class={props.compact ? "foutvlak sessie-verlopen compact" : "foutvlak sessie-verlopen"} role="alert">
          <span>
            {SESSIE_VERLOPEN}
            {meer() ? ` (${kort(b)})` : ""}
          </span>
          <button class="primair" disabled={!!bezig()} onClick={() => inloggen(b)}>
            {bezig() === b ? "Rond het af in je browser…" : "Inloggen"}
          </button>
        </div>
      )}
    </For>
  )
}
