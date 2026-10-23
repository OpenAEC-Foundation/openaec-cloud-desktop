//! [`Opslag`] over de echte API: één ruimte, paden met '/' ten opzichte van de ruimte.
//! Mappen worden bij het bladeren onthouden (pad -> map-id) en bij een upload aangemaakt als ze nog niet bestaan.
//!
//! Kent de server `api.mogelijkheden` (nieuwere servers), dan komt de hele ruimte in één verzoek (`api.boom`) en gaan
//! kleine bestanden in groepen over de lijn (`api.upload_klein`, `api.download_klein`). Een oudere server kent dat
//! niet; dan blijft alles zoals het was: bladeren per map en elk bestand apart.
//!
//! Een map die een ander met mij deelde ([`ClientOpslag::gedeeld`]) is de bovenste map (de wortel): paden zijn dan
//! ten opzichte van die map, en `boom`, `maak_mappen` en `upload_klein` krijgen de deling mee. Bij een deling om te
//! bekijken ([`Opslag::alleen_lezen`]) stuurt de motor niets naar de server.

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
    /// Een gedeelde map als wortel: de deling en het id van de map; None = de hele ruimte.
    gedeeld: Option<Wortel>,
}

/// De wortel van een gedeelde map (zie [`ClientOpslag::gedeeld`]).
#[derive(Debug, Clone)]
struct Wortel {
    deling: String,
    map: String,
    alleen_lezen: bool,
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
            gedeeld: None,
        }
    }

    /// Een map die met mij gedeeld is, als bovenste map: `deling` (het id van de deling), `map` (het id van de
    /// gedeelde map) en of de deling alleen om te bekijken is (`rechten` = `lezen`).
    pub fn gedeeld(client: Client, ruimte: &str, deling: &str, map: &str, rechten: &str) -> Self {
        let mut opslag = Self::new(client, ruimte);
        opslag.gedeeld = Some(Wortel { deling: deling.to_string(), map: map.to_string(), alleen_lezen: rechten != "schrijven" });
        opslag
    }

    /// De deling als de wortel een gedeelde map is.
    pub fn deling(&self) -> Option<&str> {
        self.gedeeld.as_ref().map(|w| w.deling.as_str())
    }

    /// Het id van de bovenste map: de gedeelde map, of None (het hoofdniveau van de ruimte).
    fn wortel(&self) -> Option<String> {
        self.gedeeld.as_ref().map(|w| w.map.clone())
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
        let mut map: Option<String> = self.wortel();
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
            return Ok(self.wortel());
        }
        if let Some(id) = self.mappen.lock().unwrap().get(mappad) {
            return Ok(Some(id.clone()));
        }
        let _beurt = self.aanmaken.lock().await;
        let mut bovenliggende: Option<String> = self.wortel();
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
        let mut te_doen: Vec<(String, Option<String>)> = vec![(String::new(), self.wortel())];
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
            let pagina = self.client.boom_in(&self.ruimte, self.deling(), start, BOOMPAGINA).await?;
            for m in pagina.mappen {
                mapregels.insert(m.name.clone(), m);
            }
            start += pagina.bestanden.len();
            bestanden.extend(pagina.bestanden);
            if !pagina.meer {
                break;
            }
        }
        // Pad per map: de namen langs de bovenliggende mappen (een kapotte keten telt niet mee). Bij een gedeelde map
        // is de wortel het hoofdniveau: hij staat zelf niet in de boom, en zijn pad is leeg.
        let wortel = self.wortel();
        let mut paden: HashMap<String, Option<String>> = HashMap::new();
        if let Some(w) = &wortel {
            paden.insert(w.clone(), Some(String::new()));
        }
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
                    None if wortel.is_none() => String::new(),
                    None => return None,
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
        // Alleen een lid van de ruimte (of met de deling erbij) mag de boom vragen; anders (of op een oudere server) per
        // map. Is de deling ingetrokken of verlopen, dan helpt per map ook niet: die fout meteen.
        let m = self.mogelijkheden().await;
        let (uit, mappen) = match m.boom && (self.gedeeld.is_none() || m.deling) {
            true => match self.lijst_als_boom().await {
                Ok(r) => r,
                Err(fout) if fout.deling_gestopt().is_some() => return Err(fout),
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
        let mut mappen: Vec<&String> = {
            let bekend = self.mappen.lock().unwrap();
            mappen.iter().filter(|m| !m.is_empty() && !bekend.contains_key(m.as_str())).collect()
        };
        if mappen.is_empty() {
            return Ok(());
        }
        // Een nieuwere server maakt ze allemaal in één verzoek; wat daar niet lukt, probeert de upload zelf nog.
        let m = self.mogelijkheden().await;
        if m.maak_mappen && (self.gedeeld.is_none() || m.deling) {
            let _beurt = self.aanmaken.lock().await;
            for stuk in mappen.chunks(1000) {
                let paden: Vec<String> = stuk.iter().map(|m| m.to_string()).collect();
                let nieuw = self.client.maak_mappen_in(&self.ruimte, self.deling(), &paden).await?;
                let mut bekend = self.mappen.lock().unwrap();
                for (pad, id) in nieuw.mappen {
                    bekend.insert(pad, id);
                }
            }
            return Ok(());
        }
        // Ondiep voor diep: een map komt pas als zijn bovenliggende map er is (zorg_voor_map loopt het pad af).
        mappen.sort_by_key(|m| (m.matches('/').count(), m.as_str()));
        for map in mappen {
            self.zorg_voor_map(map).await?;
        }
        Ok(())
    }

    fn alleen_lezen(&self) -> bool {
        self.gedeeld.as_ref().is_some_and(|w| w.alleen_lezen)
    }

    async fn klein_grenzen(&self) -> Option<Batchgrenzen> {
        let m = self.mogelijkheden().await;
        if self.gedeeld.is_some() && !m.deling {
            return None;
        }
        let (up, down) = (m.upload_klein?, m.download_klein?);
        Some(Batchgrenzen {
            max_bestand: up.max_bestand.min(down.max_bestand),
            max_aantal: up.max_aantal.min(down.max_aantal),
            max_bytes: up.max_bytes.min(down.max_bytes),
        })
    }

    async fn upload_klein(&self, bestanden: &[KleinUpload<'_>]) -> Result<Vec<Result<Upload, Fout>>, Fout> {
        // Een map die nog niet bekend is, maakt de server zelf aan (`pad`): dat scheelt een verzoek per map.
        let mut items = Vec::with_capacity(bestanden.len());
        for b in bestanden {
            let (mappad, naam) = splits(b.pad);
            let map = self.mappen.lock().unwrap().get(mappad).cloned();
            items.push(KleinBestand {
                naam: naam.to_string(),
                pad: (map.is_none() && !mappad.is_empty()).then(|| mappad.to_string()),
                map,
                basis_versie: b.basis_versie,
                inhoud: tokio::fs::read(b.lokaal).await?,
            });
        }
        let _plek = self.rem.acquire().await.map_err(|_| Fout::Controle("de overdracht is gestopt".into()))?;
        let resultaten = self.client.upload_klein_in(&self.ruimte, self.deling(), &items).await?;
        Ok(resultaten
            .into_iter()
            .zip(bestanden)
            .map(|(r, b)| {
                r.map(|(r, map)| {
                    // Het map-id uit het antwoord onthouden voor de volgende uploads in die map.
                    let mappad = splits(b.pad).0;
                    if let (Some(map), false) = (map, mappad.is_empty()) {
                        self.mappen.lock().unwrap().entry(mappad.to_string()).or_insert(map);
                    }
                    Upload { pad: samen(mappad, &r.naam), bestand: r.bestand, versie: r.versie, conflict: r.conflict }
                })
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
