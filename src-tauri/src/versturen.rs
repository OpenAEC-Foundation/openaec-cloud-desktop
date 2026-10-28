//! Venster "Versturen": een of meer bestanden of mappen versturen als overdracht, met een downloadlink en eventueel
//! een e-mail aan de ontvangers (`openaec_storage.overdracht` op de server). Het venster opent vanuit "Versturen met
//! OpenAEC" in het rechtermuismenu van de bestandsbeheerder (zie `rechtermuis`), op elk bestand en elke map, ook
//! buiten de gekoppelde mappen.
//!
//! Een selectie van meerdere items start de app in de Verkenner per item; de paden die vlak na elkaar binnenkomen,
//! gaan samen naar het venster ([`ontvang`]). Een map gaat met al zijn inhoud mee als zip (een naam op de server kan
//! geen pad bevatten). De bestanden gaan parallel, elk in delen; de stand staat in `versturen.json`, zodat een
//! onderbroken overdracht (geen verbinding, app afgesloten) verder kan waar hij was ([`hervat_versturen`]).
//!
//! Wat al in OpenAEC staat, gaat niet opnieuw over de lijn: een bestand of map in een gekoppelde map die helemaal in
//! sync is (lokaal gelijk aan de laatste ronde), kopieert de server zelf naar de overdracht ([`Bron`]; een map dan
//! met zijn mappen, zonder zip). Staat alles al in OpenAEC, dan maakt de server de hele overdracht in één verzoek
//! (`maak_van_opslag`); anders komt wat in sync is erbij (`voeg_toe_van_opslag`) en uploadt de app alleen de rest:
//! wat lokaal gewijzigd of nog niet gesynchroniseerd is. Kent de server dat niet (een oudere versie), of mag het
//! daar niet (bijv. een gearchiveerde ruimte), dan uploadt de app alles zoals altijd.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

use futures_util::stream::{self, StreamExt};
use openaec_sync::motor::stand_van;
use openaec_sync::overdracht::{self, Grenzen, NieuweOverdracht, OverdrachtInfo, OverdrachtRegel, UitOpslag, Voltooid};
use openaec_sync::{Client, Fout};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::delen::DeelFout;
use crate::instellingen::Koppeling;
use crate::{client, melding, opslag_voor, serverplek, systeemvak, AppState};

pub const VENSTER: &str = "versturen";

/// Zoveel bestanden gaan tegelijk; hun delen delen samen de plekken van de sync-instelling "overdrachten tegelijk".
const BESTANDEN_TEGELIJK: usize = 4;

/// Wacht zo lang op meer paden van dezelfde selectie voordat het venster ze krijgt.
const VERZAMELEN: Duration = Duration::from_millis(400);

/// Zo vaak kijkt de app of de server klaar is met kopiëren, en zo lang wacht hij hoogstens.
const KOPIEER_KIJKEN: Duration = Duration::from_secs(2);
const KOPIEER_HOOGSTENS: Duration = Duration::from_secs(6 * 3600);

/// Zo lang en zo vaak herinnert de app het venster aan paden die het nog niet heeft opgehaald (bijv. omdat de pagina
/// bij een eerste start nog laadt): de paden blijven in de wachtrij tot het venster ze neemt.
const HERINNER_ELKE: Duration = Duration::from_millis(500);
const HERINNER_HOOGSTENS: u32 = 40;

/// Toestand van het venster in de app.
#[derive(Default)]
pub struct Versturen {
    /// Paden die net binnenkwamen, en een teller om te zien of er sindsdien meer kwamen.
    binnen: Mutex<(Vec<String>, u64)>,
    /// Paden die het venster nog niet heeft opgehaald.
    wachtrij: Mutex<Wachtrij>,
    /// De overdracht die nu loopt.
    taak: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

/// De paden voor het venster tot het ze ophaalt ([`versturen_verzoek`]). Het venster haalt ze zelf op zodra het
/// klaar is (bij het laden, na een herinnering en als het zichtbaar wordt); een melding die het venster mist (de
/// pagina laadt nog bij een eerste start, of de webview werd opnieuw geladen) kost zo nooit de selectie, en wat het
/// venster al nam, krijgt het geen tweede keer.
#[derive(Debug, Default, PartialEq)]
pub struct Wachtrij {
    paden: Vec<String>,
}

impl Wachtrij {
    /// Paden erbij (dubbele één keer); geeft of er iets klaarstaat.
    pub fn voeg_toe(&mut self, paden: Vec<String>) -> bool {
        for p in paden {
            if !p.is_empty() && !self.paden.contains(&p) {
                self.paden.push(p);
            }
        }
        !self.paden.is_empty()
    }

    /// Alles wat klaarstaat, en de wachtrij is daarna leeg.
    pub fn neem(&mut self) -> Vec<String> {
        std::mem::take(&mut self.paden)
    }

    pub fn leeg(&self) -> bool {
        self.paden.is_empty()
    }
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

/// Het venster tonen met deze paden (leeg: alleen tonen). De paden gaan in de wachtrij; het venster haalt ze op. Tot
/// het dat gedaan heeft, krijgt het elke halve seconde een herinnering (hoogstens 20 s).
pub fn toon(app: &AppHandle, paden: Vec<String>) {
    let aantal = paden.len();
    let klaar = app.state::<Versturen>().wachtrij.lock().unwrap().voeg_toe(paden);
    log::info!("versturen: venster tonen ({aantal} nieuwe paden, wachtrij {})", if klaar { "gevuld" } else { "leeg" });
    systeemvak::verberg_status(app);
    match app.get_webview_window(VENSTER) {
        Some(venster) => systeemvak::naar_voren(&venster),
        None => log::error!("versturen: het venster ontbreekt"),
    }
    if !klaar {
        return;
    }
    let _ = app.emit_to(VENSTER, "versturen-verzoek", ());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for _ in 0..HERINNER_HOOGSTENS {
            tokio::time::sleep(HERINNER_ELKE).await;
            if app.state::<Versturen>().wachtrij.lock().unwrap().leeg() {
                return;
            }
            let _ = app.emit_to(VENSTER, "versturen-verzoek", ());
        }
        log::warn!("versturen: het venster haalde de paden niet op");
    });
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

/// De paden die op het venster wachten; daarna is de wachtrij leeg (het venster heeft ze).
#[tauri::command]
pub fn versturen_verzoek(st: tauri::State<'_, Versturen>) -> Vec<String> {
    let paden = st.wachtrij.lock().unwrap().neem();
    if !paden.is_empty() {
        log::info!("versturen: venster haalde {} paden op", paden.len());
    }
    paden
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
    /// Staat in een gekoppelde map en is helemaal in sync: staat al in OpenAEC, er hoeft niets geüpload te worden.
    #[serde(default)]
    pub in_opslag: bool,
}

fn naam_van(pad: &Path) -> String {
    pad.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| pad.to_string_lossy().to_string())
}

/// Wat er in elk pad zit (de omvang van een map wordt geteld).
pub fn item(pad: &str) -> Item {
    let p = Path::new(pad);
    let mut item = Item { pad: pad.to_string(), naam: naam_van(p), map: false, grootte: 0, bestanden: 0, fout: None, in_opslag: false };
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
pub async fn versturen_items(state: tauri::State<'_, AppState>, paden: Vec<String>) -> Result<Vec<Item>, String> {
    let koppelingen = state.instellingen.lock().unwrap().koppelingen.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let gesynct = gesynchroniseerd(&koppelingen, &paden);
        paden.iter().zip(gesynct).map(|(p, g)| Item { in_opslag: g.is_some(), ..item(p) }).collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// Een pad in een gekoppelde map dat helemaal in sync is: de koppeling, het pad erin (met '/') en, bij een bestand,
/// het bestand op de server met de versie die hier staat.
#[derive(Debug, Clone)]
pub struct InSync {
    pub koppeling: Koppeling,
    pub rel: String,
    pub bestand: Option<(String, u64)>,
}

/// Per pad: staat het in een gekoppelde map en is alles erin in sync met de laatste ronde (geen nieuwe of gewijzigde
/// bestanden)? Alleen lokaal, zonder de server: de toestand wordt per gekoppelde map één keer gelezen. De hele
/// gekoppelde map van een ruimte telt niet (die is geen map op de server), een lege map ook niet.
pub fn gesynchroniseerd(koppelingen: &[Koppeling], paden: &[String]) -> Vec<Option<InSync>> {
    let plekken: Vec<Option<(Koppeling, String)>> = paden.iter().map(|p| serverplek::zoek_in(koppelingen, p)).collect();
    let mut uit: Vec<Option<InSync>> = vec![None; paden.len()];
    let mut roots: Vec<String> = plekken.iter().flatten().map(|(k, _)| k.lokaal.clone()).collect();
    roots.sort();
    roots.dedup();
    for root in roots {
        let hier: Vec<usize> = (0..paden.len()).filter(|&i| plekken[i].as_ref().is_some_and(|(k, _)| k.lokaal == root)).collect();
        let rels: Vec<&str> = hier.iter().map(|&i| plekken[i].as_ref().unwrap().1.as_str()).collect();
        for (&i, (in_sync, anders)) in hier.iter().zip(stand_van(Path::new(&root), &rels)) {
            let (k, rel) = plekken[i].clone().unwrap();
            let is_map = Path::new(&paden[i]).is_dir();
            if !anders.is_empty() || in_sync.is_empty() || (is_map && rel.is_empty() && k.deling.is_none()) {
                continue;
            }
            let bestand = (!is_map).then(|| (in_sync[0].bestand.clone(), in_sync[0].versie));
            uit[i] = Some(InSync { koppeling: k, rel, bestand });
        }
    }
    uit
}

/// Een fout van de server vertalen naar een vaste code en een melding voor het venster.
pub fn versturenfout(f: &Fout) -> DeelFout {
    let melding = |code, tekst: &str| DeelFout { code, melding: tekst.to_string() };
    if let Fout::Netwerk(_) = f {
        return melding("geen_verbinding", "Geen verbinding met de server. Controleer je internetverbinding en probeer het opnieuw.");
    }
    // Vernieuwen van het token lukte niet meer (of de sleutel wordt geweigerd): opnieuw inloggen.
    if f.sessie_verlopen() {
        return melding("niet_ingelogd", "Je sessie is verlopen. Log opnieuw in.");
    }
    // Een ondertekende link naar de opslag (geen kwestie van rechten): de algemene tekst.
    if let Fout::Link { .. } = f {
        return DeelFout { code: "fout", melding: f.to_string() };
    }
    // Een server zonder overdrachten (oudere OpenAEC Storage): Frappe meldt dan dat de methode of module ontbreekt.
    let servertekst = f.servermelding().unwrap_or_default();
    if servertekst.contains("openaec_storage.overdracht") && (servertekst.contains("No module") || servertekst.contains("ophalen")) {
        return melding(
            "niet_op_server",
            "Versturen is op deze omgeving nog niet beschikbaar: de server moet eerst bijgewerkt worden. Probeer het later opnieuw.",
        );
    }
    match f.status() {
        Some(401) => melding("niet_ingelogd", "Je sessie is verlopen. Log opnieuw in."),
        Some(403) => melding(
            "geen_recht",
            "Je hebt geen recht om bestanden te versturen. Dat kan wie schrijver of beheerder is in een ruimte; vraag het een beheerder van de omgeving.",
        ),
        Some(404) => melding("niet_gevonden", "Deze overdracht bestaat niet (meer) op de server."),
        Some(502..=504) => melding("geen_verbinding", "De server is tijdelijk niet bereikbaar. Probeer het straks opnieuw."),
        // Een weigering van de API: de tekst uit de sync-kern (nooit de ruwe JSON).
        _ if matches!(f, Fout::Server { .. }) => DeelFout { code: "fout", melding: f.to_string() },
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
    let uitkomst: Result<Grenzen, VersturenFout> = async { Ok(verbinding(&state).await?.overdracht_grenzen().await?) }.await;
    if let Err(VersturenFout(f)) = &uitkomst {
        log::warn!("versturen: grenzen ophalen mislukt ({}): {}", f.code, f.melding);
    }
    uitkomst
}

/// Een onderdeel dat al in de opslag staat: de server kopieert het in plaats van dat de app het uploadt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bron {
    pub opslag: UitOpslag,
    /// Staat in de overdracht (de server kopieert, of kopieerde).
    #[serde(default)]
    pub toegevoegd: bool,
    /// De bestanden in de overdracht die de server ervan maakte.
    #[serde(default)]
    pub kopieen: Vec<String>,
}

/// Eén bestand van de overdracht zoals de app hem bijhoudt (voor hervatten).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Onderdeel {
    /// Het gekozen pad (een bestand of een map).
    pub pad: String,
    /// De naam op de server (bij een map `<map>.zip`, of de naam van de map als hij uit de opslag komt).
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
    /// Staat al in OpenAEC: de server kopieert het (zie [`Bron`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bron: Option<Bron>,
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
    /// Helemaal door de server gemaakt (`maak_van_opslag`): de server maakt ook de link.
    #[serde(default)]
    pub van_opslag: bool,
    /// Al klaar bij het maken (de server kopieerde meteen).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltooid: Option<Voltooid>,
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
        let leeg = Onderdeel {
            pad: pad.clone(),
            naam: veilig.clone(),
            map: false,
            grootte: 0,
            gewijzigd: None,
            zip: None,
            bestand: None,
            klaar: false,
            bron: None,
        };
        let onderdeel = if m.is_dir() {
            Onderdeel { naam: format!("{veilig}.zip"), map: true, ..leeg }
        } else {
            Onderdeel { grootte: m.len(), gewijzigd: wijzigingstijd(p), ..leeg }
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

/// Een onderdeel uit de opslag: de naam zonder `.zip` bij een map, en de grootte van wat erin staat.
fn uit_opslag(o: &mut Onderdeel, opslag: UitOpslag) {
    if o.map {
        o.naam = o.naam.trim_end_matches(".zip").to_string();
        o.grootte = overdracht::mapomvang(Path::new(&o.pad)).map(|(_, b)| b).unwrap_or(0);
    }
    o.bron = Some(Bron { opslag, toegevoegd: false, kopieen: Vec::new() });
}

/// Welke onderdelen al in de opslag staan: in sync met de laatste ronde ([`gesynchroniseerd`]), en bij een map
/// met het id op de server. Een map die de server niet vindt, wordt gewoon geüpload.
async fn zoek_bronnen(client: &Client, koppelingen: &[Koppeling], delen: &mut [Onderdeel]) {
    let paden: Vec<String> = delen.iter().map(|o| o.pad.clone()).collect();
    let k2 = koppelingen.to_vec();
    let Ok(gesynct) = tokio::task::spawn_blocking(move || gesynchroniseerd(&k2, &paden)).await else { return };
    for (o, g) in delen.iter_mut().zip(gesynct) {
        let Some(g) = g else { continue };
        let opslag = match g.bestand {
            Some((id, versie)) => Some(UitOpslag::Bestand { id, versie: Some(versie) }),
            None => opslag_voor(client.clone(), &g.koppeling).zoek_map(&g.rel).await.ok().flatten().map(|id| UitOpslag::Map { id }),
        };
        if let Some(opslag) = opslag {
            uit_opslag(o, opslag);
        }
    }
}

/// Kan de app terugvallen op uploaden? Bij een server die versturen uit de opslag niet kent, en als het daar niet mag
/// of niet (meer) kan (gearchiveerde ruimte, prullenbak, geen leesrecht): uploaden van wat hier staat mag dan wel.
fn terugvallen(f: &Fout) -> bool {
    overdracht::methode_onbekend(f)
        || matches!(f.status(), Some(403 | 404))
        || matches!(f.foutcode().as_deref(), Some("ruimte_gearchiveerd" | "verwijderd" | "buiten_deling" | "deling_ingetrokken" | "deling_verlopen"))
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
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stap {
    overdracht: String,
    index: usize,
    /// `wachten`, `inpakken`, `kopieren`, `bezig`, `klaar` of `fout`.
    status: &'static str,
    gedaan: u64,
    totaal: u64,
    fout: Option<String>,
}

/// Wat het versturen naar buiten meldt en bewaart: in de app het venster en `versturen.json`, in de tests niets.
pub struct Omgeving<'a> {
    pub meld: &'a (dyn Fn(Stap) + Send + Sync),
    pub bewaar: &'a (dyn Fn(&Lopend) + Send + Sync),
    /// Waar de zips van mappen komen.
    pub zipmap: PathBuf,
    pub gelijktijdig: usize,
    pub kijken: Duration,
}

impl Omgeving<'_> {
    fn stap(&self, overdracht: &str, index: usize, status: &'static str, gedaan: u64, totaal: u64, fout: Option<String>) {
        (self.meld)(Stap { overdracht: overdracht.to_string(), index, status, gedaan, totaal, fout });
    }
}

/// Een nieuwe overdracht beginnen op de server. Staat alles al in OpenAEC, dan in één verzoek (`maak_van_opslag`; is
/// hij klein, dan meteen met de link in `voltooid`); anders een concept, waar het versturen ([`verstuur`]) de rest
/// in zet. Kent de server versturen uit de opslag niet of mag het daar niet, dan gaat alles langs de upload.
pub async fn maak_overdracht(client: &Client, nieuw: NieuweOverdracht, mut delen: Vec<Onderdeel>) -> Result<Lopend, VersturenFout> {
    let lopend = |overdracht: String, naam: String, delen: Vec<Onderdeel>, concept_geldig_tot| Lopend {
        overdracht,
        naam,
        basis: client.basis().to_string(),
        onderdelen: delen,
        concept_geldig_tot,
        van_opslag: false,
        voltooid: None,
    };
    if delen.iter().all(|o| o.bron.is_some()) {
        let items: Vec<UitOpslag> = delen.iter().filter_map(|o| o.bron.as_ref().map(|b| b.opslag.clone())).collect();
        match client.maak_van_opslag(&nieuw, &items).await {
            Ok(v) => {
                for o in delen.iter_mut() {
                    o.bron.as_mut().unwrap().toegevoegd = true;
                    o.klaar = !v.bezig;
                }
                let naam = nieuw.naam.trim().to_string();
                let mut l = lopend(v.overdracht.clone(), naam, delen, None);
                l.van_opslag = true;
                if !v.bezig {
                    l.voltooid = Some(Voltooid {
                        overdracht: v.overdracht,
                        status: v.status,
                        token: None,
                        url: v.url.unwrap_or_default(),
                        verloopt_op: v.verloopt_op,
                        aantal_bestanden: v.aantal_bestanden,
                        totale_grootte: v.totale_grootte,
                        mail: v.mail,
                    });
                }
                return Ok(l);
            }
            Err(f) if terugvallen(&f) => {
                log::info!("versturen uit de opslag kan hier niet ({f}); de app uploadt");
                for o in delen.iter_mut() {
                    ontkoppel_bron(o);
                }
            }
            Err(f) => return Err(f.into()),
        }
    }
    let concept = client.maak_overdracht(&nieuw).await?;
    Ok(lopend(concept.overdracht, concept.naam, delen, concept.concept_geldig_tot))
}

/// Toch uploaden: een map weer als zip.
fn ontkoppel_bron(o: &mut Onderdeel) {
    if o.bron.take().is_some() && o.map {
        o.naam = format!("{}.zip", o.naam.trim_end_matches(".zip"));
        o.grootte = 0;
    }
}

/// Een nieuwe overdracht maken en het versturen starten; geeft de stand (met de onderdelen in de volgorde van de
/// paden). Een eerder onderbroken overdracht wordt eerst ingetrokken.
#[tauri::command]
pub async fn start_versturen(app: AppHandle, state: tauri::State<'_, AppState>, opdracht: Opdracht) -> Result<Lopend, VersturenFout> {
    if app.state::<Versturen>().taak.lock().unwrap().as_ref().is_some_and(|t| !t.inner().is_finished()) {
        return Err("er loopt al een overdracht; wacht tot die klaar is of stop hem".into());
    }
    let paden = opdracht.paden.clone();
    let mut delen = tauri::async_runtime::spawn_blocking(move || onderdelen(&paden)).await.map_err(|e| e.to_string())??;
    let client = verbinding(&state).await?;
    if let Some(oud) = lees_lopend() {
        let _ = client.trek_overdracht_in(&oud.overdracht).await;
        wis_lopend(&oud.overdracht);
    }
    if client.mogelijkheden().await.is_ok_and(|m| m.overdracht_van_opslag) {
        let koppelingen = state.instellingen.lock().unwrap().koppelingen.clone();
        zoek_bronnen(&client, &koppelingen, &mut delen).await;
    }
    let naam = match opdracht.naam.trim() {
        "" => delen[0].naam.clone(),
        n => n.to_string(),
    };
    let nieuw = NieuweOverdracht {
        naam,
        bericht: opdracht.bericht,
        verloopt_op: opdracht.verloopt_op,
        wachtwoord: opdracht.wachtwoord,
        ontvangers: opdracht.ontvangers,
    };
    let lopend = maak_overdracht(&client, nieuw, delen).await?;
    if lopend.voltooid.is_none() {
        bewaar_lopend(&lopend);
    }
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

/// De link uit `info`, als de server de overdracht al afmaakte.
fn voltooid_uit(info: OverdrachtInfo) -> Voltooid {
    Voltooid {
        overdracht: info.overdracht,
        status: info.status,
        token: None,
        url: info.url.unwrap_or_default(),
        verloopt_op: info.verloopt_op,
        aantal_bestanden: info.bestanden.len() as u64,
        totale_grootte: info.totale_grootte,
        mail: Vec::new(),
    }
}

fn weg_op_server(status: &str) -> VersturenFout {
    VersturenFout(DeelFout {
        code: "verlopen",
        melding: format!("Deze overdracht is {} en kan niet meer verder. Verstuur de bestanden opnieuw.", status.to_lowercase()),
    })
}

/// Hervatten: de stand naast die van de server leggen. Bestanden die de server niet (meer) kent of die lokaal
/// veranderd zijn, beginnen opnieuw; bestanden op de server die de app niet kent (aangekondigd vlak voor een
/// onderbreking), gaan weg. Wat de server uit de opslag kopieert, blijft staan.
async fn vergelijk_met_server(client: &Client, stand: &Mutex<Lopend>, omg: &Omgeving<'_>) -> Result<Option<Voltooid>, VersturenFout> {
    let overdracht = stand.lock().unwrap().overdracht.clone();
    let info = client.overdracht_info(&overdracht).await?;
    match info.status.as_str() {
        "Concept" => {}
        "Actief" => return Ok(Some(voltooid_uit(info))),
        status => return Err(weg_op_server(status)),
    }
    if stand.lock().unwrap().van_opslag {
        return Ok(None);
    }
    let bekend: Vec<String> = stand
        .lock()
        .unwrap()
        .onderdelen
        .iter()
        .flat_map(|o| o.bestand.iter().cloned().chain(o.bron.iter().flat_map(|b| b.kopieen.clone())))
        .collect();
    for b in info.bestanden.iter().filter(|b| !bekend.contains(&b.bestand)) {
        client.overdracht_bestand_weg(&overdracht, &b.bestand).await?;
    }
    let mut weg = Vec::new();
    {
        let mut l = stand.lock().unwrap();
        for o in l.onderdelen.iter_mut() {
            if let Some(bron) = o.bron.as_mut() {
                // Uit de opslag: klaar als alle kopieën klaar zijn; ontbreekt er een, dan opnieuw toevoegen.
                let op_server: Vec<_> = bron.kopieen.iter().filter_map(|id| info.bestanden.iter().find(|b| &b.bestand == id)).collect();
                if bron.toegevoegd && op_server.len() < bron.kopieen.len() {
                    weg.extend(op_server.iter().map(|b| b.bestand.clone()));
                    bron.toegevoegd = false;
                    bron.kopieen.clear();
                }
                o.klaar = bron.toegevoegd && op_server.iter().all(|b| b.klaar);
                continue;
            }
            let op_server = o.bestand.as_ref().and_then(|id| info.bestanden.iter().find(|b| &b.bestand == id));
            o.klaar = op_server.is_some_and(|b| b.klaar);
            if op_server.is_none() {
                o.bestand = None;
            }
            if o.klaar {
                continue;
            }
            let veranderd = if o.map {
                o.zip.as_ref().is_none_or(|z| std::fs::metadata(z).map(|m| m.len()).ok() != Some(o.grootte))
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
    (omg.bewaar)(&stand.lock().unwrap());
    Ok(None)
}

async fn uitvoeren(app: &AppHandle, stand: &Mutex<Lopend>, hervat: bool) -> Result<Voltooid, VersturenFout> {
    let state = app.state::<AppState>();
    let client = verbinding(&state).await?;
    let overdracht = stand.lock().unwrap().overdracht.clone();
    let gelijktijdig = state.instellingen.lock().unwrap().sync.clone().begrensd().gelijktijdig;
    let meld = |s: Stap| {
        let _ = app.emit_to(VENSTER, "versturen-stap", &s);
    };
    let omg = Omgeving { meld: &meld, bewaar: &bewaar_lopend, zipmap: zipmap(&overdracht), gelijktijdig, kijken: KOPIEER_KIJKEN };
    verstuur(&client, stand, hervat, &omg).await
}

/// Het versturen zelf: (bij hervatten) de stand naast die van de server leggen, wat al in de opslag staat erbij zetten,
/// mappen inpakken, de rest uploaden, wachten tot de server klaar is met kopiëren en de link maken.
pub async fn verstuur(client: &Client, stand: &Mutex<Lopend>, hervat: bool, omg: &Omgeving<'_>) -> Result<Voltooid, VersturenFout> {
    if let Some(v) = stand.lock().unwrap().voltooid.clone() {
        return Ok(v);
    }
    let overdracht = stand.lock().unwrap().overdracht.clone();
    if hervat {
        if let Some(klaar) = vergelijk_met_server(client, stand, omg).await? {
            return Ok(klaar);
        }
    }
    let aantal = stand.lock().unwrap().onderdelen.len();
    for i in 0..aantal {
        let o = stand.lock().unwrap().onderdelen[i].clone();
        let status = match (o.klaar, &o.bron) {
            (true, _) => "klaar",
            (false, Some(b)) if b.toegevoegd => "kopieren",
            _ => "wachten",
        };
        omg.stap(&overdracht, i, status, if o.klaar { o.grootte } else { 0 }, o.grootte, None);
    }

    // Wat al in de opslag staat: de server kopieert het (in één verzoek voor alles).
    let nieuw: Vec<usize> = (0..aantal).filter(|&i| stand.lock().unwrap().onderdelen[i].bron.as_ref().is_some_and(|b| !b.toegevoegd)).collect();
    if !nieuw.is_empty() {
        let items: Vec<UitOpslag> = nieuw.iter().map(|&i| stand.lock().unwrap().onderdelen[i].bron.as_ref().unwrap().opslag.clone()).collect();
        match client.voeg_toe_van_opslag(&overdracht, &items).await {
            Ok(t) => {
                let mut l = stand.lock().unwrap();
                let namen: Vec<String> = nieuw.iter().map(|&i| l.onderdelen[i].naam.clone()).collect();
                let paren: Vec<(&UitOpslag, &str)> = items.iter().zip(&namen).map(|(item, naam)| (item, naam.as_str())).collect();
                let kopieen = kopieen_van(&paren, &t.bestanden);
                for (&i, kopie) in nieuw.iter().zip(kopieen) {
                    let o = &mut l.onderdelen[i];
                    let bron = o.bron.as_mut().unwrap();
                    bron.toegevoegd = true;
                    bron.kopieen = kopie;
                    o.klaar = !t.bezig;
                    omg.stap(&overdracht, i, if o.klaar { "klaar" } else { "kopieren" }, if o.klaar { o.grootte } else { 0 }, o.grootte, None);
                }
                (omg.bewaar)(&l);
            }
            Err(f) if terugvallen(&f) => {
                log::info!("versturen uit de opslag kan hier niet ({f}); de app uploadt");
                let mut l = stand.lock().unwrap();
                for &i in &nieuw {
                    ontkoppel_bron(&mut l.onderdelen[i]);
                    let o = &l.onderdelen[i];
                    omg.stap(&overdracht, i, "wachten", 0, o.grootte, None);
                }
                (omg.bewaar)(&l);
            }
            Err(f) => return Err(f.into()),
        }
    }

    // Mappen inpakken (een voor een; elke zip staat in de cachemap tot de overdracht klaar is).
    for i in 0..aantal {
        let o = stand.lock().unwrap().onderdelen[i].clone();
        if !o.map || o.klaar || o.bron.is_some() || o.zip.as_ref().is_some_and(|z| Path::new(z).is_file()) {
            continue;
        }
        omg.stap(&overdracht, i, "inpakken", 0, 0, None);
        let doel = omg.zipmap.join(format!("{i}-{}", o.naam));
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
            omg.stap(&overdracht, i, "fout", 0, 0, Some(e.to_string()));
            format!("{} inpakken: {e}", o.naam)
        })?;
        let mut l = stand.lock().unwrap();
        l.onderdelen[i].zip = Some(doel.to_string_lossy().to_string());
        l.onderdelen[i].grootte = grootte;
        (omg.bewaar)(&l);
        drop(l);
        omg.stap(&overdracht, i, "wachten", 0, grootte, None);
    }

    let rem = openaec_sync::rem(omg.gelijktijdig);
    let open: Vec<usize> = (0..aantal)
        .filter(|&i| {
            let l = stand.lock().unwrap();
            !l.onderdelen[i].klaar && l.onderdelen[i].bron.is_none()
        })
        .collect();
    let taken: Vec<_> = open.into_iter().map(|i| verstuur_onderdeel(client, stand, &overdracht, i, &rem, omg)).collect();
    let resultaten: Vec<Result<(), VersturenFout>> = stream::iter(taken).buffer_unordered(BESTANDEN_TEGELIJK).collect().await;
    if let Some(f) = resultaten.into_iter().find_map(Result::err) {
        return Err(f);
    }
    if let Some(klaar) = wacht_op_kopieen(client, stand, omg).await? {
        return Ok(klaar);
    }
    Ok(client.voltooi_overdracht(&overdracht).await?)
}

/// De bestanden in de overdracht die bij elk onderdeel uit de opslag horen. De server zet ze in de volgorde van de
/// vraag: eerst de losse bestanden (zonder map), dan per map alles eronder, met de naam van de map vooraan in `map`.
fn kopieen_van(items: &[(&UitOpslag, &str)], bestanden: &[overdracht::OverdrachtBestand]) -> Vec<Vec<String>> {
    let mut los = bestanden.iter().filter(|b| b.map.is_none());
    items
        .iter()
        .map(|(item, naam)| match item {
            UitOpslag::Bestand { .. } => los.next().map(|b| vec![b.bestand.clone()]).unwrap_or_default(),
            UitOpslag::Map { .. } => {
                let naam = naam.trim().to_lowercase();
                bestanden
                    .iter()
                    .filter(|b| b.map.as_deref().is_some_and(|m| m.split('/').next().unwrap_or("").trim().to_lowercase() == naam))
                    .map(|b| b.bestand.clone())
                    .collect()
            }
        })
        .collect()
}

/// Wachten tot de server klaar is met kopiëren uit de opslag, met de voortgang naar het venster. Maakte de server de
/// overdracht zelf (`maak_van_opslag`), dan maakt hij ook de link: dan geeft dit de link terug.
async fn wacht_op_kopieen(client: &Client, stand: &Mutex<Lopend>, omg: &Omgeving<'_>) -> Result<Option<Voltooid>, VersturenFout> {
    let (overdracht, van_opslag) = {
        let l = stand.lock().unwrap();
        (l.overdracht.clone(), l.van_opslag)
    };
    let open = || stand.lock().unwrap().onderdelen.iter().any(|o| o.bron.is_some() && !o.klaar);
    if !open() && !van_opslag {
        return Ok(None);
    }
    let begin = Instant::now();
    loop {
        let info = client.overdracht_info(&overdracht).await?;
        match info.status.as_str() {
            "Actief" => {
                let mut l = stand.lock().unwrap();
                for (i, o) in l.onderdelen.iter_mut().enumerate() {
                    if !o.klaar {
                        o.klaar = true;
                        omg.stap(&overdracht, i, "klaar", o.grootte, o.grootte, None);
                    }
                }
                return Ok(Some(voltooid_uit(info)));
            }
            "Concept" => {}
            "Ingetrokken" => {
                return Err(VersturenFout(DeelFout {
                    code: "verlopen",
                    melding: "Het kopiëren op de server is niet gelukt; de overdracht is ingetrokken. Verstuur de bestanden opnieuw.".into(),
                }))
            }
            status => return Err(weg_op_server(status)),
        }
        {
            let mut l = stand.lock().unwrap();
            for (i, o) in l.onderdelen.iter_mut().enumerate() {
                let Some(bron) = o.bron.as_ref() else { continue };
                if o.klaar {
                    continue;
                }
                let klaar = if van_opslag {
                    info.bestanden.iter().all(|b| b.klaar)
                } else {
                    bron.kopieen.iter().all(|id| info.bestanden.iter().any(|b| &b.bestand == id && b.klaar))
                };
                if klaar {
                    o.klaar = true;
                    omg.stap(&overdracht, i, "klaar", o.grootte, o.grootte, None);
                }
            }
        }
        // De server maakt de link zelf bij `maak_van_opslag`; anders kan `voltooi` zodra alles klaar is.
        if !van_opslag && !open() {
            return Ok(None);
        }
        if begin.elapsed() > KOPIEER_HOOGSTENS {
            return Err("de server is na lange tijd nog niet klaar met kopiëren; probeer het later opnieuw".into());
        }
        tokio::time::sleep(omg.kijken).await;
    }
}

/// Eén onderdeel uploaden, met de voortgang naar het venster (hoogstens een paar keer per seconde).
async fn verstuur_onderdeel(
    client: &Client,
    stand: &Mutex<Lopend>,
    overdracht: &str,
    index: usize,
    rem: &openaec_sync::Rem,
    omg: &Omgeving<'_>,
) -> Result<(), VersturenFout> {
    let o = stand.lock().unwrap().onderdelen[index].clone();
    let bron = PathBuf::from(o.zip.as_deref().unwrap_or(&o.pad));
    let laatst = Mutex::new(Instant::now() - Duration::from_secs(1));
    let voortgang = |gedaan: u64, totaal: u64| {
        let mut l = laatst.lock().unwrap();
        if gedaan >= totaal || l.elapsed() >= Duration::from_millis(150) {
            *l = Instant::now();
            omg.stap(overdracht, index, "bezig", gedaan, totaal, None);
        }
    };
    let bij_start = |id: &str| {
        let mut l = stand.lock().unwrap();
        l.onderdelen[index].bestand = Some(id.to_string());
        (omg.bewaar)(&l);
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
            (omg.bewaar)(&l);
            drop(l);
            omg.stap(overdracht, index, "klaar", o.grootte, o.grootte, None);
            Ok(())
        }
        Err(f) => {
            let fout = versturenfout(&f);
            omg.stap(overdracht, index, "fout", 0, o.grootte, Some(fout.melding.clone()));
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
    tauri::async_runtime::spawn_blocking(move || crate::menupakket::werk_klassiek_bij(aan)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    let mut inst = state.instellingen.lock().unwrap();
    inst.menu_versturen = Some(aan);
    inst.bewaar().map_err(|e| e.to_string())?;
    crate::menupakket::schrijf_menu(&inst);
    Ok(aan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wachtrij_houdt_paden_tot_het_venster_ze_neemt() {
        let mut w = Wachtrij::default();
        assert!(w.leeg());
        assert!(!w.voeg_toe(vec![]));
        assert!(!w.voeg_toe(vec![String::new()]));
        // Een eerste selectie terwijl het venster nog laadt: de paden blijven staan, ook na een gemiste melding.
        assert!(w.voeg_toe(vec![r"C:\a.pdf".into(), r"C:\b.dwg".into()]));
        // Een tweede selectie (of dezelfde nog eens) erbij, zonder dubbele.
        assert!(w.voeg_toe(vec![r"C:\b.dwg".into(), r"C:\Tekeningen".into()]));
        assert!(!w.leeg());
        assert_eq!(w.neem(), [r"C:\a.pdf", r"C:\b.dwg", r"C:\Tekeningen"]);
        // Wat het venster nam, krijgt het geen tweede keer (bijv. na een herinnering of opnieuw laden).
        assert!(w.leeg());
        assert!(w.neem().is_empty());
    }

    fn server(status: u16) -> Fout {
        Fout::Server { status, tekst: json!({}).to_string() }
    }

    #[test]
    fn fouten_krijgen_een_vaste_code() {
        assert_eq!(versturenfout(&server(403)).code, "geen_recht");
        assert_eq!(versturenfout(&server(401)).code, "niet_ingelogd");
        assert_eq!(versturenfout(&server(404)).code, "niet_gevonden");
        assert_eq!(versturenfout(&server(503)).code, "geen_verbinding");
        assert_eq!(versturenfout(&Fout::SessieVerlopen).code, "niet_ingelogd");
        let link = versturenfout(&Fout::Link { status: 403, tekst: "<Error/>".into() });
        assert_eq!(link.code, "fout");
        assert!(!link.melding.contains("<Error"));
        // Een oudere server zonder overdrachten: een begrijpelijke melding in plaats van de Python-fout.
        let oud = Fout::Server {
            status: 404,
            tekst: json!({"exception": "frappe.exceptions.DoesNotExistError: Methode ophalen voor commando openaec_storage.overdracht.grenzen mislukt met No module named 'openaec_storage.overdracht'"}).to_string(),
        };
        let fout = versturenfout(&oud);
        assert_eq!(fout.code, "niet_op_server");
        assert!(!fout.melding.contains("No module"));
        let grens = Fout::Server {
            status: 417,
            tekst: json!({"exception": "frappe.exceptions.ValidationError: Een overdracht kan hoogstens 100 GB groot zijn"}).to_string(),
        };
        assert_eq!(versturenfout(&grens), DeelFout { code: "fout", melding: "Een overdracht kan hoogstens 100 GB groot zijn".into() });
    }

    #[test]
    fn terugvallen_op_uploaden() {
        assert!(terugvallen(&server(404)), "oudere server");
        let gearchiveerd = Fout::Server { status: 403, tekst: json!({"foutcode": "ruimte_gearchiveerd"}).to_string() };
        assert!(terugvallen(&gearchiveerd));
        let quotum = Fout::Server { status: 417, tekst: json!({"foutcode": "quotum_pool_vol"}).to_string() };
        assert!(!terugvallen(&quotum), "een vol quotum geldt ook voor uploaden");
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
                bron: Some(Bron { opslag: UitOpslag::Bestand { id: "S1".into(), versie: Some(2) }, toegevoegd: true, kopieen: vec!["K1".into()] }),
            }],
            concept_geldig_tot: None,
            van_opslag: false,
            voltooid: None,
        };
        let terug: Lopend = serde_json::from_slice(&serde_json::to_vec(&l).unwrap()).unwrap();
        assert_eq!(terug, l);
        // Een stand van een vorige versie (zonder bron) is nog te lezen.
        let oud: Lopend = serde_json::from_value(json!({"overdracht": "OV1", "naam": "x", "basis": "b",
            "onderdelen": [{"pad": "p", "naam": "n", "map": false, "grootte": 1}]}))
        .unwrap();
        assert_eq!(oud.onderdelen[0].bron, None);
    }

    #[test]
    fn kopieen_bij_een_onderdeel() {
        let b = |id: &str, naam: &str, map: Option<&str>| overdracht::OverdrachtBestand {
            bestand: id.into(),
            naam: naam.into(),
            map: map.map(str::to_string),
            grootte: 1,
            klaar: true,
            aantal_downloads: 0,
        };
        // Twee losse bestanden met dezelfde naam (de tweede kreeg " (2)") en een map met een submap.
        let lijst = [
            b("1", "plan.pdf", None),
            b("2", "plan (2).pdf", None),
            b("3", "a.dwg", Some("Tekeningen")),
            b("4", "b.dwg", Some("Tekeningen/2026")),
        ];
        let (bestand, map) = (UitOpslag::Bestand { id: "S".into(), versie: None }, UitOpslag::Map { id: "M".into() });
        let items = [(&bestand, "plan.pdf"), (&map, "tekeningen"), (&bestand, "plan.pdf")];
        assert_eq!(kopieen_van(&items, &lijst), vec![vec!["1".to_string()], vec!["3".into(), "4".into()], vec!["2".into()]]);
    }

    /// Versturen tegen een nagebootste server: wat al in OpenAEC staat, gaat niet opnieuw over de lijn.
    mod nagebootst {
        use super::*;
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, Request, ResponseTemplate};

        const API: &str = "/api/method/openaec_storage.api.";
        const OV: &str = "/api/method/openaec_storage.overdracht.";

        fn antwoord(message: serde_json::Value) -> ResponseTemplate {
            ResponseTemplate::new(200).set_body_json(json!({ "message": message }))
        }

        fn opslag_neemt_aan() -> ResponseTemplate {
            ResponseTemplate::new(200).insert_header("ETag", "\"e\"")
        }

        /// Een tijdelijke gekoppelde map; weg na de test.
        struct Werkmap(PathBuf);

        impl Drop for Werkmap {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        /// Een gekoppelde map met bestanden; `in_sync` (pad, bestand-id) staat in de toestand zoals hij hier is.
        fn werkmap(naam: &str, bestanden: &[(&str, &[u8])], in_sync: &[(&str, &str)]) -> (Werkmap, Koppeling) {
            let root = std::env::temp_dir().join(format!("openaec-versturen-{naam}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            for (pad, inhoud) in bestanden {
                let p = root.join(pad);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(&p, inhoud).unwrap();
            }
            let mut staat = serde_json::Map::new();
            for (pad, id) in in_sync {
                let m = std::fs::metadata(root.join(pad)).unwrap();
                let mtime = m.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
                staat.insert(pad.to_string(), json!({"bestand": id, "versie": 3, "grootte": m.len(), "mtime_ns": mtime}));
            }
            std::fs::create_dir_all(root.join(".openaec")).unwrap();
            std::fs::write(root.join(".openaec").join("staat.json"), json!({ "bestanden": staat }).to_string()).unwrap();
            let k = Koppeling::ruimte("R1".into(), "OpenAEC Studio".into(), root.to_string_lossy().to_string());
            (Werkmap(root), k)
        }

        fn pad(w: &Werkmap, rel: &str) -> String {
            w.0.join(rel).to_string_lossy().to_string()
        }

        fn nieuw() -> NieuweOverdracht {
            NieuweOverdracht { naam: "Tekeningen".into(), ontvangers: vec!["jan@voorbeeld.nl".into()], ..Default::default() }
        }

        fn stil<'a>(meld: &'a (dyn Fn(Stap) + Send + Sync), zips: &Path) -> Omgeving<'a> {
            Omgeving { meld, bewaar: &|_| {}, zipmap: zips.to_path_buf(), gelijktijdig: 2, kijken: Duration::from_millis(10) }
        }

        fn methodes(verzoeken: &[Request]) -> Vec<String> {
            verzoeken.iter().map(|r| r.url.path().rsplit('.').next().unwrap_or_default().to_string()).collect()
        }

        fn body(verzoeken: &[Request], methode: &str) -> serde_json::Value {
            let r = verzoeken.iter().find(|r| r.url.path().ends_with(&format!(".{methode}"))).expect(methode);
            serde_json::from_slice(&r.body).unwrap()
        }

        async fn post(server: &MockServer, methode: &str, message: serde_json::Value) {
            Mock::given(method("POST")).and(path(format!("{OV}{methode}"))).respond_with(antwoord(message)).mount(server).await;
        }

        async fn upload_mocks(server: &MockServer) {
            Mock::given(method("PUT")).respond_with(opslag_neemt_aan()).mount(server).await;
            post(server, "voltooi", json!({"overdracht": "OV1", "status": "Actief", "url": "https://x/overdracht/t", "mail": []})).await;
        }

        async fn bestand_mocks(server: &MockServer, naam: &str, id: &str) {
            Mock::given(method("POST"))
                .and(path(format!("{OV}bestand_start")))
                .and(body_partial_json(json!({ "naam": naam })))
                .respond_with(antwoord(json!({"bestand": id, "naam": naam, "grootte": 1, "deelgrootte": 8388608,
                    "delen": [{"nummer": 1, "url": format!("{}/s3/{id}", server.uri())}]})))
                .expect(1)
                .mount(server)
                .await;
            Mock::given(method("POST"))
                .and(path(format!("{OV}bestand_klaar")))
                .and(body_partial_json(json!({ "bestand": id })))
                .respond_with(antwoord(json!({"bestand": id, "naam": naam, "grootte": 1, "klaar": true})))
                .mount(server)
                .await;
        }

        #[tokio::test]
        async fn alles_gesynchroniseerd_gaat_in_een_verzoek_zonder_upload() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path(format!("{API}mogelijkheden")))
                .respond_with(antwoord(json!({"zoek_pad": true, "overdracht_van_opslag": true})))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path(format!("{API}zoek_pad")))
                .and(body_partial_json(json!({"pad": "Tekeningen"})))
                .respond_with(antwoord(json!({"map": "M1", "gevonden": true})))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path(format!("{OV}maak_van_opslag")))
                .respond_with(antwoord(json!({"overdracht": "OV1", "status": "Actief", "bezig": false,
                    "url": "https://x/overdracht/t", "aantal_bestanden": 3, "totale_grootte": 12, "mail": ["jan@voorbeeld.nl"]})))
                .expect(1)
                .mount(&server)
                .await;
            let (w, k) = werkmap(
                "gesynct",
                &[("plan.pdf", b"plan"), ("Tekeningen/a.dwg", b"aaaa"), ("Tekeningen/2026/b.dwg", b"bbbb")],
                &[("plan.pdf", "B1"), ("Tekeningen/a.dwg", "B2"), ("Tekeningen/2026/b.dwg", "B3")],
            );
            let client = Client::new(&server.uri(), "token a:b");
            let mut delen = onderdelen(&[pad(&w, "plan.pdf"), pad(&w, "Tekeningen")]).unwrap();
            zoek_bronnen(&client, &[k], &mut delen).await;
            assert_eq!(delen[0].bron.as_ref().unwrap().opslag, UitOpslag::Bestand { id: "B1".into(), versie: Some(3) });
            assert_eq!((delen[1].naam.as_str(), &delen[1].bron.as_ref().unwrap().opslag), ("Tekeningen", &UitOpslag::Map { id: "M1".into() }));
            let lopend = maak_overdracht(&client, nieuw(), delen).await.unwrap();
            assert!(lopend.van_opslag && lopend.onderdelen.iter().all(|o| o.klaar));
            let stand = Mutex::new(lopend);
            let v = verstuur(&client, &stand, false, &stil(&|_| {}, &w.0.join("zips"))).await.unwrap();
            assert_eq!((v.url.as_str(), v.mail.clone()), ("https://x/overdracht/t", vec!["jan@voorbeeld.nl".to_string()]));
            let verzoeken = server.received_requests().await.unwrap();
            let b = body(&verzoeken, "maak_van_opslag");
            assert_eq!(b["bestanden"], json!([{"bestand": "B1", "versie": 3}]));
            assert_eq!((b["mappen"].clone(), b["naam"].clone()), (json!(["M1"]), json!("Tekeningen")));
            // Niets geüpload, geen zip, geen concept.
            assert!(verzoeken.iter().all(|r| r.method.as_str() == "POST"));
            assert!(!methodes(&verzoeken).iter().any(|m| ["bestand_start", "maak", "voltooi"].contains(&m.as_str())));
            assert!(!w.0.join("zips").exists());
        }

        #[tokio::test]
        async fn deels_gewijzigd_alleen_dat_deel_wordt_geupload() {
            let server = MockServer::start().await;
            post(&server, "maak", json!({"overdracht": "OV1", "naam": "Tekeningen", "status": "Concept"})).await;
            post(&server, "voeg_toe_van_opslag", json!({"overdracht": "OV1", "bezig": false,
                "bestanden": [{"bestand": "K1", "naam": "plan.pdf", "grootte": 4, "klaar": true}]}))
            .await;
            bestand_mocks(&server, "notitie.txt", "OB2").await;
            upload_mocks(&server).await;
            // plan.pdf is in sync; notitie.txt is hier gewijzigd sinds de laatste ronde.
            let (w, k) = werkmap("deels", &[("plan.pdf", b"plan"), ("notitie.txt", b"oud")], &[("plan.pdf", "B1"), ("notitie.txt", "B2")]);
            std::fs::write(w.0.join("notitie.txt"), b"nieuwer").unwrap();
            let client = Client::new(&server.uri(), "token a:b");
            let mut delen = onderdelen(&[pad(&w, "plan.pdf"), pad(&w, "notitie.txt")]).unwrap();
            zoek_bronnen(&client, &[k], &mut delen).await;
            assert!(delen[0].bron.is_some() && delen[1].bron.is_none());
            let stappen = Mutex::new(Vec::new());
            let meld = |s: Stap| stappen.lock().unwrap().push((s.index, s.status));
            let stand = Mutex::new(maak_overdracht(&client, nieuw(), delen).await.unwrap());
            let v = verstuur(&client, &stand, false, &stil(&meld, &w.0.join("zips"))).await.unwrap();
            assert_eq!(v.url, "https://x/overdracht/t");
            let verzoeken = server.received_requests().await.unwrap();
            assert_eq!(body(&verzoeken, "voeg_toe_van_opslag")["bestanden"], json!([{"bestand": "B1", "versie": 3}]));
            let puts: Vec<_> = verzoeken.iter().filter(|r| r.method.as_str() == "PUT").map(|r| r.body.clone()).collect();
            assert_eq!(puts, vec![b"nieuwer".to_vec()], "alleen het gewijzigde bestand gaat over de lijn");
            assert_eq!(stand.lock().unwrap().onderdelen[0].bron.as_ref().unwrap().kopieen, vec!["K1".to_string()]);
            assert!(stappen.lock().unwrap().contains(&(0, "klaar")));
        }

        #[tokio::test]
        async fn oude_server_alles_wordt_geupload() {
            let server = MockServer::start().await;
            // Geen `maak_van_opslag` (een oudere versie): Frappe kan de methode niet ophalen.
            Mock::given(method("POST"))
                .and(path(format!("{OV}maak_van_opslag")))
                .respond_with(ResponseTemplate::new(404).set_body_json(json!({"exception":
                    "frappe.exceptions.DoesNotExistError: Failed to get method for command openaec_storage.overdracht.maak_van_opslag"})))
                .mount(&server)
                .await;
            post(&server, "maak", json!({"overdracht": "OV1", "naam": "Tekeningen", "status": "Concept"})).await;
            bestand_mocks(&server, "plan.pdf", "OB1").await;
            upload_mocks(&server).await;
            let (w, _k) = werkmap("oud", &[("plan.pdf", b"plan")], &[("plan.pdf", "B1")]);
            let client = Client::new(&server.uri(), "token a:b");
            // Ook als de app dacht dat het kon (bron gezet), valt hij terug op uploaden.
            let mut delen = onderdelen(&[pad(&w, "plan.pdf")]).unwrap();
            uit_opslag(&mut delen[0], UitOpslag::Bestand { id: "B1".into(), versie: Some(3) });
            let stand = Mutex::new(maak_overdracht(&client, nieuw(), delen).await.unwrap());
            assert!(stand.lock().unwrap().onderdelen[0].bron.is_none());
            verstuur(&client, &stand, false, &stil(&|_| {}, &w.0.join("zips"))).await.unwrap();
            let verzoeken = server.received_requests().await.unwrap();
            assert_eq!(verzoeken.iter().filter(|r| r.method.as_str() == "PUT").count(), 1);
            assert!(!methodes(&verzoeken).contains(&"voeg_toe_van_opslag".to_string()));
        }

        #[tokio::test]
        async fn oude_server_zonder_voeg_toe_uploadt_de_map_als_zip() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path(format!("{OV}voeg_toe_van_opslag")))
                .respond_with(ResponseTemplate::new(404).set_body_json(json!({"exception": "frappe.exceptions.DoesNotExistError: Failed to get method"})))
                .mount(&server)
                .await;
            bestand_mocks(&server, "Tekeningen.zip", "OB1").await;
            bestand_mocks(&server, "plan.pdf", "OB2").await;
            upload_mocks(&server).await;
            let (w, _k) = werkmap("oudmap", &[("Tekeningen/a.dwg", b"a"), ("plan.pdf", b"p")], &[("Tekeningen/a.dwg", "B1")]);
            let client = Client::new(&server.uri(), "token a:b");
            let mut delen = onderdelen(&[pad(&w, "Tekeningen"), pad(&w, "plan.pdf")]).unwrap();
            uit_opslag(&mut delen[0], UitOpslag::Map { id: "M1".into() });
            let stand = Mutex::new(Lopend {
                overdracht: "OV1".into(),
                naam: "x".into(),
                basis: server.uri(),
                onderdelen: delen,
                concept_geldig_tot: None,
                van_opslag: false,
                voltooid: None,
            });
            verstuur(&client, &stand, false, &stil(&|_| {}, &w.0.join("zips"))).await.unwrap();
            let l = stand.lock().unwrap();
            assert!(l.onderdelen[0].bron.is_none() && l.onderdelen[0].zip.is_some(), "weer een zip");
            assert!(l.onderdelen.iter().all(|o| o.klaar));
        }

        #[tokio::test]
        async fn grote_overdracht_wacht_tot_de_server_klaar_is() {
            let server = MockServer::start().await;
            post(&server, "maak_van_opslag", json!({"overdracht": "OV1", "status": "Concept", "bezig": true, "aantal_bestanden": 2})).await;
            Mock::given(method("POST"))
                .and(path(format!("{OV}info")))
                .respond_with(antwoord(json!({"overdracht": "OV1", "naam": "x", "status": "Concept",
                    "bestanden": [{"bestand": "K1", "naam": "a", "klaar": true}, {"bestand": "K2", "naam": "b", "klaar": false}]})))
                .up_to_n_times(2)
                .mount(&server)
                .await;
            post(&server, "info", json!({"overdracht": "OV1", "naam": "x", "status": "Actief", "url": "https://x/overdracht/t",
                "bestanden": [{"bestand": "K1", "naam": "a", "klaar": true}, {"bestand": "K2", "naam": "b", "klaar": true}]}))
            .await;
            let (w, _k) = werkmap("groot", &[("Tekeningen/a.dwg", b"a")], &[]);
            let client = Client::new(&server.uri(), "token a:b");
            let mut delen = onderdelen(&[pad(&w, "Tekeningen")]).unwrap();
            uit_opslag(&mut delen[0], UitOpslag::Map { id: "M1".into() });
            let lopend = maak_overdracht(&client, nieuw(), delen).await.unwrap();
            assert!(lopend.voltooid.is_none() && !lopend.onderdelen[0].klaar);
            let stand = Mutex::new(lopend);
            let stappen = Mutex::new(Vec::new());
            let meld = |s: Stap| stappen.lock().unwrap().push(s.status);
            let v = verstuur(&client, &stand, false, &stil(&meld, &w.0.join("zips"))).await.unwrap();
            assert_eq!(v.url, "https://x/overdracht/t");
            let stappen = stappen.lock().unwrap();
            assert_eq!((stappen.first(), stappen.last()), (Some(&"kopieren"), Some(&"klaar")));
            let verzoeken = server.received_requests().await.unwrap();
            assert!(!methodes(&verzoeken).contains(&"voltooi".to_string()), "de server maakt de link zelf");
        }

        #[test]
        fn niet_gesynchroniseerd_of_buiten_een_koppeling_wordt_geupload() {
            let (w, k) = werkmap("nietgesynct", &[("nieuw.pdf", b"n")], &[]);
            std::fs::create_dir_all(w.0.join("Leeg")).unwrap();
            let buiten = std::env::temp_dir().join(format!("openaec-buiten-{}.txt", std::process::id()));
            std::fs::write(&buiten, b"x").unwrap();
            let paden = [pad(&w, "nieuw.pdf"), pad(&w, "Leeg"), buiten.to_string_lossy().to_string(), w.0.to_string_lossy().to_string()];
            let uit = gesynchroniseerd(&[k], &paden);
            assert!(uit.iter().all(Option::is_none), "nieuw, leeg, buiten de koppeling en de hele ruimte: uploaden");
            let _ = std::fs::remove_file(buiten);
        }
    }
}
