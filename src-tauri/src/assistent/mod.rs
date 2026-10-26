//! De assistent: een gesprek met de AI-dienst van OpenAEC (de AI-gateway), met bijlagen, gestreamde antwoorden en
//! gesprekken die lokaal bewaard blijven.
//!
//! **Toegang.** De gateway accepteert alleen een toegangstoken van de cloud (RS256-JWT) met scope `ai`. Het token van de
//! sync (van de klantomgeving zelf, ondoorzichtig, client `openaec-cloud-desktop`) is dat niet. De assistent logt daarom
//! één keer in bij de cloud met de publieke client `tools` (PKCE, terug op `http://localhost:8765/terug`) en vraagt
//! alleen `openid email profile ai`. Die tokens staan in de sleutelbos naast die van de sync en worden net zo vernieuwd
//! (zie [`openaec_sync::sessie::Sessie`]). Er zit geen geheim in de app.

pub mod bijlagen;
pub mod gesprekken;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::{AbortHandle, Abortable};
use openaec_sync::ai::{AiClient, AiFout, Gebruik, Model, Stuk};
use openaec_sync::oauth::{self, TOOLS};
use openaec_sync::sessie::Sessie;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::{fout, instellingen, AppState, Sleutelbos};
use bijlagen::Bijlage;
use gesprekken::{Gesprek, Gesprekken, Samenvatting};

/// De cloud die de tokens voor de gateway uitgeeft.
pub const STANDAARD_CLOUD: &str = "https://cloud.open-aec.eu";
/// De app in `X-OpenAEC-Tool` (komt in het verbruik; het beleid van de omgeving kan hem toestaan of niet).
pub const TOOL: &str = "openaec-desktop";

/// De cloud bij een klantomgeving: `https://klant.cloud.open-aec.eu` hoort bij `https://cloud.open-aec.eu`; ook een
/// andere `klant.cloud.<domein>` (bijv. een testomgeving) bij `cloud.<domein>`. Anders de standaardcloud.
pub fn cloud_voor(basis: &str) -> String {
    let Ok(url) = reqwest::Url::parse(basis) else { return STANDAARD_CLOUD.into() };
    let host = url.host_str().unwrap_or_default();
    match host.split_once('.') {
        Some((_, rest)) if rest.starts_with("cloud.") && rest.contains('.') => {
            let poort = url.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{rest}{poort}", url.scheme())
        }
        _ => STANDAARD_CLOUD.into(),
    }
}

/// De host van de klantomgeving voor `X-OpenAEC-Omgeving` (zonder schema en poort).
pub fn omgeving_voor(basis: &str) -> Option<String> {
    reqwest::Url::parse(basis).ok()?.host_str().map(str::to_string)
}

/// Waar de tokens van de assistent in de sleutelbos staan: per cloud (het account), los van de sync.
fn sleutelnaam(cloud: &str) -> String {
    format!("{cloud}#ai")
}

/// Gateway, cloud en omgeving uit de instellingen (de gateway en de cloud zijn daar te overschrijven).
struct Doel {
    gateway: String,
    cloud: String,
    omgeving: Option<String>,
    basis: Option<String>,
}

fn doel(state: &AppState) -> Doel {
    let inst = state.instellingen.lock().unwrap();
    let basis = inst.basis.clone();
    let cloud = inst.assistent_cloud.clone().unwrap_or_else(|| basis.as_deref().map_or(STANDAARD_CLOUD.into(), cloud_voor));
    Doel {
        gateway: inst.assistent_gateway.clone().unwrap_or_else(|| openaec_sync::ai::STANDAARD_GATEWAY.into()),
        cloud: cloud.trim_end_matches('/').to_string(),
        omgeving: basis.as_deref().and_then(omgeving_voor),
        basis,
    }
}

/// De sessie van de assistent bij een cloud, één voor de hele app (zie `crate::sessie`).
fn sessie(cloud: &str) -> Arc<Sessie> {
    static SESSIES: Mutex<Option<HashMap<String, Arc<Sessie>>>> = Mutex::new(None);
    SESSIES
        .lock()
        .unwrap()
        .get_or_insert_with(Default::default)
        .entry(cloud.to_string())
        .or_insert_with(|| Arc::new(Sessie::met_aanvrager(cloud, Sleutelbos(sleutelnaam(cloud)), TOOLS)))
        .clone()
}

/// Een fout voor het venster: de soort (voor een knop zoals Inloggen), de melding en eventueel de wachttijd.
#[derive(Debug, Serialize)]
pub struct FoutUit {
    soort: String,
    melding: String,
    retry_na: Option<u64>,
}

impl From<AiFout> for FoutUit {
    fn from(f: AiFout) -> Self {
        let waarde = serde_json::to_value(&f).unwrap_or_default();
        Self {
            soort: waarde["soort"].as_str().unwrap_or("anders").to_string(),
            retry_na: waarde["retry_na"].as_u64(),
            melding: f.to_string(),
        }
    }
}

impl FoutUit {
    fn anders(melding: impl Into<String>) -> Self {
        Self { soort: "anders".into(), melding: melding.into(), retry_na: None }
    }
}

fn client(state: &AppState) -> Result<AiClient, FoutUit> {
    let d = doel(state);
    let omgeving = d.omgeving.ok_or_else(|| FoutUit::anders("Verbind eerst met je omgeving."))?;
    Ok(AiClient::new(&d.gateway, &omgeving, Some(TOOL), sessie(&d.cloud), crate::http()))
}

#[derive(Serialize)]
pub struct Status {
    /// Zijn er tokens voor de assistent (ook als ze verlopen zijn: dan vernieuwt de sessie ze)?
    ingelogd: bool,
    omgeving: Option<String>,
    gateway: String,
    cloud: String,
}

#[tauri::command]
pub fn assistent_status(state: tauri::State<'_, AppState>) -> Status {
    let d = doel(&state);
    Status { ingelogd: instellingen::lees_sleutel(&sleutelnaam(&d.cloud)).is_some(), omgeving: d.omgeving, gateway: d.gateway, cloud: d.cloud }
}

/// Inloggen voor de assistent bij de cloud (client `tools`, scope `ai`): opent de browser en wacht (hoogstens vijf
/// minuten) op de terugkeer. Wie al bij de cloud is ingelogd, hoeft meestal alleen toe te staan.
#[tauri::command]
pub async fn assistent_log_in(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let cloud = doel(&state).cloud;
    // De browser gaat naar `localhost`: dat kan IPv4 of IPv6 zijn, dus op beide luisteren (IPv6 als het kan).
    let v4 = tokio::net::TcpListener::bind(("127.0.0.1", TOOLS.poort))
        .await
        .map_err(|e| format!("poort {} is bezet ({e}); sluit een ander inlogvenster of een andere OpenAEC-tool die inlogt", TOOLS.poort))?;
    let v6 = tokio::net::TcpListener::bind(("::1", TOOLS.poort)).await.ok();
    let verifier = oauth::verifier();
    let staat = oauth::verifier();
    let url = oauth::autorisatie_url_voor(&TOOLS, &cloud, &oauth::challenge(&verifier), &staat);
    #[allow(deprecated)]
    app.shell().open(&url, None).map_err(fout)?;
    let wacht = async {
        let via_v4 = oauth::wacht_op_code_op(v4, staat.clone(), TOOLS.terugpad);
        match v6 {
            Some(v6) => tokio::select! {
                r = via_v4 => r,
                r = oauth::wacht_op_code_op(v6, staat.clone(), TOOLS.terugpad) => r,
            },
            None => via_v4.await,
        }
    };
    let code = tokio::time::timeout(Duration::from_secs(300), wacht)
        .await
        .map_err(|_| "inloggen duurde te lang".to_string())?
        .map_err(fout)?;
    let tokens = oauth::wissel_code_voor(&TOOLS, &cloud, &code, &verifier).await.map_err(fout)?;
    instellingen::bewaar_sleutel(&sleutelnaam(&cloud), &serde_json::to_string(&tokens).unwrap()).map_err(fout)?;
    sessie(&cloud).vergeet().await;
    Ok(())
}

/// Afmelden voor de assistent (bijv. bij het verbreken van de verbinding): de tokens gaan uit de sleutelbos.
pub async fn log_uit(state: &AppState) {
    let cloud = doel(state).cloud;
    instellingen::wis_sleutel(&sleutelnaam(&cloud));
    sessie(&cloud).vergeet().await;
}

#[tauri::command]
pub async fn assistent_modellen(state: tauri::State<'_, AppState>) -> Result<Vec<Model>, FoutUit> {
    Ok(client(&state)?.modellen().await?)
}

/// Wat het venster tijdens een antwoord krijgt.
#[derive(Clone, Serialize)]
#[serde(tag = "soort", rename_all = "snake_case")]
pub enum Gebeurtenis {
    Tekst { tekst: String },
    Denkblokken { denkblokken: String },
    Einde { reden: String },
    Gebruik { gebruik: Gebruik },
}

impl From<Stuk> for Gebeurtenis {
    fn from(s: Stuk) -> Self {
        match s {
            Stuk::Tekst(tekst) => Gebeurtenis::Tekst { tekst },
            Stuk::Denkblokken(denkblokken) => Gebeurtenis::Denkblokken { denkblokken },
            Stuk::Einde(reden) => Gebeurtenis::Einde { reden },
            Stuk::Gebruik(gebruik) => Gebeurtenis::Gebruik { gebruik },
        }
    }
}

#[derive(Serialize)]
pub struct Uitkomst {
    afgebroken: bool,
    vraag_id: Option<String>,
}

/// Lopende antwoorden per gesprek (voor de knop Stoppen).
fn lopend() -> &'static Mutex<HashMap<String, AbortHandle>> {
    static LOPEND: std::sync::OnceLock<Mutex<HashMap<String, AbortHandle>>> = std::sync::OnceLock::new();
    LOPEND.get_or_init(Default::default)
}

/// Stel de laatste vraag van het gesprek (de hele geschiedenis gaat mee); de stukjes van het antwoord komen via
/// `kanaal`. Met [`assistent_stop`] houdt het antwoord op; wat er al was, blijft.
#[tauri::command]
pub async fn assistent_vraag(state: tauri::State<'_, AppState>, gesprek: Gesprek, kanaal: Channel<Gebeurtenis>) -> Result<Uitkomst, FoutUit> {
    if gesprekken::afbeeldingen(&gesprek) > bijlagen::MAX_AFBEELDINGEN {
        return Err(FoutUit::anders(format!(
            "Dit gesprek heeft meer dan {} afbeeldingen; begin een nieuw gesprek.",
            bijlagen::MAX_AFBEELDINGEN
        )));
    }
    let client = client(&state)?;
    let mut body = serde_json::json!({ "messages": gesprekken::berichten_voor(&gesprek) });
    if let Some(model) = gesprek.model.as_deref().filter(|m| !m.is_empty()) {
        body["model"] = model.into();
    }
    let (stop, registratie) = AbortHandle::new_pair();
    if let Some(vorige) = lopend().lock().unwrap().insert(gesprek.id.clone(), stop) {
        vorige.abort();
    }
    let antwoord = Abortable::new(
        client.vraag(&body, move |stuk| {
            let _ = kanaal.send(stuk.into());
        }),
        registratie,
    )
    .await;
    lopend().lock().unwrap().remove(&gesprek.id);
    match antwoord {
        Ok(Ok(afloop)) => Ok(Uitkomst { afgebroken: false, vraag_id: afloop.vraag_id }),
        Ok(Err(f)) => Err(f.into()),
        Err(_) => Ok(Uitkomst { afgebroken: true, vraag_id: None }),
    }
}

#[tauri::command]
pub fn assistent_stop(gesprek: String) {
    if let Some(stop) = lopend().lock().unwrap().remove(&gesprek) {
        stop.abort();
    }
}

/// Een bestand als bijlage (ingesleept, gekozen of uit het rechtermuismenu). `al_tekst`: de tekens tekst die al in het
/// gesprek zitten.
#[tauri::command]
pub async fn assistent_bijlage(pad: String, al_tekst: usize) -> Result<Bijlage, String> {
    tauri::async_runtime::spawn_blocking(move || bijlagen::van_pad(std::path::Path::new(&pad), al_tekst)).await.map_err(fout)?
}

/// Een geplakt bestand (bijv. een schermafbeelding), als base64.
#[tauri::command]
pub async fn assistent_bijlage_data(naam: String, data: String, al_tekst: usize) -> Result<Bijlage, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(|_| format!("{naam}: niet te lezen"))?;
    tauri::async_runtime::spawn_blocking(move || bijlagen::van_bytes(&naam, &bytes, al_tekst)).await.map_err(fout)?
}

#[tauri::command]
pub async fn assistent_gesprekken() -> Vec<Samenvatting> {
    tauri::async_runtime::spawn_blocking(|| Gesprekken::standaard().lijst()).await.unwrap_or_default()
}

#[tauri::command]
pub async fn assistent_gesprek(id: String) -> Result<Gesprek, String> {
    tauri::async_runtime::spawn_blocking(move || Gesprekken::standaard().lees(&id)).await.map_err(fout)?
}

#[tauri::command]
pub async fn assistent_bewaar(gesprek: Gesprek) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || Gesprekken::standaard().bewaar(&gesprek)).await.map_err(fout)?
}

#[tauri::command]
pub async fn assistent_verwijder(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || Gesprekken::standaard().verwijder(&id)).await.map_err(fout)?
}

/// Het beheer van de omgeving in de browser (voor AI-tegoed: Beheer → Abonnement).
#[tauri::command]
pub fn assistent_open_beheer(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let basis = doel(&state).basis.ok_or("niet verbonden")?;
    #[allow(deprecated)]
    app.shell().open(format!("{basis}/beheer"), None).map_err(fout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_en_omgeving_bij_een_klantomgeving() {
        assert_eq!(cloud_voor("https://klant0.cloud.open-aec.eu"), "https://cloud.open-aec.eu");
        assert_eq!(cloud_voor("https://acme.cloud.test.example.nl/"), "https://cloud.test.example.nl");
        assert_eq!(cloud_voor("http://klant0.cloud.open-aec.localhost:8000"), "http://cloud.open-aec.localhost:8000");
        // Een eigen domein zonder cloud erin: de standaardcloud.
        assert_eq!(cloud_voor("https://bestanden.bedrijf.nl"), STANDAARD_CLOUD);
        assert_eq!(cloud_voor("geen adres"), STANDAARD_CLOUD);
        assert_eq!(omgeving_voor("https://klant0.cloud.open-aec.eu:443/x").as_deref(), Some("klant0.cloud.open-aec.eu"));
        assert_eq!(omgeving_voor("http://klant0.localhost:8000").as_deref(), Some("klant0.localhost"));
        assert_eq!(sleutelnaam("https://cloud.open-aec.eu"), "https://cloud.open-aec.eu#ai");
    }

    #[test]
    fn fout_voor_het_venster() {
        let f = FoutUit::from(AiFout::Plafond { retry_na: Some(120) });
        assert_eq!((f.soort.as_str(), f.retry_na), ("plafond", Some(120)));
        assert!(f.melding.contains("over 2 minuten"));
        assert_eq!(FoutUit::from(AiFout::SessieVerlopen).soort, "sessie_verlopen");
        assert_eq!(FoutUit::from(AiFout::GeenTegoed).soort, "geen_tegoed");
        let g = serde_json::to_value(Gebeurtenis::from(Stuk::Tekst("a".into()))).unwrap();
        assert_eq!(g, serde_json::json!({"soort": "tekst", "tekst": "a"}));
    }
}
