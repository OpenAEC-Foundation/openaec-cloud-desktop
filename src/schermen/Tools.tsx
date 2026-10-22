import { createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js"
import { ask } from "@tauri-apps/plugin-dialog"
import { open as openExtern } from "@tauri-apps/plugin-shell"
import { api, type DownloadVoortgang, type ToolVraag } from "../lib/api"
import { categorieNamen, DESKTOP_TOOLS, WEB_TOOLS, type Tool } from "../lib/catalogus"
import {
  eigenRepo, grootte, kanInstalleren, statusVan, voegSamen, zoekTreffer, type GeinstalleerdeTool, type ReleaseInfo, type ToolStatus,
} from "../lib/tools"
import { setMelding } from "../lib/winkel"

// Toestand op moduleniveau: blijft bewaard als je naar een ander scherm gaat en terugkomt (ook een lopende download).
const [geinstalleerd, setGeinstalleerd] = createSignal<Record<string, GeinstalleerdeTool>>({})
const [releases, setReleases] = createSignal<Record<string, ReleaseInfo>>({})
const [verversen, setVerversen] = createSignal(false)
const [laatsteControle, setLaatsteControle] = createSignal<Date | null>(null)
const [voortgang, setVoortgang] = createSignal<Record<string, DownloadVoortgang>>({})
const [bezigMet, setBezigMet] = createSignal<Record<string, boolean>>({})
const [fouten, setFouten] = createSignal<Record<string, string>>({})
const [notities, setNotities] = createSignal<Record<string, string>>({})
const [zoekterm, setZoekterm] = createSignal("")

// Releases veranderen hooguit een paar keer per week, terwijl elke ronde ruim tien GitHub-aanroepen kost tegen een
// limiet van 60 per uur per IP-adres (een kantoor deelt die). Daarom 6 uur bewaren; Vernieuwen forceert een ronde.
const CACHE_SLEUTEL = "tools.releases.v1"
const CACHE_DUUR = 6 * 60 * 60 * 1000

interface Cache {
  ts: number
  data: ReleaseInfo[]
  /** Epoch-ms waarop de GitHub-limiet vrijgeeft; tot dan heeft proberen geen zin. */
  vrijOp?: number | null
}

const leesCache = (): Cache | null => {
  try {
    return JSON.parse(localStorage.getItem(CACHE_SLEUTEL) ?? "null")
  } catch {
    return null
  }
}
const schrijfCache = (c: Cache) => {
  try {
    localStorage.setItem(CACHE_SLEUTEL, JSON.stringify(c))
  } catch {
    // alleen een gemak; zonder cache wordt er vaker gecontroleerd
  }
}

const zet = <T,>(set: (f: (v: Record<string, T>) => Record<string, T>) => void, id: string, waarde: T | undefined) =>
  set((v) => {
    const { [id]: _weg, ...rest } = v
    return waarde === undefined ? rest : { ...rest, [id]: waarde }
  })

/** Aantal geïnstalleerde tools met een update (voor de teller in de zijbalk). */
export const aantalToolUpdates = () =>
  DESKTOP_TOOLS.filter((t) => statusVan(t, geinstalleerd()[t.id], releases()[t.id]) === "update").length

const vraag = (t: Tool): ToolVraag => ({ id: t.id, displayName: t.registerNaam ?? t.naam, exeName: t.exeNaam ?? null })

const scanGeinstalleerd = async () => {
  try {
    const lijst = await api.toolsGeinstalleerd(DESKTOP_TOOLS.map(vraag))
    setGeinstalleerd(Object.fromEntries(lijst.map((t) => [t.id, t])))
  } catch {
    // buiten de app (bijv. vite preview) blijft de lijst leeg
  }
}

const haalReleases = async (forceer: boolean) => {
  if (verversen()) return
  setVerversen(true)
  try {
    const cache = leesCache()
    const bekend = Object.fromEntries((cache?.data ?? []).map((r) => [r.id, r]))
    if (cache?.data?.length) {
      setReleases(bekend)
      if (cache.ts) setLaatsteControle(new Date(cache.ts))
    }
    if (!forceer) {
      if (cache?.ts && Date.now() - cache.ts < CACHE_DUUR) return
      if (cache?.vrijOp && Date.now() < cache.vrijOp) return
    }
    const repos = DESKTOP_TOOLS.filter(eigenRepo).map((t) => {
      const [owner, repo] = t.repo!.split("/")
      return { id: t.id, owner, repo }
    })
    const data = await api.toolsReleases(repos)
    const samen = voegSamen(bekend, data)
    setReleases(samen)
    const ietsGelukt = data.some((r) => r.ok)
    const vrijOp = data.find((r) => r.rateLimitReset)?.rateLimitReset ?? null
    const ts = ietsGelukt ? Date.now() : cache?.ts ?? 0
    if (ietsGelukt) setLaatsteControle(new Date(ts))
    schrijfCache({ ts, data: Object.values(samen), vrijOp: vrijOp ? vrijOp * 1000 : null })
  } catch {
    // aanroep mislukt: bestaande gegevens blijven staan
  } finally {
    setVerversen(false)
  }
}

const vernieuw = (forceer: boolean) => {
  void scanGeinstalleerd()
  void haalReleases(forceer)
}

let luistert = false
const luisterNaarVoortgang = () => {
  if (luistert) return
  luistert = true
  api.opToolDownload((v) => setVoortgang((p) => ({ ...p, [v.id]: v }))).catch(() => (luistert = false))
}

const openUrl = (url?: string) => {
  if (url) openExtern(url).catch((e) => setMelding(`Openen mislukt: ${e}`))
}

const installeer = async (tool: Tool) => {
  const r = releases()[tool.id]
  if (!r?.assetUrl || !r.assetName) return
  zet(setBezigMet, tool.id, true)
  zet(setFouten, tool.id, undefined)
  zet(setNotities, tool.id, undefined)
  try {
    const uitkomst = await api.toolInstalleren(tool.id, r.assetUrl, r.assetName, r.assetDigest, tool.naam)
    if (uitkomst.mode === "installed") {
      zet(setNotities, tool.id, "Geïnstalleerd.")
      void scanGeinstalleerd()
    } else {
      zet(setNotities, tool.id, "De installatie is gestart; volg de stappen in het installatievenster.")
    }
  } catch (e) {
    zet(setFouten, tool.id, String(e))
  } finally {
    zet(setBezigMet, tool.id, undefined)
    zet(setVoortgang, tool.id, undefined)
  }
}

const start = async (tool: Tool) => {
  const exe = geinstalleerd()[tool.id]?.exePath
  if (!exe) return
  try {
    await api.toolStarten(exe)
    setMelding(`${tool.naam} wordt gestart`)
  } catch (e) {
    zet(setFouten, tool.id, String(e))
  }
}

const verwijder = async (tool: Tool) => {
  const zeker = await ask(`${tool.naam} van deze computer verwijderen?`, { title: "Tool verwijderen", kind: "warning", okLabel: "Verwijderen", cancelLabel: "Annuleren" }).catch(() => false)
  if (!zeker) return
  zet(setFouten, tool.id, undefined)
  try {
    const uitkomst = await api.toolVerwijderen(vraag(tool))
    zet(setNotities, tool.id, uitkomst === "removed" ? "Verwijderd." : "Het verwijderprogramma van de tool is gestart.")
    void scanGeinstalleerd()
  } catch (e) {
    zet(setFouten, tool.id, String(e))
  }
}

const statusLabel: Record<ToolStatus, [string, string]> = {
  actueel: ["Geïnstalleerd", "groen"],
  geinstalleerd_onbekend: ["Geïnstalleerd", "groen"],
  update: ["Update beschikbaar", "oranje"],
  niet_geinstalleerd: ["Niet geïnstalleerd", "grijs"],
  web: ["Webtool", "blauw"],
}

/** Waarom een desktop-tool hier (nu) niet te installeren is, als dat zo is. */
const hindernis = (tool: Tool, r: ReleaseInfo | undefined): string | null => {
  if (tool.soort !== "desktop") return null
  if (!eigenRepo(tool)) return "Community-project: installeren via de GitHub-pagina van het project."
  if (r && !r.ok) {
    if (r.error === "rate_limited" && r.rateLimitReset) {
      const tijd = new Date(r.rateLimitReset * 1000).toLocaleTimeString("nl-NL", { hour: "2-digit", minute: "2-digit" })
      return `GitHub-limiet bereikt; om ${tijd} kan het weer.`
    }
    if (r.error === "no_releases") return "Nog geen release op GitHub."
    return "Versie-informatie niet beschikbaar (geen verbinding?)."
  }
  if (r?.ok && !r.assetUrl) return "Geen installer voor dit besturingssysteem."
  return null
}

function ToolKaart(props: { tool: Tool }) {
  const t = props.tool
  const [previewKapot, setPreviewKapot] = createSignal(false)
  const inst = () => geinstalleerd()[t.id]
  const rel = () => releases()[t.id]
  const status = () => statusVan(t, inst(), rel())
  const laatste = () => (rel()?.ok ? rel()!.version : null)
  const kan = () => kanInstalleren(t, status(), rel())
  const bezig = () => !!bezigMet()[t.id]
  const v = () => voortgang()[t.id]
  const downloadt = () => !!v() && (!v()!.done || bezig())
  const pct = () => (v()?.total ? Math.min(100, Math.round((v()!.downloaded / v()!.total!) * 100)) : null)
  const github = t.repo ? `https://github.com/${t.repo}` : undefined
  const releasePagina = () => rel()?.pageUrl ?? (github ? `${github}/releases/latest` : undefined)

  // Klik op de afbeelding: starten als de tool er is, anders de webversie of de productpagina — nooit installeren.
  const snelkoppeling = () => {
    if (t.soort === "desktop" && inst()?.exePath) return { tekst: "Starten", doe: () => start(t) }
    const url = t.webUrl ?? t.siteUrl ?? github
    return url ? { tekst: t.webUrl ? "Webversie openen" : "Meer informatie", doe: () => openUrl(url) } : null
  }

  return (
    <article class="kaart toolkaart" classList={{ "met-update": status() === "update" }}>
      <button
        class="tool-preview"
        disabled={!snelkoppeling()}
        title={snelkoppeling()?.tekst}
        aria-label={snelkoppeling() ? `${t.naam}: ${snelkoppeling()!.tekst}` : t.naam}
        onClick={() => snelkoppeling()?.doe()}
      >
        <span class="tool-preview-leeg">{categorieNamen[t.categorie]}</span>
        <Show when={t.preview && !previewKapot()}>
          <img src={t.preview} alt="" loading="lazy" onError={() => setPreviewKapot(true)} />
        </Show>
        <Show when={snelkoppeling()}>
          <span class="tool-preview-actie">{snelkoppeling()!.tekst}</span>
        </Show>
      </button>

      <div class="tool-kop">
        <Show when={t.logo}>
          <img class="tool-logo" src={t.logo} alt="" width={28} height={28} />
        </Show>
        <div class="tool-titel">
          <h2>{t.naam}</h2>
          <span class="soort">
            {categorieNamen[t.categorie]}
            {t.community ? " · community" : ""}
          </span>
        </div>
        <span class={`label ${statusLabel[status()][1]}`}>{statusLabel[status()][0]}</span>
      </div>

      <p class="tool-omschrijving">{t.omschrijving}</p>

      <Show when={t.soort === "desktop"}>
        <div class="tool-versie">
          <Show when={inst()?.version}>
            <span title="Geïnstalleerde versie">v{inst()!.version}</span>
          </Show>
          <Show when={status() === "update" && laatste()}>
            <span class="pijl">→</span>
            <strong title="Nieuwste versie">v{laatste()}</strong>
          </Show>
          <Show when={!inst()?.version && laatste()}>
            <span title="Nieuwste versie">v{laatste()}</span>
          </Show>
          <Show when={!inst()?.version && !laatste()}>
            <span>–</span>
          </Show>
          <Show when={kan() && rel()?.assetSize}>
            <span class="tool-grootte">{grootte(rel()!.assetSize)}</span>
          </Show>
        </div>
      </Show>

      <Show when={downloadt()}>
        <div class="tool-voortgang">
          <span class="balk">
            <span classList={{ onbepaald: pct() === null }} style={pct() !== null ? { width: `${pct()}%` } : undefined} />
          </span>
          <span class="cijfers">{v()?.done ? "Controleren en starten…" : `Downloaden${pct() !== null ? ` ${pct()}%` : "…"}`}</span>
        </div>
      </Show>

      <Show when={fouten()[t.id]}>
        <p class="foutvlak">{fouten()[t.id]}</p>
      </Show>
      <Show when={!fouten()[t.id] && !downloadt() && notities()[t.id]}>
        <p class="succesvlak">{notities()[t.id]}</p>
      </Show>
      <Show when={!fouten()[t.id] && !downloadt() && !notities()[t.id] && hindernis(t, rel())}>
        <p class="hint">{hindernis(t, rel())}</p>
      </Show>

      <div class="knoppen tool-knoppen">
        <Show
          when={t.soort === "desktop"}
          fallback={
            <button class="primair" onClick={() => openUrl(t.webUrl)}>
              Openen
            </button>
          }
        >
          <Show when={kan()}>
            <button class="primair" disabled={bezig()} onClick={() => installeer(t)}>
              {bezig() ? "Bezig…" : status() === "update" ? "Bijwerken" : "Installeren"}
            </button>
          </Show>
          <Show when={inst()?.exePath}>
            <button classList={{ primair: !kan() }} onClick={() => start(t)}>
              Starten
            </button>
          </Show>
          <Show when={inst()}>
            <button class="gevaar-licht" disabled={bezig()} onClick={() => verwijder(t)}>
              Verwijderen
            </button>
          </Show>
        </Show>
        <span class="tool-links">
          <Show when={t.soort === "desktop" && t.webUrl}>
            <button class="tekstknop" onClick={() => openUrl(t.webUrl)}>
              Webversie
            </button>
          </Show>
          <Show when={t.soort === "desktop" ? releasePagina() : github}>
            <button class="tekstknop" onClick={() => openUrl(t.soort === "desktop" ? releasePagina() : github)}>
              GitHub
            </button>
          </Show>
        </span>
      </div>
    </article>
  )
}

export default function Tools() {
  onMount(() => {
    luisterNaarVoortgang()
    vernieuw(false)
    // Na een setup of uninstaller komt het venster weer in beeld: dan opnieuw kijken wat er geïnstalleerd is.
    const bijFocus = () => void scanGeinstalleerd()
    window.addEventListener("focus", bijFocus)
    onCleanup(() => window.removeEventListener("focus", bijFocus))
  })

  const treffer = (t: Tool) => zoekTreffer(t, zoekterm(), categorieNamen[t.categorie])
  const desktop = createMemo(() => DESKTOP_TOOLS.filter(treffer))
  const web = createMemo(() => WEB_TOOLS.filter(treffer))
  const aantalGeinstalleerd = () => DESKTOP_TOOLS.filter((t) => geinstalleerd()[t.id]).length
  const aantalUpdates = aantalToolUpdates

  return (
    <div class="scherm">
      <div class="schermkop">
        <div>
          <h1>Tools</h1>
          <p class="uitleg">
            De open source bouwsoftware van OpenAEC: installeren, bijwerken en starten. {aantalGeinstalleerd()} van {DESKTOP_TOOLS.length}{" "}
            desktop-apps op deze computer
            <Show when={aantalUpdates()}>, {aantalUpdates()} met een update</Show>.
          </p>
        </div>
        <div class="tool-balk">
          <input
            class="zoekveld"
            type="search"
            placeholder="Zoeken op naam of categorie"
            value={zoekterm()}
            onInput={(e) => setZoekterm(e.currentTarget.value)}
            spellcheck={false}
          />
          <button disabled={verversen()} onClick={() => vernieuw(true)} title={laatsteControle() ? `Laatst gecontroleerd: ${laatsteControle()!.toLocaleString("nl-NL")}` : undefined}>
            {verversen() ? "Controleren…" : "Vernieuwen"}
          </button>
        </div>
      </div>

      <Show when={desktop().length}>
        <h2 class="sectiekop">
          Desktop-apps <span class="teller">{desktop().length}</span>
        </h2>
        <div class="toolraster">
          <For each={desktop()}>{(t) => <ToolKaart tool={t} />}</For>
        </div>
      </Show>

      <Show when={web().length}>
        <h2 class="sectiekop">
          Webtools <span class="teller">{web().length}</span>
        </h2>
        <div class="toolraster">
          <For each={web()}>{(t) => <ToolKaart tool={t} />}</For>
        </div>
      </Show>

      <Show when={!desktop().length && !web().length}>
        <section class="leeg">
          <h2>Niets gevonden</h2>
          <p>Geen tool past bij "{zoekterm().trim()}".</p>
        </section>
      </Show>

      <p class="hint">
        Installers komen alleen uit de releases van OpenAEC-Foundation op GitHub. Elke download wordt gecontroleerd tegen het controlegetal van
        GitHub; op Windows start een installer alleen als hij digitaal ondertekend is. De losse OpenAEC Installer kan naast dit tabblad
        gebruikt blijven worden: beide zien dezelfde installaties.
      </p>
    </div>
  )
}
