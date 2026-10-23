//! Client voor de API van OpenAEC Storage (`openaec_storage.api.*` op de klantsite).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use sha2::{Digest, Sha256};

use crate::Fout;

const API: &str = "/api/method/openaec_storage.api.";

/// Verbinding met één klantomgeving. `autorisatie` is de volledige waarde van de Authorization-header,
/// bijv. `Bearer <OpenAEC-JWT>` of `token <sleutel>:<geheim>`.
#[derive(Clone)]
pub struct Client {
    basis: String,
    autorisatie: String,
    pub(crate) http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
pub struct Deel {
    pub nummer: u64,
    pub url: String,
}

/// Opslaggebruik volgens de server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Quotum {
    pub gebruikt_bytes: u64,
    #[serde(default)]
    pub limiet_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct UploadStart {
    pub upload: String,
    pub deelgrootte: u64,
    pub hergebruik: Vec<u64>,
    pub delen: Vec<Deel>,
}

#[derive(Debug, Serialize)]
pub struct Etag {
    pub nummer: u64,
    pub etag: String,
}

#[derive(Debug, Deserialize)]
pub struct UploadResultaat {
    pub bestand: String,
    pub naam: String,
    pub versie: u64,
    pub conflict: bool,
}

#[derive(Debug, Deserialize)]
pub struct Blokken {
    pub versie: u64,
    pub grootte: u64,
    pub blokgrootte: u64,
    pub hashes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct DownloadLink {
    pub url: String,
    pub versie: u64,
}

#[derive(Debug, Deserialize)]
pub struct Wijziging {
    pub volgnummer: u64,
    pub soort: String,
    pub doel_doctype: String,
    pub doel: String,
    pub naam: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Wijzigingen {
    pub cursor: u64,
    pub meer: bool,
    pub wijzigingen: Vec<Wijziging>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ruimte {
    pub name: String,
    pub naam: String,
    pub soort: Option<String>,
    pub rol: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MapItem {
    pub name: String,
    pub naam: String,
    /// Frappe-tijdstempel (`JJJJ-MM-DD uu:mm:ss.ffffff`, tijdzone van de site).
    #[serde(default)]
    pub modified: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BestandItem {
    pub name: String,
    pub naam: String,
    #[serde(default)]
    pub grootte: u64,
    #[serde(default)]
    pub huidige_versie: u64,
    #[serde(default)]
    pub modified: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Inhoud {
    pub mappen: Vec<MapItem>,
    pub bestanden: Vec<BestandItem>,
}

#[derive(Debug, Deserialize)]
pub struct NieuweMap {
    pub map: String,
    pub nieuw: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Versie {
    pub volgnummer: u64,
    #[serde(default)]
    pub grootte: u64,
    #[serde(default)]
    pub maker: Option<String>,
    #[serde(default)]
    pub creation: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Deling {
    pub deling: String,
    pub token: String,
    pub rechten: String,
    /// Volledig publiek adres van de deelpagina (nieuwere servers); anders zelf bouwen met `basis`.
    #[serde(default)]
    pub url: Option<String>,
}

/// Actieve vergrendeling van een bestand (wie eraan werkt, tot wanneer).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vergrendeling {
    pub gebruiker: String,
    #[serde(default)]
    pub apparaat: Option<String>,
    #[serde(default)]
    pub verloopt: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BestandInfo {
    pub bestand: String,
    pub naam: String,
    #[serde(default)]
    pub huidige_versie: u64,
    #[serde(default)]
    pub vergrendeling: Option<Vergrendeling>,
}

/// Grenzen voor kleine bestanden in één verzoek (`api.upload_klein` en `api.download_klein`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batchgrenzen {
    /// Grootste bestand in bytes.
    pub max_bestand: u64,
    /// Hoogste aantal bestanden per verzoek.
    pub max_aantal: usize,
    /// Hoogste aantal bytes per verzoek.
    pub max_bytes: u64,
}

/// Wat de server kent (`api.mogelijkheden`); een oudere server kent die methode niet.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Mogelijkheden {
    #[serde(default)]
    pub upload_klein: Option<Batchgrenzen>,
    #[serde(default)]
    pub download_klein: Option<Batchgrenzen>,
    #[serde(default)]
    pub boom: bool,
    #[serde(default)]
    pub maak_mappen: bool,
}

/// Antwoord van [`Client::maak_mappen`]: per mappad het map-id, of de reden waarom het niet lukte.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NieuweMappen {
    #[serde(default)]
    pub mappen: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub fouten: std::collections::HashMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BoomMap {
    pub name: String,
    pub naam: String,
    #[serde(default)]
    pub bovenliggende: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BoomBestand {
    pub name: String,
    pub naam: String,
    #[serde(default)]
    pub map: Option<String>,
    #[serde(default)]
    pub grootte: u64,
    #[serde(default)]
    pub huidige_versie: u64,
}

/// Alle mappen en (een pagina) bestanden van een ruimte (`api.boom`).
#[derive(Debug, Clone, Deserialize)]
pub struct Boom {
    pub mappen: Vec<BoomMap>,
    pub bestanden: Vec<BoomBestand>,
    #[serde(default)]
    pub meer: bool,
}

/// Eén klein bestand voor [`Client::upload_klein`].
#[derive(Debug, Clone)]
pub struct KleinBestand {
    pub naam: String,
    /// Map-id (None = hoofdniveau, of de map uit `pad`).
    pub map: Option<String>,
    /// Mappad met '/' als het map-id nog niet bekend is: de server zoekt de map op of maakt hem aan.
    pub pad: Option<String>,
    pub basis_versie: Option<u64>,
    pub inhoud: Vec<u8>,
}

/// Inhoud van een klein bestand uit [`Client::download_klein`].
#[derive(Debug, Clone)]
pub struct KleinInhoud {
    pub bestand: String,
    pub versie: u64,
    /// BLAKE3 van de inhoud, als de server die kent.
    pub blokhash: Option<String>,
    pub inhoud: Vec<u8>,
}

/// Antwoord per bestand van een batch.
#[derive(Debug, Deserialize)]
struct PerBestand {
    ok: bool,
    #[serde(default)]
    bestand: Option<String>,
    #[serde(default)]
    naam: Option<String>,
    #[serde(default)]
    versie: Option<u64>,
    #[serde(default)]
    conflict: Option<bool>,
    #[serde(default)]
    map: Option<String>,
    #[serde(default)]
    grootte: Option<u64>,
    #[serde(default)]
    blokhash: Option<String>,
    #[serde(default)]
    inhoud: Option<String>,
    #[serde(default)]
    fout: Option<String>,
    #[serde(default)]
    foutcode: Option<String>,
}

impl PerBestand {
    fn weigering(&self) -> Fout {
        Fout::Geweigerd { foutcode: self.foutcode.clone(), melding: self.fout.clone().unwrap_or_else(|| "onbekende fout".into()) }
    }
}

#[derive(Deserialize)]
struct Antwoord<T> {
    message: T,
}

/// HTTP-client voor de opslag: verbindingen blijven open (keep-alive, genoeg per host voor parallelle overdrachten)
/// en HTTP/2 wordt gebruikt als de server het aanbiedt. Eén client delen tussen [`Client`]s scheelt een nieuwe
/// TLS-verbinding per ronde.
pub fn nieuwe_http() -> reqwest::Client {
    reqwest::Client::builder()
        .pool_max_idle_per_host(crate::MAX_GELIJKTIJDIG * 2)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .tcp_nodelay(true)
        .build()
        .unwrap_or_default()
}

impl Client {
    pub fn new(basis_url: &str, autorisatie: &str) -> Self {
        Self::met_http(basis_url, autorisatie, nieuwe_http())
    }

    /// Als [`Client::new`], met een bestaande (gedeelde) HTTP-client; zie [`nieuwe_http`].
    pub fn met_http(basis_url: &str, autorisatie: &str, http: reqwest::Client) -> Self {
        Self { basis: basis_url.trim_end_matches('/').to_string(), autorisatie: autorisatie.to_string(), http }
    }

    async fn roep<T: DeserializeOwned>(&self, methode: &str, gegevens: Value) -> Result<T, Fout> {
        Ok(self.verstuur(methode, gegevens).await?.json::<Antwoord<T>>().await?.message)
    }

    /// Voor methoden zonder antwoord: Frappe laat `message` dan weg.
    async fn roep_leeg(&self, methode: &str, gegevens: Value) -> Result<(), Fout> {
        self.verstuur(methode, gegevens).await.map(|_| ())
    }

    async fn verstuur(&self, methode: &str, gegevens: Value) -> Result<reqwest::Response, Fout> {
        let antwoord = self
            .http
            .post(format!("{}{}{}", self.basis, API, methode))
            .header("Authorization", &self.autorisatie)
            .json(&gegevens)
            .send()
            .await?;
        let status = antwoord.status();
        if !status.is_success() {
            let tekst = antwoord.text().await.unwrap_or_default();
            return Err(Fout::Server { status: status.as_u16(), tekst });
        }
        Ok(antwoord)
    }

    /// Wat de server kent; een fout (bijv. een oudere server zonder deze methode) betekent: niets nieuws.
    pub async fn mogelijkheden(&self) -> Result<Mogelijkheden, Fout> {
        self.roep("mogelijkheden", json!({})).await
    }

    /// Veel mappen (paden met '/') in één verzoek; ontbrekende, ook tussenliggende, worden aangemaakt.
    pub async fn maak_mappen(&self, ruimte: &str, paden: &[String]) -> Result<NieuweMappen, Fout> {
        self.roep("maak_mappen", json!({"ruimte": ruimte, "paden": paden})).await
    }

    /// Alle mappen en een pagina bestanden van een ruimte in één verzoek.
    pub async fn boom(&self, ruimte: &str, start: usize, aantal: usize) -> Result<Boom, Fout> {
        self.roep("boom", json!({"ruimte": ruimte, "start": start, "aantal": aantal})).await
    }

    /// Kleine bestanden in één verzoek (multipart); per bestand het resultaat (met het map-id) of de weigering, in
    /// dezelfde volgorde.
    #[allow(clippy::type_complexity)]
    pub async fn upload_klein(
        &self,
        ruimte: &str,
        bestanden: &[KleinBestand],
    ) -> Result<Vec<Result<(UploadResultaat, Option<String>), Fout>>, Fout> {
        let lijst: Vec<Value> = bestanden
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let sha256: String = Sha256::digest(&b.inhoud).iter().map(|x| format!("{x:02x}")).collect();
                json!({"veld": format!("f{i}"), "naam": b.naam, "map": b.map, "pad": b.pad, "grootte": b.inhoud.len(),
                       "basis_versie": b.basis_versie, "sha256": sha256,
                       "blokhash": crate::blokken::hash_bytes(&b.inhoud, crate::blokken::BLOK)[0]})
            })
            .collect();
        // Alle inhoud achter elkaar in één deel: de server snijdt op `grootte` en controleert met `sha256`. Losse
        // delen per bestand kunnen bij het ontleden een regeleinde aan het eind houden.
        let alles: Vec<u8> = bestanden.iter().flat_map(|b| b.inhoud.iter().copied()).collect();
        let formulier = reqwest::multipart::Form::new()
            .text("ruimte", ruimte.to_string())
            .text("bron", "sync")
            .text("bestanden", Value::Array(lijst).to_string())
            .part("inhoud", reqwest::multipart::Part::bytes(alles).file_name("inhoud"));
        let antwoord = self
            .http
            .post(format!("{}{}upload_klein", self.basis, API))
            .header("Authorization", &self.autorisatie)
            .multipart(formulier)
            .send()
            .await?;
        let status = antwoord.status();
        if !status.is_success() {
            let tekst = antwoord.text().await.unwrap_or_default();
            return Err(Fout::Server { status: status.as_u16(), tekst });
        }
        #[derive(Deserialize)]
        struct Resultaten {
            resultaten: Vec<PerBestand>,
        }
        let r = antwoord.json::<Antwoord<Resultaten>>().await?.message.resultaten;
        if r.len() != bestanden.len() {
            return Err(Fout::Controle(format!("{} resultaten voor {} bestanden", r.len(), bestanden.len())));
        }
        Ok(r.into_iter()
            .map(|b| match (b.ok, &b.bestand, &b.naam, b.versie) {
                (true, Some(bestand), Some(naam), Some(versie)) => Ok((
                    UploadResultaat { bestand: bestand.clone(), naam: naam.clone(), versie, conflict: b.conflict.unwrap_or(false) },
                    b.map.clone(),
                )),
                _ => Err(b.weigering()),
            })
            .collect())
    }

    /// Kleine bestanden (bestand-id's) in één antwoord; per bestand de inhoud of de weigering, in dezelfde volgorde.
    pub async fn download_klein(&self, bestanden: &[String]) -> Result<Vec<Result<KleinInhoud, Fout>>, Fout> {
        use base64::Engine;
        #[derive(Deserialize)]
        struct Inhoud {
            bestanden: Vec<PerBestand>,
        }
        let r: Inhoud = self.roep("download_klein", json!({"bestanden": bestanden})).await?;
        if r.bestanden.len() != bestanden.len() {
            return Err(Fout::Controle(format!("{} antwoorden voor {} bestanden", r.bestanden.len(), bestanden.len())));
        }
        Ok(r.bestanden
            .into_iter()
            .map(|b| {
                let inhoud = match (b.ok, &b.inhoud) {
                    (true, Some(tekst)) => base64::engine::general_purpose::STANDARD
                        .decode(tekst)
                        .map_err(|e| Fout::Controle(format!("inhoud onleesbaar: {e}")))?,
                    _ => return Err(b.weigering()),
                };
                if b.grootte.is_some_and(|g| g != inhoud.len() as u64) {
                    return Err(Fout::Controle("ontvangen grootte wijkt af".into()));
                }
                Ok(KleinInhoud {
                    bestand: b.bestand.unwrap_or_default(),
                    versie: b.versie.unwrap_or_default(),
                    blokhash: b.blokhash,
                    inhoud,
                })
            })
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upload_start(
        &self,
        ruimte: &str,
        map: Option<&str>,
        naam: &str,
        grootte: u64,
        basis_versie: Option<u64>,
        blokhashes: &[String],
        bron: &str,
    ) -> Result<UploadStart, Fout> {
        self.roep(
            "upload_start",
            json!({"ruimte": ruimte, "map": map, "naam": naam, "grootte": grootte,
                   "basis_versie": basis_versie, "blokhashes": blokhashes, "bron": bron}),
        )
        .await
    }

    pub async fn upload_klaar(&self, upload: &str, delen: &[Etag]) -> Result<UploadResultaat, Fout> {
        self.roep("upload_klaar", json!({"upload": upload, "delen": delen})).await
    }

    pub async fn blokken(&self, bestand: &str) -> Result<Blokken, Fout> {
        self.roep("blokken", json!({"bestand": bestand})).await
    }

    pub async fn download(&self, bestand: &str) -> Result<DownloadLink, Fout> {
        self.roep("download", json!({"bestand": bestand})).await
    }

    pub async fn wijzigingen(&self, ruimte: &str, sinds: u64) -> Result<Wijzigingen, Fout> {
        self.roep("wijzigingen", json!({"ruimte": ruimte, "sinds": sinds})).await
    }

    pub async fn vergrendel(&self, bestand: &str, apparaat: &str, automatisch: bool) -> Result<Value, Fout> {
        let reden = if automatisch { "automatisch" } else { "handmatig" };
        self.roep("vergrendel", json!({"bestand": bestand, "apparaat": apparaat, "reden": reden})).await
    }

    pub async fn ruimtes(&self) -> Result<Vec<Ruimte>, Fout> {
        self.roep("ruimtes", json!({})).await
    }

    pub async fn bladeren(&self, ruimte: &str, map: Option<&str>) -> Result<Inhoud, Fout> {
        self.roep("bladeren", json!({"ruimte": ruimte, "map": map})).await
    }

    /// Gegevens van één bestand, met de actieve vergrendeling (of None).
    pub async fn info(&self, bestand: &str) -> Result<BestandInfo, Fout> {
        self.roep("info", json!({"bestand": bestand})).await
    }

    pub async fn maak_map(&self, ruimte: &str, naam: &str, bovenliggende: Option<&str>) -> Result<NieuweMap, Fout> {
        self.roep("maak_map", json!({"ruimte": ruimte, "naam": naam, "bovenliggende": bovenliggende})).await
    }

    /// Naar de prullenbak; de server bewaart het bestand nog 30 dagen.
    pub async fn verwijder(&self, bestand: &str) -> Result<(), Fout> {
        self.roep_leeg("verwijder", json!({"bestand": bestand})).await
    }

    /// Gebruikte opslag van de omgeving en de limiet (None = geen limiet).
    pub async fn quotum(&self) -> Result<Quotum, Fout> {
        self.roep("quotum", json!({})).await
    }

    /// Leeslink zonder account voor een bestand; de gastpagina staat op `<basis>/delen/<token>`.
    pub async fn deel_bestand(&self, bestand: &str, wachtwoord: Option<&str>) -> Result<Deling, Fout> {
        self.deel("Opslagbestand", bestand, wachtwoord, None).await
    }

    /// Leeslink zonder account voor een bestand (`Opslagbestand`) of map (`Opslagmap`); `verloopt` als
    /// `JJJJ-MM-DD` (einde van die dag) of None.
    pub async fn deel(&self, doctype: &str, doel: &str, wachtwoord: Option<&str>, verloopt: Option<&str>) -> Result<Deling, Fout> {
        self.deel_link(doctype, doel, "lezen", wachtwoord, verloopt).await
    }

    /// Uploadlink voor een map (bestanden opvragen): wie hem opent, kan bestanden aanleveren zonder account, maar
    /// ziet de inhoud van de map niet.
    pub async fn vraag_bestanden(&self, map: &str, wachtwoord: Option<&str>, verloopt: Option<&str>) -> Result<Deling, Fout> {
        self.deel_link("Opslagmap", map, "uploaden", wachtwoord, verloopt).await
    }

    async fn deel_link(
        &self,
        doctype: &str,
        doel: &str,
        rechten: &str,
        wachtwoord: Option<&str>,
        verloopt: Option<&str>,
    ) -> Result<Deling, Fout> {
        let verloopt = verloopt.map(|d| format!("{d} 23:59:59"));
        self.roep(
            "deel",
            json!({"doel_doctype": doctype, "doel": doel, "soort": "link", "rechten": rechten,
                "wachtwoord": wachtwoord, "verloopt": verloopt}),
        )
        .await
    }

    /// Alle versies van een bestand, nieuwste eerst.
    pub async fn versies(&self, bestand: &str) -> Result<Vec<Versie>, Fout> {
        self.roep("versies", json!({"bestand": bestand})).await
    }

    /// Downloadlink voor een bepaalde versie.
    pub async fn download_versie(&self, bestand: &str, versie: u64) -> Result<DownloadLink, Fout> {
        self.roep("download", json!({"bestand": bestand, "versie": versie})).await
    }

    pub fn basis(&self) -> &str {
        &self.basis
    }

    /// Hernoem of verplaats een bestand (`map` None = hoofdniveau). Met `overschrijf` wordt de inhoud een nieuwe
    /// versie van een bestaand doel.
    pub async fn verplaats(&self, bestand: &str, naam: &str, map: Option<&str>, overschrijf: bool) -> Result<UploadResultaat, Fout> {
        let r: Value = self
            .roep("verplaats", json!({"bestand": bestand, "naam": naam, "map": map, "overschrijf": overschrijf as u8}))
            .await?;
        Ok(UploadResultaat {
            bestand: r["bestand"].as_str().unwrap_or_default().to_string(),
            naam: r["naam"].as_str().unwrap_or_default().to_string(),
            versie: r["versie"].as_u64().unwrap_or_default(),
            conflict: false,
        })
    }

    /// Haal een bestand uit de prullenbak (krijgt "(hersteld)" in de naam als de plek bezet is).
    pub async fn herstel(&self, bestand: &str) -> Result<(), Fout> {
        self.roep::<Value>("herstel", json!({"bestand": bestand})).await.map(|_| ())
    }

    pub async fn verplaats_map(&self, map: &str, naam: &str, bovenliggende: Option<&str>) -> Result<(), Fout> {
        self.roep_leeg("verplaats_map", json!({"map": map, "naam": naam, "bovenliggende": bovenliggende})).await
    }

    pub async fn verwijder_map(&self, map: &str) -> Result<(), Fout> {
        self.roep_leeg("verwijder_map", json!({"map": map})).await
    }

    pub async fn ontgrendel(&self, bestand: &str) -> Result<(), Fout> {
        self.roep_leeg("ontgrendel", json!({"bestand": bestand})).await
    }
}
