use std::sync::{Arc, Mutex};

use openaec_sync::blokken::{blokgrootte, gewijzigde_blokken, hash_bytes, BLOK};
use openaec_sync::{download_bestand, download_met_voortgang, upload_met_voortgang, Client};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const MIB: u64 = 1024 * 1024;

#[test]
fn blokgrootte_volgt_de_serverregels() {
    assert_eq!(blokgrootte(0), BLOK);
    assert_eq!(blokgrootte(10 * 1024 * MIB), BLOK);
    let groot = 200 * 1024 * MIB;
    let blok = blokgrootte(groot);
    assert!(blok > BLOK);
    assert_eq!(blok % MIB, 0);
    assert!(groot.div_ceil(blok) <= 9000);
}

#[test]
fn hashes_per_blok_en_verschillen() {
    let a = vec![b'a'; 10];
    let b = [&a[..], b"bbbb"].concat();
    let ha = hash_bytes(&a, 4);
    let hb = hash_bytes(&b, 4);
    assert_eq!(ha.len(), 3);
    assert_eq!(hb.len(), 4);
    assert_eq!(gewijzigde_blokken(&ha, &hb), vec![2, 3]);
    assert_eq!(hash_bytes(&[], 4).len(), 1);
}

/// Antwoordt op GET met (een stuk van) de inhoud, op basis van de Range-header; telt de verzoeken.
struct Object {
    inhoud: Vec<u8>,
    bereiken: Arc<Mutex<Vec<String>>>,
}

impl Respond for Object {
    fn respond(&self, verzoek: &Request) -> ResponseTemplate {
        match verzoek.headers.get("range").and_then(|w| w.to_str().ok()) {
            Some(bereik) => {
                self.bereiken.lock().unwrap().push(bereik.to_string());
                let (begin, eind) = bereik.trim_start_matches("bytes=").split_once('-').unwrap();
                let (begin, eind): (usize, usize) = (begin.parse().unwrap(), eind.parse().unwrap());
                ResponseTemplate::new(206).set_body_bytes(self.inhoud[begin..=eind].to_vec())
            }
            None => ResponseTemplate::new(200).set_body_bytes(self.inhoud.clone()),
        }
    }
}

#[tokio::test]
async fn upload_stuurt_alleen_gevraagde_blokken() {
    let server = MockServer::start().await;
    let inhoud: Vec<u8> = [vec![b'A'; BLOK as usize], vec![b'X'; BLOK as usize], b"staart".to_vec()].concat();
    let dir = tempfile::tempdir().unwrap();
    let pad = dir.path().join("wolk.e57");
    std::fs::write(&pad, &inhoud).unwrap();

    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.upload_start"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {
            "upload": "tok", "deelgrootte": BLOK, "hergebruik": [1, 3],
            "delen": [{"nummer": 2, "url": format!("{}/s3/deel2", server.uri())}]
        }})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/s3/deel2"))
        .respond_with(ResponseTemplate::new(200).insert_header("ETag", "\"etag2\""))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.upload_klaar"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {
            "bestand": "b1", "naam": "wolk.e57", "versie": 2, "conflict": false
        }})))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), "token sleutel:geheim");
    let standen = Mutex::new(Vec::new());
    let resultaat = upload_met_voortgang(&client, "r1", None, "wolk.e57", &pad, Some(1), &|g, t| standen.lock().unwrap().push((g, t)))
        .await
        .unwrap();
    // Eerst meteen hoeveel er over de lijn gaat: alleen blok 2; blok 1 en de staart hergebruikt de server.
    assert_eq!(standen.lock().unwrap().clone(), vec![(0, BLOK), (BLOK, BLOK)]);
    assert_eq!(resultaat.versie, 2);
    assert!(!resultaat.conflict);

    let ontvangen = server.received_requests().await.unwrap();
    let put = ontvangen.iter().find(|r| r.method.as_str() == "PUT").unwrap();
    assert_eq!(put.body.len() as u64, BLOK);
    let klaar = ontvangen.iter().find(|r| r.url.path().ends_with("upload_klaar")).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&klaar.body).unwrap();
    assert_eq!(body["delen"], json!([{"nummer": 2, "etag": "\"etag2\""}]));
    let start = ontvangen.iter().find(|r| r.url.path().ends_with("upload_start")).unwrap();
    let start: serde_json::Value = serde_json::from_slice(&start.body).unwrap();
    assert_eq!(start["blokhashes"].as_array().unwrap().len(), 3);
    assert_eq!(start["basis_versie"], json!(1));
}

#[tokio::test]
async fn download_haalt_alleen_gewijzigde_blokken() {
    let server = MockServer::start().await;
    let oud: Vec<u8> = [vec![b'A'; BLOK as usize], vec![b'B'; BLOK as usize], b"staart".to_vec()].concat();
    let nieuw: Vec<u8> = [vec![b'A'; BLOK as usize], vec![b'C'; BLOK as usize], b"nieuwe staart!".to_vec()].concat();
    let dir = tempfile::tempdir().unwrap();
    let pad = dir.path().join("wolk.e57");
    std::fs::write(&pad, &oud).unwrap();
    let bereiken = Arc::new(Mutex::new(Vec::new()));

    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.blokken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {
            "versie": 2, "grootte": nieuw.len(), "blokgrootte": BLOK, "hashes": hash_bytes(&nieuw, BLOK)
        }})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.download"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {
            "url": format!("{}/s3/object", server.uri()), "versie": 2
        }})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/s3/object"))
        .respond_with(Object { inhoud: nieuw.clone(), bereiken: bereiken.clone() })
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), "token sleutel:geheim");
    let standen = Mutex::new(Vec::new());
    download_met_voortgang(&client, "b1", &pad, &|g, t| standen.lock().unwrap().push((g, t))).await.unwrap();
    // Over de lijn: blok 2 en de staart, niet het ongewijzigde blok 1.
    let over_de_lijn = BLOK + b"nieuwe staart!".len() as u64;
    assert_eq!(standen.lock().unwrap().first(), Some(&(0, over_de_lijn)));
    assert_eq!(standen.lock().unwrap().last(), Some(&(over_de_lijn, over_de_lijn)));

    assert_eq!(std::fs::read(&pad).unwrap(), nieuw);
    let opgehaald = bereiken.lock().unwrap().clone();
    assert_eq!(opgehaald.len(), 2, "alleen blok 2 en de staart: {opgehaald:?}");
    assert!(opgehaald.iter().all(|b| !b.starts_with("bytes=0-")), "blok 1 was ongewijzigd: {opgehaald:?}");
}

#[tokio::test]
async fn download_zonder_lokaal_bestand_haalt_alles() {
    let server = MockServer::start().await;
    let nieuw = b"klein bestand".to_vec();
    let dir = tempfile::tempdir().unwrap();
    let pad = dir.path().join("nieuw.txt");
    let bereiken = Arc::new(Mutex::new(Vec::new()));
    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.blokken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {
            "versie": 1, "grootte": nieuw.len(), "blokgrootte": BLOK, "hashes": hash_bytes(&nieuw, BLOK)
        }})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.download"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {
            "url": format!("{}/s3/object", server.uri()), "versie": 1
        }})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/s3/object"))
        .respond_with(Object { inhoud: nieuw.clone(), bereiken })
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), "token sleutel:geheim");
    download_bestand(&client, "b1", &pad).await.unwrap();
    assert_eq!(std::fs::read(&pad).unwrap(), nieuw);
}

#[tokio::test]
async fn quotum_met_en_zonder_limiet() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.quotum"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {"gebruikt_bytes": 5 * MIB, "limiet_bytes": 10 * MIB}})))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.quotum"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {"gebruikt_bytes": 7, "limiet_bytes": null}})))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri(), "token sleutel:geheim");
    let q = client.quotum().await.unwrap();
    assert_eq!((q.gebruikt_bytes, q.limiet_bytes), (5 * MIB, Some(10 * MIB)));
    let q = client.quotum().await.unwrap();
    assert_eq!((q.gebruikt_bytes, q.limiet_bytes), (7, None));
}

/// Upload van een bestand van drie delen waarbij S3 elk deel 300 ms laat duren: de delen gaan tegelijk, maar nooit
/// meer dan de rem toelaat.
async fn upload_drie_delen(plekken: usize) -> std::time::Duration {
    let server = MockServer::start().await;
    let inhoud: Vec<u8> = [vec![b'A'; BLOK as usize], vec![b'B'; BLOK as usize], b"staart".to_vec()].concat();
    let dir = tempfile::tempdir().unwrap();
    let pad = dir.path().join("groot.bin");
    std::fs::write(&pad, &inhoud).unwrap();
    let delen: Vec<_> = (1..=3).map(|n| json!({"nummer": n, "url": format!("{}/s3/deel{n}", server.uri())})).collect();
    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.upload_start"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {
            "upload": "tok", "deelgrootte": BLOK, "hergebruik": [], "delen": delen
        }})))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200).insert_header("ETag", "\"e\"").set_delay(std::time::Duration::from_millis(300)))
        .expect(3)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/method/openaec_storage.api.upload_klaar"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"message": {
            "bestand": "b1", "naam": "groot.bin", "versie": 1, "conflict": false
        }})))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri(), "token sleutel:geheim");
    let rem = openaec_sync::rem(plekken);
    let standen = Mutex::new(Vec::new());
    let begin = std::time::Instant::now();
    openaec_sync::upload_met_rem(&client, "r1", None, "groot.bin", &pad, None, &|g, t| standen.lock().unwrap().push((g, t)), &rem)
        .await
        .unwrap();
    let duur = begin.elapsed();
    let standen = standen.lock().unwrap().clone();
    let totaal = inhoud.len() as u64;
    assert_eq!(standen.first(), Some(&(0, totaal)));
    assert_eq!(standen.last(), Some(&(totaal, totaal)), "alle delen samen: {standen:?}");
    let klaar = server.received_requests().await.unwrap().into_iter().find(|r| r.url.path().ends_with("upload_klaar")).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&klaar.body).unwrap();
    let mut nummers: Vec<u64> = body["delen"].as_array().unwrap().iter().map(|d| d["nummer"].as_u64().unwrap()).collect();
    nummers.sort();
    assert_eq!(nummers, vec![1, 2, 3]);
    duur
}

#[tokio::test]
async fn delen_van_een_groot_bestand_gaan_parallel_binnen_de_rem() {
    assert!(upload_drie_delen(3).await < std::time::Duration::from_millis(800), "drie delen tegelijk");
    assert!(upload_drie_delen(1).await >= std::time::Duration::from_millis(900), "één plek: na elkaar");
}
