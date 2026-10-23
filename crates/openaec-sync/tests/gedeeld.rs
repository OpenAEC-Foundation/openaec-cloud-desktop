//! Een map die met mij gedeeld is: de motor bij alleen lezen (niets naar de server, wel een melding), en
//! ClientOpslag met de gedeelde map als wortel tegen een nagebootste API.

mod common;

use common::{schrijf, NepOpslag};
use openaec_sync::motor::{overgeslagen, sync_eenmalig, Opslag, ALLEEN_LEZEN};
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::{Client, Fout};
use serde_json::json;
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const API: &str = "/api/method/openaec_storage.api.";

fn antwoord(message: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "message": message }))
}

fn weigering(foutcode: &str) -> ResponseTemplate {
    ResponseTemplate::new(403).set_body_json(json!({ "exc_type": "PermissionError", "foutcode": foutcode }))
}

#[tokio::test]
async fn alleen_lezen_stuurt_niets_naar_de_server_en_meldt_het() {
    let dir = tempfile::tempdir().unwrap();
    let opslag = NepOpslag { alleen_lezen: true, ..Default::default() };
    opslag.zet("plan.pdf", b"v1");
    opslag.zet("Details/detail.dwg", b"d1");
    opslag.zet("weg.txt", b"w");

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.gedownload, r.geupload), (3, 0));

    // Lokaal: een wijziging, een nieuw bestand en een verwijderd bestand.
    schrijf(dir.path(), "plan.pdf", b"mijn versie");
    schrijf(dir.path(), "nieuw.txt", b"nieuw");
    std::fs::remove_file(dir.path().join("weg.txt")).unwrap();
    // Op de server een nieuwe versie van een bestand dat hier niet gewijzigd is.
    opslag.zet("Details/detail.dwg", b"d2");

    for ronde in 0..2 {
        let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
        assert!(r.fouten.is_empty(), "ronde {ronde}: {:?}", r.fouten);
        assert_eq!(r.geupload, 0);
        let mut paden: Vec<_> = r.overgeslagen.iter().map(|o| o.pad.as_str()).collect();
        paden.sort();
        assert_eq!(paden, ["nieuw.txt", "plan.pdf"], "ronde {ronde}");
        assert!(r.overgeslagen.iter().all(|o| o.reden.starts_with(ALLEEN_LEZEN) && !o.opruimbaar));
    }
    // Het lokale werk blijft staan, de server is ongewijzigd; het verwijderde bestand is terug; de nieuwe versie is er.
    assert_eq!(std::fs::read(dir.path().join("plan.pdf")).unwrap(), b"mijn versie");
    assert_eq!(opslag.inhoud("plan.pdf").unwrap(), b"v1");
    assert!(opslag.inhoud("nieuw.txt").is_none());
    assert_eq!(std::fs::read(dir.path().join("weg.txt")).unwrap(), b"w");
    assert_eq!(std::fs::read(dir.path().join("Details/detail.dwg")).unwrap(), b"d2");
    // Ook in de lijst met overgeslagen bestanden (statusvenster en Overzicht).
    assert_eq!(overgeslagen(dir.path()).len(), 2);
}

#[tokio::test]
async fn alleen_lezen_haalt_een_op_de_server_verwijderd_bestand_ook_hier_weg() {
    let dir = tempfile::tempdir().unwrap();
    let opslag = NepOpslag { alleen_lezen: true, ..Default::default() };
    opslag.zet("oud.txt", b"oud");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    opslag.bestanden.lock().unwrap().remove("oud.txt");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.verwijderd, 1);
    assert!(!dir.path().join("oud.txt").exists());
}

/// Een server die `deling` kent: de boom onder de gedeelde map W (met submap S).
async fn server_met_boom() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}mogelijkheden")))
        .respond_with(antwoord(json!({
            "upload_klein": {"max_bestand": 1048576, "max_aantal": 64, "max_bytes": 16777216},
            "download_klein": {"max_bestand": 1048576, "max_aantal": 64, "max_bytes": 16777216},
            "boom": true, "maak_mappen": true, "deling": true
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}boom")))
        .and(body_partial_json(json!({"ruimte": "R1", "deling": "D1"})))
        .respond_with(antwoord(json!({
            "wortel": "W",
            "mappen": [{"name": "S", "naam": "Details", "bovenliggende": "W"}],
            "bestanden": [
                {"name": "B1", "naam": "plan.pdf", "map": "W", "grootte": 4, "huidige_versie": 2},
                {"name": "B2", "naam": "detail.dwg", "map": "S", "grootte": 6, "huidige_versie": 1}
            ],
            "meer": false
        })))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn gedeelde_map_is_het_hoofdniveau() {
    let server = server_met_boom().await;
    let opslag = ClientOpslag::gedeeld(Client::new(&server.uri(), "token a:b"), "R1", "D1", "W", "schrijven");
    assert_eq!(opslag.deling(), Some("D1"));
    assert!(!opslag.alleen_lezen());
    let mut lijst = opslag.lijst().await.unwrap();
    lijst.sort_by(|a, b| a.pad.cmp(&b.pad));
    let paden: Vec<_> = lijst.iter().map(|b| (b.pad.as_str(), b.bestand.as_str())).collect();
    assert_eq!(paden, [("Details/detail.dwg", "B2"), ("plan.pdf", "B1")]);
    assert_eq!(opslag.alle_mappen().await.unwrap(), ["Details"]);
    assert!(ClientOpslag::gedeeld(Client::new(&server.uri(), "token a:b"), "R1", "D1", "W", "lezen").alleen_lezen());
}

#[tokio::test]
async fn uploads_en_mappen_komen_onder_de_gedeelde_map() {
    let server = server_met_boom().await;
    // Een upload op het hoofdniveau gaat naar de gedeelde map zelf.
    Mock::given(method("POST"))
        .and(path(format!("{API}upload_start")))
        .and(body_partial_json(json!({"ruimte": "R1", "map": "W", "naam": "groot.bin"})))
        .respond_with(antwoord(json!({"upload": "U1", "deelgrootte": 8388608, "hergebruik": [], "delen": []})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}upload_klaar")))
        .respond_with(antwoord(json!({"bestand": "B9", "naam": "groot.bin", "versie": 1, "conflict": false})))
        .mount(&server)
        .await;
    // Een ontbrekende map: met de deling erbij, het pad binnen de gedeelde map.
    Mock::given(method("POST"))
        .and(path(format!("{API}maak_mappen")))
        .and(body_partial_json(json!({"ruimte": "R1", "deling": "D1", "paden": ["Nieuw"]})))
        .respond_with(antwoord(json!({"mappen": {"Nieuw": "N"}, "fouten": {}})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}upload_klein")))
        .and(body_string_contains("name=\"deling\"\r\n\r\nD1"))
        .respond_with(antwoord(json!({"resultaten": [
            {"index": 0, "ok": true, "bestand": "B10", "naam": "a.txt", "versie": 1, "conflict": false, "map": "W"},
            {"index": 1, "ok": true, "bestand": "B11", "naam": "b.txt", "versie": 1, "conflict": false, "map": "N"}
        ]})))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let opslag = ClientOpslag::gedeeld(Client::new(&server.uri(), "token a:b"), "R1", "D1", "W", "schrijven");
    opslag.lijst().await.unwrap();
    let groot = dir.path().join("groot.bin");
    std::fs::write(&groot, b"").unwrap();
    opslag.upload("groot.bin", &groot, None).await.unwrap();
    opslag.bereid_mappen(&["Nieuw".to_string(), "Details".to_string()]).await.unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b, b"b").unwrap();
    let items = [
        openaec_sync::motor::KleinUpload { pad: "a.txt", lokaal: &a, basis_versie: None },
        openaec_sync::motor::KleinUpload { pad: "Nieuw/b.txt", lokaal: &b, basis_versie: None },
    ];
    let r = opslag.upload_klein(&items).await.unwrap();
    assert_eq!(r.iter().map(|u| u.as_ref().unwrap().pad.as_str()).collect::<Vec<_>>(), ["a.txt", "Nieuw/b.txt"]);
}

#[tokio::test]
async fn een_oudere_server_bladert_vanaf_de_gedeelde_map() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}mogelijkheden")))
        .respond_with(antwoord(json!({"boom": true, "maak_mappen": true})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}bladeren")))
        .and(body_partial_json(json!({"ruimte": "R1", "map": "W"})))
        .respond_with(antwoord(json!({"mappen": [], "bestanden": [{"name": "B1", "naam": "plan.pdf", "grootte": 4, "huidige_versie": 1}]})))
        .expect(1)
        .mount(&server)
        .await;
    let opslag = ClientOpslag::gedeeld(Client::new(&server.uri(), "token a:b"), "R1", "D1", "W", "lezen");
    let lijst = opslag.lijst().await.unwrap();
    assert_eq!(lijst.iter().map(|b| b.pad.as_str()).collect::<Vec<_>>(), ["plan.pdf"]);
    assert!(opslag.klein_grenzen().await.is_none());
}

#[tokio::test]
async fn ingetrokken_deling_stopt_de_ronde_met_de_foutcode() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}mogelijkheden")))
        .respond_with(antwoord(json!({"boom": true, "maak_mappen": true, "deling": true})))
        .mount(&server)
        .await;
    Mock::given(method("POST")).and(path(format!("{API}boom"))).respond_with(weigering("deling_ingetrokken")).mount(&server).await;
    // Per map proberen heeft dan geen zin.
    Mock::given(method("POST")).and(path(format!("{API}bladeren"))).respond_with(weigering("deling_ingetrokken")).expect(0).mount(&server).await;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("blijft.txt"), b"lokaal werk").unwrap();
    let opslag = ClientOpslag::gedeeld(Client::new(&server.uri(), "token a:b"), "R1", "D1", "W", "schrijven");
    let fout = sync_eenmalig(&opslag, dir.path()).await.unwrap_err();
    assert_eq!(fout.deling_gestopt().as_deref(), Some("deling_ingetrokken"));
    assert_eq!(std::fs::read(dir.path().join("blijft.txt")).unwrap(), b"lokaal werk");
}

#[test]
fn deling_gestopt_alleen_bij_de_vaste_foutcodes() {
    let server = |code: &str| Fout::Server { status: 403, tekst: json!({"foutcode": code}).to_string() };
    for code in ["deling_ingetrokken", "deling_verlopen", "deling_weg", "deling_onbekend"] {
        assert_eq!(server(code).deling_gestopt().as_deref(), Some(code));
    }
    assert_eq!(server("alleen_lezen_deling").deling_gestopt(), None);
    assert_eq!(server("buiten_deling").deling_gestopt(), None);
    assert_eq!(Fout::Controle("geen verbinding".into()).deling_gestopt(), None);
}
