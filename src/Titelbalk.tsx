import { createSignal, onCleanup, onMount, type JSX } from "solid-js"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { useAppVersie, versieLabel } from "./lib/appversie"

/** Het OpenAEC-symbool (open bouwblok), vereenvoudigd voor 16–18 px. */
export function Symbool(props: { class?: string }) {
  return (
    <svg class={props.class} viewBox="-72 -64 144 152" aria-hidden="true">
      <g stroke="#D97706" stroke-width="9" stroke-linejoin="round" stroke-linecap="round">
        <polygon points="-65,-23 0,15 0,82 -65,45" fill="#D97706" fill-opacity="0.35" />
        <polygon points="0,-58 65,-23 0,15 -65,-23" fill="#D97706" fill-opacity="0.2" />
        <path d="M65,-23V45L0,82M0,15V35" fill="none" />
      </g>
      <path d="M0,35L65,0" stroke="#F59E0B" stroke-width="8" stroke-linecap="round" />
    </svg>
  )
}

/**
 * Titelbalk van het hoofdvenster volgens de OpenAEC-standaardschil: 32 px, symbool links, appnaam met versie in het
 * midden, rechts wat er extra bij hoort (de verbonden omgeving) en de eigen vensterknoppen. Slepen en dubbelklikken
 * (maximaliseren) doet Tauri via `data-tauri-drag-region`.
 */
export default function Titelbalk(props: { rechts?: JSX.Element }) {
  const versie = useAppVersie()
  const venster = getCurrentWindow()
  const [gemaximaliseerd, setGemaximaliseerd] = createSignal(false)

  onMount(async () => {
    const bijwerken = () => venster.isMaximized().then(setGemaximaliseerd).catch(() => {})
    bijwerken()
    try {
      const stop = await venster.onResized(bijwerken)
      onCleanup(stop)
    } catch {
      /* buiten Tauri */
    }
  })

  return (
    <header class="titelbalk" data-tauri-drag-region>
      <div class="titelbalk-links" data-tauri-drag-region>
        <Symbool class="titelbalk-symbool" />
      </div>

      <div class="titelbalk-titel" data-tauri-drag-region>
        <span class="woordmerk" data-tauri-drag-region>
          Open<span>AEC</span> Desktop App
        </span>
        <span class="titelbalk-versie" data-tauri-drag-region>
          {versieLabel(versie())}
        </span>
      </div>

      <div class="titelbalk-rechts" data-tauri-drag-region>
        {props.rechts}
        <div class="vensterknoppen">
          <button class="vensterknop" title="Minimaliseren" aria-label="Minimaliseren" tabIndex={-1} onClick={() => venster.minimize()}>
            <svg viewBox="0 0 10 10" aria-hidden="true">
              <path d="M0 5.5h10" />
            </svg>
          </button>
          <button
            class="vensterknop"
            title={gemaximaliseerd() ? "Vorig formaat" : "Maximaliseren"}
            aria-label={gemaximaliseerd() ? "Vorig formaat" : "Maximaliseren"}
            tabIndex={-1}
            onClick={() => venster.toggleMaximize()}
          >
            <svg viewBox="0 0 10 10" aria-hidden="true">
              {gemaximaliseerd() ? <path d="M0.5 2.5h7v7h-7zM2.5 2.5v-2h7v7h-2" /> : <path d="M0.5 0.5h9v9h-9z" />}
            </svg>
          </button>
          {/* Sluiten verbergt het venster: de app blijft in het systeemvak synchroniseren. */}
          <button class="vensterknop sluiten" title="Sluiten" aria-label="Sluiten" tabIndex={-1} onClick={() => venster.close()}>
            <svg viewBox="0 0 10 10" aria-hidden="true">
              <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />
            </svg>
          </button>
        </div>
      </div>
    </header>
  )
}
