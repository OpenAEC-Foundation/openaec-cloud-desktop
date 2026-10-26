//! Van een mappad naar het id op de server ([`ClientOpslag::zoek_map`]): in één verzoek op een nieuwere server
//! (`zoek_pad`), en map voor map op een oudere, met paden met spaties en hoofdletters, diepe mappen, veel mappen naast
//! elkaar en dubbele mappen.

use openaec_sync::opslag::ClientOpslag;
use openaec_sync::Client;
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const API: &str = "/api/method/openaec_storage.api.";

fn antwoord(message: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "message": message }))
}

async fn mogelijk(server: &MockServer, zoek_pad: bool) {
    Mock::given(method("POST"))
        .and(path(format!("{API}mogelijkheden")))
        .respond_with(antwoord(json!({"boom": true, "maak_mappen": true, "deling": true, "zoek_pad": zoek_pad})))
        .mount(server)
        .await;
}

/// `bladeren` van een oudere server: per map-id (None = hoofdniveau) de mappen erin.
async fn map_met(server: &MockServer, map: Option<&str>, mappen: serde_json::Value, bestanden: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path(format!("{API}bladeren")))
        .and(body_partial_json(json!({"ruimte": "R1", "map": map})))
        .respond_with(antwoord(json!({"mappen": mappen, "bestanden": bestanden})))
        .mount(server)
        .await;
}

fn opslag(server: &MockServer) -> ClientOpslag {
    ClientOpslag::new(Client::new(&server.uri(), "token a:b"), "R1")
}

#[tokio::test]
async fn nieuwere_server_zoekt_het_pad_in_een_verzoek() {
    let server = MockServer::start().await;
    mogelijk(&server, true).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}zoek_pad")))
        .and(body_partial_json(json!({"ruimte": "R1", "pad": "50_projecten/05 Klant Noord/Fase 2", "soort": "map"})))
        .respond_with(antwoord(json!({"map": "M3", "gevonden": true})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}zoek_pad")))
        .and(body_partial_json(json!({"pad": "Bestaat niet"})))
        .respond_with(antwoord(json!({"map": null, "gevonden": false})))
        .mount(&server)
        .await;
    let o = opslag(&server);
    // Ook met slashes van Windows en een slash aan het eind.
    assert_eq!(o.zoek_map("50_projecten\\05 Klant Noord\\Fase 2\\").await.unwrap().as_deref(), Some("M3"));
    assert_eq!(o.zoek_map("Bestaat niet").await.unwrap(), None);
    assert_eq!(o.zoek_map("").await.unwrap(), None, "het hoofdniveau van de ruimte");
    // Geen enkele keer bladeren.
    let verzoeken = server.received_requests().await.unwrap();
    assert!(verzoeken.iter().all(|r| !r.url.path().ends_with("bladeren")));
}

#[tokio::test]
async fn gedeelde_map_stuurt_de_deling_mee() {
    let server = MockServer::start().await;
    mogelijk(&server, true).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}zoek_pad")))
        .and(body_partial_json(json!({"deling": "D1", "pad": "Sub"})))
        .respond_with(antwoord(json!({"map": "S1", "gevonden": true})))
        .expect(1)
        .mount(&server)
        .await;
    let o = ClientOpslag::gedeeld(Client::new(&server.uri(), "token a:b"), "R1", "D1", "W1", "lezen");
    assert_eq!(o.zoek_map("Sub").await.unwrap().as_deref(), Some("S1"));
    assert_eq!(o.zoek_map("").await.unwrap().as_deref(), Some("W1"), "het hoofdniveau is de gedeelde map");
}

#[tokio::test]
async fn oudere_server_diep_pad_met_spaties_en_andere_hoofdletters() {
    let server = MockServer::start().await;
    map_met(&server, None, json!([{"name": "M1", "naam": "50_projecten"}, {"name": "MX", "naam": "60_administratie"}]), json!([])).await;
    map_met(&server, Some("M1"), json!([{"name": "M2", "naam": "05 Klant Noord"}]), json!([])).await;
    map_met(&server, Some("M2"), json!([{"name": "M3", "naam": "Fase 2 - Ontwerp "}]), json!([])).await;
    map_met(&server, Some("M3"), json!([{"name": "M4", "naam": "Tekeningen (definitief)"}]), json!([])).await;
    let o = opslag(&server);
    assert_eq!(
        o.zoek_map("50_projecten/05 Klant Noord/Fase 2 - Ontwerp/Tekeningen (definitief)").await.unwrap().as_deref(),
        Some("M4")
    );
    assert_eq!(o.zoek_map("50_PROJECTEN/05 klant noord/fase 2 - ontwerp").await.unwrap().as_deref(), Some("M3"));
    assert_eq!(o.zoek_map("50_projecten/Onbekend").await.unwrap(), None);
}

#[tokio::test]
async fn oudere_server_veel_mappen_naast_elkaar() {
    let server = MockServer::start().await;
    let mut mappen: Vec<_> = (0..2500).map(|i| json!({"name": format!("M{i}"), "naam": format!("Map {i:04}")})).collect();
    mappen.push(json!({"name": "LAATST", "naam": "Zzz laatste"}));
    map_met(&server, None, json!(mappen), json!([])).await;
    let o = opslag(&server);
    assert_eq!(o.zoek_map("Zzz laatste").await.unwrap().as_deref(), Some("LAATST"));
    assert_eq!(o.zoek_map("map 1234").await.unwrap().as_deref(), Some("M1234"));
}

#[tokio::test]
async fn oudere_server_dubbele_mappen_zoekt_in_allebei() {
    let server = MockServer::start().await;
    // Twee keer "Projecten" op het hoofdniveau; de submap staat in de tweede.
    map_met(&server, None, json!([{"name": "P1", "naam": "Projecten"}, {"name": "P2", "naam": "Projecten"}]), json!([])).await;
    map_met(&server, Some("P1"), json!([]), json!([])).await;
    map_met(&server, Some("P2"), json!([{"name": "H1", "naam": "Havenkwartier"}]), json!([])).await;
    let o = opslag(&server);
    assert_eq!(o.zoek_map("Projecten/Havenkwartier").await.unwrap().as_deref(), Some("H1"));
    // Allebei passend: de map met inhoud.
    assert_eq!(o.zoek_map("Projecten").await.unwrap().as_deref(), Some("P2"));
}

#[tokio::test]
async fn precieze_schrijfwijze_gaat_voor() {
    let server = MockServer::start().await;
    map_met(&server, None, json!([{"name": "K", "naam": "tekeningen"}, {"name": "G", "naam": "Tekeningen"}]), json!([])).await;
    let o = opslag(&server);
    assert_eq!(o.zoek_map("Tekeningen").await.unwrap().as_deref(), Some("G"));
    assert_eq!(o.zoek_map("tekeningen").await.unwrap().as_deref(), Some("K"));
}

#[tokio::test]
async fn lege_map_aanmaken_maar_niet_in_een_map_om_te_bekijken() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}maak_map")))
        .and(body_partial_json(json!({"naam": "OpenAEC Foundation", "bovenliggende": null})))
        .respond_with(antwoord(json!({"map": "NIEUW", "nieuw": true})))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(opslag(&server).maak_mappad("OpenAEC Foundation/").await.unwrap().as_deref(), Some("NIEUW"));
    let alleen_lezen = ClientOpslag::gedeeld(Client::new(&server.uri(), "token a:b"), "R1", "D1", "W1", "lezen");
    assert!(alleen_lezen.maak_mappad("Nieuw").await.is_err());
}

/// Een gekoppelde map met een toestand van de laatste ronde: `in_sync` (pad, bestand-id) staat erin zoals hij hier is.
fn koppeling(bestanden: &[(&str, &[u8])], in_sync: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (pad, inhoud) in bestanden {
        let p = dir.path().join(pad);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, inhoud).unwrap();
    }
    let mut staat = serde_json::Map::new();
    for (pad, id) in in_sync {
        let m = std::fs::metadata(dir.path().join(pad)).unwrap();
        let mtime = m.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        staat.insert(pad.to_string(), json!({"bestand": id, "versie": 1, "grootte": m.len(), "mtime_ns": mtime as u64}));
    }
    std::fs::create_dir_all(dir.path().join(".openaec")).unwrap();
    std::fs::write(dir.path().join(".openaec/staat.json"), json!({ "bestanden": staat }).to_string()).unwrap();
    dir
}

async fn niet_gevonden(server: &MockServer) {
    mogelijk(server, true).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}zoek_pad")))
        .respond_with(antwoord(json!({"map": null, "gevonden": false})))
        .mount(server)
        .await;
}

#[tokio::test]
async fn lege_map_wordt_bij_delen_aangemaakt() {
    use openaec_sync::opslag::Mapplek;
    let server = MockServer::start().await;
    niet_gevonden(&server).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}maak_map")))
        .and(body_partial_json(json!({"naam": "OpenAEC Foundation"})))
        .respond_with(antwoord(json!({"map": "F1", "nieuw": true})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}maak_map")))
        .and(body_partial_json(json!({"naam": "Post in", "bovenliggende": "F1"})))
        .respond_with(antwoord(json!({"map": "F2", "nieuw": true})))
        .expect(1)
        .mount(&server)
        .await;
    // Alleen een lege submap en een bestand op de negeerlijst: voor de sync is dit een lege map.
    let dir = koppeling(&[("OpenAEC Foundation/Post in/Thumbs.db", b"x")], &[]);
    std::fs::create_dir_all(dir.path().join("OpenAEC Foundation/Post in/Leeg")).unwrap();
    let o = opslag(&server);
    assert_eq!(o.map_van_lokaal(dir.path(), "OpenAEC Foundation/Post in", true).await.unwrap(), Mapplek::Gevonden("F2".into()));
    // Zonder aanmaken: leeg.
    assert_eq!(o.map_van_lokaal(dir.path(), "OpenAEC Foundation/Post in", false).await.unwrap(), Mapplek::Leeg);
}

#[tokio::test]
async fn map_met_bestanden_die_nog_gesynchroniseerd_worden() {
    use openaec_sync::opslag::Mapplek;
    let server = MockServer::start().await;
    niet_gevonden(&server).await;
    Mock::given(method("POST")).and(path(format!("{API}maak_map"))).respond_with(antwoord(json!({"map": "X"}))).expect(0).mount(&server).await;
    let dir = koppeling(&[("Nieuw/plan.pdf", b"plan")], &[]);
    let uit = opslag(&server).map_van_lokaal(dir.path(), "Nieuw", true).await.unwrap();
    assert_eq!(uit, Mapplek::NogNietGesynchroniseerd);
}

#[tokio::test]
async fn gesynchroniseerd_bestand_weet_zijn_map() {
    use openaec_sync::opslag::Mapplek;
    let server = MockServer::start().await;
    niet_gevonden(&server).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}info")))
        .and(body_partial_json(json!({"bestand": "B1"})))
        .respond_with(antwoord(json!({"bestand": "B1", "naam": "plan.pdf", "map": "M9", "huidige_versie": 1})))
        .mount(&server)
        .await;
    let dir = koppeling(&[("Fase 2/plan.pdf", b"plan")], &[("Fase 2/plan.pdf", "B1")]);
    assert_eq!(opslag(&server).map_van_lokaal(dir.path(), "Fase 2", true).await.unwrap(), Mapplek::Gevonden("M9".into()));
}
