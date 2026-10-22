//! Gedeelde testhulp: een opslag in het geheugen met dezelfde conflictregel als de server.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

use openaec_sync::motor::{Opslag, OpslagBestand, Upload};
use openaec_sync::Fout;

pub type Inhoud = BTreeMap<String, (String, u64, Vec<u8>)>;

/// Opslag in het geheugen: pad (met '/') -> (bestand-id, versie, inhoud). Conflictregel zoals de server.
#[derive(Default)]
pub struct NepOpslag {
    pub bestanden: Mutex<Inhoud>,
    pub teller: Mutex<u64>,
    /// Paden waarvoor uploaden en downloaden mislukt (om fouten per bestand na te bootsen).
    pub weigeren: Mutex<BTreeSet<String>>,
}

impl NepOpslag {
    pub fn zet(&self, pad: &str, inhoud: &[u8]) {
        let mut b = self.bestanden.lock().unwrap();
        let mut t = self.teller.lock().unwrap();
        let versie = b.get(pad).map(|x| x.1 + 1).unwrap_or(1);
        let id = b.get(pad).map(|x| x.0.clone()).unwrap_or_else(|| {
            *t += 1;
            format!("b{}", *t)
        });
        b.insert(pad.to_string(), (id, versie, inhoud.to_vec()));
    }
    pub fn inhoud(&self, pad: &str) -> Option<Vec<u8>> {
        self.bestanden.lock().unwrap().get(pad).map(|x| x.2.clone())
    }
    pub fn paden(&self) -> Vec<String> {
        self.bestanden.lock().unwrap().keys().cloned().collect()
    }
    pub fn weiger(&self, pad: &str) {
        self.weigeren.lock().unwrap().insert(pad.to_string());
    }
    fn geweigerd(&self, pad: &str) -> Result<(), Fout> {
        if self.weigeren.lock().unwrap().contains(pad) {
            return Err(Fout::Controle(format!("{pad} geweigerd (test)")));
        }
        Ok(())
    }
}

impl Opslag for NepOpslag {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        Ok(self
            .bestanden
            .lock()
            .unwrap()
            .iter()
            .map(|(pad, (id, versie, inhoud))| OpslagBestand {
                pad: pad.clone(),
                bestand: id.clone(),
                versie: *versie,
                grootte: inhoud.len() as u64,
            })
            .collect())
    }

    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.geweigerd(pad)?;
        let inhoud = std::fs::read(lokaal)?;
        let huidig = self.bestanden.lock().unwrap().get(pad).map(|x| x.1);
        let doel = match (huidig, basis_versie) {
            (Some(h), Some(b)) if h == b => pad.to_string(),
            (None, _) => pad.to_string(),
            _ => {
                let (stam, ext) = pad.rsplit_once('.').map(|(s, e)| (s.to_string(), format!(".{e}"))).unwrap_or((pad.to_string(), String::new()));
                format!("{stam} (conflict test){ext}")
            }
        };
        self.zet(&doel, &inhoud);
        let (id, versie, _) = self.bestanden.lock().unwrap()[&doel].clone();
        Ok(Upload { pad: doel.clone(), bestand: id, versie, conflict: doel != pad })
    }

    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.geweigerd(&bestand.pad)?;
        let inhoud = self.inhoud(&bestand.pad).unwrap();
        if let Some(map) = lokaal.parent() {
            std::fs::create_dir_all(map)?;
        }
        std::fs::write(lokaal, inhoud)?;
        Ok(())
    }

    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.bestanden.lock().unwrap().remove(&bestand.pad);
        Ok(())
    }
}

pub fn schrijf(map: &Path, pad: &str, inhoud: &[u8]) {
    let p = map.join(pad);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, inhoud).unwrap();
    // mtime moet zichtbaar veranderen voor de wijzigingsdetectie
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    std::fs::File::options().write(true).open(map.join(pad)).unwrap().set_modified(later).unwrap();
}

