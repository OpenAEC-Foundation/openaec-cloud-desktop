//! OpenAEC Desktop App: koppelt lokale mappen aan ruimtes in OpenAEC Storage en houdt ze gelijk met de
//! sync-motor uit de crate `openaec-sync`.

mod activiteit;
mod bewaking;
mod instellingen;
mod systeemvak;
mod tools;
mod updates;
mod verkenner;
mod vinkjes;

#[cfg(windows)]
pub mod cloudfilter;

// Unit-tests: het app-manifest (zie build.rs) ook in de testbinary.
#[cfg(all(test, windows))]
#[link(name = "resource", kind = "static", modifiers = "+whole-archive")]
extern "C" {}

use std::path::{Path, PathBuf};
use std::time::Duration;

use openaec_sync::api::Ruimte;
use openaec_sync::motor::{aantal_bekend, bekend_bestand, sync_met_voortgang, Rapport};
use openaec_sync::oauth::{self, Tokens};
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::Client;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_shell::ShellExt;

use instellingen::{Instellingen, Koppeling};

struct AppState {
    instellingen: std::sync::Mutex<Instellingen>,
    /// Eén sync-ronde tegelijk (achtergrond of handmatig).
    bezig: tokio::sync::Mutex<()>,
    /// Wat de sync nu doet (statusvenster, statusbalk, systeemvak) en of hij gepauzeerd is.
    activiteit: activiteit::Activiteit,
}

/// Naam van de autostart-regel; de productnaam van vóór versie 0.5 (zie `run`).
const AUTOSTART_NAAM: &str = "OpenAEC Storage";

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
    inst.koppelingen.push(Koppeling { ruimte, ruimte_naam, lokaal, uitgesloten: Vec::new() });
    inst.bewaar().map_err(fout)?;
    werk_menu_bij(&inst.koppelingen);
    Ok(inst.koppelingen.clone())
}

fn werk_menu_bij(koppelingen: &[Koppeling]) {
    let mappen: Vec<String> = koppelingen.iter().map(|k| k.lokaal.clone()).collect();
    if let Err(e) = verkenner::registreer(&mappen) {
        log::warn!("rechtermuismenu bijwerken mislukt: {e}");
    }
}

/// Alle mappen van een ruimte (voor de mapselectie).
#[tauri::command]
async fn mappen_van(state: tauri::State<'_, AppState>, ruimte: String) -> Result<Vec<String>, String> {
    ClientOpslag::new(client(&state).await?, &ruimte).alle_mappen().await.map_err(fout)
}

#[derive(Serialize)]
struct MapRegel {
    id: String,
    naam: String,
    gewijzigd: Option<String>,
}

#[derive(Serialize)]
struct BestandRegel {
    id: String,
    naam: String,
    grootte: u64,
    versie: u64,
    gewijzigd: Option<String>,
    /// Wie het bestand vergrendeld heeft (e-mailadres), en tot wanneer.
    vergrendeld_door: Option<String>,
    vergrendeld_tot: Option<String>,
}

#[derive(Serialize)]
struct MapInhoud {
    mappen: Vec<MapRegel>,
    bestanden: Vec<BestandRegel>,
}

/// Hoeveel vergrendelingen tegelijk worden opgevraagd bij het bladeren.
const GELIJKTIJDIG: usize = 8;

/// Inhoud van een map in een ruimte op de server (`map` None = hoofdniveau), met per bestand de actieve vergrendeling.
#[tauri::command]
async fn bladeren(state: tauri::State<'_, AppState>, ruimte: String, map: Option<String>) -> Result<MapInhoud, String> {
    let client = client(&state).await?;
    let inhoud = client.bladeren(&ruimte, map.as_deref().filter(|m| !m.is_empty())).await.map_err(fout)?;

    // Vergrendelingen per bestand ophalen, een paar tegelijk; lukt het niet, dan blijft het veld leeg.
    let rem = std::sync::Arc::new(tokio::sync::Semaphore::new(GELIJKTIJDIG));
    let mut taken = tokio::task::JoinSet::new();
    for b in &inhoud.bestanden {
        let (client, rem, id) = (client.clone(), rem.clone(), b.name.clone());
        taken.spawn(async move {
            let _vergunning = rem.acquire_owned().await;
            let slot = client.info(&id).await.ok().and_then(|i| i.vergrendeling);
            (id, slot)
        });
    }
    let mut sloten = std::collections::HashMap::new();
    while let Some(klaar) = taken.join_next().await {
        if let Ok((id, Some(slot))) = klaar {
            sloten.insert(id, slot);
        }
    }

    Ok(MapInhoud {
        mappen: inhoud.mappen.into_iter().map(|m| MapRegel { id: m.name, naam: m.naam, gewijzigd: m.modified }).collect(),
        bestanden: inhoud
            .bestanden
            .into_iter()
            .map(|b| {
                let slot = sloten.remove(&b.name);
                BestandRegel {
                    vergrendeld_door: slot.as_ref().map(|s| s.gebruiker.clone()),
                    vergrendeld_tot: slot.and_then(|s| s.verloopt),
                    id: b.name,
                    naam: b.naam,
                    grootte: b.grootte,
                    versie: b.huidige_versie,
                    gewijzigd: b.modified,
                }
            })
            .collect(),
    })
}

#[derive(Serialize)]
struct KoppelStatus {
    lokaal: String,
    /// Aantal bestanden volgens de laatste sync-ronde.
    aantal: usize,
    /// Bestaat de lokale map nog?
    bestaat: bool,
}

#[derive(Serialize)]
struct Overzicht {
    /// Loopt er nu een sync-ronde?
    bezig: bool,
    koppelingen: Vec<KoppelStatus>,
}

/// Toestand per koppeling voor het overzicht; leest alleen lokale gegevens.
#[tauri::command]
async fn overzicht(state: tauri::State<'_, AppState>) -> Result<Overzicht, String> {
    let bezig = state.bezig.try_lock().is_err();
    let mappen: Vec<String> = state.instellingen.lock().unwrap().koppelingen.iter().map(|k| k.lokaal.clone()).collect();
    let koppelingen = tauri::async_runtime::spawn_blocking(move || {
        mappen
            .into_iter()
            .map(|lokaal| {
                let pad = Path::new(&lokaal);
                KoppelStatus { aantal: aantal_bekend(pad), bestaat: pad.is_dir(), lokaal }
            })
            .collect()
    })
    .await
    .map_err(fout)?;
    Ok(Overzicht { bezig, koppelingen })
}

/// Een gekoppelde map (of een map daarbinnen) openen in de Verkenner. Is `pad` een bestand, dan opent de map
/// waarin het staat, met het bestand geselecteerd; bestaat het bestand niet meer, dan de map waarin het stond.
#[tauri::command]
fn open_map(app: AppHandle, state: tauri::State<'_, AppState>, pad: String) -> Result<(), String> {
    let p = Path::new(&pad);
    let gekoppeld = state.instellingen.lock().unwrap().koppelingen.iter().any(|k| p.starts_with(&k.lokaal));
    if !gekoppeld {
        return Err("deze map is niet gekoppeld".into());
    }
    // Een verwijderd bestand: dan de map waarin het stond.
    let p = if p.exists() { p } else { p.parent().filter(|d| d.is_dir()).unwrap_or(p) };
    let pad = p.to_string_lossy().to_string();
    if p.is_file() {
        #[cfg(windows)]
        {
            let _ = &app;
            return std::process::Command::new("explorer").arg(format!("/select,{pad}")).spawn().map(|_| ()).map_err(fout);
        }
        #[cfg(not(windows))]
        {
            #[allow(deprecated)]
            return app.shell().open(p.parent().unwrap_or(p).to_string_lossy(), None).map_err(fout);
        }
    }
    if !p.is_dir() {
        return Err("deze map of dit bestand bestaat niet (meer) op deze computer".into());
    }
    #[cfg(windows)]
    {
        let _ = &app;
        std::process::Command::new("explorer").arg(&pad).spawn().map(|_| ()).map_err(fout)
    }
    #[cfg(not(windows))]
    {
        #[allow(deprecated)]
        app.shell().open(&pad, None).map_err(fout)
    }
}

/// Wat de sync nu doet (voor het statusvenster en de statusbalk bij het openen).
#[tauri::command]
fn activiteit_stand(state: tauri::State<'_, AppState>) -> activiteit::Stand {
    state.activiteit.stand()
}

/// Pauzeren of hervatten. Na hervatten volgt meteen een ronde voor alle mappen.
#[tauri::command]
fn pauzeer(app: AppHandle, aan: bool) {
    activiteit::zet_pauze(&app, aan);
    if !aan {
        tauri::async_runtime::spawn(async move {
            if let Ok(r) = sync_alles(&app).await {
                let _ = app.emit("sync", &r);
            }
        });
    }
}

/// Het hoofdvenster tonen (vanuit het statusvenster).
#[tauri::command]
fn toon_hoofdvenster(app: AppHandle) {
    systeemvak::toon(&app);
}

/// Opent het beheer van de opslag (ruimtes aanmaken) van de verbonden omgeving in de browser.
#[tauri::command]
fn open_beheer(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let basis = state.instellingen.lock().unwrap().basis.clone().ok_or("niet verbonden")?;
    #[allow(deprecated)]
    app.shell().open(format!("{basis}/beheer/opslag"), None).map_err(fout)
}

/// Versienummer van de app.
#[tauri::command]
fn versie(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Nu controleren op een nieuwe versie; geeft het versienummer van een klaargezette update terug (of None).
#[tauri::command]
async fn controleer_updates(app: AppHandle) -> Result<Option<String>, String> {
    updates::controleer(&app).await.map_err(fout)
}

/// Een klaargezette update installeren (na een lopende sync-ronde); de app start daarna opnieuw.
#[tauri::command]
fn installeer_update(app: AppHandle) {
    updates::installeer(&app);
}

/// Mapselectie van een koppeling instellen; de volgende ronde haalt uitgevinkte mappen lokaal weg.
#[tauri::command]
fn zet_selectie(state: tauri::State<'_, AppState>, lokaal: String, uitgesloten: Vec<String>) -> Result<Vec<Koppeling>, String> {
    let mut inst = state.instellingen.lock().unwrap();
    let k = inst.koppelingen.iter_mut().find(|k| k.lokaal == lokaal).ok_or("deze map is niet gekoppeld")?;
    k.uitgesloten = uitgesloten;
    inst.bewaar().map_err(fout)?;
    Ok(inst.koppelingen.clone())
}

/// Ontkoppelen laat de lokale bestanden staan; de statusvinkjes gaan eraf (na een eventueel lopende ronde).
#[tauri::command]
fn ontkoppel(app: AppHandle, state: tauri::State<'_, AppState>, lokaal: String) -> Result<Vec<Koppeling>, String> {
    let mut inst = state.instellingen.lock().unwrap();
    inst.koppelingen.retain(|k| k.lokaal != lokaal);
    inst.bewaar().map_err(fout)?;
    werk_menu_bij(&inst.koppelingen);
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _slot = state.bezig.lock().await;
        let _ = tauri::async_runtime::spawn_blocking(move || vinkjes::ontkoppel(&lokaal)).await;
    });
    Ok(inst.koppelingen.clone())
}

#[derive(Serialize, Clone)]
struct MapRapport {
    lokaal: String,
    ruimte_naam: String,
    rapport: Option<Rapport>,
    fout: Option<String>,
}

async fn sync_alles(app: &AppHandle) -> Result<Vec<MapRapport>, String> {
    sync_mappen(app, None).await
}

/// Sync-ronde voor de gekoppelde mappen in `alleen` (lokale paden), of voor alle mappen. Doet niets als de sync
/// gepauzeerd is; de voortgang gaat naar het statusvenster en het systeemvak.
async fn sync_mappen(app: &AppHandle, alleen: Option<&std::collections::HashSet<String>>) -> Result<Vec<MapRapport>, String> {
    let state = app.state::<AppState>();
    let _slot = state.bezig.lock().await;
    if state.activiteit.gepauzeerd() {
        return Err("synchroniseren is gepauzeerd".into());
    }
    activiteit::begin(app);
    let r = sync_ronde(app, &state, alleen).await;
    activiteit::einde(app, r.as_deref().map_err(String::as_str));
    r
}

async fn sync_ronde(
    app: &AppHandle,
    state: &AppState,
    alleen: Option<&std::collections::HashSet<String>>,
) -> Result<Vec<MapRapport>, String> {
    let client = client(state).await?;
    let koppelingen = state.instellingen.lock().unwrap().koppelingen.clone();
    let mut uit = Vec::new();
    for k in koppelingen.into_iter().filter(|k| alleen.is_none_or(|a| a.contains(&k.lokaal))) {
        let opslag = ClientOpslag::new(client.clone(), &k.ruimte);
        let waarnemer = activiteit::Koppelwaarnemer { app: app.clone(), lokaal: k.lokaal.clone() };
        let mut r = sync_met_voortgang(&opslag, Path::new(&k.lokaal), &k.uitgesloten, &waarnemer).await;
        if let Ok(rapport) = &mut r {
            let lokaal = k.lokaal.clone();
            let bijgewerkt = std::mem::take(&mut rapport.bijgewerkt);
            let foutpaden = std::mem::take(&mut rapport.foutpaden);
            let _ = tauri::async_runtime::spawn_blocking(move || vinkjes::na_ronde(&lokaal, &bijgewerkt, &foutpaden)).await;
        }
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
async fn sync_nu(app: AppHandle) -> Result<Vec<MapRapport>, String> {
    let r = sync_alles(&app).await?;
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
async fn deel(
    state: tauri::State<'_, AppState>,
    bestand: String,
    wachtwoord: Option<String>,
    verloopt: Option<String>,
) -> Result<String, String> {
    maak_deellink(&state, &bestand, wachtwoord.filter(|w| !w.is_empty()), verloopt.filter(|v| !v.is_empty())).await
}

/// Deellink voor een lokaal bestand of een lokale map in een gekoppelde map.
async fn maak_deellink(state: &AppState, pad: &str, wachtwoord: Option<String>, verloopt: Option<String>) -> Result<String, String> {
    let client = client(state).await?;
    let (doctype, id) = if Path::new(pad).is_dir() {
        let (k, rel) = zoek(state, pad)?;
        let id = ClientOpslag::new(client.clone(), &k.ruimte)
            .zoek_map(&rel)
            .await
            .map_err(fout)?
            .ok_or("deze map staat nog niet op de server; wacht tot hij gesynchroniseerd is")?;
        ("Opslagmap", id)
    } else {
        ("Opslagbestand", bestand_id(state, pad)?)
    };
    let deling = client.deel(doctype, &id, wachtwoord.as_deref(), verloopt.as_deref()).await.map_err(fout)?;
    Ok(deling.url.unwrap_or_else(|| format!("{}/delen/{}", client.basis(), deling.token)))
}

/// Uploadlink (bestanden opvragen) voor een lokale map in een gekoppelde map.
async fn maak_uploadlink(state: &AppState, pad: &str) -> Result<String, String> {
    if !Path::new(pad).is_dir() {
        return Err("bestanden opvragen kan alleen voor een map".into());
    }
    let client = client(state).await?;
    let (k, rel) = zoek(state, pad)?;
    let id = ClientOpslag::new(client.clone(), &k.ruimte)
        .zoek_map(&rel)
        .await
        .map_err(fout)?
        .ok_or("deze map staat nog niet op de server; wacht tot hij gesynchroniseerd is")?;
    let deling = client.vraag_bestanden(&id, None, None).await.map_err(fout)?;
    Ok(deling.url.unwrap_or_else(|| format!("{}/delen/{}", client.basis(), deling.token)))
}

#[tauri::command]
async fn versies(state: tauri::State<'_, AppState>, bestand: String) -> Result<Vec<openaec_sync::api::Versie>, String> {
    let id = bestand_id(&state, &bestand)?;
    client(&state).await?.versies(&id).await.map_err(fout)
}

/// Een oudere versie downloaden (opent de tijdelijke downloadlink in de browser).
#[tauri::command]
async fn open_versie(app: AppHandle, state: tauri::State<'_, AppState>, bestand: String, versie: u64) -> Result<(), String> {
    let id = bestand_id(&state, &bestand)?;
    let link = client(&state).await?.download_versie(&id, versie).await.map_err(fout)?;
    #[allow(deprecated)]
    app.shell().open(&link.url, None).map_err(fout)
}

fn melding(app: &AppHandle, tekst: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title("OpenAEC Desktop App").body(tekst).show();
}

async fn actie_uitvoeren(app: &AppHandle, actie: &str, pad: &str, naam: &str) -> Result<String, String> {
    let state = app.state::<AppState>();
    match actie {
        "deel" => {
            let link = maak_deellink(&state, pad, None, None).await?;
            use tauri_plugin_clipboard_manager::ClipboardExt;
            app.clipboard().write_text(link).map_err(fout)?;
            Ok(format!("Deellink van {naam} staat op het klembord."))
        }
        "opvragen" => {
            let link = maak_uploadlink(&state, pad).await?;
            use tauri_plugin_clipboard_manager::ClipboardExt;
            app.clipboard().write_text(link).map_err(fout)?;
            Ok(format!("Uploadlink voor {naam} staat op het klembord: wie hem opent, kan bestanden in deze map zetten."))
        }
        "vergrendel" => {
            let id = bestand_id(&state, pad)?;
            let apparaat = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "desktop".into());
            client(&state).await?.vergrendel(&id, &apparaat, false).await.map_err(fout)?;
            Ok(format!("{naam} is vergrendeld: collega's zien dat jij eraan werkt."))
        }
        "ontgrendel" => {
            let id = bestand_id(&state, pad)?;
            client(&state).await?.ontgrendel(&id).await.map_err(fout)?;
            Ok(format!("{naam} is ontgrendeld."))
        }
        _ => Err(format!("onbekende actie: {actie}")),
    }
}

/// Actie uit het rechtermuismenu van de Verkenner.
pub(crate) async fn voer_actie(app: AppHandle, actie: String, pad: String) {
    if actie == "verzend" || actie == "versies" {
        systeemvak::toon(&app);
        let _ = app.emit("actie", serde_json::json!({ "actie": actie, "pad": pad }));
        return;
    }
    let naam = Path::new(&pad).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    match actie_uitvoeren(&app, &actie, &pad, &naam).await {
        Ok(t) => melding(&app, &t),
        Err(e) => melding(&app, &format!("{naam}: {e}")),
    }
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
        cloudfilter::register(&lokaal, "OpenAEC Storage", env!("CARGO_PKG_VERSION")).map_err(fout)?;
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

/// Starten bij inloggen (aan/uit); de app start dan verborgen in het systeemvak.
#[tauri::command]
fn autostart(app: AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn zet_autostart(app: AppHandle, aan: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let a = app.autolaunch();
    if aan { a.enable() } else { a.disable() }.map_err(fout)?;
    a.is_enabled().map_err(fout)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Eén exemplaar tegelijk: een tweede start toont het bestaande venster.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _| match verkenner::actie_uit(&argv) {
            Some((actie, pad)) => {
                tauri::async_runtime::spawn(voer_actie(app.clone(), actie, pad));
            }
            None => systeemvak::toon(app),
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        // Vaste naam voor de autostart-regel (Run-sleutel op Windows, .desktop-bestand op Linux): standaard is dat de
        // productnaam, en dan zou een bestaande regel van vóór de nieuwe naam niet meer herkend worden. De
        // NSIS-uninstaller ruimt deze regel op (nsis/hooks.nsh).
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name(AUTOSTART_NAAM)
                .args([systeemvak::VERBORGEN])
                .build(),
        )
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            instellingen: std::sync::Mutex::new(Instellingen::laad()),
            bezig: tokio::sync::Mutex::new(()),
            activiteit: activiteit::Activiteit::default(),
        })
        .setup(|app| {
            systeemvak::maak(app)?;
            bewaking::start(app.handle().clone());
            updates::start(app.handle().clone());
            werk_menu_bij(&app.state::<AppState>().instellingen.lock().unwrap().koppelingen);
            activiteit::meld(app.handle());
            // Gestart vanuit het rechtermuismenu terwijl de app nog niet draaide.
            let argv: Vec<String> = std::env::args().collect();
            if let Some((actie, pad)) = verkenner::actie_uit(&argv) {
                tauri::async_runtime::spawn(voer_actie(app.handle().clone(), actie, pad));
            }
            Ok(())
        })
        .on_window_event(systeemvak::venster_gebeurtenis)
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
            maak_cloudmap,
            autostart,
            zet_autostart,
            versies,
            open_versie,
            mappen_van,
            zet_selectie,
            bladeren,
            overzicht,
            open_map,
            open_beheer,
            versie,
            controleer_updates,
            installeer_update,
            activiteit_stand,
            pauzeer,
            toon_hoofdvenster,
            tools::tools_geinstalleerd,
            tools::tools_releases,
            tools::tool_installeren,
            tools::tool_starten,
            tools::tool_verwijderen
        ])
        .run(tauri::generate_context!())
        .expect("fout bij het starten van OpenAEC Desktop App");
}
