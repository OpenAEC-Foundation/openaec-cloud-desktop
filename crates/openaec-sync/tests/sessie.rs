//! Autorisatie per verzoek tegen een nagebootste server: het toegangstoken verloopt halverwege een ronde met veel
//! bestanden (één vernieuwing, daarna slaagt alles), een ongeldig refresh-token stopt de ronde netjes, gelijktijdige
//! verzoeken vernieuwen maar één keer, en ondertekende links van de opslag krijgen geen Authorization-header.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use openaec_sync::motor::{sync_met_opties, Opties};
use openaec_sync::oauth::Tokens;
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::sessie::{Bewaring, Geheugen, Sessie, Tokenbron};
use openaec_sync::voortgang::Stil;
use openaec_sync::{Client, Fout};
use serde_json::{json, Value};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const API: &str = "/api/method/openaec_storage.api.";
const TOKEN: &str = "/api/method/frappe.integrations.oauth2.get_token";

fn nu() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn tokens(access: &str, refresh: &str, over: i64) -> Tokens {
    Tokens { access_token: access.into(), refresh_token: refresh.into(), verloopt: (nu() as i64 + over) as u64 }
}

/// Wat de nagebootste server bijhoudt.
#[derive(Default)]
struct Toestand {
    /// Toegangstokens die de server nu accepteert.
    geldig: Mutex<HashSet<String>>,
    /// Na zoveel API-verzoeken vervalt `A1` (het token verloopt halverwege de ronde); None: nooit.
    a1_vervalt_na: Option<usize>,
    api_verzoeken: AtomicUsize,
    geweigerd: AtomicUsize,
    vernieuwingen: AtomicUsize,
    /// Refresh-tokens die al gebruikt zijn (een refresh-token werkt maar één keer).
    gebruikt: Mutex<HashSet<String>>,
    /// Vernieuwen weigeren (refresh-token verlopen of ingetrokken).
    weiger_vernieuwen: bool,
    s3_verzoeken: AtomicUsize,
    s3_met_autorisatie: AtomicUsize,
}

impl Toestand {
    fn nieuw(geldig: &[&str], a1_vervalt_na: Option<usize>, weiger_vernieuwen: bool) -> Arc<Self> {
        Arc::new(Self {
            geldig: Mutex::new(geldig.iter().map(|t| t.to_string()).collect()),
            a1_vervalt_na,
            weiger_vernieuwen,
            ..Default::default()
        })
    }
}

fn bericht(message: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "message": message }))
}

/// De lijst `bestanden` uit het multipart-formulier van `upload_klein`.
fn bestanden_uit_formulier(body: &[u8]) -> Vec<Value> {
    let tekst = String::from_utf8_lossy(body);
    let begin = tekst.find("name=\"bestanden\"").expect("veld bestanden");
    let rest = &tekst[begin..];
    let json_begin = rest.find("\r\n\r\n").unwrap() + 4;
    let json_eind = rest[json_begin..].find("\r\n--").unwrap();
    serde_json::from_str(&rest[json_begin..json_begin + json_eind]).unwrap()
}

/// De API: controleert de Authorization-header zoals Frappe (401 met `AuthenticationError`).
struct Nepapi(Arc<Toestand>, String);

impl Respond for Nepapi {
    fn respond(&self, verzoek: &Request) -> ResponseTemplate {
        let t = &self.0;
        let n = t.api_verzoeken.fetch_add(1, Ordering::SeqCst) + 1;
        if t.a1_vervalt_na.is_some_and(|na| n > na) {
            t.geldig.lock().unwrap().remove("A1");
        }
        let token = verzoek.headers.get("authorization").and_then(|w| w.to_str().ok()).unwrap_or("").trim_start_matches("Bearer ").to_string();
        if !t.geldig.lock().unwrap().contains(&token) {
            t.geweigerd.fetch_add(1, Ordering::SeqCst);
            return ResponseTemplate::new(401).set_body_json(json!({"exc_type": "AuthenticationError"}));
        }
        let methode = verzoek.url.path().trim_start_matches(API);
        let gegevens: Value = serde_json::from_slice(&verzoek.body).unwrap_or(Value::Null);
        match methode {
            "mogelijkheden" => {
                let grenzen = json!({"max_bestand": 100, "max_aantal": 5, "max_bytes": 10000});
                bericht(json!({"upload_klein": grenzen, "download_klein": grenzen, "boom": false}))
            }
            "ruimtes" => bericht(json!([{"name": "R1", "naam": "Project", "rol": "beheerder"}])),
            "bladeren" => bericht(json!({"mappen": [], "bestanden": []})),
            "upload_start" => {
                let naam = gegevens["naam"].as_str().unwrap();
                bericht(json!({"upload": format!("U-{naam}"), "deelgrootte": 8388608, "hergebruik": [],
                               "delen": [{"nummer": 1, "url": format!("{}/s3/{naam}", self.1)}]}))
            }
            "upload_klaar" => {
                let naam = gegevens["upload"].as_str().unwrap().trim_start_matches("U-");
                bericht(json!({"bestand": format!("B-{naam}"), "naam": naam, "versie": 1, "conflict": false}))
            }
            "upload_klein" => {
                let resultaten: Vec<Value> = bestanden_uit_formulier(&verzoek.body)
                    .iter()
                    .map(|b| json!({"ok": true, "bestand": format!("B-{}", b["naam"].as_str().unwrap()), "naam": b["naam"],
                                    "versie": 1, "conflict": false, "map": null}))
                    .collect();
                bericht(json!({ "resultaten": resultaten }))
            }
            andere => panic!("onverwacht verzoek {andere}"),
        }
    }
}

/// Het token-adres van Frappe: een refresh-token werkt één keer en geeft een nieuw paar.
struct Neptoken(Arc<Toestand>);

impl Respond for Neptoken {
    fn respond(&self, verzoek: &Request) -> ResponseTemplate {
        let t = &self.0;
        t.vernieuwingen.fetch_add(1, Ordering::SeqCst);
        let body = String::from_utf8_lossy(&verzoek.body).to_string();
        let refresh = body.split('&').find_map(|d| d.strip_prefix("refresh_token=")).unwrap_or("").to_string();
        if t.weiger_vernieuwen || !t.gebruikt.lock().unwrap().insert(refresh.clone()) {
            return ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"}));
        }
        let n = t.vernieuwingen.load(Ordering::SeqCst);
        let access = format!("A{}", n + 1);
        t.geldig.lock().unwrap().insert(access.clone());
        ResponseTemplate::new(200).set_body_json(json!({"access_token": access, "refresh_token": format!("R{}", n + 1), "expires_in": 900}))
    }
}

/// De opslag achter ondertekende links: hoort nooit een Authorization-header te krijgen.
struct Neps3(Arc<Toestand>);

impl Respond for Neps3 {
    fn respond(&self, verzoek: &Request) -> ResponseTemplate {
        self.0.s3_verzoeken.fetch_add(1, Ordering::SeqCst);
        if verzoek.headers.get("authorization").is_some() {
            self.0.s3_met_autorisatie.fetch_add(1, Ordering::SeqCst);
        }
        ResponseTemplate::new(200).insert_header("ETag", "\"e1\"")
    }
}

async fn nepserver(t: &Arc<Toestand>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path(TOKEN)).respond_with(Neptoken(t.clone())).mount(&server).await;
    Mock::given(path_regex("^/s3/")).respond_with(Neps3(t.clone())).mount(&server).await;
    Mock::given(method("POST")).and(path_regex("^/api/method/openaec_storage\\.api\\.")).respond_with(Nepapi(t.clone(), server.uri())).mount(&server).await;
    server
}

/// Gedeelde bewaring, zodat de test kan zien wat de sessie bewaarde (en kan "inloggen").
#[derive(Clone, Default)]
struct Gedeeld(Arc<Geheugen>);

impl Bewaring for Gedeeld {
    fn lees(&self) -> Option<String> {
        self.0.lees()
    }
    fn bewaar(&self, waarde: &str) -> Result<(), String> {
        self.0.bewaar(waarde)
    }
}

fn nieuwe_sessie(server: &MockServer, begin: Tokens) -> (Arc<Sessie>, Gedeeld) {
    let bewaring = Gedeeld(Arc::new(Geheugen::met(serde_json::to_string(&begin).unwrap())));
    (Arc::new(Sessie::new(&server.uri(), bewaring.clone())), bewaring)
}

/// 30 kleine bestanden (in groepen van 5 in één verzoek) en 10 grotere (elk met een deel naar de opslag).
fn veel_bestanden(map: &std::path::Path) {
    for i in 0..30 {
        std::fs::write(map.join(format!("klein-{i:02}.txt")), format!("klein {i}")).unwrap();
    }
    for i in 0..10 {
        std::fs::write(map.join(format!("groot-{i:02}.bin")), vec![i as u8; 200]).unwrap();
    }
}

#[tokio::test]
async fn token_verloopt_halverwege_een_ronde_en_alles_slaagt_na_een_vernieuwing() {
    let t = Toestand::nieuw(&["A1"], Some(8), false);
    let server = nepserver(&t).await;
    // Lokaal nog een kwartier geldig: de server trekt het token halverwege toch in.
    let (sessie, bewaring) = nieuwe_sessie(&server, tokens("A1", "R1", 900));
    let client = Client::met_bron(&server.uri(), sessie, openaec_sync::api::nieuwe_http());
    let dir = tempfile::tempdir().unwrap();
    veel_bestanden(dir.path());

    let opslag = ClientOpslag::new(client, "R1").met_gelijktijdig(8);
    let opties = Opties { gelijktijdig: 8, ..Default::default() };
    let r = sync_met_opties(&opslag, dir.path(), &[], &Stil, &opties).await.unwrap();

    assert!(r.fouten.is_empty(), "fouten: {:?}", r.fouten);
    assert!(!r.sessie_verlopen);
    assert_eq!(r.geupload, 40);
    assert!(t.geweigerd.load(Ordering::SeqCst) >= 1, "het token moest onderweg geweigerd worden");
    assert_eq!(t.vernieuwingen.load(Ordering::SeqCst), 1, "precies één vernieuwing");
    // Het nieuwe paar is meteen bewaard (het oude refresh-token werkt niet meer).
    let bewaard: Tokens = serde_json::from_str(&bewaring.lees().unwrap()).unwrap();
    assert_eq!((bewaard.access_token.as_str(), bewaard.refresh_token.as_str()), ("A2", "R2"));
    // De delen naar de opslag gingen zonder Authorization-header.
    assert_eq!(t.s3_verzoeken.load(Ordering::SeqCst), 10);
    assert_eq!(t.s3_met_autorisatie.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ongeldig_refresh_token_stopt_de_ronde_netjes_en_na_inloggen_gaat_hij_verder() {
    let t = Toestand::nieuw(&["A1"], Some(6), true);
    let server = nepserver(&t).await;
    let (sessie, bewaring) = nieuwe_sessie(&server, tokens("A1", "R1", 900));
    let client = Client::met_bron(&server.uri(), sessie.clone(), openaec_sync::api::nieuwe_http());
    let dir = tempfile::tempdir().unwrap();
    veel_bestanden(dir.path());
    let opslag = ClientOpslag::new(client.clone(), "R1").met_gelijktijdig(8);
    let opties = Opties { gelijktijdig: 8, ..Default::default() };

    let r = sync_met_opties(&opslag, dir.path(), &[], &Stil, &opties).await.unwrap();
    assert!(r.sessie_verlopen, "de ronde moet stoppen");
    assert!(r.fouten.is_empty(), "geen fout per bestand: {:?}", r.fouten);
    assert!(r.foutpaden.is_empty());
    assert!(r.geupload < 40);
    // Eén poging om te vernieuwen; daarna vraagt de sessie de server niet steeds opnieuw met hetzelfde refresh-token.
    assert_eq!(t.vernieuwingen.load(Ordering::SeqCst), 1);
    assert!(matches!(client.ruimtes().await, Err(Fout::SessieVerlopen)));
    assert_eq!(t.vernieuwingen.load(Ordering::SeqCst), 1);
    assert_eq!(Fout::SessieVerlopen.to_string(), "Je sessie is verlopen. Log opnieuw in.");

    // Opnieuw ingelogd: nieuwe tokens in de bewaring; de volgende ronde doet de rest.
    t.geldig.lock().unwrap().insert("A9".into());
    bewaring.bewaar(&serde_json::to_string(&tokens("A9", "R9", 900)).unwrap()).unwrap();
    sessie.vergeet().await;
    let opslag = ClientOpslag::new(client, "R1").met_gelijktijdig(8);
    let r2 = sync_met_opties(&opslag, dir.path(), &[], &Stil, &opties).await.unwrap();
    assert!(!r2.sessie_verlopen);
    assert!(r2.fouten.is_empty(), "fouten: {:?}", r2.fouten);
    assert_eq!(r.geupload + r2.geupload, 40);
}

#[tokio::test]
async fn gelijktijdige_verzoeken_vernieuwen_maar_een_keer() {
    // Lokaal al verlopen: vernieuwen vóór het verzoek.
    let t = Toestand::nieuw(&[], None, false);
    let server = nepserver(&t).await;
    let (sessie, _) = nieuwe_sessie(&server, tokens("A1", "R1", -10));
    let client = Client::met_bron(&server.uri(), sessie, openaec_sync::api::nieuwe_http());
    let taken: Vec<_> = (0..20).map(|_| tokio::spawn({ let c = client.clone(); async move { c.ruimtes().await } })).collect();
    for taak in taken {
        assert_eq!(taak.await.unwrap().unwrap()[0].name, "R1");
    }
    assert_eq!(t.vernieuwingen.load(Ordering::SeqCst), 1);

    // Lokaal nog geldig, maar de server weigert het (bijv. ingetrokken na een uitrol): na de 401's één vernieuwing.
    let t = Toestand::nieuw(&[], None, false);
    let server = nepserver(&t).await;
    let (sessie, _) = nieuwe_sessie(&server, tokens("A1", "R1", 900));
    let client = Client::met_bron(&server.uri(), sessie, openaec_sync::api::nieuwe_http());
    let taken: Vec<_> = (0..20).map(|_| tokio::spawn({ let c = client.clone(); async move { c.ruimtes().await } })).collect();
    for taak in taken {
        taak.await.unwrap().unwrap();
    }
    assert!(t.geweigerd.load(Ordering::SeqCst) >= 1);
    assert_eq!(t.vernieuwingen.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn api_sleutel_wordt_niet_vernieuwd() {
    let t = Toestand::nieuw(&["token a:b"], None, false);
    let server = nepserver(&t).await;
    let sessie = Arc::new(Sessie::new(&server.uri(), Geheugen::met("token a:b")));
    assert_eq!(sessie.geef().await.unwrap(), "token a:b");
    let client = Client::met_bron(&server.uri(), sessie, openaec_sync::api::nieuwe_http());
    client.ruimtes().await.unwrap();
    // Een geweigerde sleutel: geen vernieuwing, wel een nette melding (geen ruwe JSON).
    let fout = Client::new(&server.uri(), "token x:y").ruimtes().await.unwrap_err();
    assert!(matches!(fout, Fout::SessieVerlopen));
    assert!(!fout.to_string().contains("exc_type"));
    assert_eq!(t.vernieuwingen.load(Ordering::SeqCst), 0);
}

/// Downloadlinks: zonder Authorization-header, en bij een verlopen link (403) een nieuwe link in plaats van een
/// nieuwe sessie.
#[tokio::test]
async fn ondertekende_downloadlink_zonder_autorisatie_en_vervangen_als_hij_verlopen_is() {
    let server = MockServer::start().await;
    let links = Arc::new(AtomicUsize::new(0));
    let met_autorisatie = Arc::new(AtomicUsize::new(0));
    let uri = server.uri();
    let l = links.clone();
    Mock::given(method("POST"))
        .and(path(format!("{API}download")))
        .respond_with(move |_: &Request| {
            let n = l.fetch_add(1, Ordering::SeqCst);
            let naam = if n == 0 { "oud" } else { "nieuw" };
            bericht(json!({"url": format!("{uri}/s3/{naam}"), "versie": 1}))
        })
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}blokken")))
        .respond_with(bericht(json!({"versie": 1, "grootte": 5, "blokgrootte": 4194304, "hashes": null})))
        .mount(&server)
        .await;
    for (naam, status) in [("oud", 403u16), ("nieuw", 206)] {
        let m = met_autorisatie.clone();
        Mock::given(method("GET"))
            .and(path(format!("/s3/{naam}")))
            .respond_with(move |v: &Request| {
                if v.headers.get("authorization").is_some() {
                    m.fetch_add(1, Ordering::SeqCst);
                }
                match status {
                    403 => ResponseTemplate::new(403).set_body_string("<Error><Code>AccessDenied</Code></Error>"),
                    _ => ResponseTemplate::new(206).set_body_bytes(b"hallo".to_vec()),
                }
            })
            .mount(&server)
            .await;
    }
    let dir = tempfile::tempdir().unwrap();
    let doel = dir.path().join("a.txt");
    let client = Client::new(&server.uri(), "token a:b");
    openaec_sync::download_bestand(&client, "B1", &doel).await.unwrap();
    assert_eq!(std::fs::read(&doel).unwrap(), b"hallo");
    assert_eq!(links.load(Ordering::SeqCst), 2, "een nieuwe link na de 403");
    assert_eq!(met_autorisatie.load(Ordering::SeqCst), 0);
}

#[test]
fn foutteksten_voor_mensen_zonder_ruwe_json() {
    let server = |status: u16, antwoord: Value| Fout::Server { status, tekst: antwoord.to_string() };
    let tekst = |f: Fout| f.to_string();
    assert_eq!(tekst(server(401, json!({"exc_type": "AuthenticationError"}))), "Je sessie is verlopen. Log opnieuw in.");
    assert_eq!(tekst(server(403, json!({"exc_type": "PermissionError"}))), "Je hebt hiervoor geen rechten.");
    assert_eq!(tekst(server(403, json!({"exception": "frappe.exceptions.PermissionError"}))), "Je hebt hiervoor geen rechten.");
    assert_eq!(tekst(server(404, json!({"exc_type": "DoesNotExistError"}))), "Dit bestand of deze map bestaat niet (meer) op de server.");
    let validatie = json!({"exception": "frappe.exceptions.ValidationError: Deze naam bestaat al"});
    assert_eq!(tekst(server(417, validatie)), "Deze naam bestaat al");
    assert_eq!(tekst(server(417, json!({"exc_type": "ValidationError"}))), "De server weigerde dit verzoek.");
    assert!(tekst(server(500, json!({"exc_type": "Exception", "exception": "Traceback …"}))).starts_with("Er ging iets mis op de server"));
    assert!(tekst(Fout::Server { status: 502, tekst: "<html>Bad gateway</html>".into() }).contains("tijdelijk niet bereikbaar"));
    assert!(tekst(Fout::Link { status: 403, tekst: "<Error/>".into() }).contains("link naar de opslag is verlopen"));
    for status in [400, 401, 403, 404, 409, 417, 500, 503] {
        let t = tekst(server(status, json!({"exc_type": "X", "foutcode": "y"})));
        assert!(!t.contains('{') && !t.contains("exc_type"), "{status}: {t}");
    }
}
