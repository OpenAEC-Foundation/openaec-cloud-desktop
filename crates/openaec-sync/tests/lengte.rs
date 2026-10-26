//! De opslag (S3-compatibel) weigert een PUT naar een ondertekende URL zonder `Content-Length` met 411
//! (`MissingContentLength`). Deze nagebootste opslag doet hetzelfde; elk uploadpad moet de lengte meesturen, ook bij
//! een leeg bestand.

use openaec_sync::{upload_bestand, Client};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Antwoordt zoals de opslag: zonder `Content-Length` (of met een lengte die niet klopt) een 411, anders een ETag.
struct StrengeOpslag;

impl Respond for StrengeOpslag {
    fn respond(&self, verzoek: &Request) -> ResponseTemplate {
        let lengte = verzoek.headers.get("content-length").and_then(|w| w.to_str().ok()).and_then(|t| t.parse::<usize>().ok());
        match lengte {
            Some(n) if n == verzoek.body.len() => ResponseTemplate::new(200).insert_header("ETag", "\"etag1\""),
            _ => ResponseTemplate::new(411).set_body_string(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>MissingContentLength</Code>\
                 <Message>You must provide the Content-Length HTTP header.</Message></Error>",
            ),
        }
    }
}

/// Een server die één deel vraagt (zoals bij een nieuw bestand) en de upload afrondt als versie 1.
async fn server_voor(naam: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.upload_start"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {
            "upload": "tok", "deelgrootte": 8 * 1024 * 1024, "hergebruik": [],
            "delen": [{"nummer": 1, "url": format!("{}/s3/deel1", server.uri())}]
        }})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT")).and(path("/s3/deel1")).respond_with(StrengeOpslag).expect(1).mount(&server).await;
    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.upload_klaar"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {
            "bestand": "b1", "naam": naam, "versie": 1, "conflict": false
        }})))
        .expect(1)
        .mount(&server)
        .await;
    server
}

async fn upload(inhoud: &[u8], naam: &str) -> (MockServer, Result<openaec_sync::UploadResultaat, openaec_sync::Fout>) {
    let server = server_voor(naam).await;
    let dir = tempfile::tempdir().unwrap();
    let pad = dir.path().join(naam);
    std::fs::write(&pad, inhoud).unwrap();
    let client = Client::new(&server.uri(), "token sleutel:geheim");
    let resultaat = upload_bestand(&client, "r1", None, naam, &pad, None).await;
    (server, resultaat)
}

#[tokio::test]
async fn leeg_bestand_stuurt_content_length_nul() {
    let (server, resultaat) = upload(b"", "__init__.py").await;
    let resultaat = resultaat.expect("een leeg bestand moet zonder 411 uploaden");
    assert_eq!(resultaat.versie, 1);

    let ontvangen = server.received_requests().await.unwrap();
    let put = ontvangen.iter().find(|r| r.method.as_str() == "PUT").unwrap();
    assert_eq!(put.headers.get("content-length").unwrap(), "0");
    assert!(put.body.is_empty());
    // Het lege bestand komt als echte versie op de server: grootte 0, één blokhash, en de afronding met de ETag.
    let start = ontvangen.iter().find(|r| r.url.path().ends_with("upload_start")).unwrap();
    let start: serde_json::Value = serde_json::from_slice(&start.body).unwrap();
    assert_eq!(start["grootte"], json!(0));
    assert_eq!(start["blokhashes"].as_array().unwrap().len(), 1);
    let klaar = ontvangen.iter().find(|r| r.url.path().ends_with("upload_klaar")).unwrap();
    let klaar: serde_json::Value = serde_json::from_slice(&klaar.body).unwrap();
    assert_eq!(klaar["delen"], json!([{"nummer": 1, "etag": "\"etag1\""}]));
}

#[tokio::test]
async fn gewoon_bestand_stuurt_content_length() {
    let inhoud = b"# Roadmap\n\n- eerste punt\n".repeat(40);
    let (server, resultaat) = upload(&inhoud, "ROADMAP.md").await;
    resultaat.expect("een gewoon bestand moet zonder 411 uploaden");
    let ontvangen = server.received_requests().await.unwrap();
    let put = ontvangen.iter().find(|r| r.method.as_str() == "PUT").unwrap();
    assert_eq!(put.headers.get("content-length").unwrap(), inhoud.len().to_string().as_str());
}

#[tokio::test]
async fn batch_van_kleine_bestanden_stuurt_content_length() {
    // Ook de batch (multipart, naar de server zelf) heeft een vaste lengte, met lege bestanden ertussen.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.upload_klein"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {"resultaten": [
            {"ok": true, "bestand": "b1", "naam": "leeg.txt", "versie": 1},
            {"ok": true, "bestand": "b2", "naam": "a.txt", "versie": 1}
        ]}})))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::new(&server.uri(), "token sleutel:geheim");
    let bestanden = [
        openaec_sync::api::KleinBestand { naam: "leeg.txt".into(), map: None, pad: None, basis_versie: None, inhoud: Vec::new() },
        openaec_sync::api::KleinBestand { naam: "a.txt".into(), map: None, pad: None, basis_versie: None, inhoud: b"abc".to_vec() },
    ];
    let r = client.upload_klein("r1", &bestanden).await.unwrap();
    assert!(r.iter().all(|x| x.is_ok()));
    let ontvangen = server.received_requests().await.unwrap();
    let lengte: usize = ontvangen[0].headers.get("content-length").expect("Content-Length op de batch").to_str().unwrap().parse().unwrap();
    assert_eq!(lengte, ontvangen[0].body.len());
}
