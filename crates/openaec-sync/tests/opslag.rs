//! ClientOpslag tegen een nagebootste API: bladeren wordt een platte lijst met paden, ontbrekende mappen
//! worden bij een upload aangemaakt, lege Frappe-antwoorden worden geaccepteerd.

use openaec_sync::motor::Opslag;
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::Client;
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const API: &str = "/api/method/openaec_storage.api.";

fn antwoord(message: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "message": message }))
}

#[tokio::test]
async fn lijst_loopt_de_mappen_recursief_af() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}bladeren")))
        .and(body_partial_json(json!({"ruimte": "R1", "map": null})))
        .respond_with(antwoord(json!({
            "mappen": [{"name": "M1", "naam": "Tekeningen"}],
            "bestanden": [{"name": "B1", "naam": "lees mij.txt", "grootte": 3, "huidige_versie": 2}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}bladeren")))
        .and(body_partial_json(json!({"map": "M1"})))
        .respond_with(antwoord(json!({
            "mappen": [],
            "bestanden": [{"name": "B2", "naam": "plan.pdf", "grootte": 10, "huidige_versie": 1}]
        })))
        .mount(&server)
        .await;

    let opslag = ClientOpslag::new(Client::new(&server.uri(), "token a:b"), "R1");
    let mut lijst = opslag.lijst().await.unwrap();
    lijst.sort_by(|a, b| a.pad.cmp(&b.pad));
    let paden: Vec<_> = lijst.iter().map(|b| (b.pad.as_str(), b.bestand.as_str(), b.versie)).collect();
    assert_eq!(paden, vec![("Tekeningen/plan.pdf", "B2", 1), ("lees mij.txt", "B1", 2)]);
}

#[tokio::test]
async fn upload_maakt_ontbrekende_mappen_en_geeft_conflictpad_terug() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}maak_map")))
        .and(body_partial_json(json!({"naam": "A", "bovenliggende": null})))
        .respond_with(antwoord(json!({"map": "MA", "nieuw": true})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}maak_map")))
        .and(body_partial_json(json!({"naam": "B", "bovenliggende": "MA"})))
        .respond_with(antwoord(json!({"map": "MB", "nieuw": true})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}upload_start")))
        .and(body_partial_json(json!({"map": "MB", "naam": "x.txt", "basis_versie": 4})))
        .respond_with(antwoord(json!({"upload": "U1", "deelgrootte": 8388608, "hergebruik": [1], "delen": []})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}upload_klaar")))
        .respond_with(antwoord(json!({"bestand": "B9", "naam": "x (conflict Jan 2026-10-20).txt", "versie": 1, "conflict": true})))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let lokaal = dir.path().join("x.txt");
    std::fs::write(&lokaal, b"abc").unwrap();
    let opslag = ClientOpslag::new(Client::new(&server.uri(), "token a:b"), "R1");
    let eerste = opslag.upload("A/B/x.txt", &lokaal, Some(4)).await.unwrap();
    assert!(eerste.conflict);
    assert_eq!(eerste.pad, "A/B/x (conflict Jan 2026-10-20).txt");
    // tweede keer: mappen komen uit het geheugen (expect(1) hierboven)
    opslag.upload("A/B/x.txt", &lokaal, Some(4)).await.unwrap();
}

#[tokio::test]
async fn verwijderen_accepteert_een_leeg_antwoord() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}verwijder")))
        .and(body_partial_json(json!({"bestand": "B1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    let opslag = ClientOpslag::new(Client::new(&server.uri(), "token a:b"), "R1");
    let b = openaec_sync::motor::OpslagBestand { pad: "a".into(), bestand: "B1".into(), versie: 1, grootte: 1 };
    opslag.verwijder(&b).await.unwrap();
}
