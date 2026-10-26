//! Venster "Versturen": een of meer bestanden of mappen versturen als overdracht, met een downloadlink en eventueel
//! een e-mail aan de ontvangers (`openaec_storage.overdracht` op de server). Het venster opent vanuit "Versturen met
//! OpenAEC" in het rechtermuismenu van de bestandsbeheerder (zie `rechtermuis`), op elk bestand en elke map, ook
//! buiten de gekoppelde mappen.
//!
//! Een selectie van meerdere items start de app in de Verkenner per item; de paden die vlak na elkaar binnenkomen,
//! gaan samen naar het venster ([`ontvang`]). Een map gaat met al zijn inhoud mee als zip (een naam op de server kan
//! geen pad bevatten). De bestanden gaan parallel, elk in delen; de stand staat in `versturen.json`, zodat een
//! onderbroken overdracht (geen verbinding, app afgesloten) verder kan waar hij was ([`hervat_versturen`]).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

use futures_util::stream::{self, StreamExt};
use openaec_sync::overdracht::{self, Grenzen, NieuweOverdracht, OverdrachtRegel, Voltooid};
use openaec_sync::{Client, Fout};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::delen::DeelFout;
use crate::{client, melding, systeemvak, AppState};

pub const VENSTER: &str = "versturen";

/// Zoveel bestanden gaan tegelijk; hun delen delen samen de plekken van de sync-instelling "overdrachten tegelijk".
const BESTANDEN_TEGELIJK: usize = 4;

/// Wacht zo lang op meer paden van dezelfde selectie voordat het venster ze krijgt.
const VERZAMELEN: Duration = Duration::from_millis(400);

/// Toestand van het venster in de app.
#[derive(Default)]
pub struct Versturen {
    /// Paden die net binnenkwamen, en een teller om te zien of er sindsdien meer kwamen.
    binnen: Mutex<(Vec<String>, u64)>,
    /// Het laatste verzoek aan het venster (het venster leest het bij het openen).
    verzoek: Mutex<Vec<String>>,
    /// De overdracht die nu loopt.
    taak: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

/// Paden uit het rechtermuismenu: verzamelen en dan samen naar het venster.
pub fn ontvang(app: &AppHandle, paden: Vec<String>) {
    let st = app.state::<Versturen>();
    let nummer = {
        let mut binnen = st.binnen.lock().unwrap();
        for p in paden {
            if !binnen.0.contains(&p) {
                binnen.0.push(p);
            }
        }
        binnen.1 += 1;
        binnen.1
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(VERZAMELEN).await;
        let paden = {
            let st = app.state::<Versturen>();
            let mut binnen = st.binnen.lock().unwrap();
            if binnen.1 != nummer {
                return;
            }
            std::mem::take(&mut binnen.0)
        };
        toon(&app, paden);
    });
}

/// Het venster tonen met deze paden (leeg: alleen tonen).
pub fn toon(app: &AppHandle, paden: Vec<String>) {
    if !paden.is_empty() {
        *app.state::<Versturen>().verzoek.lock().unwrap() = paden.clone();
    }
    systeemvak::verberg_status(app);
    if let Some(venster) = app.get_webview_window(VENSTER) {
        if !venster.is_visible().unwrap_or(false) {
            let _ = venster.center();
        }
        let _ = venster.unminimize();
        let _ = venster.show();
        let _ = venster.set_focus();
    }
    if !paden.is_empty() {
        let _ = app.emit_to(VENSTER, "versturen-verzoek", &paden);
    }
}

/// Het venster openen (vanuit het hoofdvenster), eventueel met paden.
#[tauri::command]
pub fn open_versturen(app: AppHandle, paden: Option<Vec<String>>) {
    toon(&app, paden.unwrap_or_default());
}

#[tauri::command]
pub fn sluit_versturen(app: AppHandle) {
    if let Some(venster) = app.get_webview_window(VENSTER) {
        let _ = venster.hide();
    }
}

/// Het laatste verzoek (de paden die het venster moet tonen).
#[tauri::command]
pub fn versturen_verzoek(st: tauri::State<'_, Versturen>) -> Vec<String> {
    st.verzoek.lock().unwrap().clone()
}

/// Een gekozen bestand of map, zoals het venster hem toont.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub pad: String,
    pub naam: String,
    pub map: bool,
    /// Bytes (bij een map alles erin samen).
    pub grootte: u64,
    /// Aantal bestanden (1 bij een bestand).
    pub bestanden: u64,
    /// Bestaat niet (meer), of is niet te lezen.
    pub fout: Option<String>,
}

fn naam_van(pad: &Path) -> String {
    pad.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| pad.to_string_lossy().to_string())
}

/// Wat er in elk pad zit (de omvang van een map wordt geteld).
pub fn item(pad: &str) -> Item {
    let p = Path::new(pad);
    let mut item = Item { pad: pad.to_string(), naam: naam_van(p), map: false, grootte: 0, bestanden: 0, fout: None };
    match std::fs::metadata(p) {
        Ok(m) if m.is_dir() => {
            item.map = true;
            match overdracht::mapomvang(p) {
                Ok((aantal, bytes)) => (item.bestanden, item.grootte) = (aantal, bytes),
                Err(e) => item.fout = Some(format!("niet te lezen: {e}")),
            }
        }
        Ok(m) => (item.bestanden, item.grootte) = (1, m.len()),
        Err(_) => item.fout = Some("bestaat niet (meer)".into()),
    }
    item
}

#[tauri::command]
pub async fn versturen_items(paden: Vec<String>) -> Result<Vec<Item>, String> {
    tauri::async_runtime::spawn_blocking(move || paden.iter().map(|p| item(p)).collect()).await.map_err(|e| e.to_string())
}

/// Een fout van de server vertalen naar een vaste code en een melding voor het venster.
pub fn versturenfout(f: &Fout) -> DeelFout {
    let melding = |code, tekst: &str| DeelFout { code, melding: tekst.to_string() };
    if let Fout::Netwerk(_) = f {
        return melding("geen_verbinding", "Geen verbinding met de server. Controleer je internetverbinding en probeer het opnieuw.");
    }
    match f.status() {
        Some(401) => melding("niet_ingelogd", "Je sessie is verlopen. Log opnieuw in."),
        Some(403) => melding(
            "geen_recht",
            "Je hebt geen recht om bestanden te versturen. Dat kan wie schrijver of beheerder is in een ruimte; vraag het een beheerder van de omgeving.",
        ),
        Some(404) => melding("niet_gevonden", "Deze overdracht bestaat niet (meer) op de server."),
        Some(502..=504) => melding("geen_verbinding", "De server is tijdelijk niet bereikbaar. Probeer het straks opnieuw."),
        _ => DeelFout { code: "fout", melding: f.servermelding().unwrap_or_else(|| f.to_string()) },
    }
}

impl From<Fout> for VersturenFout {
    fn from(f: Fout) -> Self {
        VersturenFout(versturenfout(&f))
    }
}

/// Een fout voor het venster (als [`DeelFout`]); zonder omgeving of inlog de code `niet_ingelogd`.
#[derive(Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct VersturenFout(pub DeelFout);

impl From<String> for VersturenFout {
    fn from(melding: String) -> Self {
        VersturenFout(DeelFout { code: "fout", melding })
    }
}

impl From<&str> for VersturenFout {
    fn from(melding: &str) -> Self {
        melding.to_string().into()
    }
}

async fn verbinding(state: &AppState) -> Result<Client, VersturenFout> {
    client(state).await.map_err(|e| {
        let melding = if e.contains("niet verbonden") {
            "Er is nog geen omgeving gekoppeld. Log in om bestanden te versturen.".to_string()
        } else {
            format!("Je bent niet (meer) ingelogd. Log in om bestanden te versturen ({e}).")
        };
        VersturenFout(DeelFout { code: "niet_ingelogd", melding })
    })
}

/// De grenzen van de server; meteen ook de test of er een omgeving is, of je ingelogd bent en mag versturen.
#[tauri::command]
pub async fn versturen_grenzen(state: tauri::State<'_, AppState>) -> Result<Grenzen, VersturenFout> {
    Ok(verbinding(&state).await?.overdracht_grenzen().await?)
}

/// Eén bestand van de overdracht zoals de app hem bijhoudt (voor hervatten).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Onderdeel {
    /// Het gekozen pad (een bestand of een map).
    pub pad: String,
    /// De naam op de server (bij een map `<map>.zip`).
    pub naam: String,
    pub map: bool,
    /// Bytes van wat er geüpload wordt (bij een map: de zip, zodra die er is).
    pub grootte: u64,
    /// Wijzigingstijd van het bestand (seconden), om te zien of het sinds een onderbreking veranderd is.
    #[serde(default)]
    pub gewijzigd: Option<u64>,
    /// De zip van een map (in de cachemap van de app).
    #[serde(default)]
    pub zip: Option<String>,
    /// Id van het bestand in de overdracht, zodra het aangekondigd is.
    #[serde(default)]
    pub bestand: Option<String>,
    #[serde(default)]
    pub klaar: bool,
}

/// Een overdracht die bezig is (of onderbroken), zoals hij in `versturen.json` staat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lopend {
    pub overdracht: String,
    pub naam: String,
    /// De omgeving waar hij bij hoort.
    pub basis: String,
    pub onderdelen: Vec<Onderdeel>,
    #[serde(default)]
    pub concept_geldig_tot: Option<String>,
}

fn standbestand() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("OpenAEC Storage").join("versturen.json")
}

fn zipmap(overdracht: &str) -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("OpenAEC Storage").join("versturen").join(overdracht)
}

fn lees_lopend() -> Option<Lopend> {
    std::fs::read(standbestand()).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn bewaar_lopend(l: &Lopend) {
    let pad = standbestand();
    let _ = std::fs::create_dir_all(pad.parent().unwrap());
    if let Err(e) = std::fs::write(&pad, serde_json::to_vec_pretty(l).unwrap()) {
        log::warn!("stand van de overdracht bewaren: {e}");
    }
}

/// De bewaarde stand en de zips van deze overdracht weg.
fn wis_lopend(overdracht: &str) {
    if lees_lopend().is_some_and(|l| l.overdracht == overdracht) {
        let _ = std::fs::remove_file(standbestand());
    }
    let _ = std::fs::remove_dir_all(zipmap(overdracht));
}

fn wijzigingstijd(pad: &Path) -> Option<u64> {
    std::fs::metadata(pad).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// De onderdelen voor deze paden: een bestand met zijn veilige naam, een map als `<map>.zip`.
pub fn onderdelen(paden: &[String]) -> Result<Vec<Onderdeel>, String> {
    let mut uit: Vec<Onderdeel> = Vec::new();
    for pad in paden {
        let p = Path::new(pad);
        let m = std::fs::metadata(p).map_err(|_| format!("{} bestaat niet (meer)", naam_van(p)))?;
        let veilig = openaec_sync::namen::veilige_naam(&naam_van(p));
        let onderdeel = if m.is_dir() {
            Onderdeel { pad: pad.clone(), naam: format!("{veilig}.zip"), map: true, grootte: 0, gewijzigd: None, zip: None, bestand: None, klaar: false }
        } else {
            Onderdeel { pad: pad.clone(), naam: veilig, map: false, grootte: m.len(), gewijzigd: wijzigingstijd(p), zip: None, bestand: None, klaar: false }
        };
        if !uit.iter().any(|o| o.pad == onderdeel.pad) {
            uit.push(onderdeel);
        }
    }
    if uit.is_empty() {
        return Err("kies een of meer bestanden of mappen".into());
    }
    Ok(uit)
}

/// Wat het venster invult.
#[derive(Debug, Clone, Deserialize)]
pub struct Opdracht {
    pub paden: Vec<String>,
    pub naam: String,
    #[serde(default)]
    pub bericht: Option<String>,
    #[serde(default)]
    pub verloopt_op: Option<String>,
    #[serde(default)]
    pub wachtwoord: Option<String>,
    #[serde(default)]
    pub ontvangers: Vec<String>,
}

/// Voortgang van één onderdeel voor het venster.
#[derive(Debug, Clone, Serialize)]
struct Stap {
    overdracht: String,
    index: usize,
    /// `wachten`, `inpakken`, `bezig`, `klaar` of `fout`.
    status: &'static str,
    gedaan: u64,
    totaal: u64,
    fout: Option<String>,
}

fn meld_stap(app: &AppHandle, overdracht: &str, index: usize, status: &'static str, gedaan: u64, totaal: u64, fout: Option<String>) {
    let stap = Stap { overdracht: overdracht.to_string(), index, status, gedaan, totaal, fout };
    let _ = app.emit_to(VENSTER, "versturen-stap", &stap);
}

/// Een nieuwe overdracht maken en het versturen starten; geeft de stand (met de onderdelen in de volgorde van de
/// paden). Een eerder onderbroken overdracht wordt eerst ingetrokken.
#[tauri::command]
pub async fn start_versturen(app: AppHandle, state: tauri::State<'_, AppState>, opdracht: Opdracht) -> Result<Lopend, VersturenFout> {
    if app.state::<Versturen>().taak.lock().unwrap().as_ref().is_some_and(|t| !t.inner().is_finished()) {
        return Err("er loopt al een overdracht; wacht tot die klaar is of stop hem".into());
    }
    let paden = opdracht.paden.clone();
    let delen = tauri::async_runtime::spawn_blocking(move || onderdelen(&paden)).await.map_err(|e| e.to_string())??;
    let client = verbinding(&state).await?;
    if let Some(oud) = lees_lopend() {
        let _ = client.trek_overdracht_in(&oud.overdracht).await;
        wis_lopend(&oud.overdracht);
    }
    let naam = match opdracht.naam.trim() {
        "" => delen[0].naam.clone(),
        n => n.to_string(),
    };
    let concept = client
        .maak_overdracht(&NieuweOverdracht {
            naam,
            bericht: opdracht.bericht,
            verloopt_op: opdracht.verloopt_op,
            wachtwoord: opdracht.wachtwoord,
            ontvangers: opdracht.ontvangers,
        })
        .await?;
    let lopend = Lopend {
        overdracht: concept.overdracht,
        naam: concept.naam,
        basis: client.basis().to_string(),
        onderdelen: delen,
        concept_geldig_tot: concept.concept_geldig_tot,
    };
    bewaar_lopend(&lopend);
    begin(&app, lopend.clone(), false);
    Ok(lopend)
}

/// Een onderbroken overdracht (van deze omgeving), als die er is en niet al loopt.
#[tauri::command]
pub fn onderbroken_versturen(app: AppHandle, state: tauri::State<'_, AppState>) -> Option<Lopend> {
    if app.state::<Versturen>().taak.lock().unwrap().as_ref().is_some_and(|t| !t.inner().is_finished()) {
        return None;
    }
    let basis = state.instellingen.lock().unwrap().basis.clone()?;
    lees_lopend().filter(|l| l.basis == basis)
}

/// Een onderbroken overdracht verder zetten waar hij was.
#[tauri::command]
pub fn hervat_versturen(app: AppHandle) -> Result<Lopend, VersturenFout> {
    if app.state::<Versturen>().taak.lock().unwrap().as_ref().is_some_and(|t| !t.inner().is_finished()) {
        return Err("deze overdracht loopt al".into());
    }
    let lopend = lees_lopend().ok_or("er is geen onderbroken overdracht")?;
    begin(&app, lopend.clone(), true);
    Ok(lopend)
}

/// Stoppen. Met `intrekken` gaat de overdracht ook op de server weg (en de bewaarde stand); anders kan hij later
/// verder.
#[tauri::command]
pub async fn stop_versturen(app: AppHandle, state: tauri::State<'_, AppState>, intrekken: bool) -> Result<(), VersturenFout> {
    if let Some(taak) = app.state::<Versturen>().taak.lock().unwrap().take() {
        taak.abort();
    }
    if intrekken {
        if let Some(l) = lees_lopend() {
            let r = verbinding(&state).await?.trek_overdracht_in(&l.overdracht).await;
            wis_lopend(&l.overdracht);
            r?;
        }
    }
    Ok(())
}

fn begin(app: &AppHandle, lopend: Lopend, hervat: bool) {
    let app2 = app.clone();
    let taak = tauri::async_runtime::spawn(async move {
        let overdracht = lopend.overdracht.clone();
        let stand = Mutex::new(lopend);
        let uitkomst = uitvoeren(&app2, &stand, hervat).await;
        match uitkomst {
            Ok(v) => {
                wis_lopend(&overdracht);
                use tauri_plugin_clipboard_manager::ClipboardExt;
                let _ = app2.clipboard().write_text(v.url.clone());
                let tekst = if v.mail.is_empty() {
                    "Verstuurd: de downloadlink staat op het klembord.".to_string()
                } else {
                    format!("Verstuurd naar {}; de downloadlink staat ook op het klembord.", v.mail.join(", "))
                };
                melding(&app2, &tekst);
                let _ = app2.emit_to(VENSTER, "versturen-klaar", &v);
            }
            Err(f) => {
                // Weg op de server (verlopen, ingetrokken): niet meer te hervatten.
                if matches!(f.0.code, "niet_gevonden" | "verlopen") {
                    wis_lopend(&overdracht);
                } else {
                    bewaar_lopend(&stand.lock().unwrap());
                }
                let _ = app2.emit_to(VENSTER, "versturen-fout", serde_json::json!({ "overdracht": overdracht, "fout": f.0 }));
            }
        }
    });
    *app.state::<Versturen>().taak.lock().unwrap() = Some(taak);
}

/// Hervatten: de stand naast die van de server leggen. Bestanden die de server niet (meer) kent of die lokaal
/// veranderd zijn, beginnen opnieuw; bestanden op de server die de app niet kent (aangekondigd vlak voor een
/// onderbreking), gaan weg.
async fn vergelijk_met_server(client: &Client, stand: &Mutex<Lopend>) -> Result<Option<Voltooid>, VersturenFout> {
    let overdracht = stand.lock().unwrap().overdracht.clone();
    let info = client.overdracht_info(&overdracht).await?;
    match info.status.as_str() {
        "Concept" => {}
        "Actief" => {
            return Ok(Some(Voltooid {
                overdracht: info.overdracht,
                status: info.status,
                token: None,
                url: info.url.unwrap_or_default(),
                verloopt_op: info.verloopt_op,
                aantal_bestanden: info.bestanden.len() as u64,
                totale_grootte: info.totale_grootte,
                mail: Vec::new(),
            }))
        }
        status => {
            return Err(VersturenFout(DeelFout {
                code: "verlopen",
                melding: format!(
                    "Deze overdracht is {} en kan niet meer verder. Verstuur de bestanden opnieuw.",
                    status.to_lowercase()
                ),
            }))
        }
    }
    let bekend: Vec<String> = stand.lock().unwrap().onderdelen.iter().filter_map(|o| o.bestand.clone()).collect();
    for b in info.bestanden.iter().filter(|b| !bekend.contains(&b.bestand)) {
        client.overdracht_bestand_weg(&overdracht, &b.bestand).await?;
    }
    let mut weg = Vec::new();
    {
        let mut l = stand.lock().unwrap();
        for o in l.onderdelen.iter_mut() {
            let op_server = o.bestand.as_ref().and_then(|id| info.bestanden.iter().find(|b| &b.bestand == id));
            o.klaar = op_server.is_some_and(|b| b.klaar);
            if op_server.is_none() {
                o.bestand = None;
            }
            if o.klaar {
                continue;
            }
            let veranderd = if o.map {
                o.zip.as_ref().is_none_or(|z| std::fs::metadata(z).map(|m| m.len()) .ok() != Some(o.grootte))
            } else {
                let p = Path::new(&o.pad);
                let nu = (std::fs::metadata(p).map(|m| m.len()).ok(), wijzigingstijd(p));
                nu != (Some(o.grootte), o.gewijzigd)
            };
            if veranderd {
                if let Some(id) = o.bestand.take() {
                    weg.push(id);
                }
                if o.map {
                    o.zip = None;
                    o.grootte = 0;
                } else {
                    let p = Path::new(&o.pad);
                    o.grootte = std::fs::metadata(p).map(|m| m.len()).map_err(|_| format!("{} bestaat niet meer", o.naam))?;
                    o.gewijzigd = wijzigingstijd(p);
                }
            }
        }
    }
    for id in weg {
        client.overdracht_bestand_weg(&overdracht, &id).await?;
    }
    bewaar_lopend(&stand.lock().unwrap());
    Ok(None)
}

async fn uitvoeren(app: &AppHandle, stand: &Mutex<Lopend>, hervat: bool) -> Result<Voltooid, VersturenFout> {
    let state = app.state::<AppState>();
    let client = verbinding(&state).await?;
    let overdracht = stand.lock().unwrap().overdracht.clone();
    if hervat {
        if let Some(klaar) = vergelijk_met_server(&client, stand).await? {
            return Ok(klaar);
        }
    }
    let aantal = stand.lock().unwrap().onderdelen.len();
    for i in 0..aantal {
        let o = stand.lock().unwrap().onderdelen[i].clone();
        meld_stap(app, &overdracht, i, if o.klaar { "klaar" } else { "wachten" }, if o.klaar { o.grootte } else { 0 }, o.grootte, None);
    }

    // Mappen inpakken (een voor een; elke zip staat in de cachemap tot de overdracht klaar is).
    for i in 0..aantal {
        let o = stand.lock().unwrap().onderdelen[i].clone();
        if !o.map || o.klaar || o.zip.as_ref().is_some_and(|z| Path::new(z).is_file()) {
            continue;
        }
        meld_stap(app, &overdracht, i, "inpakken", 0, 0, None);
        let doel = zipmap(&overdracht).join(format!("{i}-{}", o.naam));
        let bron = PathBuf::from(&o.pad);
        let doel2 = doel.clone();
        let r = tauri::async_runtime::spawn_blocking(move || {
            std::fs::create_dir_all(doel2.parent().unwrap())?;
            overdracht::pak_map(&bron, &doel2)?;
            std::fs::metadata(&doel2).map(|m| m.len())
        })
        .await
        .map_err(|e| e.to_string())?;
        let grootte = r.map_err(|e| {
            meld_stap(app, &overdracht, i, "fout", 0, 0, Some(e.to_string()));
            format!("{} inpakken: {e}", o.naam)
        })?;
        let mut l = stand.lock().unwrap();
        l.onderdelen[i].zip = Some(doel.to_string_lossy().to_string());
        l.onderdelen[i].grootte = grootte;
        bewaar_lopend(&l);
        drop(l);
        meld_stap(app, &overdracht, i, "wachten", 0, grootte, None);
    }

    let gelijktijdig = state.instellingen.lock().unwrap().sync.clone().begrensd().gelijktijdig;
    let rem = openaec_sync::rem(gelijktijdig);
    let open: Vec<usize> = (0..aantal).filter(|&i| !stand.lock().unwrap().onderdelen[i].klaar).collect();
    let taken: Vec<_> = open.into_iter().map(|i| verstuur_onderdeel(app, &client, stand, &overdracht, i, &rem)).collect();
    let resultaten: Vec<Result<(), VersturenFout>> = stream::iter(taken).buffer_unordered(BESTANDEN_TEGELIJK).collect().await;
    if let Some(f) = resultaten.into_iter().find_map(Result::err) {
        return Err(f);
    }
    Ok(client.voltooi_overdracht(&overdracht).await?)
}

/// Eén onderdeel uploaden, met de voortgang naar het venster (hoogstens een paar keer per seconde).
async fn verstuur_onderdeel(
    app: &AppHandle,
    client: &Client,
    stand: &Mutex<Lopend>,
    overdracht: &str,
    index: usize,
    rem: &openaec_sync::Rem,
) -> Result<(), VersturenFout> {
    let o = stand.lock().unwrap().onderdelen[index].clone();
    let bron = PathBuf::from(o.zip.as_deref().unwrap_or(&o.pad));
    let laatst = Mutex::new(Instant::now() - Duration::from_secs(1));
    let voortgang = |gedaan: u64, totaal: u64| {
        let mut l = laatst.lock().unwrap();
        if gedaan >= totaal || l.elapsed() >= Duration::from_millis(150) {
            *l = Instant::now();
            meld_stap(app, overdracht, index, "bezig", gedaan, totaal, None);
        }
    };
    let bij_start = |id: &str| {
        let mut l = stand.lock().unwrap();
        l.onderdelen[index].bestand = Some(id.to_string());
        bewaar_lopend(&l);
    };
    let mut r =
        overdracht::verstuur_bestand(client, overdracht, &bron, &o.naam, o.bestand.as_deref(), rem, &bij_start, &voortgang).await;
    // Sinds de onderbreking veranderd: weghalen en opnieuw beginnen.
    if let (Err(Fout::Controle(_)), Some(id)) = (&r, &o.bestand) {
        client.overdracht_bestand_weg(overdracht, id).await?;
        r = overdracht::verstuur_bestand(client, overdracht, &bron, &o.naam, None, rem, &bij_start, &voortgang).await;
    }
    match r {
        Ok(_) => {
            let mut l = stand.lock().unwrap();
            l.onderdelen[index].klaar = true;
            bewaar_lopend(&l);
            drop(l);
            meld_stap(app, overdracht, index, "klaar", o.grootte, o.grootte, None);
            Ok(())
        }
        Err(f) => {
            let fout = versturenfout(&f);
            meld_stap(app, overdracht, index, "fout", 0, o.grootte, Some(fout.melding.clone()));
            Err(VersturenFout(fout))
        }
    }
}

/// De eigen overdrachten, nieuwste eerst.
#[tauri::command]
pub async fn mijn_overdrachten(state: tauri::State<'_, AppState>) -> Result<Vec<OverdrachtRegel>, VersturenFout> {
    Ok(verbinding(&state).await?.mijn_overdrachten().await?)
}

/// Een overdracht intrekken: de link werkt niet meer. Is het de overdracht die nu loopt of onderbroken is, dan stopt
/// die ook.
#[tauri::command]
pub async fn trek_overdracht_in(app: AppHandle, state: tauri::State<'_, AppState>, overdracht: String) -> Result<String, VersturenFout> {
    if lees_lopend().is_some_and(|l| l.overdracht == overdracht) {
        if let Some(taak) = app.state::<Versturen>().taak.lock().unwrap().take() {
            taak.abort();
        }
        wis_lopend(&overdracht);
    }
    Ok(verbinding(&state).await?.trek_overdracht_in(&overdracht).await?)
}

/// Staat "Versturen met OpenAEC" in het rechtermuismenu?
#[tauri::command]
pub fn menu_versturen(state: tauri::State<'_, AppState>) -> bool {
    state.instellingen.lock().unwrap().menu_versturen_aan()
}

/// "Versturen met OpenAEC" in het rechtermuismenu aan- of uitzetten.
#[tauri::command]
pub async fn zet_menu_versturen(state: tauri::State<'_, AppState>, aan: bool) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || crate::rechtermuis::werk_bij(aan)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    let mut inst = state.instellingen.lock().unwrap();
    inst.menu_versturen = Some(aan);
    inst.bewaar().map_err(|e| e.to_string())?;
    Ok(aan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn server(status: u16) -> Fout {
        Fout::Server { status, tekst: json!({}).to_string() }
    }

    #[test]
    fn fouten_krijgen_een_vaste_code() {
        assert_eq!(versturenfout(&server(403)).code, "geen_recht");
        assert_eq!(versturenfout(&server(401)).code, "niet_ingelogd");
        assert_eq!(versturenfout(&server(404)).code, "niet_gevonden");
        assert_eq!(versturenfout(&server(503)).code, "geen_verbinding");
        let grens = Fout::Server {
            status: 417,
            tekst: json!({"exception": "frappe.exceptions.ValidationError: Een overdracht kan hoogstens 100 GB groot zijn"}).to_string(),
        };
        assert_eq!(versturenfout(&grens), DeelFout { code: "fout", melding: "Een overdracht kan hoogstens 100 GB groot zijn".into() });
    }

    #[test]
    fn onderdelen_uit_bestanden_en_mappen() {
        let werk = std::env::temp_dir().join(format!("openaec-versturen-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&werk);
        std::fs::create_dir_all(werk.join("Tekeningen")).unwrap();
        std::fs::write(werk.join("Tekeningen").join("a.dwg"), b"abc").unwrap();
        std::fs::write(werk.join("con.txt"), b"").unwrap();
        let pad = |p: &str| werk.join(p).to_string_lossy().to_string();
        let o = onderdelen(&[pad("Tekeningen"), pad("con.txt"), pad("Tekeningen")]).unwrap();
        assert_eq!(o.len(), 2, "een dubbel pad gaat er één keer in");
        assert_eq!((o[0].naam.as_str(), o[0].map), ("Tekeningen.zip", true));
        // Een naam die op Windows niet kan, krijgt een veilige naam (zoals de server eist).
        assert_eq!((o[1].naam.as_str(), o[1].grootte, o[1].map), ("con_.txt", 0, false));
        assert!(onderdelen(&[pad("bestaat-niet")]).unwrap_err().contains("bestaat niet"));
        assert!(onderdelen(&[]).is_err());

        let i = item(&pad("Tekeningen"));
        assert_eq!((i.map, i.bestanden, i.grootte, i.fout), (true, 1, 3, None));
        assert!(item(&pad("weg")).fout.is_some());
        let _ = std::fs::remove_dir_all(&werk);
    }

    #[test]
    fn stand_terug_te_lezen() {
        let l = Lopend {
            overdracht: "OV1".into(),
            naam: "Tekeningen".into(),
            basis: "https://x".into(),
            onderdelen: vec![Onderdeel {
                pad: "C:\\a.pdf".into(),
                naam: "a.pdf".into(),
                map: false,
                grootte: 3,
                gewijzigd: Some(1),
                zip: None,
                bestand: Some("B1".into()),
                klaar: false,
            }],
            concept_geldig_tot: None,
        };
        let terug: Lopend = serde_json::from_slice(&serde_json::to_vec(&l).unwrap()).unwrap();
        assert_eq!(terug, l);
    }
}
