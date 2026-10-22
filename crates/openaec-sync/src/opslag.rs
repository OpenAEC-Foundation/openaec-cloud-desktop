//! [`Opslag`] over de echte API: één ruimte, paden met '/' ten opzichte van de ruimte.
//! Mappen worden bij het bladeren onthouden (pad -> map-id) en bij een upload aangemaakt als ze nog niet bestaan.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use crate::motor::{Opslag, OpslagBestand, Upload, Voortgang};
use crate::{download_met_voortgang, upload_met_voortgang, Client, Fout};

pub struct ClientOpslag {
    client: Client,
    ruimte: String,
    mappen: Mutex<HashMap<String, String>>,
    /// Mappen één voor één aanmaken: parallelle uploads (importeren) die dezelfde ontbrekende map nodig hebben,
    /// maken hem anders allemaal tegelijk aan.
    aanmaken: tokio::sync::Mutex<()>,
}

fn splits(pad: &str) -> (&str, &str) {
    pad.rsplit_once('/').unwrap_or(("", pad))
}

fn samen(map: &str, naam: &str) -> String {
    if map.is_empty() { naam.to_string() } else { format!("{map}/{naam}") }
}

impl ClientOpslag {
    pub fn new(client: Client, ruimte: &str) -> Self {
        Self { client, ruimte: ruimte.to_string(), mappen: Mutex::new(HashMap::new()), aanmaken: tokio::sync::Mutex::new(()) }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Alle mappen van de ruimte als paden met '/', gesorteerd (voor de mapselectie).
    pub async fn alle_mappen(&self) -> Result<Vec<String>, Fout> {
        self.lijst().await?;
        let mut mappen: Vec<String> = self.mappen.lock().unwrap().keys().cloned().collect();
        mappen.sort();
        Ok(mappen)
    }

    /// Map-id voor een bestaand mappad (zonder iets aan te maken); None als de map niet (meer) bestaat.
    pub async fn zoek_map(&self, mappad: &str) -> Result<Option<String>, Fout> {
        let mut map: Option<String> = None;
        for deel in mappad.split('/').filter(|d| !d.is_empty()) {
            let inhoud = self.client.bladeren(&self.ruimte, map.as_deref()).await?;
            match inhoud.mappen.into_iter().find(|m| m.naam == deel) {
                Some(m) => map = Some(m.name),
                None => return Ok(None),
            }
        }
        Ok(map)
    }

    /// Map-id voor een mappad ("" = hoofdniveau), mappen die nog ontbreken worden aangemaakt.
    async fn zorg_voor_map(&self, mappad: &str) -> Result<Option<String>, Fout> {
        if mappad.is_empty() {
            return Ok(None);
        }
        if let Some(id) = self.mappen.lock().unwrap().get(mappad) {
            return Ok(Some(id.clone()));
        }
        let _beurt = self.aanmaken.lock().await;
        let mut bovenliggende: Option<String> = None;
        let mut pad = String::new();
        for deel in mappad.split('/') {
            pad = samen(&pad, deel);
            let bekend = self.mappen.lock().unwrap().get(&pad).cloned();
            let id = match bekend {
                Some(id) => id,
                None => {
                    let id = self.client.maak_map(&self.ruimte, deel, bovenliggende.as_deref()).await?.map;
                    self.mappen.lock().unwrap().insert(pad.clone(), id.clone());
                    id
                }
            };
            bovenliggende = Some(id);
        }
        Ok(bovenliggende)
    }
}

impl Opslag for ClientOpslag {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        let mut uit = Vec::new();
        let mut mappen = HashMap::new();
        let mut te_doen: Vec<(String, Option<String>)> = vec![(String::new(), None)];
        while let Some((pad, map)) = te_doen.pop() {
            let inhoud = self.client.bladeren(&self.ruimte, map.as_deref()).await?;
            for m in inhoud.mappen {
                let sub = samen(&pad, &m.naam);
                mappen.insert(sub.clone(), m.name.clone());
                te_doen.push((sub, Some(m.name)));
            }
            for b in inhoud.bestanden {
                uit.push(OpslagBestand {
                    pad: samen(&pad, &b.naam),
                    bestand: b.name,
                    versie: b.huidige_versie,
                    grootte: b.grootte,
                });
            }
        }
        *self.mappen.lock().unwrap() = mappen;
        Ok(uit)
    }

    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.upload_met_voortgang(pad, lokaal, basis_versie, &|_, _| {}).await
    }

    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.download_met_voortgang(bestand, lokaal, &|_, _| {}).await
    }

    async fn upload_met_voortgang(
        &self,
        pad: &str,
        lokaal: &Path,
        basis_versie: Option<u64>,
        voortgang: Voortgang<'_>,
    ) -> Result<Upload, Fout> {
        let (mappad, naam) = splits(pad);
        let map = self.zorg_voor_map(mappad).await?;
        let r = upload_met_voortgang(&self.client, &self.ruimte, map.as_deref(), naam, lokaal, basis_versie, voortgang).await?;
        Ok(Upload { pad: samen(mappad, &r.naam), bestand: r.bestand, versie: r.versie, conflict: r.conflict })
    }

    async fn download_met_voortgang(&self, bestand: &OpslagBestand, lokaal: &Path, voortgang: Voortgang<'_>) -> Result<(), Fout> {
        if let Some(map) = lokaal.parent() {
            tokio::fs::create_dir_all(map).await?;
        }
        download_met_voortgang(&self.client, &bestand.bestand, lokaal, voortgang).await
    }

    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.client.verwijder(&bestand.bestand).await
    }
}
