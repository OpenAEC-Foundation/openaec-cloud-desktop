//! Instellingen van de app (server en gekoppelde mappen) in `instellingen.json` in de gebruikersmap;
//! de sleutel staat apart in de sleutelbos van het besturingssysteem.

use std::path::PathBuf;

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
        Self { ruimte, ruimte_naam, lokaal, uitgesloten: Vec::new(), deling: None, wortel: None, rechten: None, gedeeld_door: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    #[serde(default)]
    pub koppelingen: Vec<Koppeling>,
    #[serde(default)]
    pub sync: SyncInstellingen,
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
