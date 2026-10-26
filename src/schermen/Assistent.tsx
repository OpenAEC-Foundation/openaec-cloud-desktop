// Het scherm Assistent: een gesprek met de AI-dienst van OpenAEC. Links de bewaarde gesprekken, rechts het gesprek met
// gestreamde antwoorden (markdown), de modelkeuze (met het EU-label) en het invoervak. Bestanden kunnen erin gesleept,
// gekozen of geplakt worden; per bijlage een chip met naam, grootte en een knop om hem weg te halen.
//
// De toestand staat op moduleniveau: een antwoord loopt door (en wordt bewaard) als je even naar een ander scherm gaat.

import { createEffect, createSignal, For, onCleanup, onMount, Show } from "solid-js"
import { getCurrentWebview } from "@tauri-apps/api/webview"
import { ask, open } from "@tauri-apps/plugin-dialog"
import { api, type AssistentStatus } from "../lib/api"
import {
  alsFout, datumTekst, gebruikTekst, KIESBARE_EXTENSIES, kiesModel, modelLabel, nieuwGesprek, PRIVACYMELDING, tekstInBijlagen,
  titelVan, verwerk, type AiFout, type Bericht, type Bijlage, type Gesprek, type Model, type Samenvatting,
} from "../lib/assistent"
import { grootte } from "../lib/opmaak"
import { koppelingen, setVoorAssistent, voorAssistent } from "../lib/winkel"
import Markdown from "../Markdown"

const PRIVACY_SLEUTEL = "assistent-privacy-gezien"
const MODEL_SLEUTEL = "assistent-model"

const lees = (sleutel: string) => {
  try {
    return localStorage.getItem(sleutel)
  } catch {
    return null
  }
}
const schrijf = (sleutel: string, waarde: string) => {
  try {
    localStorage.setItem(sleutel, waarde)
  } catch {
    /* geen opslag: dan volgende keer opnieuw */
  }
}

const [status, setStatus] = createSignal<AssistentStatus | null>(null)
const [modellen, setModellen] = createSignal<Model[]>([])
const [modelFout, setModelFout] = createSignal<AiFout | null>(null)
const [lijst, setLijst] = createSignal<Samenvatting[]>([])
const [gesprek, setGesprek] = createSignal<Gesprek>(nieuwGesprek(null))
const [klaar, setKlaar] = createSignal<Bijlage[]>([])
const [invoer, setInvoer] = createSignal("")
const [bezig, setBezig] = createSignal(false)
const [antwoord, setAntwoord] = createSignal<Bericht | null>(null)
/** De fout bij het laatste antwoord (met de soort, voor de knop erbij). */
const [fout, setFout] = createSignal<AiFout | null>(null)
const [bijlageFout, setBijlageFout] = createSignal("")
const [privacyGezien, setPrivacyGezien] = createSignal(lees(PRIVACY_SLEUTEL) === "ja")
const [inloggen, setInloggen] = createSignal(false)

const ververLijst = () => api.assistentGesprekken().then(setLijst).catch(() => {})

async function laadModellen() {
  try {
    const m = await api.assistentModellen()
    setModellen(m)
    setModelFout(null)
    const g = gesprek()
    if (!g.berichten.length || !g.model) setGesprek({ ...g, model: kiesModel(m, g.model ?? lees(MODEL_SLEUTEL)) })
  } catch (e) {
    setModelFout(alsFout(e))
  }
}

async function start() {
  try {
    setStatus(await api.assistentStatus())
  } catch {
    return
  }
  ververLijst()
  if (status()?.ingelogd) laadModellen()
  else setModelFout(null)
}

async function logIn() {
  setInloggen(true)
  try {
    await api.assistentLogIn()
    setStatus(await api.assistentStatus())
    if (fout()?.soort === "sessie_verlopen") setFout(null)
    await laadModellen()
  } catch (e) {
    setModelFout({ soort: "anders", melding: `Inloggen mislukt: ${e}` })
  }
  setInloggen(false)
}

function nieuw() {
  if (bezig()) return
  setGesprek(nieuwGesprek(kiesModel(modellen(), gesprek().model ?? lees(MODEL_SLEUTEL))))
  setKlaar([])
  setFout(null)
  setBijlageFout("")
}

async function kies(s: Samenvatting) {
  if (bezig()) return
  try {
    setGesprek(await api.assistentGesprek(s.id))
    setFout(null)
    setBijlageFout("")
  } catch (e) {
    setBijlageFout(String(e))
    ververLijst()
  }
}

async function verwijder(s: Samenvatting) {
  const zeker = await ask(`"${s.titel}" verwijderen? Dit kan niet ongedaan worden.`, { title: "Gesprek verwijderen", kind: "warning" }).catch(() => false)
  if (!zeker) return
  await api.assistentVerwijder(s.id).catch((e) => setBijlageFout(String(e)))
  if (gesprek().id === s.id) nieuw()
  ververLijst()
}

const afbeeldingen = () =>
  [...gesprek().berichten.flatMap((b) => b.bijlagen ?? []), ...klaar()].filter((b) => b.soort === "afbeelding").length

async function voegToe(maak: (alTekst: number) => Promise<Bijlage>) {
  try {
    const b = await maak(tekstInBijlagen(gesprek().berichten, klaar()))
    if (b.soort === "afbeelding" && afbeeldingen() >= 20) {
      setBijlageFout("Er kunnen hoogstens 20 afbeeldingen in één gesprek. Begin een nieuw gesprek.")
      return
    }
    setKlaar([...klaar(), b])
    setBijlageFout("")
  } catch (e) {
    setBijlageFout(String(e))
  }
}

async function voegPadenToe(paden: string[]) {
  for (const pad of paden) await voegToe((alTekst) => api.assistentBijlage(pad, alTekst))
}

async function kiesBestanden() {
  const gekozen = await open({
    multiple: true,
    defaultPath: koppelingen()[0]?.lokaal,
    filters: [
      { name: "Afbeeldingen, documenten en tekst", extensions: KIESBARE_EXTENSIES },
      { name: "Alle bestanden", extensions: ["*"] },
    ],
  })
  if (Array.isArray(gekozen)) await voegPadenToe(gekozen)
  else if (typeof gekozen === "string") await voegPadenToe([gekozen])
}

function alsBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer)
  let tekst = ""
  for (let i = 0; i < bytes.length; i += 0x8000) tekst += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
  return btoa(tekst)
}

async function plak(e: ClipboardEvent) {
  const bestanden = [...(e.clipboardData?.files ?? [])]
  if (!bestanden.length) return
  e.preventDefault()
  for (const f of bestanden) {
    const naam = f.name && f.name !== "image.png" ? f.name : `geplakt-${new Date().toISOString().slice(11, 19).replace(/:/g, "")}.png`
    const data = alsBase64(await f.arrayBuffer())
    await voegToe((alTekst) => api.assistentBijlageData(naam, data, alTekst))
  }
}

async function bewaar(g: Gesprek) {
  await api.assistentBewaar(g).catch((e) => setBijlageFout(`Gesprek bewaren mislukt: ${e}`))
  ververLijst()
}

/** Vraag het antwoord op de geschiedenis van `g` (die eindigt met een vraag) en bewaar het. */
async function vraag(g: Gesprek) {
  setBezig(true)
  setFout(null)
  setGesprek(g)
  await bewaar(g)
  setAntwoord({ rol: "assistant", tekst: "", model: g.model ?? null })
  let afgebroken = false
  try {
    const r = await api.assistentVraag(g, (ev) => setAntwoord((a) => (a ? verwerk(a, ev) : a)))
    afgebroken = r.afgebroken
  } catch (e) {
    const f = alsFout(e)
    setFout(f)
    setAntwoord((a) => (a ? { ...a, fout: f.melding } : a))
  }
  const slot: Bericht = { ...antwoord()!, ...(afgebroken ? { afgebroken: true } : {}) }
  const klaarGesprek = { ...g, berichten: [...g.berichten, slot], bijgewerkt: Date.now() }
  // Alleen bijwerken als hetzelfde gesprek nog open staat (anders blijft het open gesprek staan).
  if (gesprek().id === g.id) setGesprek(klaarGesprek)
  setAntwoord(null)
  setBezig(false)
  await bewaar(klaarGesprek)
}

function verstuur() {
  const tekst = invoer().trim()
  if (bezig() || (!tekst && !klaar().length) || !privacyGezien()) return
  const g = gesprek()
  const vraagBericht: Bericht = { rol: "user", tekst, ...(klaar().length ? { bijlagen: klaar() } : {}) }
  const titel = g.berichten.length ? g.titel : titelVan(tekst, klaar())
  setInvoer("")
  setKlaar([])
  setBijlageFout("")
  if (g.model) schrijf(MODEL_SLEUTEL, g.model)
  vraag({ ...g, titel, berichten: [...g.berichten, vraagBericht], bijgewerkt: Date.now() })
}

/** Het laatste antwoord mislukte: opnieuw vragen (zonder het mislukte antwoord). */
function opnieuw() {
  const g = gesprek()
  const berichten = [...g.berichten]
  while (berichten.length && berichten[berichten.length - 1].rol === "assistant" && berichten[berichten.length - 1].fout) berichten.pop()
  if (!berichten.length || bezig()) return
  vraag({ ...g, berichten })
}

function stop() {
  api.assistentStop(gesprek().id).catch(() => {})
}

function Chip(props: { bijlage: Bijlage; weg?: () => void }) {
  return (
    <span class="chip" title={props.bijlage.naam}>
      <Show when={props.bijlage.soort === "afbeelding" && props.bijlage} fallback={<span class="chip-icoon" aria-hidden="true" />}>
        {(b) => <img class="chip-beeld" src={b().data_url} alt="" />}
      </Show>
      <span class="chip-naam">{props.bijlage.naam}</span>
      <span class="chip-grootte">{grootte(props.bijlage.grootte)}</span>
      <Show when={props.weg}>
        <button class="chip-weg" title="Weghalen" aria-label={`${props.bijlage.naam} weghalen`} onClick={props.weg}>
          ×
        </button>
      </Show>
    </span>
  )
}

const GEEN_HERKANSING = ["plafond", "geen_lid", "gast", "tool_niet_toegestaan"]

/** De knop bij een fout: inloggen, beheer openen of opnieuw proberen. */
function FoutKnop(props: { f: AiFout }) {
  return (
    <Show
      when={props.f.soort === "sessie_verlopen"}
      fallback={
        <Show
          when={props.f.soort === "geen_tegoed"}
          fallback={
            // Bij een plafond, geen lidmaatschap of een gast helpt opnieuw proberen niet.
            <Show when={!GEEN_HERKANSING.includes(props.f.soort)}>
              <button class="klein" onClick={opnieuw}>
                Opnieuw proberen
              </button>
            </Show>
          }
        >
          <button class="klein" onClick={() => api.assistentOpenBeheer().catch(() => {})}>
            Beheer openen
          </button>
        </Show>
      }
    >
      <button class="primair klein" disabled={inloggen()} onClick={() => logIn().then(() => !fout() && opnieuw())}>
        {inloggen() ? "Rond het af in je browser…" : "Inloggen"}
      </button>
    </Show>
  )
}

function Berichtblok(props: { bericht: Bericht; laatste: boolean; lopend?: boolean }) {
  const modelNaam = () => {
    const m = modellen().find((x) => x.id === props.bericht.model)
    return m ? modelLabel(m) : props.bericht.model ?? ""
  }
  return (
    <Show
      when={props.bericht.rol === "assistant"}
      fallback={
        <div class="bericht vraag">
          <Show when={props.bericht.bijlagen?.length}>
            <div class="chips">
              <For each={props.bericht.bijlagen}>{(b) => <Chip bijlage={b} />}</For>
            </div>
          </Show>
          <Show when={props.bericht.tekst}>
            <div class="vraag-tekst">{props.bericht.tekst}</div>
          </Show>
        </div>
      }
    >
      <div class="bericht antwoord">
        <Show when={props.bericht.tekst} fallback={<Show when={props.lopend}><span class="typt" aria-label="De assistent schrijft"><i /><i /><i /></span></Show>}>
          <Markdown tekst={props.bericht.tekst} />
        </Show>
        <Show when={props.bericht.fout}>
          <div class="foutvlak antwoord-fout" role="alert">
            <span>{props.bericht.fout}</span>
            <Show when={props.laatste && fout()}>{(f) => <FoutKnop f={f()} />}</Show>
          </div>
        </Show>
        <Show when={!props.lopend && (props.bericht.gebruik || props.bericht.afgebroken || modelNaam())}>
          <div class="antwoord-meta">
            <Show when={modelNaam()}>
              <span>{modelNaam()}</span>
            </Show>
            <Show when={props.bericht.gebruik}>
              <span>{gebruikTekst(props.bericht.gebruik)}</span>
            </Show>
            <Show when={props.bericht.afgebroken}>
              <span>gestopt</span>
            </Show>
          </div>
        </Show>
      </div>
    </Show>
  )
}

export default function Assistent() {
  let gesprekVlak: HTMLDivElement | undefined
  let invoerVak: HTMLTextAreaElement | undefined
  const [sleept, setSleept] = createSignal(false)

  onMount(async () => {
    start()
    try {
      const stop = await getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type === "enter" || e.payload.type === "over") setSleept(true)
        else if (e.payload.type === "leave") setSleept(false)
        else if (e.payload.type === "drop") {
          setSleept(false)
          voegPadenToe(e.payload.paths)
        }
      })
      onCleanup(stop)
    } catch {
      /* buiten Tauri */
    }
    invoerVak?.focus()
  })

  // Uit het rechtermuismenu van de Verkenner ("Vragen aan de assistent").
  createEffect(() => {
    const paden = voorAssistent()
    if (!paden.length) return
    setVoorAssistent([])
    voegPadenToe(paden)
  })

  // Meescrollen met het antwoord, zolang je onderaan staat.
  createEffect(() => {
    antwoord()?.tekst
    gesprek().berichten.length
    const vlak = gesprekVlak
    if (vlak && vlak.scrollHeight - vlak.scrollTop - vlak.clientHeight < 120) queueMicrotask(() => (vlak.scrollTop = vlak.scrollHeight))
  })

  const kanVersturen = () => !bezig() && privacyGezien() && (invoer().trim().length > 0 || klaar().length > 0)
  const ingelogd = () => status()?.ingelogd ?? false

  return (
    <div class="assistent" classList={{ sleept: sleept() }}>
      <aside class="gesprekken">
        <button class="primair breed" disabled={bezig()} onClick={nieuw}>
          Nieuw gesprek
        </button>
        <div class="lijstkop">Gesprekken</div>
        <ul class="gesprekkenlijst">
          <For each={lijst()} fallback={<li class="gesprekken-leeg">Nog geen gesprekken.</li>}>
            {(s) => (
              <li class="gesprekregel" classList={{ actief: s.id === gesprek().id }}>
                <button class="gesprekknop" disabled={bezig()} onClick={() => kies(s)} title={s.titel}>
                  <span class="gesprek-titel">{s.titel || "Gesprek"}</span>
                  <span class="gesprek-datum">{datumTekst(s.bijgewerkt)}</span>
                </button>
                <button class="gesprek-weg" title="Gesprek verwijderen" aria-label={`${s.titel} verwijderen`} disabled={bezig() && s.id === gesprek().id} onClick={() => verwijder(s)}>
                  ×
                </button>
              </li>
            )}
          </For>
        </ul>
      </aside>

      <section class="gesprek">
        <header class="gesprek-kop">
          <h1 class="gesprek-kop-titel" title={gesprek().titel}>{gesprek().berichten.length ? gesprek().titel : "Assistent"}</h1>
          <Show when={modellen().length}>
            <label class="modelkeuze">
              <span class="verborgen">Model</span>
              <select
                value={gesprek().model ?? ""}
                disabled={bezig()}
                onChange={(e) => {
                  setGesprek({ ...gesprek(), model: e.currentTarget.value })
                  schrijf(MODEL_SLEUTEL, e.currentTarget.value)
                }}
              >
                <For each={modellen()}>{(m) => <option value={m.id}>{modelLabel(m)}</option>}</For>
              </select>
            </label>
          </Show>
        </header>

        <Show when={status() && !ingelogd()}>
          <div class="kaart assistent-inloggen">
            <h2>Inloggen voor de assistent</h2>
            <p class="uitleg">
              De assistent gebruikt je OpenAEC-account. Log één keer in; ben je al ingelogd in de browser, dan hoef je alleen toe te staan.
            </p>
            <button class="primair" disabled={inloggen()} onClick={logIn}>
              {inloggen() ? "Rond het af in je browser…" : "Inloggen"}
            </button>
          </div>
        </Show>
        <Show when={modelFout()}>
          {(f) => (
            <div class="foutvlak sessie-verlopen" role="alert">
              <span>{f().melding}</span>
              <Show when={f().soort === "sessie_verlopen"} fallback={<button class="klein" onClick={laadModellen}>Opnieuw proberen</button>}>
                <button class="primair klein" disabled={inloggen()} onClick={logIn}>
                  {inloggen() ? "Rond het af in je browser…" : "Inloggen"}
                </button>
              </Show>
            </div>
          )}
        </Show>

        <div class="gesprek-vlak" ref={gesprekVlak}>
          <Show
            when={gesprek().berichten.length || antwoord()}
            fallback={
              <div class="gesprek-leeg">
                <div class="leeg-icoon">?</div>
                <h2>Waarmee kan ik helpen?</h2>
                <p>Stel een vraag, of sleep bestanden hierheen: afbeeldingen, PDF, Word, Excel, tekst en code.</p>
                <p class="privacy-regel">{PRIVACYMELDING}</p>
              </div>
            }
          >
            <For each={gesprek().berichten}>
              {(b, i) => <Berichtblok bericht={b} laatste={!antwoord() && i() === gesprek().berichten.length - 1} />}
            </For>
            <Show when={antwoord()}>{(a) => <Berichtblok bericht={a()} laatste lopend />}</Show>
          </Show>
        </div>

        <div class="opsteller">
          <Show when={!privacyGezien()}>
            <div class="privacy" role="note">
              <span>{PRIVACYMELDING}</span>
              <button
                class="klein"
                onClick={() => {
                  setPrivacyGezien(true)
                  schrijf(PRIVACY_SLEUTEL, "ja")
                  invoerVak?.focus()
                }}
              >
                Begrepen
              </button>
            </div>
          </Show>
          <Show when={bijlageFout()}>
            <div class="foutvlak bijlage-fout" role="alert">
              {bijlageFout()}
            </div>
          </Show>
          <Show when={klaar().length}>
            <div class="chips">
              <For each={klaar()}>{(b, i) => <Chip bijlage={b} weg={() => setKlaar(klaar().filter((_, k) => k !== i()))} />}</For>
            </div>
          </Show>
          <div class="invoerregel">
            <button class="bijlageknop" title="Bestand toevoegen" aria-label="Bestand toevoegen" disabled={bezig()} onClick={kiesBestanden}>
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M21 11.5 12.6 19.9a5 5 0 0 1-7.1-7.1l8.5-8.5a3.3 3.3 0 0 1 4.7 4.7l-8.5 8.5a1.7 1.7 0 0 1-2.4-2.4l7.8-7.8" />
              </svg>
            </button>
            <textarea
              ref={invoerVak}
              rows={2}
              placeholder={ingelogd() ? "Stel een vraag of sleep een bestand hierheen" : "Log eerst in om een vraag te stellen"}
              title="Enter verstuurt, Shift+Enter is een nieuwe regel"
              value={invoer()}
              onInput={(e) => setInvoer(e.currentTarget.value)}
              onPaste={plak}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
                  e.preventDefault()
                  verstuur()
                }
              }}
            />
            <Show
              when={bezig()}
              fallback={
                <button class="primair" disabled={!kanVersturen() || !ingelogd()} onClick={verstuur}>
                  Versturen
                </button>
              }
            >
              <button class="stopknop" onClick={stop}>
                Stoppen
              </button>
            </Show>
          </div>
        </div>
        <div class="sleepvlak" aria-hidden="true">
          <span>Laat los om toe te voegen</span>
        </div>
      </section>
    </div>
  )
}
