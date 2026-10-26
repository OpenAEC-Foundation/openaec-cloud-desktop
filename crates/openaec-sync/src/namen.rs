//! Namen die niet overal kunnen, en de negeerlijst.
//!
//! Een naam die op Windows niet kan, is daar met normale paden niet te openen, te verplaatsen of te verwijderen, en
//! een sync-programma blijft er dan op vastlopen. Hulpprogramma's maken ze vaak per ongeluk (een shell-omleiding als
//! `2>nul` in een Unix-shell op Windows geeft een bestand dat letterlijk `nul` heet). De server weigert dezelfde namen
//! (foutcode `ongeldige_naam`, zie `namen.py` in openaec-storage), dus de motor stuurt ze niet en zet ze in de lijst
//! met overgeslagen bestanden. Lokaal lezen en opruimen gaat op Windows via het voorvoegsel `\\?\` ([`lang_pad`]).
//!
//! Ongeldig ([`naamprobleem`]):
//! - de apparaatnamen CON, PRN, AUX, NUL, CONIN$, CONOUT$, COM0-9 en LPT0-9 (ook met de superscriptcijfers ¹²³),
//!   zonder onderscheid tussen hoofd- en kleine letters en ook met een extensie (`nul.txt`, `con.log`);
//! - namen die op een punt of spatie eindigen;
//! - de tekens `<>:"/\|?*` en controletekens;
//! - namen langer dan [`MAX_NAAM`] tekens en paden langer dan [`MAX_PAD`] tekens.

use std::path::{Component, Path, PathBuf, Prefix};

/// Langste naam (tekens), gelijk aan de server.
pub const MAX_NAAM: usize = 255;
/// Langste pad binnen een gekoppelde map (tekens). Langer kan de Verkenner niet meer aan, ook als de sync het kan.
pub const MAX_PAD: usize = 1000;

const VERBODEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const APPARATEN: &[&str] = &["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];
const CIJFERS: &[char] = &['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '¹', '²', '³'];

/// Waarom een naam of pad niet kan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probleem {
    /// Een apparaatnaam van Windows (bijv. `nul`, `con.log`).
    Gereserveerd(String),
    /// Eindigt op een punt of spatie.
    PuntOfSpatie,
    /// Een teken dat niet in een naam kan.
    Teken(char),
    Controleteken,
    TeLang,
    Leeg,
}

impl Probleem {
    /// Is het de naam zelf (en niet de lengte van het pad)? Zulke bestanden kun je veilig opruimen of hernoemen.
    pub fn in_de_naam(&self) -> bool {
        !matches!(self, Probleem::TeLang)
    }
}

impl std::fmt::Display for Probleem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Probleem::Gereserveerd(naam) => write!(f, "{naam} is een gereserveerde naam op Windows"),
            Probleem::PuntOfSpatie => write!(f, "een naam kan niet op een punt of spatie eindigen"),
            Probleem::Teken(t) => write!(f, "het teken {t} kan niet in een naam"),
            Probleem::Controleteken => write!(f, "een naam kan geen controleteken bevatten"),
            Probleem::TeLang => write!(f, "de naam of het pad is te lang"),
            Probleem::Leeg => write!(f, "de naam is leeg"),
        }
    }
}

/// Het deel van de naam vóór de eerste punt, zonder spaties aan het eind (zo kijkt Windows naar apparaatnamen).
fn stam(naam: &str) -> &str {
    naam.split('.').next().unwrap_or("").trim_end_matches(' ')
}

/// Is dit een apparaatnaam van Windows (ook met extensie, in alle hoofdletters)?
pub fn is_gereserveerd(naam: &str) -> bool {
    let stam = stam(naam).to_uppercase();
    APPARATEN.contains(&stam.as_str())
        || ((stam.starts_with("COM") || stam.starts_with("LPT"))
            && stam.chars().count() == 4
            && stam.chars().last().is_some_and(|c| CIJFERS.contains(&c)))
}

/// Wat er mis is met één naam, of None als hij overal kan.
pub fn naamprobleem(naam: &str) -> Option<Probleem> {
    if naam.is_empty() || naam == "." || naam == ".." {
        return Some(Probleem::Leeg);
    }
    if naam.chars().count() > MAX_NAAM {
        return Some(Probleem::TeLang);
    }
    if let Some(t) = naam.chars().find(|c| VERBODEN.contains(c)) {
        return Some(Probleem::Teken(t));
    }
    if naam.chars().any(|c| (c as u32) < 32) {
        return Some(Probleem::Controleteken);
    }
    if naam.ends_with('.') || naam.ends_with(' ') {
        return Some(Probleem::PuntOfSpatie);
    }
    if is_gereserveerd(naam) {
        return Some(Probleem::Gereserveerd(stam(naam).to_string()));
    }
    None
}

/// Wat er mis is met een pad (met '/'): het eerste deel met een probleem, of een te lang pad.
pub fn padprobleem(pad: &str) -> Option<Probleem> {
    if pad.chars().count() > MAX_PAD {
        return Some(Probleem::TeLang);
    }
    pad.split('/').find_map(naamprobleem)
}

/// Een naam die overal kan, zo dicht mogelijk bij het origineel: een apparaatnaam krijgt een `_` achter de stam
/// (`nul` → `nul_`, `con.log` → `con_.log`), verboden tekens en controletekens worden `_`, en een punt of spatie
/// aan het eind wordt `_`. Een naam zonder probleem blijft gelijk.
pub fn veilige_naam(naam: &str) -> String {
    if naamprobleem(naam).is_none_or(|p| !p.in_de_naam()) {
        return naam.to_string();
    }
    if naam.is_empty() || naam == "." || naam == ".." {
        return "_".repeat(naam.len().max(1));
    }
    let mut uit: String = naam.chars().map(|c| if VERBODEN.contains(&c) || (c as u32) < 32 { '_' } else { c }).collect();
    let einde = uit.trim_end_matches(['.', ' ']).len();
    let staart = uit.len() - einde;
    uit.truncate(einde);
    uit.push_str(&"_".repeat(staart));
    if is_gereserveerd(&uit) {
        let lengte = uit.split('.').next().unwrap_or("").len();
        uit.insert(lengte, '_');
    }
    uit
}

/// De naam zoals de server hem bewaart: zonder witruimte vooraan en achteraan (de server haalt die weg). Een spatie
/// vooraan kan hier gewoon; achteraan is het al een [`Probleem::PuntOfSpatie`].
pub fn servernaam(naam: &str) -> &str {
    naam.trim()
}

/// [`servernaam`] voor elk deel van een pad (met '/').
pub fn serverpad(pad: &str) -> String {
    pad.split('/').map(servernaam).collect::<Vec<_>>().join("/")
}

/// Moet deze (geldige) naam letterlijk geopend worden ([`lang_pad`])? Bij witruimte vooraan: zo blijft die in elke
/// laag van Windows staan.
pub fn letterlijk_nodig(naam: &str) -> bool {
    naam.starts_with(char::is_whitespace)
}

/// [`veilige_naam`] voor elk deel van een pad (met '/').
pub fn veilig_pad(pad: &str) -> String {
    pad.split('/').map(veilige_naam).collect::<Vec<_>>().join("/")
}

/// Een pad dat Windows letterlijk neemt (`\\?\C:\…` of `\\?\UNC\server\…`), zodat ook `nul` of `rapport.` te openen,
/// te hernoemen en te verwijderen is. Alleen voor een volledig pad; elders (en op andere systemen) blijft het gelijk.
pub fn lang_pad(pad: &Path) -> PathBuf {
    if !cfg!(windows) || !pad.is_absolute() {
        return pad.to_path_buf();
    }
    let mut delen = pad.components();
    let Some(Component::Prefix(voorvoegsel)) = delen.next() else { return pad.to_path_buf() };
    let mut uit = match voorvoegsel.kind() {
        Prefix::Verbatim(_) | Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(..) => return pad.to_path_buf(),
        Prefix::Disk(schijf) => format!(r"\\?\{}:", schijf as char),
        Prefix::UNC(server, deel) => format!(r"\\?\UNC\{}\{}", server.to_string_lossy(), deel.to_string_lossy()),
        _ => return pad.to_path_buf(),
    };
    // Letterlijk: geen '.', '..' of '/' meer, dus de delen zelf aan elkaar.
    let mut stapel: Vec<String> = Vec::new();
    for deel in delen {
        match deel {
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
            Component::ParentDir => {
                stapel.pop();
            }
            Component::Normal(n) => stapel.push(n.to_string_lossy().to_string()),
        }
    }
    for deel in stapel {
        uit.push('\\');
        uit.push_str(&deel);
    }
    PathBuf::from(uit)
}

/// Verwijder een bestand met een lastige naam veilig: op Windows via [`lang_pad`]. Met `prullenbak` gaat het eerst
/// onder een veilige naam naar `<root>/.openaec/opgeruimd/` en vandaar naar de prullenbak; lukt dat niet, dan wordt
/// het daar verwijderd. `rel` is het pad met '/' ten opzichte van `root`.
pub fn verwijder_veilig(root: &Path, rel: &str, prullenbak: Option<fn(&Path) -> bool>) -> std::io::Result<()> {
    let pad = lang_pad(&rel.split('/').fold(root.to_path_buf(), |p, d| p.join(d)));
    if !std::fs::symlink_metadata(&pad)?.is_file() {
        return Err(std::io::Error::other("geen bestand"));
    }
    let Some(prullenbak) = prullenbak else {
        return std::fs::remove_file(&pad);
    };
    let opgeruimd = root.join(crate::motor::TOESTANDSMAP).join("opgeruimd");
    std::fs::create_dir_all(&opgeruimd)?;
    let tijd = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let naam = veilige_naam(rel.rsplit('/').next().unwrap_or(rel));
    let doel = opgeruimd.join(format!("{tijd} {naam}"));
    std::fs::rename(&pad, lang_pad(&doel))?;
    if !prullenbak(&doel) {
        std::fs::remove_file(lang_pad(&doel))?;
    }
    Ok(())
}

/// Patronen die altijd genegeerd worden: tijdelijke bestanden van programma's en systeembestanden van de Verkenner
/// en macOS. Een eigen `.openaecignore` in de gekoppelde map komt erbij.
pub const STANDAARD_NEGEREN: &[&str] = &["~$*", "*.tmp", ".DS_Store", "Thumbs.db", "desktop.ini"];

/// Naam van de negeerlijst in de hoofdmap van een gekoppelde map.
pub const NEGEERBESTAND: &str = ".openaecignore";

/// Bestanden die niet gesynchroniseerd worden (niet geüpload, niet gedownload, niet verwijderd).
///
/// Eén patroon per regel; `#` begint een opmerking. Zonder `/` geldt een patroon voor elke naam in het pad (een
/// bestand of een map ergens onderweg); met `/` voor het hele pad vanaf de hoofdmap (een `/` vooraan mag). Een `/`
/// aan het eind: alleen mappen. `*` is een willekeurig stuk binnen een naam, `**` ook over mappen heen, `?` één
/// teken. Hoofdletters maken niet uit.
#[derive(Debug, Clone)]
pub struct Negeerlijst {
    patronen: Vec<(String, bool)>,
}

impl Default for Negeerlijst {
    fn default() -> Self {
        Self::standaard()
    }
}

impl Negeerlijst {
    /// Alleen de standaardpatronen ([`STANDAARD_NEGEREN`]).
    pub fn standaard() -> Self {
        Self::uit_tekst("")
    }

    /// De standaardpatronen plus die uit `tekst` (de inhoud van een `.openaecignore`).
    pub fn uit_tekst(tekst: &str) -> Self {
        let patronen = STANDAARD_NEGEREN
            .iter()
            .copied()
            .chain(tekst.lines().map(str::trim))
            .filter(|r| !r.is_empty() && !r.starts_with('#'))
            .map(|r| {
                let alleen_mappen = r.ends_with('/');
                (r.trim_end_matches('/').trim_start_matches('/').to_lowercase(), alleen_mappen)
            })
            .filter(|(p, _)| !p.is_empty())
            .collect();
        Self { patronen }
    }

    /// De negeerlijst van een gekoppelde map (standaard als er geen `.openaecignore` is).
    pub fn laad(root: &Path) -> Self {
        Self::uit_tekst(&std::fs::read_to_string(root.join(NEGEERBESTAND)).unwrap_or_default())
    }

    /// Wordt het bestand `pad` (met '/') genegeerd?
    pub fn negeert(&self, pad: &str) -> bool {
        let pad = pad.to_lowercase();
        let delen: Vec<&str> = pad.split('/').collect();
        self.patronen.iter().any(|(patroon, alleen_mappen)| {
            if patroon.contains('/') || patroon.contains("**") {
                // Het pad zelf, of een map erboven.
                (1..=delen.len())
                    .filter(|&n| !alleen_mappen || n < delen.len())
                    .any(|n| past(patroon.as_bytes(), delen[..n].join("/").as_bytes()))
            } else {
                let mappen = if *alleen_mappen { &delen[..delen.len() - 1] } else { &delen[..] };
                mappen.iter().any(|d| past(patroon.as_bytes(), d.as_bytes()))
            }
        })
    }
}

/// Past `tekst` op het patroon (`*` binnen een naam, `**` over mappen heen, `?` één teken)?
fn past(patroon: &[u8], tekst: &[u8]) -> bool {
    match (patroon.first(), tekst.first()) {
        (None, None) => true,
        (Some(b'*'), _) if patroon.get(1) == Some(&b'*') => {
            let rest = patroon[2..].strip_prefix(b"/").unwrap_or(&patroon[2..]);
            (0..=tekst.len()).any(|i| past(rest, &tekst[i..]))
        }
        (Some(b'*'), _) => {
            (0..=tekst.len()).take_while(|&i| i == 0 || tekst[i - 1] != b'/').any(|i| past(&patroon[1..], &tekst[i..]))
        }
        (Some(b'?'), Some(t)) if *t != b'/' => past(&patroon[1..], &tekst[1..]),
        (Some(p), Some(t)) if p == t => past(&patroon[1..], &tekst[1..]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apparaatnamen_ook_met_extensie_en_superscript() {
        for naam in [
            "nul", "NUL", "Nul", "con", "prn", "aux", "nul.txt", "con.log", "aux.tar.gz", "nul .txt", "COM1", "com9", "COM0",
            "lpt1", "LPT9.doc", "LPT0", "COM¹", "com²", "LPT³.txt", "CONIN$", "conout$",
        ] {
            assert!(matches!(naamprobleem(naam), Some(Probleem::Gereserveerd(_))), "{naam}");
            assert!(naamprobleem(naam).unwrap().to_string().contains("gereserveerde naam"));
        }
    }

    #[test]
    fn namen_die_erop_lijken_kunnen_wel() {
        for naam in [
            "nul_", "nul_.txt", "null", "nullen.txt", "connect.txt", "auxiliary", "com10", "LPT", "com", "lpt1x.txt",
            "con-tract.pdf", "Console.log", ".nul", "x.nul", "COM1_", "COM⁴", "plan (conflict Jan 2026-10-23 1200).pdf",
        ] {
            assert_eq!(naamprobleem(naam), None, "{naam}");
        }
    }

    #[test]
    fn punt_spatie_tekens_lengte() {
        assert_eq!(naamprobleem("rapport."), Some(Probleem::PuntOfSpatie));
        assert_eq!(naamprobleem("rapport "), Some(Probleem::PuntOfSpatie));
        for t in VERBODEN {
            assert_eq!(naamprobleem(&format!("a{t}b.txt")), Some(Probleem::Teken(*t)));
        }
        assert_eq!(naamprobleem("a\tb"), Some(Probleem::Controleteken));
        assert_eq!(naamprobleem("a\u{0}b"), Some(Probleem::Controleteken));
        assert_eq!(naamprobleem(&"a".repeat(255)), None);
        assert_eq!(naamprobleem(&"a".repeat(256)), Some(Probleem::TeLang));
        assert_eq!(naamprobleem(""), Some(Probleem::Leeg));
        assert_eq!(padprobleem(&"map/".repeat(300)), Some(Probleem::TeLang));
        assert_eq!(padprobleem("Tekeningen/nul/plan.pdf"), Some(Probleem::Gereserveerd("nul".into())));
        assert_eq!(padprobleem("Tekeningen/2026/plan.pdf"), None);
        assert!(!Probleem::TeLang.in_de_naam());
        assert!(Probleem::PuntOfSpatie.in_de_naam());
    }

    #[test]
    fn veilige_namen() {
        assert_eq!(veilige_naam("nul"), "nul_");
        assert_eq!(veilige_naam("NUL"), "NUL_");
        assert_eq!(veilige_naam("con.log"), "con_.log");
        assert_eq!(veilige_naam("aux.tar.gz"), "aux_.tar.gz");
        assert_eq!(veilige_naam("rapport."), "rapport_");
        assert_eq!(veilige_naam("a. "), "a__");
        assert_eq!(veilige_naam("a:b?.txt"), "a_b_.txt");
        assert_eq!(veilige_naam("plan.pdf"), "plan.pdf");
        assert_eq!(veilige_naam("com1."), "com1_");
        assert_eq!(veilig_pad("Map/nul/con.txt"), "Map/nul_/con_.txt");
        for naam in ["nul", "con.log", "a:b?.txt", "rapport.", "LPT¹"] {
            assert_eq!(naamprobleem(&veilige_naam(naam)), None, "{naam}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn lang_pad_op_windows() {
        assert_eq!(lang_pad(Path::new(r"C:\Werk\nul")), PathBuf::from(r"\\?\C:\Werk\nul"));
        assert_eq!(lang_pad(Path::new(r"C:\Werk\.\a\..\nul")), PathBuf::from(r"\\?\C:\Werk\nul"));
        assert_eq!(lang_pad(Path::new(r"\\server\deel\map\con.txt")), PathBuf::from(r"\\?\UNC\server\deel\map\con.txt"));
        assert_eq!(lang_pad(Path::new(r"\\?\C:\al\goed")), PathBuf::from(r"\\?\C:\al\goed"));
        assert_eq!(lang_pad(Path::new(r"relatief\nul")), PathBuf::from(r"relatief\nul"));
    }

    #[test]
    fn negeerlijst_standaard_en_eigen_patronen() {
        let n = Negeerlijst::standaard();
        for pad in ["~$rapport.docx", "Map/~$plan.xlsx", "a.tmp", "Map/B.TMP", ".DS_Store", "x/Thumbs.db", "thumbs.db", "Desktop.ini"] {
            assert!(n.negeert(pad), "{pad}");
        }
        for pad in ["rapport.docx", "tmp", "a.tmpx", "Map/plan.pdf", "desktop.ini.bak"] {
            assert!(!n.negeert(pad), "{pad}");
        }
        let n = Negeerlijst::uit_tekst("# eigen regels\n\n*.bak\nbuild/\n/Export/*.pdf\ncache/**\n");
        assert!(n.negeert("a.bak") && n.negeert("Map/b.BAK"));
        assert!(n.negeert("build/uit.o") && n.negeert("src/build/uit.o"));
        assert!(!n.negeert("build"), "alleen een map heet zo");
        assert!(n.negeert("Export/plan.pdf") && !n.negeert("Ander/Export/plan.pdf") && !n.negeert("Export/sub/plan.pdf"));
        assert!(n.negeert("cache/a/b/c.txt"));
        assert!(n.negeert("a.tmp"), "de standaard blijft gelden");
        assert!(!n.negeert("plan.pdf"));
    }

    #[test]
    fn verwijderen_van_een_gewoon_bestand() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("map")).unwrap();
        std::fs::write(dir.path().join("map/weg.txt"), b"x").unwrap();
        verwijder_veilig(dir.path(), "map/weg.txt", None).unwrap();
        assert!(!dir.path().join("map/weg.txt").exists());
        // Met prullenbak: eerst onder een veilige naam in de toestandsmap; weigert de prullenbak, dan weg.
        std::fs::write(dir.path().join("twee.txt"), b"x").unwrap();
        verwijder_veilig(dir.path(), "twee.txt", Some(|_| false)).unwrap();
        assert!(!dir.path().join("twee.txt").exists());
        let opgeruimd = dir.path().join(crate::motor::TOESTANDSMAP).join("opgeruimd");
        assert_eq!(std::fs::read_dir(opgeruimd).unwrap().count(), 0);
        assert!(verwijder_veilig(dir.path(), "bestaat-niet.txt", None).is_err());
    }
}
