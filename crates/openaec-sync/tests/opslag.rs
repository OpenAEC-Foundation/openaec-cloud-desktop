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

/// De app draait een ronde als losse taak (`tokio::spawn`): de future van een ronde met de echte opslag moet `Send`
/// zijn, ook nu de overdrachten parallel lopen.
#[test]
fn een_ronde_met_de_echte_opslag_is_send() {
    fn moet_send<T: Send>(_: &T) {}
    let opslag = ClientOpslag::new(Client::new("http://localhost", "token a:b"), "R1");
    let (w, opties) = (openaec_sync::voortgang::Stil, openaec_sync::motor::Opties::default());
    let ronde = openaec_sync::motor::sync_met_opties(&opslag, std::path::Path::new("."), &[], &w, &opties);
    moet_send(&ronde);
}

fn mogelijkheden(server: &MockServer) -> Mock {
    let grenzen = json!({"max_bestand": 1048576, "max_aantal": 64, "max_bytes": 16777216});
    let _ = server;
    Mock::given(method("POST"))
        .and(path(format!("{API}mogelijkheden")))
        .respond_with(antwoord(json!({"upload_klein": grenzen, "download_klein": grenzen, "boom": true, "maak_mappen": true})))
}

#[tokio::test]
async fn oude_server_kent_geen_kleine_bestanden_in_een_verzoek() {
    let server = MockServer::start().await;
    let opslag = ClientOpslag::new(Client::new(&server.uri(), "token a:b"), "R1");
    assert_eq!(opslag.klein_grenzen().await, None);
}

#[tokio::test]
async fn nieuwe_server_geeft_de_hele_ruimte_in_een_verzoek() {
    let server = MockServer::start().await;
    mogelijkheden(&server).expect(1).mount(&server).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}boom")))
        .and(body_partial_json(json!({"ruimte": "R1", "start": 0})))
        .respond_with(antwoord(json!({
            "mappen": [{"name": "M2", "naam": "2026", "bovenliggende": "M1"}, {"name": "M1", "naam": "Tekeningen", "bovenliggende": null}],
            "bestanden": [
                {"name": "B1", "naam": "plan.pdf", "map": "M2", "grootte": 10, "huidige_versie": 3},
                {"name": "B2", "naam": "lees mij.txt", "map": null, "grootte": 3, "huidige_versie": 1}
            ],
            "meer": false
        })))
        // lijst en alle_mappen; mogelijkheden maar één keer.
        .expect(2)
        .mount(&server)
        .await;
    let opslag = ClientOpslag::new(Client::new(&server.uri(), "token a:b"), "R1");
    let mut lijst = opslag.lijst().await.unwrap();
    lijst.sort_by(|a, b| a.pad.cmp(&b.pad));
    let paden: Vec<_> = lijst.iter().map(|b| (b.pad.as_str(), b.bestand.as_str(), b.versie)).collect();
    assert_eq!(paden, vec![("Tekeningen/2026/plan.pdf", "B1", 3), ("lees mij.txt", "B2", 1)]);
    assert_eq!(opslag.alle_mappen().await.unwrap(), vec!["Tekeningen", "Tekeningen/2026"]);
    assert_eq!(opslag.klein_grenzen().await.unwrap().max_aantal, 64);
}

#[tokio::test]
async fn kleine_bestanden_gaan_in_een_multipartverzoek() {
    let server = MockServer::start().await;
    mogelijkheden(&server).mount(&server).await;
    // Een map die nog niet bekend is, regelt de server in hetzelfde verzoek (geen maak_map).
    Mock::given(method("POST"))
        .and(path(format!("{API}maak_map")))
        .respond_with(antwoord(json!({"map": "MN", "nieuw": true})))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}upload_klein")))
        .respond_with(antwoord(json!({"resultaten": [
            {"index": 0, "ok": true, "bestand": "B1", "naam": "a.txt", "versie": 1, "conflict": false, "map": "MN"},
            {"index": 1, "ok": false, "fout": "In de map Notities zijn alleen deze bestandssoorten toegestaan: .txt", "foutcode": "extensie_niet_toegestaan"},
            {"index": 2, "ok": true, "bestand": "B3", "naam": "c (conflict Jan 2026-10-23 1200).txt", "versie": 1, "conflict": true, "map": null}
        ]})))
        .expect(2)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    for (naam, inhoud) in [("a.txt", "inhoud a"), ("b.exe", "inhoud b"), ("c.txt", "inhoud c")] {
        std::fs::write(dir.path().join(naam), inhoud).unwrap();
    }
    let (a, b, c) = (dir.path().join("a.txt"), dir.path().join("b.exe"), dir.path().join("c.txt"));
    let opslag = ClientOpslag::new(Client::new(&server.uri(), "token a:b"), "R1");
    let items = [
        openaec_sync::motor::KleinUpload { pad: "Notities/a.txt", lokaal: &a, basis_versie: None },
        openaec_sync::motor::KleinUpload { pad: "Notities/b.exe", lokaal: &b, basis_versie: Some(2) },
        openaec_sync::motor::KleinUpload { pad: "c.txt", lokaal: &c, basis_versie: Some(1) },
    ];
    let r = opslag.upload_klein(&items).await.unwrap();
    assert_eq!(r[0].as_ref().unwrap().pad, "Notities/a.txt");
    let fout = r[1].as_ref().unwrap_err();
    assert_eq!(fout.foutcode().as_deref(), Some("extensie_niet_toegestaan"));
    assert!(fout.to_string().contains(".txt"));
    let conflict = r[2].as_ref().unwrap();
    assert!(conflict.conflict);
    assert_eq!(conflict.pad, "c (conflict Jan 2026-10-23 1200).txt");

    let verzoek = server.received_requests().await.unwrap().into_iter().find(|v| v.url.path().ends_with("upload_klein")).unwrap();
    let body = String::from_utf8_lossy(&verzoek.body);
    assert!(verzoek.headers.get("content-type").unwrap().to_str().unwrap().starts_with("multipart/form-data"));
    assert!(body.contains("name=\"inhoud\"") && body.contains("inhoud ainhoud binhoud c"), "{body}");
    let lijst = body.split("name=\"bestanden\"").nth(1).unwrap();
    assert!(lijst.contains("\"pad\":\"Notities\"") && lijst.contains("\"basis_versie\":2") && lijst.contains("\"blokhash\"") && lijst.contains("\"sha256\""));
    // Het map-id uit het antwoord wordt onthouden: een volgende upload in die map gebruikt het.
    let tweede = [openaec_sync::motor::KleinUpload { pad: "Notities/a.txt", lokaal: &a, basis_versie: Some(1) }];
    let _ = opslag.upload_klein(&tweede).await;
    let verzoeken = server.received_requests().await.unwrap();
    let laatste = verzoeken.iter().rev().find(|v| v.url.path().ends_with("upload_klein")).unwrap();
    assert!(String::from_utf8_lossy(&laatste.body).contains("\"map\":\"MN\""));
}

#[tokio::test]
async fn kleine_downloads_worden_gecontroleerd_en_weggeschreven() {
    use base64::Engine;
    let server = MockServer::start().await;
    mogelijkheden(&server).mount(&server).await;
    let inhoud = b"klein bestand".to_vec();
    let hash = openaec_sync::blokken::hash_bytes(&inhoud, openaec_sync::blokken::BLOK)[0].clone();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&inhoud);
    Mock::given(method("POST"))
        .and(path(format!("{API}download_klein")))
        .and(body_partial_json(json!({"bestanden": ["B1", "B2", "B3"]})))
        .respond_with(antwoord(json!({"bestanden": [
            {"index": 0, "ok": true, "bestand": "B1", "versie": 1, "grootte": inhoud.len(), "blokhash": hash, "inhoud": b64},
            {"index": 1, "ok": true, "bestand": "B2", "versie": 1, "grootte": inhoud.len(), "blokhash": "verkeerd", "inhoud": b64},
            {"index": 2, "ok": false, "fout": "Geen toegang tot deze plek", "foutcode": null}
        ]})))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let opslag = ClientOpslag::new(Client::new(&server.uri(), "token a:b"), "R1");
    let bestand = |id: &str| openaec_sync::motor::OpslagBestand { pad: id.into(), bestand: id.into(), versie: 1, grootte: 13 };
    let (b1, b2, b3) = (bestand("B1"), bestand("B2"), bestand("B3"));
    let (p1, p2, p3) = (dir.path().join("diep/een.txt"), dir.path().join("twee.txt"), dir.path().join("drie.txt"));
    let r = opslag.download_klein(&[(&b1, &p1), (&b2, &p2), (&b3, &p3)]).await.unwrap();
    assert!(r[0].is_ok());
    assert_eq!(std::fs::read(&p1).unwrap(), inhoud);
    assert!(r[1].as_ref().unwrap_err().to_string().contains("blokhash"));
    assert!(!p2.exists(), "een onjuiste download komt er niet");
    assert!(r[2].is_err());
}

#[tokio::test]
async fn nieuwe_server_maakt_alle_mappen_in_een_verzoek() {
    let server = MockServer::start().await;
    mogelijkheden(&server).mount(&server).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}maak_mappen")))
        .and(body_partial_json(json!({"ruimte": "R1", "paden": ["A", "A/B", "C"]})))
        .respond_with(antwoord(json!({"mappen": {"A": "MA", "A/B": "MB", "C": "MC"}, "fouten": {}})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST")).and(path(format!("{API}maak_map"))).respond_with(antwoord(json!({}))).expect(0).mount(&server).await;
    let opslag = ClientOpslag::new(Client::new(&server.uri(), "token a:b"), "R1");
    opslag.bereid_mappen(&["A".into(), "A/B".into(), "C".into()]).await.unwrap();
    // Al bekend: geen nieuw verzoek.
    opslag.bereid_mappen(&["A/B".into()]).await.unwrap();
}
