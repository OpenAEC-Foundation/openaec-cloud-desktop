//! Instellingen van de app (server en gekoppelde mappen) in `instellingen.json` in de gebruikersmap;
//! de sleutel staat apart in de sleutelbos van het besturingssysteem.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const DIENST: &str = "OpenAEC Cloud";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Koppeling {
    pub ruimte: String,
    pub ruimte_naam: String,
    pub lokaal: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Instellingen {
    pub basis: Option<String>,
    #[serde(default)]
    pub koppelingen: Vec<Koppeling>,
}

fn bestand() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("OpenAEC Cloud").join("instellingen.json")
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
