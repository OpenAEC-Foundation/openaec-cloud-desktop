//! De logica van het menu, los van COM (zo is hij op elk platform te testen): welke items zichtbaar zijn bij welke
//! paden, de opdrachtregel voor de app, het bestand `menu.json` dat de app schrijft en het manifest van het pakket.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// CLSID van de COM-server (het hoofditem "OpenAEC"). Staat ook in het manifest van het pakket.
pub const CLSID: &str = "94D876CC-9A6C-42EF-9BA3-595E1C4EA969";
/// Vaste namen (`GetCanonicalName`) van de items.
pub const NAAM_HOOFDITEM: &str = "CF46FCEE-666E-4320-A127-12C9B252763D";
pub const NAAM_DELEN: &str = "65968EBC-CD73-4028-9AFF-838852583AA4";
pub const NAAM_VERSTUREN: &str = "B9340BFF-D72B-4DF4-96F3-112654AE21B0";

/// Tekst van het hoofditem.
pub const LABEL_HOOFDITEM: &str = "OpenAEC";

/// Naam van het pakket met identiteit (de familienaam volgt uit deze naam en de uitgever).
pub const PAKKETNAAM: &str = "OpenAEC.DesktopApp.Verkenner";
/// De uitgever: precies het onderwerp van het certificaat waarmee de installers ondertekend zijn. De bouwstraat
/// neemt het over uit de handtekening van de opdrachtregel (zie .github/workflows/build.yml), zodat ze altijd gelijk
/// zijn; dit is de waarde van nu.
pub const UITGEVER: &str = "CN=Impertio Studio B.V., O=Impertio Studio B.V., STREET=Burg de Raadtsingel 31, L=Dordrecht, S=Zuid-Holland, C=NL, PostalCode=3311 JG";

/// Map in de installatiemap met de DLL's, de pakketten en het logo.
pub const MAP: &str = "shellmenu";
/// Bestandsnaam van de DLL (in `shellmenu\<arch>\`).
pub const DLL: &str = "openaec_shellmenu.dll";
/// Het logo van het pakket (in `shellmenu\`).
pub const LOGO: &str = "logo.png";

/// Omgevingsvariabele voor de rooktest: een ander `menu.json`.
pub const ENV_MENU: &str = "OPENAEC_SHELLMENU_MENU";
/// Omgevingsvariabele voor de rooktest: een extra CLSID die de DLL accepteert (een test-CLSID in HKCU).
pub const ENV_TEST_CLSID: &str = "OPENAEC_SHELLMENU_TEST_CLSID";

/// De items onder "OpenAEC".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opdracht {
    Delen,
    Versturen,
}

impl Opdracht {
    pub const ALLE: [Opdracht; 2] = [Opdracht::Delen, Opdracht::Versturen];

    /// De actie op de opdrachtregel van de app (`--actie <actie> <paden…>`), dezelfde als in het klassieke menu.
    pub fn actie(self) -> &'static str {
        match self {
            Opdracht::Delen => "delen",
            Opdracht::Versturen => "versturen",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Opdracht::Delen => "Delen…",
            Opdracht::Versturen => "Versturen met OpenAEC",
        }
    }

    pub fn canonieke_naam(self) -> &'static str {
        match self {
            Opdracht::Delen => NAAM_DELEN,
            Opdracht::Versturen => NAAM_VERSTUREN,
        }
    }
}

/// Wat de app voor het menu opschrijft (`%APPDATA%\OpenAEC Storage\menu.json`), zodat de DLL geen netwerk of
/// instellingen van de app nodig heeft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Menu {
    /// Het programma dat een keuze start (de app). Leeg: de DLL zoekt `openaec-cloud.exe` boven zijn eigen map.
    pub exe: Option<String>,
    /// De gekoppelde mappen: "Delen…" staat alleen op paden daarbinnen.
    pub mappen: Vec<String>,
    /// "Versturen met OpenAEC" (uit te zetten in de instellingen van de app).
    pub versturen: bool,
}

impl Default for Menu {
    /// Zonder `menu.json` (de app heeft nog niet gedraaid): alleen Versturen, net als in de app standaard.
    fn default() -> Self {
        Menu { exe: None, mappen: Vec::new(), versturen: true }
    }
}

impl Menu {
    /// Inlezen; ontbreekt het bestand of klopt het niet, dan de standaard.
    pub fn laad(pad: &Path) -> Menu {
        std::fs::read(pad).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Wegschrijven, alleen als er iets verandert (via een tijdelijk bestand, zodat de DLL nooit een half bestand
    /// leest). Geeft of het bestand veranderde.
    pub fn bewaar(&self, pad: &Path) -> std::io::Result<bool> {
        let inhoud = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        if std::fs::read(pad).ok().as_deref() == Some(&inhoud[..]) {
            return Ok(false);
        }
        if let Some(map) = pad.parent() {
            std::fs::create_dir_all(map)?;
        }
        let tijdelijk = pad.with_extension("json.nieuw");
        std::fs::write(&tijdelijk, &inhoud)?;
        std::fs::rename(&tijdelijk, pad)?;
        Ok(true)
    }

    /// Staat dit item in het menu voor deze selectie?
    pub fn zichtbaar(&self, opdracht: Opdracht, paden: &[String]) -> bool {
        if paden.is_empty() {
            return false;
        }
        match opdracht {
            Opdracht::Delen => paden.iter().all(|p| self.mappen.iter().any(|m| binnen(p, m))),
            Opdracht::Versturen => self.versturen,
        }
    }

    /// De zichtbare items voor deze selectie (leeg: dan is ook "OpenAEC" zelf verborgen).
    pub fn items(&self, paden: &[String]) -> Vec<Opdracht> {
        Opdracht::ALLE.into_iter().filter(|o| self.zichtbaar(*o, paden)).collect()
    }
}

/// Waar `menu.json` staat: naast `instellingen.json` van de app (`%APPDATA%\OpenAEC Storage`), of waar
/// `OPENAEC_SHELLMENU_MENU` naar wijst (rooktest).
pub fn menu_pad() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(ENV_MENU).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("APPDATA").filter(|p| !p.is_empty()).map(|a| PathBuf::from(a).join("OpenAEC Storage").join("menu.json"))
}

/// Een pad op de manier van Windows vergelijkbaar maken: `\` als scheidingsteken, zonder scheidingsteken aan het
/// eind (behalve bij een stationsletter), en zonder hoofdletters.
fn normaal(pad: &str) -> String {
    let mut p = pad.replace('/', "\\");
    while p.ends_with('\\') && !p.ends_with(":\\") && p.len() > 1 {
        p.pop();
    }
    p.to_lowercase()
}

/// Ligt `pad` in `map` (of is het `map` zelf)? Hoofdletters tellen niet; "C:\Werk\Project AB" ligt niet in
/// "C:\Werk\Project A".
pub fn binnen(pad: &str, map: &str) -> bool {
    let (p, m) = (normaal(pad), normaal(map));
    if m.is_empty() {
        return false;
    }
    if p == m {
        return true;
    }
    let voorvoegsel = if m.ends_with('\\') { m } else { format!("{m}\\") };
    p.starts_with(&voorvoegsel)
}

/// De argumenten voor de app: `--actie <actie> <paden…>` (alle paden in één keer; de app verzamelt ze tot één
/// overdracht, en "Delen…" neemt het eerste pad, net als in het klassieke menu).
pub fn argumenten(opdracht: Opdracht, paden: &[String]) -> Vec<String> {
    let mut uit = vec!["--actie".to_string(), opdracht.actie().to_string()];
    uit.extend(paden.iter().filter(|p| !p.is_empty()).cloned());
    uit
}

/// Een opdrachtregel zoals CreateProcess hem krijgt (de regels van CommandLineToArgvW), voor het logboek en de tests;
/// het starten zelf gaat via `std::process::Command`, dat dezelfde regels volgt.
pub fn opdrachtregel(exe: &str, argumenten: &[String]) -> String {
    let mut uit = format!("\"{exe}\"");
    for a in argumenten {
        uit.push(' ');
        uit.push_str(&windows_argument(a));
    }
    uit
}

/// Eén argument volgens de regels van CommandLineToArgvW: tussen aanhalingstekens als het een spatie, tab of `"`
/// bevat (of leeg is), met backslashes vóór een `"` en aan het eind verdubbeld.
pub fn windows_argument(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"', '\n']) {
        return a.to_string();
    }
    let mut uit = String::from("\"");
    let mut backslashes = 0;
    for c in a.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                uit.push_str(&"\\".repeat(backslashes * 2 + 1));
                uit.push('"');
                backslashes = 0;
            }
            _ => {
                uit.push_str(&"\\".repeat(backslashes));
                uit.push(c);
                backslashes = 0;
            }
        }
    }
    uit.push_str(&"\\".repeat(backslashes * 2));
    uit.push('"');
    uit
}

/// Processorarchitectuur van een pakket: de Verkenner laadt alleen een DLL van zijn eigen architectuur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architectuur {
    X64,
    Arm64,
}

impl Architectuur {
    pub fn naam(self) -> &'static str {
        match self {
            Architectuur::X64 => "x64",
            Architectuur::Arm64 => "arm64",
        }
    }

    pub fn van(naam: &str) -> Option<Architectuur> {
        match naam {
            "x64" => Some(Architectuur::X64),
            "arm64" => Some(Architectuur::Arm64),
            _ => None,
        }
    }

    /// De DLL ten opzichte van de installatiemap.
    pub fn dll(self) -> String {
        format!("{MAP}\\{}\\{DLL}", self.naam())
    }

    /// Het ondertekende pakket ten opzichte van de installatiemap.
    pub fn pakket(self) -> String {
        format!("{MAP}\\OpenAEC-Verkenner-{}.msix", self.naam())
    }
}

/// Wat er in het manifest van het pakket wisselt.
#[derive(Debug, Clone)]
pub struct Pakketgegevens<'a> {
    pub naam: &'a str,
    pub uitgever: &'a str,
    /// Vier getallen (`0.8.1.0`).
    pub versie: String,
    pub architectuur: Architectuur,
    pub clsid: &'a str,
    /// De app in de installatiemap.
    pub exe: &'a str,
    pub weergavenaam: &'a str,
}

impl Pakketgegevens<'_> {
    /// Het pakket van de app (de echte naam, uitgever en CLSID).
    pub fn app(versie: &str, architectuur: Architectuur) -> Pakketgegevens<'static> {
        Pakketgegevens {
            naam: PAKKETNAAM,
            uitgever: UITGEVER,
            versie: pakketversie(versie),
            architectuur,
            clsid: CLSID,
            exe: "openaec-cloud.exe",
            weergavenaam: "OpenAEC Desktop App",
        }
    }
}

/// `0.8.1` → `0.8.1.0` (een pakketversie heeft vier delen; een achtervoegsel als `-beta` valt weg).
pub fn pakketversie(versie: &str) -> String {
    let kern = versie.split(['-', '+']).next().unwrap_or("");
    let mut delen: Vec<u32> = kern.split('.').filter_map(|d| d.parse().ok()).take(4).collect();
    delen.resize(4, 0);
    delen.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(".")
}

fn xml(tekst: &str) -> String {
    tekst.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Het sjabloon van het manifest.
pub const MANIFEST_SJABLOON: &str = include_str!("../pakket/AppxManifest.xml");

/// Het manifest van het pakket met identiteit (een *sparse package*: alleen het manifest zit in het pakket, de app,
/// de DLL en het logo staan in de installatiemap, de *external location*).
pub fn manifest(g: &Pakketgegevens) -> String {
    let uit = MANIFEST_SJABLOON
        .replace("{{NAAM}}", &xml(g.naam))
        .replace("{{UITGEVER}}", &xml(g.uitgever))
        .replace("{{VERSIE}}", &xml(&g.versie))
        .replace("{{ARCHITECTUUR}}", g.architectuur.naam())
        .replace("{{CLSID}}", g.clsid)
        .replace("{{DLL}}", &xml(&g.architectuur.dll()))
        .replace("{{LOGO}}", &xml(&format!("{MAP}\\{LOGO}")))
        .replace("{{EXE}}", &xml(g.exe))
        .replace("{{WEERGAVENAAM}}", &xml(g.weergavenaam));
    debug_assert!(!uit.contains("{{"), "niet alles in het manifest is ingevuld");
    uit
}

// De tests gebruiken Windows-paden.
#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn s(p: &[&str]) -> Vec<String> {
        p.iter().map(|p| p.to_string()).collect()
    }

    fn menu() -> Menu {
        Menu { exe: Some(r"C:\Apps\openaec-cloud.exe".into()), mappen: s(&[r"C:\Werk\Project A\", "D:/Archief"]), versturen: true }
    }

    #[test]
    fn delen_alleen_als_alles_in_een_gekoppelde_map_ligt() {
        let m = menu();
        assert!(m.zichtbaar(Opdracht::Delen, &s(&[r"C:\Werk\Project A\tekening.pdf"])));
        assert!(m.zichtbaar(Opdracht::Delen, &s(&[r"c:\werk\project a"])));
        assert!(m.zichtbaar(Opdracht::Delen, &s(&[r"C:\Werk\Project A\a.pdf", r"D:\Archief\2024\b.dwg"])));
        assert!(!m.zichtbaar(Opdracht::Delen, &s(&[r"C:\Werk\Project A\a.pdf", r"C:\Werk\los.txt"])));
        assert!(!m.zichtbaar(Opdracht::Delen, &s(&[r"C:\Werk\Project AB\a.pdf"])));
        assert!(!m.zichtbaar(Opdracht::Delen, &s(&[r"C:\Werk"])));
        assert!(!m.zichtbaar(Opdracht::Delen, &[]));
        assert!(!Menu::default().zichtbaar(Opdracht::Delen, &s(&[r"C:\Werk\Project A\a.pdf"])));
    }

    #[test]
    fn versturen_op_alles_tenzij_uitgezet() {
        let mut m = menu();
        assert!(m.zichtbaar(Opdracht::Versturen, &s(&[r"C:\Users\Jan\Downloads\x.zip"])));
        assert_eq!(m.items(&s(&[r"C:\Werk\Project A\a.pdf"])), [Opdracht::Delen, Opdracht::Versturen]);
        assert_eq!(m.items(&s(&[r"E:\los.txt"])), [Opdracht::Versturen]);
        m.versturen = false;
        assert_eq!(m.items(&s(&[r"E:\los.txt"])), []);
        assert_eq!(m.items(&s(&[r"D:\Archief\x"])), [Opdracht::Delen]);
        assert_eq!(m.items(&[]), []);
        // Zonder menu.json: Versturen aan, Delen nergens.
        assert_eq!(Menu::default().items(&s(&[r"C:\x"])), [Opdracht::Versturen]);
    }

    #[test]
    fn binnen_een_map() {
        assert!(binnen(r"C:\a\b", r"C:\a"));
        assert!(binnen(r"C:\a\b", "C:\\"));
        assert!(binnen("C:\\", "C:\\"));
        assert!(binnen(r"\\server\deel\x", r"\\server\deel\"));
        assert!(!binnen(r"C:\ab", r"C:\a"));
        assert!(!binnen(r"C:\a", r"C:\a\b"));
        assert!(!binnen(r"C:\a", ""));
    }

    #[test]
    fn opdrachtregel_met_alle_paden() {
        let paden = s(&[r"C:\Werk\Project A\a.pdf", r"C:\b.txt", ""]);
        let a = argumenten(Opdracht::Versturen, &paden);
        assert_eq!(a, s(&["--actie", "versturen", r"C:\Werk\Project A\a.pdf", r"C:\b.txt"]));
        assert_eq!(argumenten(Opdracht::Delen, &paden[..1])[..2], s(&["--actie", "delen"]));
        assert_eq!(
            opdrachtregel(r"C:\Program Files\OpenAEC\openaec-cloud.exe", &a),
            r#""C:\Program Files\OpenAEC\openaec-cloud.exe" --actie versturen "C:\Werk\Project A\a.pdf" C:\b.txt"#
        );
    }

    #[test]
    fn argumenten_volgens_de_regels_van_windows() {
        assert_eq!(windows_argument("a"), "a");
        assert_eq!(windows_argument(""), "\"\"");
        assert_eq!(windows_argument("a b"), "\"a b\"");
        // Een map met een spatie eindigt op een backslash: die wordt verdubbeld, anders escapet hij het `"`.
        assert_eq!(windows_argument(r"C:\Mijn map\"), r#""C:\Mijn map\\""#);
        assert_eq!(windows_argument(r#"a"b"#), r#""a\"b""#);
        assert_eq!(windows_argument(r#"a\"b"#), r#""a\\\"b""#);
    }

    #[test]
    fn menu_bewaren_en_laden() {
        let map = std::env::temp_dir().join(format!("openaec-shellmenu-test-{}", std::process::id()));
        let pad = map.join("menu.json");
        assert_eq!(Menu::laad(&pad), Menu::default());
        assert!(menu().bewaar(&pad).unwrap());
        assert!(!menu().bewaar(&pad).unwrap());
        assert_eq!(Menu::laad(&pad), menu());
        std::fs::write(&pad, b"{kapot").unwrap();
        assert_eq!(Menu::laad(&pad), Menu::default());
        // Velden die ontbreken krijgen de standaard.
        std::fs::write(&pad, br#"{"mappen":["C:\\a"]}"#).unwrap();
        assert_eq!(Menu::laad(&pad), Menu { exe: None, mappen: s(&[r"C:\a"]), versturen: true });
        let _ = std::fs::remove_dir_all(&map);
    }

    #[test]
    fn pakketversie_heeft_vier_delen() {
        assert_eq!(pakketversie("0.8.1"), "0.8.1.0");
        assert_eq!(pakketversie("1.2.3-beta.1"), "1.2.3.0");
        assert_eq!(pakketversie("2"), "2.0.0.0");
        assert_eq!(pakketversie(env!("CARGO_PKG_VERSION")).split('.').count(), 4);
    }

    #[test]
    fn manifest_voor_de_app() {
        let m = manifest(&Pakketgegevens::app("0.8.1", Architectuur::Arm64));
        assert!(!m.contains("{{"));
        assert!(m.contains(r#"Name="OpenAEC.DesktopApp.Verkenner""#));
        assert!(m.contains(&format!(r#"Publisher="{UITGEVER}""#)));
        assert!(m.contains(r#"Version="0.8.1.0""#));
        assert!(m.contains(r#"ProcessorArchitecture="arm64""#));
        assert!(m.contains(r#"Executable="openaec-cloud.exe""#));
        assert!(m.contains(r#"Path="shellmenu\arm64\openaec_shellmenu.dll""#));
        assert!(m.contains(r#"<uap10:AllowExternalContent>true</uap10:AllowExternalContent>"#));
        assert!(m.contains(r#"Category="windows.fileExplorerContextMenus""#));
        // Het menu op bestanden en op mappen, met dezelfde COM-klasse als de server.
        assert_eq!(m.matches(&format!(r#"Clsid="{CLSID}""#)).count(), 2);
        assert!(m.contains(&format!(r#"<com:Class Id="{CLSID}""#)));
        assert!(m.contains(r#"<desktop5:ItemType Type="*">"#) && m.contains(r#"<desktop5:ItemType Type="Directory">"#));
        assert_eq!(Architectuur::X64.pakket(), r"shellmenu\OpenAEC-Verkenner-x64.msix");
        assert_eq!(Architectuur::van("arm64"), Some(Architectuur::Arm64));
        assert_eq!(Architectuur::van("x86"), None);
    }

    #[test]
    fn uitgever_zonder_xml_tekens() {
        // Het onderwerp komt letterlijk in het manifest; signtool weigert het pakket als het niet precies klopt.
        assert!(!UITGEVER.contains(['&', '<', '>', '"']));
        assert!(UITGEVER.starts_with("CN=Impertio Studio B.V., O=Impertio Studio B.V.,"));
    }
}
