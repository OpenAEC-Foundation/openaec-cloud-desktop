// De sessie is verlopen en vernieuwen lukt niet meer: één duidelijke vraag om opnieuw in te loggen, met de knop
// Inloggen. Voor het overzicht in het hoofdvenster en het statusvenster; de sync gaat verder zodra het gelukt is.

import { createSignal } from "solid-js"
import { api } from "./lib/api"

export const SESSIE_VERLOPEN = "Je sessie is verlopen. Log opnieuw in."

export default function SessieVerlopen(props: { basis: string; klaar: (tekst: string) => void; compact?: boolean }) {
  const [bezig, setBezig] = createSignal(false)

  const inloggen = async () => {
    setBezig(true)
    try {
      await api.logIn(props.basis)
      props.klaar("Je bent weer ingelogd; de synchronisatie gaat verder.")
    } catch (e) {
      props.klaar(`Inloggen mislukt: ${e}`)
    }
    setBezig(false)
  }

  return (
    <div class={props.compact ? "foutvlak sessie-verlopen compact" : "foutvlak sessie-verlopen"} role="alert">
      <span>{SESSIE_VERLOPEN}</span>
      <button class="primair" disabled={bezig() || !props.basis} onClick={inloggen}>
        {bezig() ? "Rond het af in je browser…" : "Inloggen"}
      </button>
    </div>
  )
}
