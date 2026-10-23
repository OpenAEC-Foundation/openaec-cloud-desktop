//! [`Opslag`] over de echte API: één ruimte, paden met '/' ten opzichte van de ruimte.
//! Mappen worden bij het bladeren onthouden (pad -> map-id) en bij een upload aangemaakt als ze nog niet bestaan.
//!
//! Kent de server `api.mogelijkheden` (nieuwere servers), dan komt de hele ruimte in één verzoek (`api.boom`) en gaan
//! kleine bestanden in groepen over de lijn (`api.upload_klein`, `api.download_klein`). Een oudere server kent dat
//! niet; dan blijft alles zoals het was: bladeren per map en elk bestand apart.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::api::{Batchgrenzen, KleinBestand, Mogelijkheden};
use crate::blokken::{hash_bytes, BLOK};
use crate::motor::{KleinUpload, Opslag, OpslagBestand, Upload, Voortgang};
use crate::{download_met_rem, rem, upload_met_rem, Client, Fout, Rem, STANDAARD_GELIJKTIJDIG};

/// Bestanden per pagina van `api.boom`.
const BOOMPAGINA: usize = 10_000;

pub struct ClientOpslag {
    client: Client,
    ruimte: String,
    mappen: Mutex<HashMap<String, String>>,
    /// Mappen één voor één aanmaken: parallelle uploads (importeren) die dezelfde ontbrekende map nodig hebben,
    /// maken hem anders allemaal tegelijk aan.
    aanmaken: tokio::sync::Mutex<()>,
    /// Gedeeld door alle overdrachten van deze opslag: zoveel verzoeken met bestandsinhoud tegelijk.
    rem: Arc<Rem>,
    /// Wat de server kent; één keer gevraagd per opslag (dus per ronde in de app).
    mogelijk: tokio::sync::OnceCell<Mogelijkheden>,
}

fn splits(pad: &str) -> (&str, &str) {
    pad.rsplit_once('/').unwrap_or(("", pad))
}

fn samen(map: &str, naam: &str) -> String {
    if map.is_empty() { naam.to_string() } else { format!("{map}/{naam}") }
}

impl ClientOpslag {
    pub fn new(client: Client, ruimte: &str) -> Self {
        Self {
            client,
            ruimte: ruimte.to_string(),
            mappen: Mutex::new(HashMap::new()),
            aanmaken: tokio::sync::Mutex::new(()),
            rem: Arc::new(rem(STANDAARD_GELIJKTIJDIG)),
            mogelijk: tokio::sync::OnceCell::new(),
        }
    }

    /// Zoveel delen en blokken tegelijk over de lijn, over alle bestanden samen (1..=32; standaard 8).
    pub fn met_gelijktijdig(mut self, n: usize) -> Self {
        self.rem = Arc::new(rem(n));
        self
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Wat de server kent; een fout betekent een oudere server (niets nieuws).
    pub async fn mogelijkheden(&self) -> &Mogelijkheden {
        self.mogelijk.get_or_init(|| async { self.client.mogelijkheden().await.unwrap_or_default() }).await
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

    /// De ruimte map voor map aflopen (`api.bladeren`): werkt op elke server, ook voor wie via een deling bij een
    /// map kan.
    async fn lijst_per_map(&self) -> Result<(Vec<OpslagBestand>, HashMap<String, String>), Fout> {
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
        Ok((uit, mappen))
    }

    /// De hele ruimte in één verzoek per pagina (`api.boom`), met de paden uit de mapboom.
    async fn lijst_als_boom(&self) -> Result<(Vec<OpslagBestand>, HashMap<String, String>), Fout> {
        let (mut mapregels, mut bestanden, mut start) = (HashMap::new(), Vec::new(), 0);
        loop {
            let pagina = self.client.boom(&self.ruimte, start, BOOMPAGINA).await?;
            for m in pagina.mappen {
                mapregels.insert(m.name.clone(), m);
            }
            start += pagina.bestanden.len();
            bestanden.extend(pagina.bestanden);
            if !pagina.meer {
                break;
            }
        }
        // Pad per map: de namen langs de bovenliggende mappen (een kapotte keten telt niet mee).
        let mut paden: HashMap<String, Option<String>> = HashMap::new();
        fn pad_van(id: &str, regels: &HashMap<String, crate::api::BoomMap>, paden: &mut HashMap<String, Option<String>>, diepte: usize) -> Option<String> {
            if let Some(p) = paden.get(id) {
                return p.clone();
            }
            let regel = regels.get(id)?;
            let pad = match &regel.bovenliggende {
                None => Some(regel.naam.clone()),
                Some(_) if diepte > 256 => None,
                Some(ouder) => pad_van(ouder, regels, paden, diepte + 1).map(|p| samen(&p, &regel.naam)),
            };
            paden.insert(id.to_string(), pad.clone());
            pad
        }
        let mut mappen = HashMap::new();
        for id in mapregels.keys() {
            if let Some(pad) = pad_van(id, &mapregels, &mut paden, 0) {
                mappen.insert(pad, id.clone());
            }
        }
        let uit = bestanden
            .into_iter()
            .filter_map(|b| {
                let map = match &b.map {
                    None => String::new(),
                    Some(id) => paden.get(id).cloned().flatten()?,
                };
                Some(OpslagBestand { pad: samen(&map, &b.naam), bestand: b.name, versie: b.huidige_versie, grootte: b.grootte })
            })
            .collect();
        Ok((uit, mappen))
    }
}

impl Opslag for ClientOpslag {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        // Alleen een lid van de ruimte mag de boom vragen; anders (of op een oudere server) per map.
        let (uit, mappen) = match self.mogelijkheden().await.boom {
            true => match self.lijst_als_boom().await {
                Ok(r) => r,
                Err(_) => self.lijst_per_map().await?,
            },
            false => self.lijst_per_map().await?,
        };
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
        let r = upload_met_rem(&self.client, &self.ruimte, map.as_deref(), naam, lokaal, basis_versie, voortgang, &self.rem).await?;
        Ok(Upload { pad: samen(mappad, &r.naam), bestand: r.bestand, versie: r.versie, conflict: r.conflict })
    }

    async fn download_met_voortgang(&self, bestand: &OpslagBestand, lokaal: &Path, voortgang: Voortgang<'_>) -> Result<(), Fout> {
        if let Some(map) = lokaal.parent() {
            tokio::fs::create_dir_all(map).await?;
        }
        download_met_rem(&self.client, &bestand.bestand, lokaal, voortgang, &self.rem).await
    }

    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.client.verwijder(&bestand.bestand).await
    }

    async fn bereid_mappen(&self, mappen: &[String]) -> Result<(), Fout> {
        // Ondiep voor diep: een map komt pas als zijn bovenliggende map er is (zorg_voor_map loopt het pad af).
        let mut mappen: Vec<&String> = mappen.iter().filter(|m| !m.is_empty()).collect();
        mappen.sort_by_key(|m| (m.matches('/').count(), m.as_str()));
        for map in mappen {
            self.zorg_voor_map(map).await?;
        }
        Ok(())
    }

    async fn klein_grenzen(&self) -> Option<Batchgrenzen> {
        let m = self.mogelijkheden().await;
        let (up, down) = (m.upload_klein?, m.download_klein?);
        Some(Batchgrenzen {
            max_bestand: up.max_bestand.min(down.max_bestand),
            max_aantal: up.max_aantal.min(down.max_aantal),
            max_bytes: up.max_bytes.min(down.max_bytes),
        })
    }

    async fn upload_klein(&self, bestanden: &[KleinUpload<'_>]) -> Result<Vec<Result<Upload, Fout>>, Fout> {
        let mut items = Vec::with_capacity(bestanden.len());
        for b in bestanden {
            let (mappad, naam) = splits(b.pad);
            items.push(KleinBestand {
                naam: naam.to_string(),
                map: self.zorg_voor_map(mappad).await?,
                basis_versie: b.basis_versie,
                inhoud: tokio::fs::read(b.lokaal).await?,
            });
        }
        let _plek = self.rem.acquire().await.map_err(|_| Fout::Controle("de overdracht is gestopt".into()))?;
        let resultaten = self.client.upload_klein(&self.ruimte, &items).await?;
        Ok(resultaten
            .into_iter()
            .zip(bestanden)
            .map(|(r, b)| {
                r.map(|r| Upload { pad: samen(splits(b.pad).0, &r.naam), bestand: r.bestand, versie: r.versie, conflict: r.conflict })
            })
            .collect())
    }

    async fn download_klein(&self, bestanden: &[(&OpslagBestand, &Path)]) -> Result<Vec<Result<(), Fout>>, Fout> {
        let ids: Vec<String> = bestanden.iter().map(|(b, _)| b.bestand.clone()).collect();
        let inhoud = {
            let _plek = self.rem.acquire().await.map_err(|_| Fout::Controle("de overdracht is gestopt".into()))?;
            self.client.download_klein(&ids).await?
        };
        let mut uit = Vec::with_capacity(bestanden.len());
        for (r, (_, lokaal)) in inhoud.into_iter().zip(bestanden) {
            uit.push(match r {
                Ok(k) => schrijf_klein(lokaal, &k.inhoud, k.blokhash.as_deref()).await,
                Err(fout) => Err(fout),
            });
        }
        Ok(uit)
    }
}

/// Een klein gedownload bestand controleren (BLAKE3 als de server hem kent) en atomair op zijn plek zetten.
async fn schrijf_klein(lokaal: &Path, inhoud: &[u8], blokhash: Option<&str>) -> Result<(), Fout> {
    if blokhash.is_some_and(|h| hash_bytes(inhoud, BLOK)[0] != h) {
        return Err(Fout::Controle("blokhash van de download klopt niet".into()));
    }
    if let Some(map) = lokaal.parent() {
        tokio::fs::create_dir_all(map).await?;
    }
    let tijdelijk = lokaal.with_extension("openaec-download");
    tokio::fs::write(&tijdelijk, inhoud).await?;
    if let Err(fout) = tokio::fs::rename(&tijdelijk, lokaal).await {
        tokio::fs::remove_file(&tijdelijk).await.ok();
        return Err(fout.into());
    }
    Ok(())
}
