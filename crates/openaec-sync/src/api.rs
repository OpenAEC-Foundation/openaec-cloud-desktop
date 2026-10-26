//! Client voor de API van OpenAEC Storage (`openaec_storage.api.*` op de klantsite).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use sha2::{Digest, Sha256};

use std::sync::Arc;

use crate::sessie::{Tokenbron, VasteSleutel};
use crate::Fout;

const API: &str = "/api/method/openaec_storage.api.";

/// Verbinding met één klantomgeving. De Authorization-header komt bij elk verzoek uit de [`Tokenbron`]: een vaste
/// waarde (`token <sleutel>:<geheim>`, zie [`Client::new`]) of een [`Sessie`](crate::sessie::Sessie) die het OAuth-token
/// op tijd vernieuwt. Weigert de API een verzoek met 401, dan vraagt de client de bron één keer om een nieuwe waarde en
/// probeert hij het nog één keer; lukt dat niet, dan is het [`Fout::SessieVerlopen`]. Verzoeken naar ondertekende links
/// van de opslag (`http` direct) gaan zonder Authorization-header.
#[derive(Clone)]
pub struct Client {
    basis: String,
    autorisatie: Arc<dyn Tokenbron>,
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

#[derive(Debug, Serialize, Deserialize)]
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
    /// Alleen bij een link; een deling met een gebruiker heeft geen token.
    #[serde(default)]
    pub token: Option<String>,
    pub rechten: String,
    /// Volledig publiek adres van de deelpagina (nieuwere servers); anders zelf bouwen met `basis`.
    #[serde(default)]
    pub url: Option<String>,
    /// Bij delen met een adres dat nog geen gebruiker was: hoe de uitnodiging als gast (met een OpenAEC-account)
    /// ging: `al_lid`, `lid` of `uitgenodigd` (nieuwere servers met Beheer); anders leeg.
    #[serde(default)]
    pub uitnodiging: Option<String>,
}

/// Een bestaande deling van een map of bestand (`delingen`): een link of een gebruiker, met de rechten.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelingInfo {
    /// Id van de deling (om in te trekken).
    pub name: String,
    /// `link` of `gebruiker`.
    pub soort: String,
    #[serde(default)]
    pub gebruiker: Option<String>,
    /// `lezen`, `schrijven` (alleen gebruiker) of `uploaden` (alleen link naar een map).
    pub rechten: String,
    #[serde(default)]
    pub verloopt: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub creation: Option<String>,
    /// Zit er een wachtwoord op? (nieuwere servers; anders None)
    #[serde(default)]
    pub beveiligd: Option<bool>,
    /// Is de vervaldatum voorbij? (nieuwere servers; anders None)
    #[serde(default)]
    pub verlopen: Option<bool>,
    /// Wie de deling maakte (nieuwere servers).
    #[serde(default)]
    pub gemaakt_door: Option<String>,
    #[serde(default)]
    pub gemaakt_door_naam: Option<String>,
}

/// Wat de ingelogde gebruiker met een map of bestand mag (`toegang`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toegang {
    /// Hoogste recht hier: `lezen`, `schrijven` of `beheren` (uit de rol in de ruimte of een deling).
    #[serde(default)]
    pub rechten: Option<String>,
    /// Mag hier delen, en de delingen hier wijzigen en intrekken.
    #[serde(default)]
    pub mag_delen: bool,
    /// Het hoogste recht dat aan een ander gegeven mag worden (None: niet delen).
    #[serde(default)]
    pub max_rechten: Option<String>,
}

/// Een collega om mee te delen (`collegas`): een gebruiker van de omgeving, met zijn rol in de ruimte of None.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collega {
    pub gebruiker: String,
    pub naam: String,
    #[serde(default)]
    pub lid_als: Option<String>,
}

/// Lid van een ruimte met zijn rol (beheerder, schrijver, lezer of gast).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ruimtelid {
    pub gebruiker: String,
    pub rol: String,
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
    /// De map waar het bestand in staat (None: het hoofdniveau van de ruimte).
    #[serde(default)]
    pub map: Option<String>,
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
    /// De parameter `deling` (een gedeelde map als bovenste map) bij `boom`, `maak_mappen`, `upload_klein` enz.
    #[serde(default)]
    pub deling: bool,
    /// `zoek_pad`: een pad in één verzoek naar het id van een map of bestand.
    #[serde(default)]
    pub zoek_pad: bool,
    /// `overdracht.voeg_toe_van_opslag` en `maak_van_opslag`: versturen wat al in de opslag staat, zonder upload.
    #[serde(default)]
    pub overdracht_van_opslag: bool,
}

/// Iets dat een ander met mij deelde (`gedeeld_met_mij`): een map (te koppelen) of een bestand (te openen).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gedeeld {
    /// Id van de deling: de wortel voor de sync bij een map.
    pub deling: String,
    /// `Opslagmap` of `Opslagbestand`.
    pub doel_doctype: String,
    /// Id van de gedeelde map of het gedeelde bestand.
    pub doel: String,
    pub naam: String,
    pub ruimte: String,
    #[serde(default)]
    pub ruimte_naam: Option<String>,
    /// `lezen` of `schrijven`.
    pub rechten: String,
    #[serde(default)]
    pub gedeeld_door: Option<String>,
    #[serde(default)]
    pub gedeeld_door_naam: Option<String>,
    #[serde(default)]
    pub sinds: Option<String>,
    #[serde(default)]
    pub verloopt: Option<String>,
    /// Ook lid van de ruimte (met `rol`).
    #[serde(default)]
    pub al_lid: bool,
    #[serde(default)]
    pub rol: Option<String>,
    /// Alleen bij een bestand: grootte en huidige versie.
    #[serde(default)]
    pub grootte: Option<u64>,
    #[serde(default)]
    pub versie: Option<u64>,
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

/// Voeg `deling` toe aan de gegevens van een verzoek (alleen als er een is: een oudere server kent hem niet).
fn met_deling(mut gegevens: Value, deling: Option<&str>) -> Value {
    if let Some(deling) = deling {
        gegevens["deling"] = json!(deling);
    }
    gegevens
}

/// Begin van het formulier voor `upload_klein`: de ruimte en zo nodig de deling.
fn formulier_basis(ruimte: &str, deling: Option<&str>) -> reqwest::multipart::Form {
    let formulier = reqwest::multipart::Form::new().text("ruimte", ruimte.to_string());
    match deling {
        Some(deling) => formulier.text("deling", deling.to_string()),
        None => formulier,
    }
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
        Self::met_bron(basis_url, Arc::new(VasteSleutel(autorisatie.to_string())), http)
    }

    /// Client die de autorisatie bij elk verzoek uit `bron` haalt (bijv. een gedeelde
    /// [`Sessie`](crate::sessie::Sessie)).
    pub fn met_bron(basis_url: &str, bron: Arc<dyn Tokenbron>, http: reqwest::Client) -> Self {
        Self { basis: basis_url.trim_end_matches('/').to_string(), autorisatie: bron, http }
    }

    /// Verstuur een verzoek naar de API met de autorisatie van nu. Bij een 401: één keer een nieuwe waarde van de bron
    /// en één keer opnieuw; blijft het 401 of is er niets te vernieuwen, dan [`Fout::SessieVerlopen`]. `maak` bouwt het
    /// verzoek (zonder autorisatie) en kan dus twee keer aangeroepen worden.
    async fn met_autorisatie(&self, maak: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, Fout> {
        let waarde = self.autorisatie.geef().await?;
        let mut antwoord = maak().header(reqwest::header::AUTHORIZATION, &waarde).send().await?;
        if antwoord.status() == reqwest::StatusCode::UNAUTHORIZED {
            let nieuw = self.autorisatie.na_weigering(&waarde).await?.ok_or(Fout::SessieVerlopen)?;
            antwoord = maak().header(reqwest::header::AUTHORIZATION, &nieuw).send().await?;
            if antwoord.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(Fout::SessieVerlopen);
            }
        }
        let status = antwoord.status();
        if !status.is_success() {
            let tekst = antwoord.text().await.unwrap_or_default();
            return Err(Fout::Server { status: status.as_u16(), tekst });
        }
        Ok(antwoord)
    }

    async fn roep<T: DeserializeOwned>(&self, methode: &str, gegevens: Value) -> Result<T, Fout> {
        Ok(self.verstuur(methode, gegevens).await?.json::<Antwoord<T>>().await?.message)
    }

    /// Voor methoden zonder antwoord: Frappe laat `message` dan weg.
    async fn roep_leeg(&self, methode: &str, gegevens: Value) -> Result<(), Fout> {
        self.verstuur(methode, gegevens).await.map(|_| ())
    }

    async fn verstuur(&self, methode: &str, gegevens: Value) -> Result<reqwest::Response, Fout> {
        self.verstuur_naar(API, methode, gegevens).await
    }

    /// Als `roep`, voor een methode in een andere module van de server (`voorvoegsel` zoals [`API`]).
    pub(crate) async fn roep_op<T: DeserializeOwned>(&self, voorvoegsel: &str, methode: &str, gegevens: Value) -> Result<T, Fout> {
        Ok(self.verstuur_naar(voorvoegsel, methode, gegevens).await?.json::<Antwoord<T>>().await?.message)
    }

    /// Als `roep_leeg`, voor een methode in een andere module van de server.
    pub(crate) async fn roep_leeg_op(&self, voorvoegsel: &str, methode: &str, gegevens: Value) -> Result<(), Fout> {
        self.verstuur_naar(voorvoegsel, methode, gegevens).await.map(|_| ())
    }

    async fn verstuur_naar(&self, voorvoegsel: &str, methode: &str, gegevens: Value) -> Result<reqwest::Response, Fout> {
        let url = format!("{}{}{}", self.basis, voorvoegsel, methode);
        self.met_autorisatie(|| self.http.post(&url).json(&gegevens)).await
    }

    /// Wat de server kent; een fout (bijv. een oudere server zonder deze methode) betekent: niets nieuws.
    pub async fn mogelijkheden(&self) -> Result<Mogelijkheden, Fout> {
        self.roep("mogelijkheden", json!({})).await
    }

    /// Veel mappen (paden met '/') in één verzoek; ontbrekende, ook tussenliggende, worden aangemaakt.
    pub async fn maak_mappen(&self, ruimte: &str, paden: &[String]) -> Result<NieuweMappen, Fout> {
        self.maak_mappen_in(ruimte, None, paden).await
    }

    /// Als [`Client::maak_mappen`]; met `deling` zijn de paden binnen de gedeelde map.
    pub async fn maak_mappen_in(&self, ruimte: &str, deling: Option<&str>, paden: &[String]) -> Result<NieuweMappen, Fout> {
        self.roep("maak_mappen", met_deling(json!({"ruimte": ruimte, "paden": paden}), deling)).await
    }

    /// Alle mappen en een pagina bestanden van een ruimte in één verzoek.
    pub async fn boom(&self, ruimte: &str, start: usize, aantal: usize) -> Result<Boom, Fout> {
        self.boom_in(ruimte, None, start, aantal).await
    }

    /// Als [`Client::boom`]; met `deling` alles onder de gedeelde map (die zelf niet in `mappen` staat).
    pub async fn boom_in(&self, ruimte: &str, deling: Option<&str>, start: usize, aantal: usize) -> Result<Boom, Fout> {
        self.roep("boom", met_deling(json!({"ruimte": ruimte, "start": start, "aantal": aantal}), deling)).await
    }

    /// Wat anderen met mij deelden (mappen en bestanden), nieuwste eerst; alleen delingen die nu gelden.
    pub async fn gedeeld_met_mij(&self) -> Result<Vec<Gedeeld>, Fout> {
        self.roep("gedeeld_met_mij", json!({})).await
    }

    /// Kleine bestanden in één verzoek (multipart); per bestand het resultaat (met het map-id) of de weigering, in
    /// dezelfde volgorde.
    #[allow(clippy::type_complexity)]
    pub async fn upload_klein(
        &self,
        ruimte: &str,
        bestanden: &[KleinBestand],
    ) -> Result<Vec<Result<(UploadResultaat, Option<String>), Fout>>, Fout> {
        self.upload_klein_in(ruimte, None, bestanden).await
    }

    /// Als [`Client::upload_klein`]; met `deling` zijn `pad` en `map` binnen de gedeelde map.
    #[allow(clippy::type_complexity)]
    pub async fn upload_klein_in(
        &self,
        ruimte: &str,
        deling: Option<&str>,
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
        let lijst = Value::Array(lijst).to_string();
        let url = format!("{}{}upload_klein", self.basis, API);
        // Het formulier per poging opnieuw (bij een 401 gaat het verzoek nog één keer).
        let antwoord = self
            .met_autorisatie(|| {
                let formulier = formulier_basis(ruimte, deling)
                    .text("bron", "sync")
                    .text("bestanden", lijst.clone())
                    .part("inhoud", reqwest::multipart::Part::bytes(alles.clone()).file_name("inhoud"));
                self.http.post(&url).multipart(formulier)
            })
            .await?;
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
        self.wijzigingen_in(ruimte, None, sinds).await
    }

    /// Als [`Client::wijzigingen`]; met `deling` alleen die binnen de gedeelde map.
    pub async fn wijzigingen_in(&self, ruimte: &str, deling: Option<&str>, sinds: u64) -> Result<Wijzigingen, Fout> {
        self.roep("wijzigingen", met_deling(json!({"ruimte": ruimte, "sinds": sinds}), deling)).await
    }

    pub async fn vergrendel(&self, bestand: &str, apparaat: &str, automatisch: bool) -> Result<Value, Fout> {
        let reden = if automatisch { "automatisch" } else { "handmatig" };
        self.roep("vergrendel", json!({"bestand": bestand, "apparaat": apparaat, "reden": reden})).await
    }

    pub async fn ruimtes(&self) -> Result<Vec<Ruimte>, Fout> {
        self.roep("ruimtes", json!({})).await
    }

    pub async fn bladeren(&self, ruimte: &str, map: Option<&str>) -> Result<Inhoud, Fout> {
        self.bladeren_in(ruimte, None, map).await
    }

    /// Als [`Client::bladeren`]; met `deling` is de gedeelde map het hoofdniveau (`map` None).
    pub async fn bladeren_in(&self, ruimte: &str, deling: Option<&str>, map: Option<&str>) -> Result<Inhoud, Fout> {
        self.roep("bladeren", met_deling(json!({"ruimte": ruimte, "map": map}), deling)).await
    }

    /// Gegevens van één bestand, met de actieve vergrendeling (of None).
    pub async fn info(&self, bestand: &str) -> Result<BestandInfo, Fout> {
        self.roep("info", json!({"bestand": bestand})).await
    }

    pub async fn maak_map(&self, ruimte: &str, naam: &str, bovenliggende: Option<&str>) -> Result<NieuweMap, Fout> {
        self.roep("maak_map", json!({"ruimte": ruimte, "naam": naam, "bovenliggende": bovenliggende})).await
    }

    /// Als [`Client::maak_map`]; met `deling` binnen een map die met mij gedeeld is (`bovenliggende` None = die map).
    pub async fn maak_map_in(
        &self,
        ruimte: &str,
        deling: Option<&str>,
        naam: &str,
        bovenliggende: Option<&str>,
    ) -> Result<NieuweMap, Fout> {
        match deling {
            Some(d) => self.roep("maak_map", json!({"deling": d, "naam": naam, "bovenliggende": bovenliggende})).await,
            None => self.maak_map(ruimte, naam, bovenliggende).await,
        }
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

    /// Link zonder account met de gegeven rechten (`lezen`, of `uploaden` bij een map); `verloopt` als `JJJJ-MM-DD`.
    pub async fn deel_link(
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

    /// Delen met een bestaande gebruiker (e-mailadres) van de omgeving: `lezen` of `schrijven`, op deze map (en
    /// alles eronder) of dit bestand. De server weigert een onbekende gebruiker.
    pub async fn deel_met_gebruiker(
        &self,
        doctype: &str,
        doel: &str,
        gebruiker: &str,
        rechten: &str,
        verloopt: Option<&str>,
    ) -> Result<Deling, Fout> {
        let verloopt = verloopt.map(|d| format!("{d} 23:59:59"));
        self.roep(
            "deel",
            json!({"doel_doctype": doctype, "doel": doel, "soort": "gebruiker", "gebruiker": gebruiker,
                "rechten": rechten, "verloopt": verloopt}),
        )
        .await
    }

    /// De actieve (niet ingetrokken) delingen van een map of bestand; verlopen delingen staan er nog in.
    pub async fn delingen(&self, doctype: &str, doel: &str) -> Result<Vec<DelingInfo>, Fout> {
        self.roep("delingen", json!({"doel_doctype": doctype, "doel": doel})).await
    }

    /// Een deling intrekken: de link werkt niet meer, of de gebruiker verliest de extra rechten.
    pub async fn trek_in(&self, deling: &str) -> Result<(), Fout> {
        self.roep_leeg("trek_in", json!({"deling": deling})).await
    }

    /// De rechten van een deling met een collega wijzigen (`lezen`, `schrijven` of `beheren`); nooit meer dan je zelf
    /// hebt (de server weigert met `meer_dan_eigen_recht`).
    pub async fn wijzig_deling(&self, deling: &str, rechten: &str) -> Result<(), Fout> {
        self.roep::<Value>("wijzig_deling", json!({"deling": deling, "rechten": rechten})).await.map(|_| ())
    }

    /// Wat ik met een map of bestand mag: mijn recht, of ik mag delen en het hoogste recht dat ik mag geven.
    pub async fn toegang(&self, doctype: &str, doel: &str) -> Result<Toegang, Fout> {
        self.roep("toegang", json!({"doel_doctype": doctype, "doel": doel})).await
    }

    /// De leden van een ruimte, via het documentadres van Frappe (`/api/resource/Ruimte/<id>`): elk lid mag de
    /// ruimte lezen. Er is (nog) geen eigen methode voor in `openaec_storage.api`.
    /// Collega's om mee te delen: gebruikers van de omgeving gezocht op e-mailadres en naam (hoogstens 20), met hun
    /// rol in `ruimte`. Een oudere server kent deze methode niet; gebruik dan [`Client::ruimteleden`].
    pub async fn collegas(&self, tekst: &str, ruimte: Option<&str>) -> Result<Vec<Collega>, Fout> {
        self.roep("collegas", json!({"tekst": tekst, "ruimte": ruimte})).await
    }

    pub async fn ruimteleden(&self, ruimte: &str) -> Result<Vec<Ruimtelid>, Fout> {
        #[derive(Deserialize)]
        struct Document {
            #[serde(default)]
            leden: Vec<Ruimtelid>,
        }
        #[derive(Deserialize)]
        struct Data {
            data: Document,
        }
        let mut url = reqwest::Url::parse(&self.basis).map_err(|e| Fout::Controle(format!("ongeldig adres: {e}")))?;
        url.path_segments_mut()
            .map_err(|_| Fout::Controle("ongeldig adres".into()))?
            .pop_if_empty()
            .extend(["api", "resource", "Ruimte", ruimte]);
        let antwoord = self.met_autorisatie(|| self.http.get(url.clone())).await?;
        Ok(antwoord.json::<Data>().await?.data.leden)
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
