//! OpenAEC Desktop App: koppelt lokale mappen aan ruimtes in OpenAEC Storage en houdt ze gelijk met de
//! sync-motor uit de crate `openaec-sync`.

mod activiteit;
mod assistent;
mod bewaking;
mod delen;
mod hoofdmap;
mod instellingen;
mod itemstatus;
mod menupakket;
mod opaanvraag;
mod navigatie;
mod opruimen;
pub mod rechtermuis;
mod schatting;
mod serverplek;
mod sloten;
mod systeemvak;
mod tools;
mod updates;
mod verkenner;
mod versturen;
mod vinkjes;

#[cfg(windows)]
pub mod cloudfilter;

// Unit-tests: het app-manifest (zie build.rs) ook in de testbinary.
#[cfg(all(test, windows))]
#[link(name = "resource", kind = "static", modifiers = "+whole-archive")]
extern "C" {}

use std::path::{Path, PathBuf};
use std::time::Duration;

use openaec_sync::api::{Gedeeld, Ruimte};
use openaec_sync::motor::{aantal_bekend, bekend_bestand, sync_met_opties, Opties, Rapport};
use openaec_sync::oauth;
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::sessie::{Bewaring, Sessie, Tokenbron};
use openaec_sync::{Client, Fout, SESSIE_VERLOPEN};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_shell::ShellExt;

use instellingen::{Instellingen, Koppeling, SyncInstellingen};

struct AppState {
    instellingen: std::sync::Mutex<Instellingen>,
    /// Eén sync-ronde tegelijk (achtergrond of handmatig).
    bezig: tokio::sync::Mutex<()>,
    /// Wat de sync nu doet (statusvenster, statusbalk, systeemvak) en of hij gepauzeerd is.
    activiteit: activiteit::Activiteit,
    /// Wat het deelvenster nu deelt (zie `delen`).
    deelverzoek: std::sync::Mutex<Option<delen::Deelverzoek>>,
}

/// Naam van de autostart-regel; de productnaam van vóór versie 0.5 (zie `run`).
const AUTOSTART_NAAM: &str = "OpenAEC Storage";

fn fout(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// De sleutel of tokens van een omgeving in de sleutelbos van het systeem (zie `instellingen`).
struct Sleutelbos(String);

impl Bewaring for Sleutelbos {
    fn lees(&self) -> Option<String> {
        instellingen::lees_sleutel(&self.0)
    }

    fn bewaar(&self, waarde: &str) -> Result<(), String> {
        instellingen::bewaar_sleutel(&self.0, waarde).map_err(fout)
    }
}

type Sessies = std::collections::HashMap<String, std::sync::Arc<Sessie>>;

/// De sessie van een omgeving, één voor de hele app: rondes, bewaking, het deelvenster, Versturen en de bestanden op
/// aanvraag halen hun autorisatie bij elk verzoek hier. Haar slot zorgt dat er hoogstens één vernieuwing tegelijk loopt
/// (het oude refresh-token vervalt daarbij); wie wachtte, gebruikt meteen het nieuwe token.
fn sessie(basis: &str) -> std::sync::Arc<Sessie> {
    static SESSIES: std::sync::Mutex<Option<Sessies>> = std::sync::Mutex::new(None);
    let mut sessies = SESSIES.lock().unwrap();
    sessies
        .get_or_insert_with(Default::default)
        .entry(basis.to_string())
        .or_insert_with(|| std::sync::Arc::new(Sessie::new(basis, Sleutelbos(basis.to_string()))))
        .clone()
}

/// Waarde voor de Authorization-header buiten een [`Client`] (de bestanden op aanvraag): met `afgewezen` None de
/// huidige waarde (zo nodig eerst vernieuwd), met de waarde die de server net met 401 weigerde de waarde om het nog één
/// keer mee te proberen. Lukt dat niet meer, dan verschijnt de melding dat de sessie verlopen is.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) async fn autorisatie(app: &AppHandle, basis: &str, afgewezen: Option<&str>) -> Result<String, String> {
    let s = sessie(basis);
    let uitkomst = match afgewezen {
        None => s.geef().await,
        Some(a) => s.na_weigering(a).await.and_then(|w| w.ok_or(Fout::SessieVerlopen)),
    };
    uitkomst.map_err(|f| {
        if f.sessie_verlopen() {
            sessie_verlopen(app);
        }
        fout(f)
    })
}

/// De sessie is verlopen en vernieuwen lukt niet meer: één melding, en in het overzicht en het statusvenster de vraag
/// om opnieuw in te loggen. Rondes wachten tot er opnieuw is ingelogd (zie [`opnieuw_ingelogd`]).
pub(crate) fn sessie_verlopen(app: &AppHandle) {
    if app.state::<AppState>().activiteit.zet_sessie_verlopen(true) {
        log::warn!("sessie verlopen; opnieuw inloggen nodig");
        melding(app, SESSIE_VERLOPEN);
        activiteit::meld(app);
    }
}

/// Opnieuw ingelogd (of verbonden met een sleutel): de sessie leest de sleutelbos opnieuw, de melding verdwijnt en een
/// gestopte ronde gaat verder.
async fn opnieuw_ingelogd(app: &AppHandle, basis: &str) {
    sessie(basis).vergeet().await;
    let was_verlopen = app.state::<AppState>().activiteit.zet_sessie_verlopen(false);
    activiteit::meld(app);
    if was_verlopen {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(r) = sync_alles(&app).await {
                let _ = app.emit("sync", &r);
            }
        });
    }
}

/// Eén HTTP-client voor de hele app: verbindingen (en TLS) blijven tussen rondes en verzoeken open.
fn http() -> reqwest::Client {
    static HTTP: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    HTTP.get_or_init(openaec_sync::api::nieuwe_http).clone()
}

/// Client voor de verbonden omgeving; de autorisatie komt bij elk verzoek uit de [`sessie`].
async fn client(state: &AppState) -> Result<Client, String> {
    let basis = state.instellingen.lock().unwrap().basis.clone().ok_or("niet verbonden")?;
    if instellingen::lees_sleutel(&basis).is_none() {
        return Err("niet ingelogd; verbind opnieuw".into());
    }
    Ok(Client::met_bron(&basis, sessie(&basis), http()))
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
async fn verbind(app: AppHandle, state: tauri::State<'_, AppState>, basis: String, autorisatie: String) -> Result<Vec<Ruimte>, String> {
    let basis = basis.trim().trim_end_matches('/').to_string();
    let ruimtes = Client::new(&basis, autorisatie.trim()).ruimtes().await.map_err(|e| match e {
        Fout::SessieVerlopen => "Deze API-sleutel wordt niet geaccepteerd. Controleer de sleutel en het geheim.".to_string(),
        e => fout(e),
    })?;
    instellingen::bewaar_sleutel(&basis, autorisatie.trim()).map_err(fout)?;
    onthoud_basis(&state, basis.clone())?;
    opnieuw_ingelogd(&app, &basis).await;
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
    onthoud_basis(&state, basis.clone())?;
    opnieuw_ingelogd(&app, &basis).await;
    Ok(ruimtes)
}

#[tauri::command]
async fn verbreek(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let basis = {
        let mut inst = state.instellingen.lock().unwrap();
        let basis = inst.basis.take();
        if let Some(b) = &basis {
            instellingen::wis_sleutel(b);
        }
        inst.bewaar().map_err(fout)?;
        basis
    };
    if let Some(b) = basis {
        sessie(&b).vergeet().await;
    }
    // Ook de assistent meldt zich af (zijn tokens horen bij hetzelfde account).
    assistent::log_uit(&state).await;
    state.activiteit.zet_sessie_verlopen(false);
    activiteit::meld(&app);
    Ok(())
}

#[tauri::command]
async fn ruimtes(state: tauri::State<'_, AppState>) -> Result<Vec<Ruimte>, String> {
    client(&state).await?.ruimtes().await.map_err(fout)
}

#[tauri::command]
fn koppel(state: tauri::State<'_, AppState>, ruimte: String, ruimte_naam: String, lokaal: String) -> Result<Vec<Koppeling>, String> {
    voeg_koppeling_toe(&state, Koppeling::ruimte(ruimte, ruimte_naam, lokaal))
}

fn voeg_koppeling_toe(state: &AppState, k: Koppeling) -> Result<Vec<Koppeling>, String> {
    std::fs::create_dir_all(&k.lokaal).map_err(fout)?;
    let mut inst = state.instellingen.lock().unwrap();
    if inst.koppelingen.iter().any(|b| Path::new(&b.lokaal) == Path::new(&k.lokaal)) {
        return Err("deze map is al gekoppeld".into());
    }
    inst.koppelingen.push(k);
    inst.bewaar().map_err(fout)?;
    werk_menu_bij(&inst);
    Ok(inst.koppelingen.clone())
}

/// Wat anderen met mij deelden (mappen en bestanden).
#[tauri::command]
async fn gedeeld_met_mij(state: tauri::State<'_, AppState>) -> Result<Vec<Gedeeld>, String> {
    client(&state).await?.gedeeld_met_mij().await.map_err(fout)
}

/// Een map die met mij gedeeld is koppelen aan een lokale map, net als een ruimte: de sync gebruikt de gedeelde map
/// als bovenste map, met de rechten van de deling.
#[tauri::command]
fn koppel_gedeeld(state: tauri::State<'_, AppState>, gedeeld: Gedeeld, lokaal: String) -> Result<Vec<Koppeling>, String> {
    if gedeeld.doel_doctype != "Opslagmap" {
        return Err("alleen een gedeelde map kan gekoppeld worden; een gedeeld bestand open je".into());
    }
    voeg_koppeling_toe(&state, gedeelde_koppeling(gedeeld, lokaal)?)
}

/// De koppeling voor een map die met mij gedeeld is (een gedeeld bestand kan niet).
fn gedeelde_koppeling(gedeeld: Gedeeld, lokaal: String) -> Result<Koppeling, String> {
    if gedeeld.doel_doctype != "Opslagmap" {
        return Err("alleen een gedeelde map kan gekoppeld worden; een gedeeld bestand open je".into());
    }
    let mut k = Koppeling::ruimte(gedeeld.ruimte, gedeeld.naam, lokaal);
    k.deling = Some(gedeeld.deling);
    k.wortel = Some(gedeeld.doel);
    k.rechten = Some(gedeeld.rechten);
    k.gedeeld_door = gedeeld.gedeeld_door_naam.or(gedeeld.gedeeld_door);
    Ok(k)
}

/// Een gedeeld bestand of een gedeelde map openen op `/opslag` in de browser (daar ook een nieuwe versie uploaden).
#[tauri::command]
fn open_gedeeld(app: AppHandle, state: tauri::State<'_, AppState>, deling: String) -> Result<(), String> {
    let basis = state.instellingen.lock().unwrap().basis.clone().ok_or("niet verbonden")?;
    #[allow(deprecated)]
    app.shell().open(format!("{basis}/opslag?deling={deling}"), None).map_err(fout)
}

/// De opslag voor een koppeling: de hele ruimte, of een gedeelde map als bovenste map.
fn opslag_voor(client: Client, k: &Koppeling) -> ClientOpslag {
    match (&k.deling, &k.wortel) {
        (Some(deling), Some(wortel)) => {
            ClientOpslag::gedeeld(client, &k.ruimte, deling, wortel, k.rechten.as_deref().unwrap_or("lezen"))
        }
        _ => ClientOpslag::new(client, &k.ruimte),
    }
}

/// Melding als een gedeelde map niet meer gesynct wordt, bij de foutcode van de server.
fn gestopt_tekst(naam: &str, foutcode: &str) -> String {
    let reden = match foutcode {
        "deling_ingetrokken" => "de deling is ingetrokken",
        "deling_verlopen" => "de deling is verlopen",
        "deling_weg" => "de gedeelde map is verwijderd",
        _ => "de map is niet meer met je gedeeld",
    };
    format!("{naam}: {reden}. De koppeling is gestopt; de bestanden op deze computer blijven staan.")
}

/// Een gedeelde map stopt met syncen (ingetrokken, verlopen, weg): de koppeling gaat eraf, de lokale bestanden blijven
/// staan, met een melding. Na een lopende ronde gaan ook de statusvinkjes eraf.
fn stop_koppeling(app: &AppHandle, lokaal: &str, tekst: &str) {
    let state = app.state::<AppState>();
    {
        let mut inst = state.instellingen.lock().unwrap();
        inst.koppelingen.retain(|k| k.lokaal != lokaal);
        if let Err(e) = inst.bewaar() {
            log::warn!("instellingen bewaren mislukt: {e}");
        }
        werk_menu_bij(&inst);
    }
    activiteit::vergeet_koppeling(app, lokaal);
    let lokaal = lokaal.to_string();
    let _ = tauri::async_runtime::spawn_blocking(move || vinkjes::ontkoppel(&lokaal));
    melding(app, tekst);
    let koppelingen = state.instellingen.lock().unwrap().koppelingen.clone();
    let _ = app.emit("koppelingen", &koppelingen);
}

/// Het rechtermuismenu en het item "OpenAEC" in het navigatiepaneel bijwerken (bij het starten en na elke
/// (ont)koppeling of een nieuwe hoofdmap).
fn werk_menu_bij(inst: &Instellingen) {
    let mappen: Vec<String> = inst.koppelingen.iter().map(|k| k.lokaal.clone()).collect();
    if let Err(e) = verkenner::registreer(&mappen) {
        log::warn!("rechtermuismenu bijwerken mislukt: {e}");
    }
    menupakket::schrijf_menu(inst);
    let koppelingen: Vec<(String, String)> = inst.koppelingen.iter().map(|k| (k.ruimte_naam.clone(), k.lokaal.clone())).collect();
    werk_navigatie_bij(inst.hoofdmap.clone(), koppelingen);
}

/// Het item in het navigatiepaneel op de achtergrond bijwerken (een snelkoppeling maken gaat via COM, en dat hoort
/// niet onder het slot van de instellingen). Komen er snel na elkaar wijzigingen, dan wint de laatste. Mappen die
/// zelf als sync-root geregistreerd zijn, hebben van Windows al een item (zie `navigatie`).
fn werk_navigatie_bij(hoofdmap: Option<String>, koppelingen: Vec<(String, String)>) {
    #[cfg(windows)]
    {
        type Wens = (Option<String>, Vec<(String, String)>);
        static GEWENST: std::sync::Mutex<Option<Wens>> = std::sync::Mutex::new(None);
        static BEZIG: std::sync::Mutex<()> = std::sync::Mutex::new(());
        *GEWENST.lock().unwrap() = Some((hoofdmap, koppelingen));
        std::thread::spawn(|| {
            let _bezig = BEZIG.lock().unwrap_or_else(|e| e.into_inner());
            let Some((hoofdmap, koppelingen)) = GEWENST.lock().unwrap().take() else { return };
            let koppelingen: Vec<navigatie::Koppeling> = koppelingen
                .into_iter()
                .map(|(naam, lokaal)| {
                    let eigen = cloudfilter::eigen_sync_root(&lokaal);
                    (naam, lokaal, eigen)
                })
                .collect();
            let hoofdmap = hoofdmap.as_deref().map(|h| (h, cloudfilter::eigen_sync_root(h)));
            let doel = navigatie::doel(hoofdmap, &koppelingen);
            let icoon = match std::env::current_exe() {
                Ok(exe) => format!("{},0", exe.display()),
                Err(e) => return log::warn!("navigatiepaneel: {e}"),
            };
            let map = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("OpenAEC Storage").join("Verkenner");
            if let Err(e) = navigatie::pas_toe(navigatie::CLSID, hoofdmap::NAAM, &icoon, &doel, &map) {
                log::warn!("item in het navigatiepaneel bijwerken mislukt: {e}");
            }
        });
    }
    #[cfg(not(windows))]
    let _ = (hoofdmap, koppelingen);
}

/// Een map van een ruimte voor de mapselectie, met wat erin staat (alles eronder samen).
#[derive(Serialize)]
struct Mapkeuze {
    pad: String,
    bytes: u64,
    bestanden: u64,
}

/// Alle mappen van een ruimte (voor "Mappen kiezen…"), gesorteerd, met de omvang per map uit de bestanden op de
/// server (de hele ruimte in één verzoek als de server dat kent, anders map voor map).
#[tauri::command]
/// Met `lokaal` die van die koppeling (ook een gedeelde map).
async fn mappen_van(state: tauri::State<'_, AppState>, ruimte: String, lokaal: Option<String>) -> Result<Vec<Mapkeuze>, String> {
    use openaec_sync::motor::Opslag;
    let k = lokaal.and_then(|l| state.instellingen.lock().unwrap().koppelingen.iter().find(|k| k.lokaal == l).cloned());
    let client = client(&state).await?;
    let opslag = match &k {
        Some(k) => opslag_voor(client, k),
        None => ClientOpslag::new(client, &ruimte),
    };
    let bestanden = opslag.lijst().await.map_err(fout)?;
    let mappen = opslag.alle_mappen().await.map_err(fout)?;
    let omvang = openaec_sync::selectie::mapomvang(bestanden.iter().map(|b| (b.pad.as_str(), b.grootte)));
    let alle = openaec_sync::selectie::met_bovenliggende(mappen);
    Ok(alle
        .into_iter()
        .map(|pad| {
            let o = omvang.get(&pad).copied().unwrap_or_default();
            Mapkeuze { pad, bytes: o.bytes, bestanden: o.bestanden }
        })
        .collect())
}

/// Wat er van deze mappen (paden met '/') op deze computer staat: aantal bestanden en bytes (bestanden die alleen
/// online staan, tellen niet mee voor de bytes). Voor de bevestiging bij het uitvinken.
#[tauri::command]
async fn lokale_omvang(lokaal: String, mappen: Vec<String>) -> Result<openaec_sync::selectie::Omvang, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut uit = openaec_sync::selectie::Omvang::default();
        for map in openaec_sync::selectie::opgeschoond(&mappen) {
            let mut te_doen = vec![map.split('/').fold(PathBuf::from(&lokaal), |p, d| p.join(d))];
            while let Some(m) = te_doen.pop() {
                let Ok(items) = std::fs::read_dir(&m) else { continue };
                for item in items.flatten() {
                    match item.file_type() {
                        Ok(t) if t.is_dir() => te_doen.push(item.path()),
                        Ok(t) if t.is_file() => {
                            uit.bestanden += 1;
                            #[cfg(windows)]
                            let online = cloudfilter::attributen(&item.path()).is_some_and(cloudfilter::is_ontwaterd);
                            #[cfg(not(windows))]
                            let online = false;
                            if !online {
                                uit.bytes += item.metadata().map(|m| m.len()).unwrap_or(0);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        uit
    })
    .await
    .map_err(fout)
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

/// Een nieuwe map op de server, in `map` (None = hoofdniveau van de ruimte of de gedeelde map). Bestaat er op die plek
/// al een map met die naam, dan geeft de server die terug (`nieuw` false).
#[tauri::command]
async fn maak_map(
    state: tauri::State<'_, AppState>,
    ruimte: String,
    map: Option<String>,
    deling: Option<String>,
    naam: String,
) -> Result<openaec_sync::api::NieuweMap, String> {
    let naam = naam.trim().to_string();
    if naam.is_empty() {
        return Err("geef de map een naam".into());
    }
    let client = client(&state).await?;
    let map = map.filter(|m| !m.is_empty());
    client.maak_map_in(&ruimte, deling.as_deref(), &naam, map.as_deref()).await.map_err(fout)
}

/// Inhoud van een map in een ruimte op de server (`map` None = hoofdniveau), met per bestand de actieve vergrendeling.
/// Met `deling` in een map die met mij gedeeld is (hoofdniveau = de gedeelde map).
#[tauri::command]
async fn bladeren(
    state: tauri::State<'_, AppState>,
    ruimte: String,
    map: Option<String>,
    deling: Option<String>,
) -> Result<MapInhoud, String> {
    let client = client(&state).await?;
    let map = map.filter(|m| !m.is_empty());
    let inhoud = client.bladeren_in(&ruimte, deling.as_deref(), map.as_deref()).await.map_err(fout)?;

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

/// Mapselectie van een koppeling instellen; de volgende ronde haalt uitgevinkte mappen lokaal weg (wat lokaal gewijzigd
/// is, gaat eerst nog naar de server). `nieuwe_mappen` en `bekende_mappen` (de mappen die de gebruiker zag) bepalen wat
/// er met mappen gebeurt die later op de server verschijnen.
#[tauri::command]
fn zet_selectie(
    state: tauri::State<'_, AppState>,
    lokaal: String,
    uitgesloten: Vec<String>,
    nieuwe_mappen: Option<openaec_sync::selectie::NieuweMappen>,
    bekende_mappen: Option<Vec<String>>,
) -> Result<Vec<Koppeling>, String> {
    let mut inst = state.instellingen.lock().unwrap();
    let k = inst.koppelingen.iter_mut().find(|k| k.lokaal == lokaal).ok_or("deze map is niet gekoppeld")?;
    k.uitgesloten = openaec_sync::selectie::opgeschoond(&uitgesloten);
    if let Some(n) = nieuwe_mappen {
        k.nieuwe_mappen = n;
    }
    if let Some(b) = bekende_mappen {
        k.bekende_mappen = b;
    }
    inst.bewaar().map_err(fout)?;
    Ok(inst.koppelingen.clone())
}

/// Ontkoppelen laat de lokale bestanden staan; de statusvinkjes gaan eraf (na een eventueel lopende ronde).
#[tauri::command]
fn ontkoppel(app: AppHandle, state: tauri::State<'_, AppState>, lokaal: String) -> Result<Vec<Koppeling>, String> {
    let mut inst = state.instellingen.lock().unwrap();
    inst.koppelingen.retain(|k| k.lokaal != lokaal);
    inst.bewaar().map_err(fout)?;
    werk_menu_bij(&inst);
    activiteit::vergeet_koppeling(&app, &lokaal);
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
    // Wachten op opnieuw inloggen (zie `opnieuw_ingelogd`): niet bij elke wijziging opnieuw proberen.
    if state.activiteit.sessie_is_verlopen() {
        return Err(SESSIE_VERLOPEN.into());
    }
    activiteit::begin(app);
    let r = sync_ronde(app, &state, alleen).await;
    activiteit::einde(app, r.as_deref().map_err(String::as_str));
    // De lijst met overgeslagen bestanden voor het statusvenster en het overzicht.
    if let Ok(lijst) = opruimen::overgeslagen(app.state::<AppState>()).await {
        let _ = app.emit("overgeslagen", &lijst);
    }
    // Na elke ronde het opslaggebruik verversen (overzicht en statusvenster luisteren naar "quotum").
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app2.state::<AppState>();
        match haal_quotum(&state).await {
            Ok(q) => {
                let _ = app2.emit("quotum", &q);
            }
            Err(e) => log::debug!("quotum ophalen mislukt: {e}"),
        }
    });
    r
}

async fn haal_quotum(state: &AppState) -> Result<openaec_sync::api::Quotum, String> {
    client(state).await?.quotum().await.map_err(fout)
}

/// Opslaggebruik van de omgeving: gebruikt en limiet (None = geen limiet).
#[tauri::command]
async fn quotum(state: tauri::State<'_, AppState>) -> Result<openaec_sync::api::Quotum, String> {
    haal_quotum(&state).await
}

async fn sync_ronde(
    app: &AppHandle,
    state: &AppState,
    alleen: Option<&std::collections::HashSet<String>>,
) -> Result<Vec<MapRapport>, String> {
    let client = client(state).await?;
    let (koppelingen, sync, op_aanvraag) = {
        let inst = state.instellingen.lock().unwrap();
        let op_aanvraag: std::collections::HashSet<String> =
            inst.koppelingen.iter().filter(|k| inst.in_hoofdmap(k)).map(|k| k.lokaal.clone()).collect();
        (inst.koppelingen.clone(), inst.sync.clone().begrensd(), op_aanvraag)
    };
    let opties = Opties {
        gelijktijdig: sync.gelijktijdig,
        gereserveerd: sync.gereserveerd,
        prullenbak: opruimen::prullenbak(),
        ..Default::default()
    };
    let koppelingen: Vec<Koppeling> = koppelingen.into_iter().filter(|k| alleen.is_none_or(|a| a.contains(&k.lokaal))).collect();
    // Gedeelde mappen: de rechten kunnen veranderd zijn (bijv. van bewerken naar bekijken); volg de server.
    let koppelingen = if koppelingen.iter().any(|k| k.deling.is_some()) {
        werk_rechten_bij(state, koppelingen, client.gedeeld_met_mij().await.ok())
    } else {
        koppelingen
    };
    let mut uit = Vec::new();
    for k in koppelingen {
        let root = Path::new(&k.lokaal);
        if !root.is_dir() {
            // Een verwijderde of niet aangesloten map: niet synchroniseren, anders lijkt alles lokaal verwijderd.
            let fout = Some("de map ontbreekt op deze computer".to_string());
            uit.push(MapRapport { lokaal: k.lokaal.clone(), ruimte_naam: k.ruimte_naam.clone(), rapport: None, fout });
            continue;
        }
        let binnen = opslag_voor(client.clone(), &k).met_gelijktijdig(sync.gelijktijdig);
        let opslag = opaanvraag::Verkenneropslag::new(&binnen, root, op_aanvraag.contains(&k.lokaal), k.nieuwe_mappen, &k.bekende_mappen);
        let waarnemer = activiteit::Koppelwaarnemer::new(app.clone(), k.lokaal.clone());
        let mut r = sync_met_opties(&opslag, root, &k.uitgesloten, &waarnemer, &opties).await;
        // Sessie verlopen en niet meer te vernieuwen: de ronde stopt (ook voor de volgende mappen), met één melding en
        // zonder fouten per bestand. Wat al gelukt was, krijgt hieronder gewoon zijn vinkje.
        let verlopen = match &r {
            Ok(rapport) => rapport.sessie_verlopen,
            Err(f) => f.sessie_verlopen(),
        };
        // Ingetrokken, verlopen of weg: niet elke ronde opnieuw proberen, maar de koppeling netjes stoppen.
        if let Some(code) = r.as_ref().err().and_then(|f| f.deling_gestopt()).filter(|_| k.deling.is_some()) {
            let tekst = gestopt_tekst(&k.ruimte_naam, &code);
            stop_koppeling(app, &k.lokaal, &tekst);
            uit.push(MapRapport { lokaal: k.lokaal, ruimte_naam: k.ruimte_naam, rapport: None, fout: Some(tekst) });
            continue;
        }
        if let Ok(rapport) = &mut r {
            // Wat de motor zelf deed met een lastige naam (opgeruimd, onder een veilige naam): één melding per map.
            if !rapport.meldingen.is_empty() {
                let lijst = rapport.meldingen.iter().take(3).cloned().collect::<Vec<_>>().join("
");
                let meer = rapport.meldingen.len().saturating_sub(3);
                let staart = if meer > 0 { format!("
… en nog {meer}") } else { String::new() };
                melding(app, &format!("{}: bestanden met een naam die niet kan
{lijst}{staart}", k.ruimte_naam));
            }
            let lokaal = k.lokaal.clone();
            let bijgewerkt = std::mem::take(&mut rapport.bijgewerkt);
            let mut foutpaden = std::mem::take(&mut rapport.foutpaden);
            let mut fouten: Vec<(String, String)> = rapport.fouten.iter().filter_map(|f| vinkjes::splits_fout(f, &foutpaden)).collect();
            // Wat wacht na eerdere mislukte pogingen, houdt in de Verkenner zijn foutstatus.
            foutpaden.extend(rapport.wachtend.iter().map(|w| w.pad.clone()));
            fouten.extend(rapport.wachtend.iter().map(|w| (w.pad.clone(), w.reden.clone())));
            let overgeslagen: Vec<(String, String)> = rapport.overgeslagen.iter().map(|o| (o.pad.clone(), o.reden.clone())).collect();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                vinkjes::na_ronde(&lokaal, &bijgewerkt, &foutpaden);
                vinkjes::eigen_staten(&lokaal, &fouten, &overgeslagen);
            })
            .await;
        }
        uit.push(MapRapport {
            lokaal: k.lokaal,
            ruimte_naam: k.ruimte_naam,
            fout: if verlopen { Some(SESSIE_VERLOPEN.to_string()) } else { r.as_ref().err().map(fout) },
            rapport: r.ok(),
        });
        if verlopen {
            sessie_verlopen(app);
            break;
        }
    }
    Ok(uit)
}

/// De rechten van gedeelde koppelingen gelijk zetten met `gedeeld_met_mij` (als dat lukte) en bewaren als er iets
/// veranderde. Staat een deling er niet meer in, dan merkt de ronde dat aan de foutcode van de server.
fn werk_rechten_bij(state: &AppState, mut koppelingen: Vec<Koppeling>, lijst: Option<Vec<Gedeeld>>) -> Vec<Koppeling> {
    let Some(lijst) = lijst else { return koppelingen };
    let mut veranderd = false;
    for k in koppelingen.iter_mut() {
        let nu = k.deling.as_ref().and_then(|d| lijst.iter().find(|g| &g.deling == d)).map(|g| g.rechten.clone());
        if nu.is_some() && nu != k.rechten {
            k.rechten = nu;
            veranderd = true;
        }
    }
    if veranderd {
        let mut inst = state.instellingen.lock().unwrap();
        for k in &koppelingen {
            if let Some(b) = inst.koppelingen.iter_mut().find(|b| b.lokaal == k.lokaal) {
                b.rechten = k.rechten.clone();
            }
        }
        let _ = inst.bewaar();
    }
    koppelingen
}

/// Instellingen van de sync (aantal overdrachten tegelijk).
#[tauri::command]
fn sync_instellingen(state: tauri::State<'_, AppState>) -> SyncInstellingen {
    state.instellingen.lock().unwrap().sync.clone().begrensd()
}

/// Instellingen van de sync wijzigen; gelden vanaf de volgende ronde.
#[tauri::command]
fn zet_sync_instellingen(state: tauri::State<'_, AppState>, sync: SyncInstellingen) -> Result<SyncInstellingen, String> {
    let mut inst = state.instellingen.lock().unwrap();
    inst.sync = sync.begrensd();
    inst.bewaar().map_err(fout)?;
    Ok(inst.sync.clone())
}

#[tauri::command]
async fn sync_nu(app: AppHandle) -> Result<Vec<MapRapport>, String> {
    let r = sync_alles(&app).await?;
    let _ = app.emit("sync", &r);
    Ok(r)
}

/// Koppeling en pad (met '/') voor een lokaal bestand (zie `serverplek`: ook met andere hoofdletters of een kort pad).
fn zoek(state: &AppState, bestand: &str) -> Result<(Koppeling, String), String> {
    let koppelingen = state.instellingen.lock().unwrap().koppelingen.clone();
    serverplek::zoek_in(&koppelingen, bestand).ok_or_else(|| "dit bestand staat niet in een gekoppelde map".into())
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
        let opslag = opslag_voor(client.clone(), &k);
        ("Opslagmap", serverplek::map_op_server(&opslag, Path::new(&k.lokaal), &rel, true).await.map_err(|f| f.melding)?)
    } else {
        ("Opslagbestand", bestand_id(state, pad)?)
    };
    let deling = client.deel(doctype, &id, wachtwoord.as_deref(), verloopt.as_deref()).await.map_err(fout)?;
    Ok(deling.url.unwrap_or_else(|| format!("{}/delen/{}", client.basis(), deling.token.unwrap_or_default())))
}

/// Uploadlink (bestanden opvragen) voor een lokale map in een gekoppelde map.
async fn maak_uploadlink(state: &AppState, pad: &str) -> Result<String, String> {
    if !Path::new(pad).is_dir() {
        return Err("bestanden opvragen kan alleen voor een map".into());
    }
    let client = client(state).await?;
    let (k, rel) = zoek(state, pad)?;
    let opslag = opslag_voor(client.clone(), &k);
    let id = serverplek::map_op_server(&opslag, Path::new(&k.lokaal), &rel, true).await.map_err(|f| f.melding)?;
    let deling = client.vraag_bestanden(&id, None, None).await.map_err(fout)?;
    Ok(deling.url.unwrap_or_else(|| format!("{}/delen/{}", client.basis(), deling.token.unwrap_or_default())))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn melding_bij_een_gestopte_deling() {
        assert_eq!(
            gestopt_tekst("Tekeningen", "deling_ingetrokken"),
            "Tekeningen: de deling is ingetrokken. De koppeling is gestopt; de bestanden op deze computer blijven staan."
        );
        assert!(gestopt_tekst("T", "deling_verlopen").contains("verlopen"));
        assert!(gestopt_tekst("T", "deling_weg").contains("verwijderd"));
        assert!(gestopt_tekst("T", "deling_onbekend").contains("niet meer met je gedeeld"));
    }

    #[test]
    fn opslag_voor_een_gedeelde_koppeling() {
        use openaec_sync::motor::Opslag;
        let client = Client::new("https://voorbeeld.test", "token a:b");
        let mut k = Koppeling::ruimte("R1".into(), "Tekeningen".into(), "C:/T".into());
        assert_eq!(opslag_voor(client.clone(), &k).deling(), None);
        k.deling = Some("D1".into());
        k.wortel = Some("M1".into());
        k.rechten = Some("lezen".into());
        let opslag = opslag_voor(client.clone(), &k);
        assert_eq!(opslag.deling(), Some("D1"));
        assert!(opslag.alleen_lezen());
        k.rechten = Some("beheren".into());
        assert!(!opslag_voor(client, &k).alleen_lezen());
    }
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

/// Actie uit het rechtermuismenu van de Verkenner: "Delen…" opent het deelvenster, "Versturen met OpenAEC" het
/// venster Versturen (met alle paden), "Versies bekijken" het hoofdvenster; de rest gebeurt op de achtergrond met een
/// melding.
pub(crate) async fn voer_actie(app: AppHandle, actie: String, paden: Vec<String>) {
    if verkenner::venster_voor(&actie) == Some(versturen::VENSTER) {
        return versturen::ontvang(&app, paden);
    }
    let Some(pad) = paden.into_iter().next() else { return };
    match verkenner::venster_voor(&actie) {
        Some(delen::VENSTER) => return delen::toon(&app, delen::Deelverzoek { pad: Some(pad), doel: None }),
        Some(_) => {
            systeemvak::toon(&app);
            let _ = app.emit("actie", serde_json::json!({ "actie": actie, "pad": pad }));
            return;
        }
        None => {}
    }
    let naam = Path::new(&pad).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    match actie_uitvoeren(&app, &actie, &pad, &naam).await {
        Ok(t) => melding(&app, &t),
        Err(e) => melding(&app, &format!("{naam}: {e}")),
    }
}

/// Maak van een lokale map een cloud-map (alleen Windows): bestanden van de ruimte verschijnen als
/// placeholder en worden pas opgehaald als je ze opent. Geeft het aantal placeholders terug.
/// Met `gedeeld` een map die met mij gedeeld is: de placeholders komen uit die map (de hydrator downloadt met de
/// rechten van de deling).
#[tauri::command]
async fn maak_cloudmap(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    ruimte: String,
    lokaal: String,
    gedeeld: Option<Gedeeld>,
) -> Result<u32, String> {
    #[cfg(windows)]
    {
        use openaec_sync::motor::Opslag;
        let client = client(&state).await?;
        let opslag = match &gedeeld {
            Some(g) => ClientOpslag::gedeeld(client.clone(), &g.ruimte, &g.deling, &g.doel, &g.rechten),
            None => ClientOpslag::new(client.clone(), &ruimte),
        };
        let bestanden = opslag.lijst().await.map_err(fout)?;
        std::fs::create_dir_all(&lokaal).map_err(fout)?;
        cloudfilter::register(&lokaal, "OpenAEC Storage", env!("CARGO_PKG_VERSION")).map_err(fout)?;
        cloudfilter::set_hydrator(hydrator(app, client.basis().to_string()));
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
        let _ = (&app, &state, &ruimte, &lokaal, &gedeeld);
        Err("cloud-mappen zijn alleen op Windows beschikbaar".into())
    }
}

/// Hydrator voor placeholders: haalt het gevraagde bereik van de huidige versie op via een downloadlink.
/// Draait op de callback-thread van Windows, dus blocking.
#[cfg(windows)]
fn hydrator(app: AppHandle, basis: String) -> impl Fn(&str, i64, i64) -> Vec<u8> + Send + Sync + 'static {
    move |bestand, begin, lengte| {
        let http = reqwest::blocking::Client::new();
        let verzoek = serde_json::json!({ "bestand": bestand });
        let url = format!("{basis}/api/method/openaec_storage.api.download");
        let link: Option<String> = api_blocking(&app, &basis, |autorisatie| http.post(&url).header("Authorization", autorisatie).json(&verzoek).send())
            .ok()
            .and_then(|r| r.json::<serde_json::Value>().ok())
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

/// Een verzoek naar de API vanuit blocking code (de callback-threads van Windows, zonder async runtime): met de
/// autorisatie van de sessie, en na een 401 één keer vernieuwd en opnieuw. Geeft het geslaagde antwoord, of een
/// melding voor mensen (nooit de ruwe JSON van de server).
#[cfg(windows)]
pub(crate) fn api_blocking(
    app: &AppHandle,
    basis: &str,
    verstuur: impl Fn(&str) -> reqwest::Result<reqwest::blocking::Response>,
) -> Result<reqwest::blocking::Response, String> {
    const GEEN_VERBINDING: &str = "Geen verbinding met de server. Controleer je internetverbinding en probeer het opnieuw.";
    let waarde = tauri::async_runtime::block_on(autorisatie(app, basis, None))?;
    let mut antwoord = verstuur(&waarde).map_err(|_| GEEN_VERBINDING.to_string())?;
    if antwoord.status() == reqwest::StatusCode::UNAUTHORIZED {
        let nieuw = tauri::async_runtime::block_on(autorisatie(app, basis, Some(&waarde)))?;
        antwoord = verstuur(&nieuw).map_err(|_| GEEN_VERBINDING.to_string())?;
        if antwoord.status() == reqwest::StatusCode::UNAUTHORIZED {
            sessie_verlopen(app);
            return Err(SESSIE_VERLOPEN.into());
        }
    }
    let status = antwoord.status();
    if !status.is_success() {
        return Err(openaec_sync::serverfout_tekst(status.as_u16(), &antwoord.text().unwrap_or_default()));
    }
    Ok(antwoord)
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
    // Vanuit de installer: het pakket voor het moderne rechtermuismenu registreren of afmelden, zonder de app te starten.
    if let Some(code) = menupakket::opdrachtregel(&std::env::args().collect::<Vec<_>>()) {
        std::process::exit(code);
    }
    // Een selectie van meerdere items start de app per item: wacht zo nodig tot de eerste start klaar is om door te
    // sturen (zie `rechtermuis::wacht_op_eerste_start`).
    #[cfg(windows)]
    if std::env::args().any(|a| a == "--actie") {
        rechtermuis::wacht_op_eerste_start("com.openaec.cloud");
    }
    tauri::Builder::default()
        // Eén exemplaar tegelijk: een tweede start toont het bestaande venster, of geeft zijn actie door.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _| match verkenner::actie_met_paden(&argv) {
            Some((actie, paden)) => {
                tauri::async_runtime::spawn(voer_actie(app.clone(), actie, paden));
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
            deelverzoek: std::sync::Mutex::new(None),
        })
        .manage(versturen::Versturen::default())
        .setup(|app| {
            systeemvak::maak(app)?;
            bewaking::start(app.handle().clone());
            updates::start(app.handle().clone());
            werk_menu_bij(&app.state::<AppState>().instellingen.lock().unwrap());
            // "Versturen met OpenAEC" in het rechtermuismenu, op elk bestand en elke map (het pad naar de app kan
            // veranderd zijn na een update of een verplaatste AppImage).
            let menu_aan = app.state::<AppState>().instellingen.lock().unwrap().menu_versturen_aan();
            // Met het moderne menu van Windows 11 (menupakket) vervalt het klassieke werkwoord.
            tauri::async_runtime::spawn_blocking(move || menupakket::bij_start(menu_aan));
            activiteit::meld(app.handle());
            // Gestart vanuit het rechtermuismenu terwijl de app nog niet draaide.
            let argv: Vec<String> = std::env::args().collect();
            if let Some((actie, paden)) = verkenner::actie_met_paden(&argv) {
                tauri::async_runtime::spawn(voer_actie(app.handle().clone(), actie, paden));
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
            gedeeld_met_mij,
            koppel_gedeeld,
            open_gedeeld,
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
            lokale_omvang,
            zet_selectie,
            hoofdmap::hoofdmap_status,
            hoofdmap::zet_hoofdmap,
            hoofdmap::koppel_in_hoofdmap,
            hoofdmap::koppel_gedeeld_in_hoofdmap,
            hoofdmap::verplaats_naar_hoofdmap,
            bladeren,
            maak_map,
            overzicht,
            open_map,
            open_beheer,
            versie,
            controleer_updates,
            installeer_update,
            activiteit_stand,
            pauzeer,
            toon_hoofdvenster,
            quotum,
            sync_instellingen,
            zet_sync_instellingen,
            opruimen::overgeslagen,
            opruimen::ruim_overgeslagen_op,
            delen::open_delen,
            delen::sluit_delen,
            delen::delen_verzoek,
            delen::deeldoel,
            delen::deel_met_collega,
            delen::maak_link,
            delen::delingen_van,
            delen::trek_deling_in,
            delen::wijzig_deling,
            delen::toegang_van,
            delen::ruimteleden,
            delen::collegas,
            delen::kopieer,
            versturen::open_versturen,
            versturen::sluit_versturen,
            versturen::versturen_verzoek,
            versturen::versturen_items,
            versturen::versturen_grenzen,
            versturen::start_versturen,
            versturen::onderbroken_versturen,
            versturen::hervat_versturen,
            versturen::stop_versturen,
            versturen::mijn_overdrachten,
            versturen::trek_overdracht_in,
            versturen::menu_versturen,
            versturen::zet_menu_versturen,
            tools::tools_geinstalleerd,
            tools::tools_releases,
            tools::tool_installeren,
            tools::tool_starten,
            tools::tool_verwijderen,
            assistent::assistent_status,
            assistent::assistent_log_in,
            assistent::assistent_modellen,
            assistent::assistent_vraag,
            assistent::assistent_stop,
            assistent::assistent_bijlage,
            assistent::assistent_bijlage_data,
            assistent::assistent_gesprekken,
            assistent::assistent_gesprek,
            assistent::assistent_bewaar,
            assistent::assistent_verwijder,
            assistent::assistent_open_beheer
        ])
        .run(tauri::generate_context!())
        .expect("fout bij het starten van OpenAEC Desktop App");
}
