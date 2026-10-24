//! Instellingen van de app (server en gekoppelde mappen) in `instellingen.json` in de gebruikersmap;
//! de sleutel staat apart in de sleutelbos van het besturingssysteem.

use std::path::PathBuf;

use openaec_sync::selectie::NieuweMappen;
use serde::{Deserialize, Serialize};

const DIENST: &str = "OpenAEC Storage";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Koppeling {
    pub ruimte: String,
    pub ruimte_naam: String,
    pub lokaal: String,
    /// Mappen (paden met '/') die niet op deze computer komen (mapselectie).
    #[serde(default)]
    pub uitgesloten: Vec<String>,
    /// Nieuwe mappen op de server: automatisch meenemen (standaard) of alleen wat gekozen is.
    #[serde(default)]
    pub nieuwe_mappen: NieuweMappen,
    /// De mappen van de server toen er gekozen werd; bij [`NieuweMappen::AlleenGekozen`] blijft een map die hier niet
    /// in staat weg.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bekende_mappen: Vec<String>,
    /// Een map die met mij gedeeld is (geen hele ruimte): de deling, de gedeelde map (de wortel), de rechten
    /// (`lezen`, `schrijven` of `beheren`) en wie hem deelde. `ruimte_naam` is dan de naam van de map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deling: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wortel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rechten: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gedeeld_door: Option<String>,
}

impl Koppeling {
    /// Een koppeling aan een hele ruimte.
    pub fn ruimte(ruimte: String, ruimte_naam: String, lokaal: String) -> Self {
        Self {
            ruimte,
            ruimte_naam,
            lokaal,
            uitgesloten: Vec::new(),
            nieuwe_mappen: Default::default(),
            bekende_mappen: Vec::new(),
            deling: None,
            wortel: None,
            rechten: None,
            gedeeld_door: None,
        }
    }
}

/// Hoe de sync werkt (voor alle gekoppelde mappen).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncInstellingen {
    /// Zoveel overdrachten tegelijk (1–32; standaard 8).
    #[serde(default = "standaard_gelijktijdig")]
    pub gelijktijdig: usize,
    /// Bestanden met een gereserveerde of ongeldige naam: overslaan (standaard), opruimen als ze klein zijn, of
    /// onder een veilige naam uploaden.
    #[serde(default)]
    pub gereserveerd: openaec_sync::motor::Gereserveerd,
}

fn standaard_gelijktijdig() -> usize {
    openaec_sync::STANDAARD_GELIJKTIJDIG
}

impl Default for SyncInstellingen {
    fn default() -> Self {
        Self { gelijktijdig: standaard_gelijktijdig(), gereserveerd: Default::default() }
    }
}

impl SyncInstellingen {
    /// Binnen de grenzen gebracht (een met de hand aangepast bestand kan er buiten liggen).
    pub fn begrensd(mut self) -> Self {
        self.gelijktijdig = self.gelijktijdig.clamp(1, openaec_sync::MAX_GELIJKTIJDIG);
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Instellingen {
    pub basis: Option<String>,
    /// De hoofdmap: één map met per ruimte een submap, als eigen plek in het navigatiepaneel van de Verkenner en met
    /// bestanden op aanvraag (zie `hoofdmap`). None: nog niet ingesteld (de app stelt [`standaard_hoofdmap`] voor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoofdmap: Option<String>,
    #[serde(default)]
    pub koppelingen: Vec<Koppeling>,
    #[serde(default)]
    pub sync: SyncInstellingen,
}

/// Voorstel voor de hoofdmap: `OpenAEC` in de gebruikersmap.
pub fn standaard_hoofdmap() -> PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir).join("OpenAEC")
}

/// Naam van de submap voor een ruimte in de hoofdmap: de naam van de ruimte, met tekens die in een mapnaam niet kunnen
/// vervangen, en met " (2)", " (3)" … als die naam al bestaat (zonder op hoofdletters te letten).
pub fn submapnaam(ruimte_naam: &str, bestaand: &[String]) -> String {
    let schoon: String = ruimte_naam
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect();
    let schoon = schoon.trim().trim_end_matches(['.', ' ']).to_string();
    let schoon = if schoon.is_empty() || openaec_sync::namen::naamprobleem(&schoon).is_some() { format!("{schoon}_") } else { schoon };
    let bezet = |n: &str| bestaand.iter().any(|b| b.eq_ignore_ascii_case(n));
    if !bezet(&schoon) {
        return schoon;
    }
    (2..).map(|i| format!("{schoon} ({i})")).find(|n| !bezet(n)).unwrap()
}

/// Submap van de hoofdmap voor mappen die met mij gedeeld zijn (`<hoofdmap>\Gedeeld met mij\<naam>`).
pub const GEDEELD_MET_MIJ: &str = "Gedeeld met mij";

impl Instellingen {
    /// Staat deze koppeling in de hoofdmap (bestanden op aanvraag)? Een ruimte direct eronder, een gedeelde map onder
    /// [`GEDEELD_MET_MIJ`].
    pub fn in_hoofdmap(&self, k: &Koppeling) -> bool {
        let Some(h) = self.hoofdmap.as_deref().map(std::path::Path::new) else { return false };
        let ouder = std::path::Path::new(&k.lokaal).parent();
        ouder == Some(h) || (k.deling.is_some() && ouder == Some(h.join(GEDEELD_MET_MIJ).as_path()))
    }
}

fn bestand() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("OpenAEC Storage").join("instellingen.json")
}

impl Instellingen {
    pub fn laad() -> Self {
        std::fs::read(bestand()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn bewaar(&self) -> std::io::Result<()> {
        let pad = bestand();
        std::fs::create_dir_all(pad.parent().unwrap())?;
        std::fs::write(pad, serde_json::to_vec_pretty(self).unwrap())
    }
}

pub fn bewaar_sleutel(basis: &str, autorisatie: &str) -> Result<(), keyring::Error> {
    keyring::Entry::new(DIENST, basis)?.set_password(autorisatie)
}

pub fn lees_sleutel(basis: &str) -> Option<String> {
    keyring::Entry::new(DIENST, basis).ok()?.get_password().ok()
}

pub fn wis_sleutel(basis: &str) {
    if let Ok(e) = keyring::Entry::new(DIENST, basis) {
        let _ = e.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submapnaam_veilig_en_uniek() {
        assert_eq!(submapnaam("Project A", &[]), "Project A");
        assert_eq!(submapnaam("A/B: C?", &[]), "A_B_ C_");
        assert_eq!(submapnaam("Rapport.", &[]), "Rapport");
        assert_eq!(submapnaam("nul", &[]), "nul_");
        assert_eq!(submapnaam("", &[]), "_");
        let bestaand = vec!["project a".to_string(), "Project A (2)".to_string()];
        assert_eq!(submapnaam("Project A", &bestaand), "Project A (3)");
    }

    #[test]
    fn oude_koppeling_zonder_selectie() {
        let k: Koppeling = serde_json::from_str(r#"{"ruimte":"R1","ruimte_naam":"P","lokaal":"P"}"#).unwrap();
        assert_eq!(k.nieuwe_mappen, NieuweMappen::Automatisch);
        assert!(k.bekende_mappen.is_empty() && k.uitgesloten.is_empty());
        let inst: Instellingen = serde_json::from_str(r#"{"basis":null}"#).unwrap();
        assert!(inst.hoofdmap.is_none());
    }

    #[test]
    fn koppeling_in_de_hoofdmap() {
        let hoofdmap = std::env::temp_dir().join("OpenAEC");
        let pad = |p: &std::path::Path| p.to_string_lossy().to_string();
        let inst = Instellingen { hoofdmap: Some(pad(&hoofdmap)), ..Default::default() };
        let mut k: Koppeling = serde_json::from_str(r#"{"ruimte":"R1","ruimte_naam":"P","lokaal":""}"#).unwrap();
        k.lokaal = pad(&hoofdmap.join("P"));
        assert!(inst.in_hoofdmap(&k));
        k.lokaal = pad(&std::env::temp_dir().join("Werk").join("P"));
        assert!(!inst.in_hoofdmap(&k));
        k.lokaal = pad(&hoofdmap.join("P").join("Sub"));
        assert!(!inst.in_hoofdmap(&k));
        // Een gedeelde map onder "Gedeeld met mij"; een ruimte daar niet.
        k.lokaal = pad(&hoofdmap.join(GEDEELD_MET_MIJ).join("Tekeningen"));
        assert!(!inst.in_hoofdmap(&k));
        k.deling = Some("D1".into());
        assert!(inst.in_hoofdmap(&k));
    }

    #[test]
    fn een_oude_koppeling_is_een_hele_ruimte() {
        let k: Koppeling = serde_json::from_str(r#"{"ruimte":"R1","ruimte_naam":"Project","lokaal":"C:\\P"}"#).unwrap();
        assert!(k.deling.is_none() && k.rechten.is_none());
        // En zo blijft hij ook bewaard: zonder de velden van een deling.
        assert!(!serde_json::to_string(&k).unwrap().contains("deling"));
    }

    #[test]
    fn gedeelde_koppeling_met_rechten() {
        let mut k = Koppeling::ruimte("R1".into(), "Tekeningen".into(), "C:\\T".into());
        k.deling = Some("D1".into());
        k.wortel = Some("M1".into());
        k.rechten = Some("beheren".into());
        let terug: Koppeling = serde_json::from_str(&serde_json::to_string(&k).unwrap()).unwrap();
        assert_eq!((terug.deling.as_deref(), terug.wortel.as_deref()), (Some("D1"), Some("M1")));
    }
}
