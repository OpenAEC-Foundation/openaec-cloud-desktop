//! OpenAEC Cloud desktop: koppelt lokale mappen aan ruimtes in OpenAEC Storage en houdt ze gelijk
//! met de sync-motor uit de crate `openaec-sync`.

mod bewaking;
mod instellingen;

#[cfg(windows)]
pub mod cloudfilter;

use std::path::{Path, PathBuf};
use std::time::Duration;

use openaec_sync::api::Ruimte;
use openaec_sync::motor::{bekend_bestand, sync_eenmalig, Rapport};
use openaec_sync::oauth::{self, Tokens};
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::Client;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_shell::ShellExt;

use instellingen::{Instellingen, Koppeling};

struct AppState {
    instellingen: std::sync::Mutex<Instellingen>,
    /// Eén sync-ronde tegelijk (achtergrond of handmatig).
    bezig: tokio::sync::Mutex<()>,
}

fn fout(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Voorkomt dat twee taken tegelijk het token vernieuwen (het oude refresh-token vervalt daarbij).
static VERNIEUWEN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Waarde voor de Authorization-header: een API-sleutel zoals bewaard, of een OAuth-token dat zo nodig
/// eerst wordt vernieuwd.
async fn autorisatie(basis: &str) -> Result<String, String> {
    let _slot = VERNIEUWEN.lock().await;
    let bewaard = instellingen::lees_sleutel(basis).ok_or("niet ingelogd; verbind opnieuw")?;
    let Ok(tokens) = serde_json::from_str::<Tokens>(&bewaard) else {
        return Ok(bewaard);
    };
    if !tokens.verlopen() {
        return Ok(tokens.autorisatie());
    }
    let nieuw = oauth::vernieuw(basis, &tokens).await.map_err(|e| format!("sessie verlopen, log opnieuw in ({e})"))?;
    instellingen::bewaar_sleutel(basis, &serde_json::to_string(&nieuw).unwrap()).map_err(fout)?;
    Ok(nieuw.autorisatie())
}

async fn client(state: &AppState) -> Result<Client, String> {
    let basis = state.instellingen.lock().unwrap().basis.clone().ok_or("niet verbonden")?;
    Ok(Client::new(&basis, &autorisatie(&basis).await?))
}

fn onthoud_basis(state: &AppState, basis: String) -> Result<(), String> {
    let mut inst = state.instellingen.lock().unwrap();
    inst.basis = Some(basis);
    inst.bewaar().map_err(fout)
}

#[derive(Serialize)]
struct Status {
    basis: Option<String>,
    verbonden: bool,
    koppelingen: Vec<Koppeling>,
}

#[tauri::command]
fn status(state: tauri::State<'_, AppState>) -> Status {
    let inst = state.instellingen.lock().unwrap();
    let verbonden = inst.basis.as_deref().is_some_and(|b| instellingen::lees_sleutel(b).is_some());
    Status { basis: inst.basis.clone(), verbonden, koppelingen: inst.koppelingen.clone() }
}

/// Verbinden met een klantomgeving; geeft de ruimtes terug (dient meteen als test van de sleutel).
#[tauri::command]
async fn verbind(state: tauri::State<'_, AppState>, basis: String, autorisatie: String) -> Result<Vec<Ruimte>, String> {
    let basis = basis.trim().trim_end_matches('/').to_string();
    let ruimtes = Client::new(&basis, autorisatie.trim()).ruimtes().await.map_err(fout)?;
    instellingen::bewaar_sleutel(&basis, autorisatie.trim()).map_err(fout)?;
    onthoud_basis(&state, basis)?;
    Ok(ruimtes)
}

/// Inloggen met het OpenAEC-account: opent de browser en wacht (max. 5 minuten) op de terugkeer.
#[tauri::command]
async fn log_in(app: AppHandle, state: tauri::State<'_, AppState>, basis: String) -> Result<Vec<Ruimte>, String> {
    let basis = basis.trim().trim_end_matches('/').to_string();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", oauth::POORT))
        .await
        .map_err(|e| format!("poort {} is bezet ({e}); sluit een eerder inlogvenster", oauth::POORT))?;
    let verifier = oauth::verifier();
    let state_waarde = oauth::verifier();
    let url = oauth::autorisatie_url(&basis, &oauth::challenge(&verifier), &state_waarde);
    #[allow(deprecated)]
    app.shell().open(&url, None).map_err(fout)?;
    let code = tokio::time::timeout(Duration::from_secs(300), oauth::wacht_op_code(listener, state_waarde))
        .await
        .map_err(|_| "inloggen duurde te lang".to_string())?
        .map_err(fout)?;
    let tokens = oauth::wissel_code(&basis, &code, &verifier).await.map_err(fout)?;
    let ruimtes = Client::new(&basis, &tokens.autorisatie()).ruimtes().await.map_err(fout)?;
    instellingen::bewaar_sleutel(&basis, &serde_json::to_string(&tokens).unwrap()).map_err(fout)?;
    onthoud_basis(&state, basis)?;
    Ok(ruimtes)
}

#[tauri::command]
fn verbreek(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut inst = state.instellingen.lock().unwrap();
    if let Some(basis) = inst.basis.take() {
        instellingen::wis_sleutel(&basis);
    }
    inst.bewaar().map_err(fout)
}

#[tauri::command]
async fn ruimtes(state: tauri::State<'_, AppState>) -> Result<Vec<Ruimte>, String> {
    client(&state).await?.ruimtes().await.map_err(fout)
}

#[tauri::command]
fn koppel(state: tauri::State<'_, AppState>, ruimte: String, ruimte_naam: String, lokaal: String) -> Result<Vec<Koppeling>, String> {
    std::fs::create_dir_all(&lokaal).map_err(fout)?;
    let mut inst = state.instellingen.lock().unwrap();
    if inst.koppelingen.iter().any(|k| Path::new(&k.lokaal) == Path::new(&lokaal)) {
        return Err("deze map is al gekoppeld".into());
    }
    inst.koppelingen.push(Koppeling { ruimte, ruimte_naam, lokaal });
    inst.bewaar().map_err(fout)?;
    Ok(inst.koppelingen.clone())
}

/// Ontkoppelen laat de lokale bestanden staan.
#[tauri::command]
fn ontkoppel(state: tauri::State<'_, AppState>, lokaal: String) -> Result<Vec<Koppeling>, String> {
    let mut inst = state.instellingen.lock().unwrap();
    inst.koppelingen.retain(|k| k.lokaal != lokaal);
    inst.bewaar().map_err(fout)?;
    Ok(inst.koppelingen.clone())
}

#[derive(Serialize, Clone)]
struct MapRapport {
    lokaal: String,
    ruimte_naam: String,
    rapport: Option<Rapport>,
    fout: Option<String>,
}

async fn sync_alles(state: &AppState) -> Result<Vec<MapRapport>, String> {
    sync_mappen(state, None).await
}

/// Sync-ronde voor de gekoppelde mappen in `alleen` (lokale paden), of voor alle mappen.
async fn sync_mappen(
    state: &AppState,
    alleen: Option<&std::collections::HashSet<String>>,
) -> Result<Vec<MapRapport>, String> {
    let _slot = state.bezig.lock().await;
    let client = client(state).await?;
    let koppelingen = state.instellingen.lock().unwrap().koppelingen.clone();
    let mut uit = Vec::new();
    for k in koppelingen.into_iter().filter(|k| alleen.is_none_or(|a| a.contains(&k.lokaal))) {
        let opslag = ClientOpslag::new(client.clone(), &k.ruimte);
        let r = sync_eenmalig(&opslag, Path::new(&k.lokaal)).await;
        uit.push(MapRapport {
            lokaal: k.lokaal,
            ruimte_naam: k.ruimte_naam,
            fout: r.as_ref().err().map(fout),
            rapport: r.ok(),
        });
    }
    Ok(uit)
}

#[tauri::command]
async fn sync_nu(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<Vec<MapRapport>, String> {
    let r = sync_alles(&state).await?;
    let _ = app.emit("sync", &r);
    Ok(r)
}

/// Koppeling en pad (met '/') voor een lokaal bestand.
fn zoek(state: &AppState, bestand: &str) -> Result<(Koppeling, String), String> {
    let pad = PathBuf::from(bestand);
    let inst = state.instellingen.lock().unwrap();
    for k in &inst.koppelingen {
        if let Ok(rel) = pad.strip_prefix(&k.lokaal) {
            let rel = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/");
            return Ok((k.clone(), rel));
        }
    }
    Err("dit bestand staat niet in een gekoppelde map".into())
}

fn bestand_id(state: &AppState, bestand: &str) -> Result<String, String> {
    let (k, rel) = zoek(state, bestand)?;
    bekend_bestand(Path::new(&k.lokaal), &rel)
        .map(|(id, _)| id)
        .ok_or_else(|| "dit bestand is nog niet gesynchroniseerd".into())
}

#[tauri::command]
async fn vergrendel(state: tauri::State<'_, AppState>, bestand: String) -> Result<(), String> {
    let id = bestand_id(&state, &bestand)?;
    let apparaat = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "desktop".into());
    client(&state).await?.vergrendel(&id, &apparaat, false).await.map(|_| ()).map_err(fout)
}

#[tauri::command]
async fn ontgrendel(state: tauri::State<'_, AppState>, bestand: String) -> Result<(), String> {
    let id = bestand_id(&state, &bestand)?;
    client(&state).await?.ontgrendel(&id).await.map_err(fout)
}

/// Leeslink voor een lokaal (gesynchroniseerd) bestand.
#[tauri::command]
async fn deel(state: tauri::State<'_, AppState>, bestand: String, wachtwoord: Option<String>) -> Result<String, String> {
    let id = bestand_id(&state, &bestand)?;
    let client = client(&state).await?;
    let wachtwoord = wachtwoord.filter(|w| !w.is_empty());
    let deling = client.deel_bestand(&id, wachtwoord.as_deref()).await.map_err(fout)?;
    Ok(format!("{}/delen/{}", client.basis(), deling.token))
}

/// Maak van een lokale map een cloud-map (alleen Windows): bestanden van de ruimte verschijnen als
/// placeholder en worden pas opgehaald als je ze opent. Geeft het aantal placeholders terug.
#[tauri::command]
async fn maak_cloudmap(state: tauri::State<'_, AppState>, ruimte: String, lokaal: String) -> Result<u32, String> {
    #[cfg(windows)]
    {
        use openaec_sync::motor::Opslag;
        let client = client(&state).await?;
        let bestanden = ClientOpslag::new(client.clone(), &ruimte).lijst().await.map_err(fout)?;
        std::fs::create_dir_all(&lokaal).map_err(fout)?;
        cloudfilter::register(&lokaal, "OpenAEC Cloud", env!("CARGO_PKG_VERSION")).map_err(fout)?;
        cloudfilter::set_hydrator(hydrator(client));
        cloudfilter::connect(&lokaal).map_err(fout)?;
        let mut n = 0u32;
        for b in bestanden {
            let (map, naam) = b.pad.rsplit_once('/').map(|(m, n)| (Some(m), n)).unwrap_or((None, b.pad.as_str()));
            let doel = match map {
                Some(m) => m.split('/').fold(PathBuf::from(&lokaal), |p, d| p.join(d)),
                None => PathBuf::from(&lokaal),
            };
            std::fs::create_dir_all(&doel).map_err(fout)?;
            if cloudfilter::create_file_placeholder(&doel.to_string_lossy(), naam, b.grootte as i64, &b.bestand).is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }
    #[cfg(not(windows))]
    {
        let _ = (&state, &ruimte, &lokaal);
        Err("cloud-mappen zijn alleen op Windows beschikbaar".into())
    }
}

/// Hydrator voor placeholders: haalt het gevraagde bereik van de huidige versie op via een downloadlink.
/// Draait op de callback-thread van Windows, dus blocking.
#[cfg(windows)]
fn hydrator(client: Client) -> impl Fn(&str, i64, i64) -> Vec<u8> + Send + Sync + 'static {
    let basis = client.basis().to_string();
    move |bestand, begin, lengte| {
        // Callback-thread van Windows (geen async runtime): token zo nodig vernieuwen via block_on.
        let Ok(autorisatie) = tauri::async_runtime::block_on(autorisatie(&basis)) else { return Vec::new() };
        let http = reqwest::blocking::Client::new();
        let link: Option<String> = http
            .post(format!("{basis}/api/method/openaec_storage.api.download"))
            .header("Authorization", &autorisatie)
            .json(&serde_json::json!({ "bestand": bestand }))
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json::<serde_json::Value>())
            .ok()
            .and_then(|v| v["message"]["url"].as_str().map(str::to_string));
        let Some(url) = link else { return Vec::new() };
        if lengte <= 0 {
            return Vec::new();
        }
        http.get(url)
            .header("Range", format!("bytes={}-{}", begin, begin + lengte - 1))
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes())
            .map(|b| b.to_vec())
            .unwrap_or_default()
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            instellingen: std::sync::Mutex::new(Instellingen::laad()),
            bezig: tokio::sync::Mutex::new(()),
        })
        .setup(|app| {
            bewaking::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            status,
            verbind,
            log_in,
            verbreek,
            ruimtes,
            koppel,
            ontkoppel,
            sync_nu,
            vergrendel,
            ontgrendel,
            deel,
            maak_cloudmap
        ])
        .run(tauri::generate_context!())
        .expect("fout bij het starten van OpenAEC Cloud");
}
