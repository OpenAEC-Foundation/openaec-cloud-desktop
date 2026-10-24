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

impl Instellingen {
    /// Staat deze koppeling in de hoofdmap (een ruimte met bestanden op aanvraag)?
    pub fn in_hoofdmap(&self, k: &Koppeling) -> bool {
        self.hoofdmap.as_deref().is_some_and(|h| std::path::Path::new(&k.lokaal).parent() == Some(std::path::Path::new(h)))
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
    }
}
