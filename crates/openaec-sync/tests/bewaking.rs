//! Wanneer moet er gesynchroniseerd worden: lokale gebeurtenissen filteren en de wijzigingenlijst van de server.

use std::path::Path;

use openaec_sync::bewaking::{nieuwe_wijzigingen, telt_mee};
use openaec_sync::Client;
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn interne_en_tijdelijke_bestanden_tellen_niet_mee() {
    let root = Path::new("/werk/Project");
    assert!(telt_mee(root, &root.join("Tekeningen").join("plan.pdf")));
    assert!(!telt_mee(root, &root.join(".openaec").join("staat.json")));
    assert!(!telt_mee(root, &root.join(".openaec").join("staat.json.tmp")));
    assert!(!telt_mee(root, &root.join("model.openaec-download")));
    assert!(!telt_mee(root, Path::new("/elders/x.txt")));
    // een gewone map die toevallig zo begint telt wel mee
    assert!(telt_mee(root, &root.join(".openaec-oud").join("x.txt")));
}

fn wijzigingen(cursor: u64, meer: bool, aantal: usize) -> ResponseTemplate {
    let lijst: Vec<_> = (0..aantal)
        .map(|i| json!({"volgnummer": cursor - i as u64, "soort": "gewijzigd", "doel_doctype": "Opslagbestand", "doel": "B1", "naam": "a.txt"}))
        .collect();
    ResponseTemplate::new(200).set_body_json(json!({"message": {"cursor": cursor, "meer": meer, "wijzigingen": lijst}}))
}

#[tokio::test]
async fn cursor_loopt_alle_paginas_af() {
    let server = MockServer::start().await;
    let pad = "/api/method/openaec_storage.api.wijzigingen";
    Mock::given(method("POST")).and(path(pad)).and(body_partial_json(json!({"sinds": 0})))
        .respond_with(wijzigingen(1000, true, 2)).mount(&server).await;
    Mock::given(method("POST")).and(path(pad)).and(body_partial_json(json!({"sinds": 1000})))
        .respond_with(wijzigingen(1500, false, 1)).mount(&server).await;
    Mock::given(method("POST")).and(path(pad)).and(body_partial_json(json!({"sinds": 1500})))
        .respond_with(wijzigingen(1500, false, 0)).mount(&server).await;

    let client = Client::new(&server.uri(), "token a:b");
    assert_eq!(nieuwe_wijzigingen(&client, "R1", 0).await.unwrap(), (true, 1500));
    assert_eq!(nieuwe_wijzigingen(&client, "R1", 1500).await.unwrap(), (false, 1500));
}
