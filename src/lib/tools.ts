// Pure logica van het tabblad Tools: versies vergelijken, de status per tool, zoeken en het samenvoegen van
// release-informatie. Zonder imports van Solid of Tauri, zodat `node --test` het direct kan draaien.
// Versievergelijking en statusmodel komen uit de OpenAEC Installer (src/lib/status.ts, zie NOTICE).

import type { Tool } from "./catalogus.ts"

export interface GeinstalleerdeTool {
  id: string
  displayName: string
  version: string | null
  exePath: string | null
  /** UninstallString uit het register (Windows); op Linux altijd null. */
  uninstall: string | null
}

export interface ReleaseInfo {
  id: string
  ok: boolean
  error: string | null
  tag: string | null
  version: string | null
  assetName: string | null
  assetUrl: string | null
  assetSize: number | null
  /** SHA-256 van GitHub ("sha256:<hex>"). */
  assetDigest: string | null
  pageUrl: string | null
  publishedAt: string | null
  /** Bij error "rate_limited": epoch-seconden waarop de GitHub-limiet vrijgeeft. */
  rateLimitReset?: number | null
}

export type ToolStatus =
  | "niet_geinstalleerd"
  | "actueel"
  | "update"
  | "geinstalleerd_onbekend" // geïnstalleerd, maar de laatste versie is onbekend (bijv. GitHub-limiet)
  | "web"

/** De enige organisatie waarvan het tabblad installeert (zelfde regel als in de Rust-code). */
export const ORGANISATIE = "OpenAEC-Foundation"

/**
 * Vergelijk twee versies numeriek per segment (semver "0.8.7" én calver "2026.7.10"); ontbrekende segmenten tellen als
 * 0. Geeft -1, 0 of 1.
 */
export function vergelijkVersies(a: string, b: string): number {
  const delen = (v: string) =>
    v
      .trim()
      .replace(/^[vV]/, "")
      .split(/[.+-]/)
      .map((p) => parseInt(p, 10))
      .filter((n) => !Number.isNaN(n))
  const pa = delen(a)
  const pb = delen(b)
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const na = pa[i] ?? 0
    const nb = pb[i] ?? 0
    if (na < nb) return -1
    if (na > nb) return 1
  }
  return 0
}

export function statusVan(tool: Tool, geinstalleerd: GeinstalleerdeTool | undefined, release: ReleaseInfo | undefined): ToolStatus {
  if (tool.soort === "web") return "web"
  if (!geinstalleerd) return "niet_geinstalleerd"
  const laatste = release?.ok ? release.version : null
  if (!laatste || !geinstalleerd.version) return "geinstalleerd_onbekend"
  return vergelijkVersies(geinstalleerd.version, laatste) < 0 ? "update" : "actueel"
}

/** Komt de tool uit een repo van de eigen organisatie (en mag hij dus hier geïnstalleerd worden)? */
export function eigenRepo(tool: Tool): boolean {
  const eigenaar = tool.repo?.split("/")[0]
  return !!eigenaar && eigenaar.toLowerCase() === ORGANISATIE.toLowerCase()
}

/** Kan de tool nu geïnstalleerd of bijgewerkt worden vanuit dit tabblad? */
export function kanInstalleren(tool: Tool, status: ToolStatus, release: ReleaseInfo | undefined): boolean {
  return (
    tool.soort === "desktop" &&
    eigenRepo(tool) &&
    !!release?.ok &&
    !!release.assetUrl &&
    !!release.assetName &&
    (status === "niet_geinstalleerd" || status === "update")
  )
}

/** Zoeken op naam, omschrijving en categorie (zonder hoofdlettergevoeligheid). */
export function zoekTreffer(tool: Tool, zoekterm: string, categorieNaam: string): boolean {
  const q = zoekterm.trim().toLowerCase()
  if (!q) return true
  return tool.naam.toLowerCase().includes(q) || tool.omschrijving.toLowerCase().includes(q) || categorieNaam.toLowerCase().includes(q)
}

/**
 * Nieuwe release-informatie samenvoegen met wat al bekend was: een mislukte ronde (bijv. de GitHub-limiet) mag eerder
 * opgehaalde versie-informatie nooit wissen.
 */
export function voegSamen(bekend: Record<string, ReleaseInfo>, nieuw: ReleaseInfo[]): Record<string, ReleaseInfo> {
  const uit = { ...bekend }
  for (const r of nieuw) {
    if (r.ok || !uit[r.id]?.ok) uit[r.id] = r
  }
  return uit
}

/** Bestandsgrootte leesbaar ("12,3 MB"). */
export function grootte(bytes: number | null | undefined): string {
  if (!bytes) return ""
  const mb = bytes / (1024 * 1024)
  return mb >= 1 ? `${mb.toLocaleString("nl-NL", { maximumFractionDigits: 1 })} MB` : `${Math.ceil(bytes / 1024)} kB`
}
