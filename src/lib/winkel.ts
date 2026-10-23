// Gedeelde toestand van het hoofdvenster: verbinding, ruimtes, koppelingen, sync-rapporten en de activiteit van
// de sync. De schermen lezen hieruit en roepen de acties hieronder aan.

import { createSignal } from "solid-js"
import { api, type Koppeling, type KoppelStatus, type MapRapport, type Quotum, type Ruimte, type Stand } from "./api"

export type Scherm = "overzicht" | "ruimtes" | "delen" | "tools" | "instellingen"

export const [scherm, setScherm] = createSignal<Scherm>("overzicht")
export const [basis, setBasis] = createSignal("")
export const [verbonden, setVerbonden] = createSignal(false)
export const [bezig, setBezig] = createSignal(false)
export const [melding, setMelding] = createSignal("")
export const [ruimtes, setRuimtes] = createSignal<Ruimte[]>([])
export const [ruimtesLaden, setRuimtesLaden] = createSignal(false)
export const [ruimteFout, setRuimteFout] = createSignal("")
export const [ruimtesOpgehaald, setRuimtesOpgehaald] = createSignal(false)
export const [koppelingen, setKoppelingen] = createSignal<Koppeling[]>([])
/** Laatste rapport per gekoppelde map, met het tijdstip ervan. */
export const [rapporten, setRapporten] = createSignal<Record<string, MapRapport & { tijd: Date }>>({})
export const [koppelStatus, setKoppelStatus] = createSignal<Record<string, KoppelStatus>>({})
export const [stand, setStand] = createSignal<Stand | null>(null)
/** Opslaggebruik van de omgeving (null = nog niet bekend of niet op te halen). */
export const [quotum, setQuotum] = createSignal<Quotum | null>(null)
/** Bestand of map in het deelscherm. */
export const [bestand, setBestand] = createSignal("")

/** Voer een actie uit met een melding in de statusbalk; fouten komen daar ook. */
export const doe = async (wat: string, f: () => Promise<unknown>) => {
  setBezig(true)
  setMelding(wat + "…")
  try {
    await f()
  } catch (e) {
    setMelding(`${wat} mislukt: ${e}`)
  }
  setBezig(false)
}

export const verwerkSync = (r: MapRapport[]) => {
  const tijd = new Date()
  setRapporten({ ...rapporten(), ...Object.fromEntries(r.map((m) => [m.lokaal, { ...m, tijd }])) })
  verversOverzicht()
}

let laatsteRuimtes = 0

/** Ruimtes opnieuw ophalen; `altijd` = ook als dat net gebeurd is (vernieuwknop). */
export const laadRuimtes = async (altijd = false) => {
  if (!verbonden() || ruimtesLaden()) return
  if (!altijd && Date.now() - laatsteRuimtes < 15000) return
  laatsteRuimtes = Date.now()
  setRuimtesLaden(true)
  try {
    setRuimtes(await api.ruimtes())
    setRuimteFout("")
    setRuimtesOpgehaald(true)
  } catch (e) {
    setRuimteFout(String(e))
  }
  setRuimtesLaden(false)
}

export const verversOverzicht = async () => {
  try {
    const o = await api.overzicht()
    setKoppelStatus(Object.fromEntries(o.koppelingen.map((k) => [k.lokaal, k])))
  } catch {
    // alleen lokale gegevens; een fout hier is niet belangrijk genoeg voor een melding
  }
}

/** Gegevens verversen als het venster (weer) in beeld komt. */
export const ververs = () => {
  laadRuimtes()
  verversOverzicht()
  api.quotum().then(setQuotum).catch(() => {})
  api.activiteit().then(setStand).catch(() => {})
}
