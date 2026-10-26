import { createSignal } from "solid-js"

/** Versie zoals de app hem toont: "v2026.44.0" (leeg zolang hij niet bekend is). */
export const versieLabel = (versie: string | null | undefined) => (versie ? `v${versie.replace(/^v/, "")}` : "")

const [appVersie, setAppVersie] = createSignal("")
let geladen = false

/** Het versienummer van de app (Tauri `getVersion()`), één keer opgehaald en gedeeld door titelbalk en instellingen. */
export function useAppVersie() {
  if (!geladen) {
    geladen = true
    import("@tauri-apps/api/app")
      .then(({ getVersion }) => getVersion())
      .then(setAppVersie)
      .catch(() => {})
  }
  return appVersie
}
