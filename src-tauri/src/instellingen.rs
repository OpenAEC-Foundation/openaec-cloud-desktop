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
}

/// Hoe de sync werkt (voor alle gekoppelde mappen).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncInstellingen {
    /// Zoveel overdrachten tegelijk (1–32; standaard 8).
    #[serde(default = "standaard_gelijktijdig")]
    pub gelijktijdig: usize,
}

fn standaard_gelijktijdig() -> usize {
    openaec_sync::STANDAARD_GELIJKTIJDIG
}

impl Default for SyncInstellingen {
    fn default() -> Self {
        Self { gelijktijdig: standaard_gelijktijdig() }
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
