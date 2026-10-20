//! Client voor de API van OpenAEC Storage (`openaec_storage.api.*` op de klantsite).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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

#[derive(Debug, Clone, Deserialize)]
pub struct Ruimte {
    pub name: String,
    pub naam: String,
    pub soort: Option<String>,
    pub rol: String,
}

#[derive(Debug, Deserialize)]
pub struct MapItem {
    pub name: String,
    pub naam: String,
}

#[derive(Debug, Deserialize)]
pub struct BestandItem {
    pub name: String,
    pub naam: String,
    #[serde(default)]
    pub grootte: u64,
    #[serde(default)]
    pub huidige_versie: u64,
}

#[derive(Debug, Deserialize)]
pub struct Inhoud {
    pub mappen: Vec<MapItem>,
    pub bestanden: Vec<BestandItem>,
}

#[derive(Debug, Deserialize)]
pub struct NieuweMap {
    pub map: String,
    pub nieuw: bool,
}

#[derive(Deserialize)]
struct Antwoord<T> {
    message: T,
}

impl Client {
    pub fn new(basis_url: &str, autorisatie: &str) -> Self {
        Self {
            basis: basis_url.trim_end_matches('/').to_string(),
            autorisatie: autorisatie.to_string(),
            http: reqwest::Client::new(),
        }
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

    pub async fn maak_map(&self, ruimte: &str, naam: &str, bovenliggende: Option<&str>) -> Result<NieuweMap, Fout> {
        self.roep("maak_map", json!({"ruimte": ruimte, "naam": naam, "bovenliggende": bovenliggende})).await
    }

    /// Naar de prullenbak; de server bewaart het bestand nog 30 dagen.
    pub async fn verwijder(&self, bestand: &str) -> Result<(), Fout> {
        self.roep_leeg("verwijder", json!({"bestand": bestand})).await
    }

    pub async fn ontgrendel(&self, bestand: &str) -> Result<(), Fout> {
        self.roep_leeg("ontgrendel", json!({"bestand": bestand})).await
    }
}
