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
export const ALLE_TOOLS: Tool[] = [
  // ─── Desktop-tools ───
  {
    id: "open-3d-studio",
    naam: "Open 3D Studio",
    verborgen: true,
    soort: "desktop",
    categorie: "bim",
    repo: "OpenAEC-Foundation/open-3d-studio",
    registerNaam: "Open 3D Studio",
    exeNaam: "open-3d-studio.exe",
    omschrijving:
      "3D-modelleerstudio met IFC als eigen uitvoer. Teken parametrische componenten op geladen IFC- en DXF-modellen, met sheets, aantallen en STL/PDF-export.",
  },
  {
    id: "open-calc-studio",
    naam: "Open Calc Studio",
    preview: "/tools/previews/open-calc-studio.jpg",
    soort: "desktop",
    categorie: "calculatie",
    repo: "OpenAEC-Foundation/open-calc-studio",
    registerNaam: "Open Calc Studio",
    // Deze tool zet mainBinaryName, waardoor de exe de productnaam draagt.
    exeNaam: "Open Calc Studio.exe",
    webUrl: "https://open-calc-studio.open-aec.com/",
    omschrijving: "Open source calculatie en begroting voor de bouwsector.",
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
    omschrijving: "PDF's bekijken, bewerken en annoteren — gericht op bouwdocumentatie en tekeningen.",
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
    omschrijving: "Open source projectplanning en werkcoördinatie voor bouwprojecten.",
  },
  {
    id: "open-2d-studio",
    naam: "Open 2D Studio",
    preview: "/tools/previews/open-2d-studio.jpg",
    soort: "desktop",
    categorie: "cad",
    repo: "OpenAEC-Foundation/open-2d-studio",
    registerNaam: "Open 2D Studio",
    exeNaam: "open-2d-studio.exe",
    webUrl: "https://open-2d-studio.open-aec.com/",
    omschrijving: "2D-tekeningen en documenten bekijken en bewerken.",
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
    webUrl: "https://hakanseven12.github.io/OpenCADStudio/",
    omschrijving: "Volwaardige 2D- en 3D-CAD-applicatie met DWG/DXF-ondersteuning (community-project).",
    community: true,
  },
  {
    id: "open-calculations-studio",
    naam: "Open Calculations Studio",
    preview: "/tools/previews/open-calculations-studio.jpg",
    soort: "desktop",
    categorie: "constructief",
    repo: "OpenAEC-Foundation/Open-Calculations-Studio",
    registerNaam: "Open Calculations Studio",
    exeNaam: "open-calculations-studio.exe",
    webUrl: "https://open-calculations-studio.open-aec.com/",
    omschrijving: "Constructieve handberekeningen in een leesbare rekentaal, herleidbaar en deelbaar.",
  },
  {
    id: "open-frame-studio",
    naam: "Open Frame Studio",
    soort: "desktop",
    categorie: "constructief",
    repo: "OpenAEC-Foundation/open-frame-studio",
    registerNaam: "Open Frame Studio",
    exeNaam: "open-frame-studio.exe",
    omschrijving: "Parametrisch kozijn- en raamwerkontwerp voor de bouw.",
  },
  {
    id: "open-fem2d-studio",
    naam: "Open FEM2D Studio",
    soort: "desktop",
    categorie: "constructief",
    repo: "OpenAEC-Foundation/open-fem2d-studio",
    registerNaam: "Open FEM2D Studio",
    exeNaam: "open-fem2d-studio.exe",
    omschrijving: "2D-eindige-elementenanalyse voor constructies.",
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
    omschrijving: "Viewer voor sonderingen: CPT's in GEF- en BRO-XML-formaat.",
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
    omschrijving: "Open source IFC-viewer voor BIM-modellen, ook op de bouwplaats.",
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
    omschrijving: "Lokale spraak-naar-tekst en vergadernotities — helemaal op je eigen computer.",
  },
  {
    id: "open-stl-3dmap-studio",
    naam: "Open STL-3DMap Studio",
    soort: "desktop",
    categorie: "gis",
    repo: "OpenAEC-Foundation/Open-STL-3DMap-Studio",
    registerNaam: "Open STL-3DMap Studio",
    exeNaam: "Open STL-3DMap Studio.exe",
    omschrijving: "Maak 3D-printbare meerkleurige stadskaarten van Nederland op basis van 3DBAG en BGT.",
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
    omschrijving: "Inspecties, opleveringen en kwaliteitsborging met checklists.",
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
    omschrijving: "Pointcloud-viewer voor renovatie en analyse.",
  },

  // ─── Webtools ───
  {
    id: "open-energy-studio",
    naam: "Open Energy Studio",
    preview: "/tools/previews/open-energy-studio.jpg",
    logo: "/tools/logos/open-energy-studio.png",
    soort: "web",
    categorie: "energie",
    webUrl: "https://open-energy-studio.open-aec.com/",
    siteUrl: "https://open-aec.com/open-energy-studio/",
    omschrijving: "Energieprestatie volgens NTA 8800: BENG, energielabel en TOjuli voor woningen en utiliteit.",
  },
  {
    id: "open-heatloss-studio",
    naam: "Open Heatloss Studio",
    preview: "/tools/previews/open-heatloss-studio.jpg",
    soort: "web",
    categorie: "energie",
    webUrl: "https://open-heatloss-studio.open-aec.com/",
    omschrijving: "Warmteverliesberekeningen voor gebouwen volgens NEN-EN 12831 / ISSO 51.",
  },
  {
    id: "open-books",
    naam: "Open Books Studio",
    preview: "/tools/previews/open-books.jpg",
    soort: "web",
    categorie: "documenten",
    webUrl: "https://open-books.open-aec.com/",
    omschrijving: "Digitalisering van historische bouwliteratuur.",
  },
  {
    id: "open-bim-validator",
    naam: "Open BIM Validator Studio",
    preview: "/tools/previews/open-bim-validator.jpg",
    soort: "web",
    categorie: "bim",
    repo: "OpenAEC-Foundation/OpenAEC-BIM-validator",
    webUrl: "https://open-aec.com/bim-validator/",
    omschrijving: "Valideer IFC-modellen tegen standaarden zoals IDS, NL-BIM Basis ILS en de RVB BIM Norm.",
  },
  {
    id: "pile-plan-studio",
    naam: "Pile Plan Studio",
    soort: "web",
    categorie: "constructief",
    repo: "OpenAEC-Foundation/pile-plan-studio",
    webUrl: "https://pile-plan-studio.open-aec.com/",
    omschrijving: "Funderingspalenplannen opstellen en beheren, met positionering en technische uitvoer.",
  },
]

/** De zichtbare catalogus. */
export const TOOLS: Tool[] = ALLE_TOOLS.filter((t) => !t.verborgen)
export const DESKTOP_TOOLS = TOOLS.filter((t) => t.soort === "desktop")
export const WEB_TOOLS = TOOLS.filter((t) => t.soort === "web")
