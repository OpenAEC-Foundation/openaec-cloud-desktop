// Catalogus van de OpenAEC-tools voor het tabblad Tools. Overgenomen uit de OpenAEC Installer (src/data/catalog.ts,
// zie NOTICE); alleen de Nederlandse omschrijvingen.
//
// - Desktop-tools hebben installers in hun GitHub-releases (Windows-setup en AppImage). De geïnstalleerde versie komt
//   op Windows uit het register (`registerNaam` = DisplayName in de Uninstall-sleutel, `exeNaam` = terugval om de exe
//   te vinden) en op Linux uit de beheerde AppImage-map.
// - Webtools draaien in de browser en worden daar geopend.
// - Installeren gaat alleen vanuit repo's van OpenAEC-Foundation; een tool uit een andere repo (community) toont wel
//   zijn status, maar wordt via zijn eigen GitHub-pagina geïnstalleerd.

export type ToolSoort = "desktop" | "web"

export type Categorie =
  | "calculatie"
  | "cad"
  | "constructief"
  | "geotechniek"
  | "bim"
  | "planning"
  | "documenten"
  | "ai"
  | "energie"
  | "veld"
  | "gis"

export const categorieNamen: Record<Categorie, string> = {
  calculatie: "Calculatie",
  cad: "CAD",
  constructief: "Constructief",
  geotechniek: "Geotechniek",
  bim: "BIM",
  planning: "Planning",
  documenten: "Documenten",
  ai: "AI",
  energie: "Energie",
  veld: "Veldwerk",
  gis: "GIS",
}

export interface Tool {
  id: string
  naam: string
  soort: ToolSoort
  categorie: Categorie
  /** GitHub-repo als "eigenaar/naam" (desktop-tools). */
  repo?: string
  /** DisplayName in de Uninstall-sleutel van Windows. */
  registerNaam?: string
  /**
   * Begin van de naam van het installer-asset, voor een release met de setups van meerdere apps (zoals Open Office
   * Studio). Zonder prefix telt elk asset.
   */
  assetPrefix?: string
  /** Verwachte naam van de exe (terugval voor de padbepaling). */
  exeNaam?: string
  /** Webversie of demo. */
  webUrl?: string
  /** Productpagina op open-aec.com. */
  siteUrl?: string
  /** Meegeleverde schermafbeelding onder /tools/previews/; zonder valt de kaart terug op de categorie. */
  preview?: string
  /** Eigen logo onder /tools/logos/. */
  logo?: string
  omschrijving: string
  /** Project buiten de OpenAEC-Foundation-organisatie. */
  community?: boolean
  /** Tijdelijk uit het overzicht (en uit detectie en release-check) gehouden. */
  verborgen?: boolean
}

/** Alle tools, ook de verborgen. Gebruik TOOLS tenzij je bewust ook de verborgen nodig hebt. */
// Alleen tools die op https://open-aec.com/ staan, in de volgorde van het ecosysteemoverzicht daar (gelezen
// 2026-11-19), gelijk aan de catalogus van de OpenAEC Installer. Deze app zelf (OpenAEC SuperCloud) staat er niet in.
export const ALLE_TOOLS: Tool[] = [
  // ─── Desktop-tools ───
  {
    id: "open-bim-suite",
    naam: "Open BIM Suite",
    preview: "/tools/previews/open-bim-suite.jpg",
    soort: "desktop",
    categorie: "bim",
    repo: "OpenAEC-Foundation/open-bim-suite",
    registerNaam: "Open BIM Suite",
    exeNaam: "open-3d-viewer.exe",
    webUrl: "https://open-3d-viewer.open-aec.com/?product=modeller",
    siteUrl: "https://open-aec.com/open-bim-suite/",
    omschrijving: "N in één: de toekomstige everything-app voor BIM. Nu al Viewer, Modeller en BIM Validator in één programma.",
  },
  {
    id: "open-cad-studio",
    naam: "Open CAD Studio",
    preview: "/tools/previews/open-cad-studio.jpg",
    soort: "desktop",
    categorie: "cad",
    repo: "HakanSeven12/OpenCADStudio",
    registerNaam: "Open CAD Studio",
    exeNaam: "OpenCADStudio.exe",
    webUrl: "https://www.opencadstudio.com/app/",
    siteUrl: "https://open-aec.com/open-cad-studio/",
    omschrijving: "Volwaardige 2D- en 3D-CAD in Rust die DWG en DXF leest en schrijft. Communityproject van Hakan Seven.",
    community: true,
  },
  {
    id: "open-pdf-studio",
    naam: "Open PDF Studio",
    preview: "/tools/previews/open-pdf-studio.jpg",
    soort: "desktop",
    categorie: "documenten",
    repo: "OpenAEC-Foundation/open-pdf-studio",
    registerNaam: "Open PDF Studio",
    exeNaam: "open-pdf-studio.exe",
    webUrl: "https://open-pdf-studio.open-aec.com/",
    siteUrl: "https://open-aec.com/open-pdf-studio/",
    omschrijving: "PDF-editor met eigen Rust-engine: annoteren, meten op schaal, formulieren en redigeren. In 39 talen.",
  },
  {
    id: "open-calculations-studio",
    naam: "Open CalcSheet Studio",
    preview: "/tools/previews/open-calculations-studio.jpg",
    soort: "desktop",
    categorie: "constructief",
    repo: "OpenAEC-Foundation/Open-CalcSheet-Studio",
    registerNaam: "Open CalcSheet Studio",
    exeNaam: "open-calcsheet-studio.exe",
    webUrl: "https://open-calculations-studio.open-aec.com/",
    siteUrl: "https://open-aec.com/open-calcsheet-studio/",
    omschrijving: "Constructieve handberekeningen met eenheden, formules, tekeningen en IFC-export.",
  },
  {
    id: "open-planner-studio",
    naam: "Open Planner Studio",
    preview: "/tools/previews/open-planner-studio.jpg",
    soort: "desktop",
    categorie: "planning",
    repo: "OpenAEC-Foundation/open-planner-studio",
    registerNaam: "Open Planner Studio",
    exeNaam: "open-planner-studio.exe",
    webUrl: "https://open-planner-studio.open-aec.com/",
    siteUrl: "https://open-aec.com/open-planner-studio/",
    omschrijving: "Planning voor bouw, GWW en civiel: Gantt, kritiek pad, resources en voortgang.",
  },
  {
    id: "open-pointcloud-studio",
    naam: "Open Pointcloud Studio",
    preview: "/tools/previews/open-pointcloud-studio.jpg",
    soort: "desktop",
    categorie: "bim",
    repo: "OpenAEC-Foundation/open-pointcloud-studio",
    registerNaam: "Open Pointcloud Studio",
    exeNaam: "open-pointcloud-studio.exe",
    webUrl: "https://open-pointcloud-studio.open-aec.com/",
    siteUrl: "https://open-aec.com/open-pointcloud-studio/",
    omschrijving: "Puntenwolken openen, meten en doorsnijden: LAS, LAZ, E57 en complete scanprojecten.",
  },
  {
    id: "open-calc-studio",
    naam: "Open Calc Studio",
    preview: "/tools/previews/open-calc-studio.jpg",
    soort: "desktop",
    categorie: "calculatie",
    repo: "OpenAEC-Foundation/open-calc-studio",
    registerNaam: "Open Calc Studio",
    exeNaam: "Open Calc Studio.exe",
    webUrl: "https://open-calc-studio.open-aec.com/",
    siteUrl: "https://open-aec.com/open-calc-studio/",
    omschrijving: "Kostenramingen en begrotingen voor bouw, GWW en civiele projecten.",
  },
  {
    id: "open-office-studio",
    naam: "Open Office Studio",
    preview: "/tools/previews/open-office-studio.jpg",
    soort: "desktop",
    categorie: "documenten",
    repo: "OpenAEC-Foundation/open-office-studio",
    assetPrefix: "Open.Office.Studio_",
    registerNaam: "Open Office Studio",
    exeNaam: "open-office-studio.exe",
    siteUrl: "https://open-aec.com/open-office-studio/",
    omschrijving: "Tekst, rekenblad, presentaties, notities, e-mail en Access-databases op je eigen computer.",
  },
  {
    id: "open-geotechniek-studio",
    naam: "Open Geotechniek Studio",
    preview: "/tools/previews/open-geotechniek-studio.jpg",
    soort: "desktop",
    categorie: "geotechniek",
    repo: "OpenAEC-Foundation/open-geotechniek-studio",
    registerNaam: "Open Geotechniek Studio",
    exeNaam: "open-geotechniek-studio.exe",
    webUrl: "https://open-geotechniek-studio.open-aec.com/",
    siteUrl: "https://open-aec.com/open-geotechniek-studio/",
    omschrijving: "Bekijk en analyseer sonderingen (CPT, GEF en BRO-xml) voor fundering en grondwerk.",
  },
  {
    id: "open-field-studio",
    naam: "Open Field Studio",
    preview: "/tools/previews/open-field-studio.jpg",
    soort: "desktop",
    categorie: "veld",
    repo: "OpenAEC-Foundation/Open-Field-Studio",
    registerNaam: "Open Field Studio",
    exeNaam: "open-field-studio.exe",
    webUrl: "https://open-field-studio.open-aec.com/",
    siteUrl: "https://open-aec.com/open-field-studio/",
    omschrijving: "Inspectie en oplevering: gebreken op de plattegrond, checklists (NEN 2767) en handtekeningen. Werkt offline.",
  },
  {
    id: "monty-ifc-viewer",
    naam: "Monty IFC Viewer",
    preview: "/tools/previews/monty-ifc-viewer.jpg",
    soort: "desktop",
    categorie: "bim",
    repo: "OpenAEC-Foundation/monty-ifc-viewer",
    registerNaam: "Monty IFC Viewer",
    exeNaam: "monty-ifc-viewer.exe",
    webUrl: "https://monty-ifc-viewer.open-aec.com/",
    siteUrl: "https://open-aec.com/monty-ifc-viewer/",
    omschrijving: "IFC-viewer voor montage op de bouw: open BIM-modellen snel bekijken op tablet en telefoon.",
  },
  {
    id: "open-paint-studio",
    naam: "Open Paint Studio",
    preview: "/tools/previews/open-paint-studio.jpg",
    soort: "desktop",
    categorie: "documenten",
    repo: "OpenAEC-Foundation/open-paint-studio",
    registerNaam: "OpenAEC Paint Studio",
    exeNaam: "openaec-paint-studio.exe",
    siteUrl: "https://open-aec.com/open-paint-studio/",
    omschrijving: "Eenvoudige beeldbewerker: schetsen en markeren, met bewerkbare vectorobjecten en MCP voor AI.",
  },
  {
    id: "open-bim-validator",
    naam: "Open BIM Validator",
    preview: "/tools/previews/open-bim-validator.jpg",
    soort: "desktop",
    categorie: "bim",
    repo: "OpenAEC-Foundation/OpenAEC-BIM-validator",
    registerNaam: "BIM Validator",
    exeNaam: "openaec-bim-validator.exe",
    siteUrl: "https://open-aec.com/open-bim-validator/",
    omschrijving: "Modellen samen controleren: IFC-modellen naast elkaar, toetsen aan IDS, botsingen zoeken en issues vastleggen in BCF.",
  },
  {
    id: "open-speech-studio",
    naam: "Open Speech Studio",
    preview: "/tools/previews/open-speech-studio.jpg",
    soort: "desktop",
    categorie: "ai",
    repo: "OpenAEC-Foundation/open-speech-studio",
    registerNaam: "Open Speech Studio",
    exeNaam: "open-speech-studio.exe",
    siteUrl: "https://open-aec.com/open-speech-studio/",
    omschrijving: "Lokale spraak-naar-tekst en notulen voor bouwoverleg, zonder cloud.",
  },
  // ─── Webtools ───
  {
    id: "y-app",
    naam: "Y-app",
    preview: "/tools/previews/y-app.jpg",
    soort: "web",
    categorie: "planning",
    webUrl: "https://y-app.impertio.app",
    siteUrl: "https://open-aec.com/y-app/",
    omschrijving: "De everything-app: mail, ERP, uren, offertes, projecten en planning in één tool, op ERPNext.",
  },
  {
    id: "pile-plan-studio",
    naam: "Open Pile Plan Studio",
    preview: "/tools/previews/pile-plan-studio.jpg",
    soort: "web",
    categorie: "constructief",
    webUrl: "https://pile-plan-studio.open-aec.com/",
    siteUrl: "https://open-aec.com/open-pile-plan-studio/",
    omschrijving: "Vergelijk en optimaliseer paalconfiguraties per belastinglocatie tot een praktisch paalplan.",
  },
  {
    id: "open-energy-studio",
    naam: "Open Energy Studio",
    preview: "/tools/previews/open-energy-studio.jpg",
    logo: "/tools/logos/open-energy-studio.png",
    soort: "web",
    categorie: "energie",
    webUrl: "https://open-energy-studio.open-aec.com/",
    siteUrl: "https://open-aec.com/open-energy-studio/",
    omschrijving: "Energieprestatie volgens NTA 8800: BENG, energielabel en TOjuli, met rapport.",
  },
  {
    id: "ifccad",
    naam: "IFCCAD",
    preview: "/tools/previews/ifccad.jpg",
    soort: "web",
    categorie: "cad",
    webUrl: "https://ifccad-explorer.open-aec.com/",
    siteUrl: "https://open-aec.com/ifccad/",
    omschrijving: "Open CAD-formaat op IFCX, met native alpha, maatvoering en DWG/DXF-uitwisseling via de Format Explorer.",
  },
  {
    id: "open-heatloss-studio",
    naam: "Open Heatloss Studio",
    preview: "/tools/previews/open-heatloss-studio.jpg",
    soort: "web",
    categorie: "energie",
    webUrl: "https://open-heatloss-studio.open-aec.com/",
    siteUrl: "https://open-aec.com/open-heatloss-studio/",
    omschrijving: "Warmteverliesberekening per ruimte volgens NEN 12831: transmissie en ventilatie.",
  },
  {
    id: "open-books",
    naam: "Open Books Studio",
    preview: "/tools/previews/open-books.jpg",
    soort: "web",
    categorie: "documenten",
    webUrl: "https://open-books.open-aec.com/",
    siteUrl: "https://open-aec.com/open-books/",
    omschrijving: "Oude bouwkundige boeken ingescand en doorzoekbaar, voor onderzoek en restauratie.",
  },
]

/** De zichtbare catalogus. */
export const TOOLS: Tool[] = ALLE_TOOLS.filter((t) => !t.verborgen)
export const DESKTOP_TOOLS = TOOLS.filter((t) => t.soort === "desktop")
export const WEB_TOOLS = TOOLS.filter((t) => t.soort === "web")
