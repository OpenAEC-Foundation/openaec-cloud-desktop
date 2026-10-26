//! Overdrachten tegen een nagebootste server en opslag: maken, bestanden in delen uploaden (ook een leeg bestand, met
//! `Content-Length: 0`), hervatten na een onderbroken upload, nieuwe links als een link verlopen is, voltooien, de
//! eigen lijst en intrekken. En een map als zip.

use std::io::Read;
use std::sync::Mutex;

use openaec_sync::overdracht::{pak_map, verstuur_bestand, NieuweOverdracht};
use openaec_sync::{rem, Client};
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const API: &str = "/api/method/openaec_storage.overdracht.";

fn antwoord(message: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "message": message }))
}

/// De PUT's naar de opslag: (pad, Content-Length-header, inhoud).
fn puts(verzoeken: &[Request]) -> Vec<(String, Option<String>, Vec<u8>)> {
    verzoeken
        .iter()
        .filter(|r| r.method.as_str() == "PUT")
        .map(|r| {
            let lengte = r.headers.get("content-length").and_then(|w| w.to_str().ok()).map(str::to_string);
            (r.url.path().to_string(), lengte, r.body.clone())
        })
        .collect()
}

async fn opslag_neemt_aan(server: &MockServer) {
    Mock::given(method("PUT"))
        .and(path_regex("^/s3/"))
        .respond_with(ResponseTemplate::new(200).insert_header("ETag", "\"e\""))
        .mount(server)
        .await;
}

#[tokio::test]
async fn maak_stuurt_de_velden_mee_en_laat_lege_weg() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}maak")))
        .and(body_partial_json(json!({
            "naam": "Tekeningen", "bericht": "Zie bijlage", "verloopt_op": "2026-11-30", "wachtwoord": null,
            "ontvangers": ["a@voorbeeld.nl", "b@voorbeeld.nl"]
        })))
        .respond_with(antwoord(json!({"overdracht": "OV1", "naam": "Tekeningen", "status": "Concept",
            "verloopt_op": "2026-11-30 23:59:59", "max_bytes": 100_000_000_000u64, "concept_geldig_tot": "2026-10-27 10:00:00"})))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::new(&server.uri(), "token a:b");
    let concept = client
        .maak_overdracht(&NieuweOverdracht {
            naam: " Tekeningen ".into(),
            bericht: Some("Zie bijlage".into()),
            verloopt_op: Some("2026-11-30".into()),
            wachtwoord: Some(String::new()),
            ontvangers: vec!["a@voorbeeld.nl".into(), " ".into(), "b@voorbeeld.nl".into()],
        })
        .await
        .unwrap();
    assert_eq!((concept.overdracht.as_str(), concept.status.as_str()), ("OV1", "Concept"));
}

#[tokio::test]
async fn bestanden_in_delen_ook_een_leeg_bestand() {
    let server = MockServer::start().await;
    opslag_neemt_aan(&server).await;
    let s3 = |p: &str| format!("{}/s3/{p}", server.uri());
    // Een bestand van 10 bytes in delen van 4: drie delen.
    Mock::given(method("POST"))
        .and(path(format!("{API}bestand_start")))
        .and(body_partial_json(json!({"overdracht": "OV1", "naam": "tekst.txt", "grootte": 10})))
        .respond_with(antwoord(json!({"bestand": "B1", "naam": "tekst.txt", "grootte": 10, "deelgrootte": 4,
            "delen": [{"nummer": 1, "url": s3("B1/1")}, {"nummer": 2, "url": s3("B1/2")}, {"nummer": 3, "url": s3("B1/3")}]})))
        .expect(1)
        .mount(&server)
        .await;
    // Een leeg bestand: één deel zonder inhoud.
    Mock::given(method("POST"))
        .and(path(format!("{API}bestand_start")))
        .and(body_partial_json(json!({"overdracht": "OV1", "naam": "leeg.txt", "grootte": 0})))
        .respond_with(antwoord(json!({"bestand": "B2", "naam": "leeg.txt", "grootte": 0, "deelgrootte": 8_388_608,
            "delen": [{"nummer": 1, "url": s3("B2/1")}]})))
        .expect(1)
        .mount(&server)
        .await;
    for (id, naam, grootte) in [("B1", "tekst.txt", 10), ("B2", "leeg.txt", 0)] {
        Mock::given(method("POST"))
            .and(path(format!("{API}bestand_klaar")))
            .and(body_partial_json(json!({"overdracht": "OV1", "bestand": id})))
            .respond_with(antwoord(json!({"bestand": id, "naam": naam, "grootte": grootte, "klaar": true})))
            .expect(1)
            .mount(&server)
            .await;
    }

    let map = tempfile::tempdir().unwrap();
    std::fs::write(map.path().join("tekst.txt"), b"0123456789").unwrap();
    std::fs::write(map.path().join("leeg.txt"), b"").unwrap();
    let client = Client::new(&server.uri(), "token a:b");
    let rem = rem(4);
    let ids = Mutex::new(Vec::new());
    let stappen = Mutex::new(Vec::new());
    let r = verstuur_bestand(
        &client,
        "OV1",
        &map.path().join("tekst.txt"),
        "tekst.txt",
        None,
        &rem,
        &|id| ids.lock().unwrap().push(id.to_string()),
        &|gedaan, totaal| stappen.lock().unwrap().push((gedaan, totaal)),
    )
    .await
    .unwrap();
    assert!(r.klaar);
    let leeg = verstuur_bestand(&client, "OV1", &map.path().join("leeg.txt"), "leeg.txt", None, &rem, &|_| {}, &|_, _| {}).await.unwrap();
    assert_eq!((leeg.bestand.as_str(), leeg.grootte), ("B2", 0));
    assert_eq!(ids.lock().unwrap().as_slice(), ["B1"]);
    assert_eq!(stappen.lock().unwrap().last(), Some(&(10, 10)));

    let mut ontvangen = puts(&server.received_requests().await.unwrap());
    ontvangen.sort();
    let verwacht: Vec<(String, Option<String>, Vec<u8>)> = vec![
        ("/s3/B1/1".into(), Some("4".into()), b"0123".to_vec()),
        ("/s3/B1/2".into(), Some("4".into()), b"4567".to_vec()),
        ("/s3/B1/3".into(), Some("2".into()), b"89".to_vec()),
        // Zonder Content-Length weigert de opslag een lege PUT (411).
        ("/s3/B2/1".into(), Some("0".into()), Vec::new()),
    ];
    assert_eq!(ontvangen, verwacht);
    // De SHA-256 gaat mee bij het aankondigen.
    let start = server.received_requests().await.unwrap().into_iter().find(|r| r.url.path().ends_with("bestand_start")).unwrap();
    let gegevens: serde_json::Value = serde_json::from_slice(&start.body).unwrap();
    assert_eq!(gegevens["sha256"], "84d89877f0d4041efb6bf91a16f0248f2fd573e6af05c19f96bedb9f882f7882");
}

#[tokio::test]
async fn hervatten_stuurt_alleen_wat_ontbreekt() {
    let server = MockServer::start().await;
    opslag_neemt_aan(&server).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}bestand_stand")))
        .and(body_partial_json(json!({"overdracht": "OV1", "bestand": "B1"})))
        .respond_with(antwoord(json!({"bestand": "B1", "naam": "tekst.txt", "grootte": 10, "klaar": false, "deelgrootte": 4,
            "aanwezig": [{"nummer": 1, "grootte": 4, "etag": "\"a\""}, {"nummer": 3, "grootte": 2, "etag": "\"c\""}],
            "ontbrekend": [2], "delen": [{"nummer": 2, "url": format!("{}/s3/B1/2", server.uri())}]})))
        // De tweede keer bij het veranderde bestand.
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}bestand_klaar")))
        .respond_with(antwoord(json!({"bestand": "B1", "naam": "tekst.txt", "grootte": 10, "klaar": true})))
        .expect(1)
        .mount(&server)
        .await;
    // Geen nieuw bestand aankondigen.
    Mock::given(method("POST")).and(path(format!("{API}bestand_start"))).respond_with(antwoord(json!({}))).expect(0).mount(&server).await;

    let map = tempfile::tempdir().unwrap();
    let pad = map.path().join("tekst.txt");
    std::fs::write(&pad, b"0123456789").unwrap();
    let client = Client::new(&server.uri(), "token a:b");
    let eerste = Mutex::new(None);
    verstuur_bestand(&client, "OV1", &pad, "tekst.txt", Some("B1"), &rem(4), &|_| {}, &|g, t| {
        eerste.lock().unwrap().get_or_insert((g, t));
    })
    .await
    .unwrap();
    // De voortgang begint bij wat de opslag al had.
    assert_eq!(*eerste.lock().unwrap(), Some((6, 10)));
    assert_eq!(puts(&server.received_requests().await.unwrap()), vec![("/s3/B1/2".to_string(), Some("4".to_string()), b"4567".to_vec())]);

    // Is het lokale bestand intussen veranderd, dan zegt de client dat (de app haalt het dan weg en begint opnieuw).
    std::fs::write(&pad, b"anders").unwrap();
    let fout = verstuur_bestand(&client, "OV1", &pad, "tekst.txt", Some("B1"), &rem(4), &|_| {}, &|_, _| {}).await;
    assert!(fout.unwrap_err().to_string().contains("veranderd"));
}

#[tokio::test]
async fn een_verlopen_link_wordt_vervangen() {
    let server = MockServer::start().await;
    let s3 = |p: &str| format!("{}/s3/{p}", server.uri());
    Mock::given(method("PUT")).and(path("/s3/oud/1")).respond_with(ResponseTemplate::new(403)).mount(&server).await;
    opslag_neemt_aan(&server).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}bestand_start")))
        .respond_with(antwoord(json!({"bestand": "B1", "naam": "a.bin", "grootte": 3, "deelgrootte": 8_388_608,
            "delen": [{"nummer": 1, "url": s3("oud/1")}]})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}bestand_stand")))
        .respond_with(antwoord(json!({"bestand": "B1", "naam": "a.bin", "grootte": 3, "klaar": false, "deelgrootte": 8_388_608,
            "aanwezig": [], "ontbrekend": [1], "delen": [{"nummer": 1, "url": s3("nieuw/1")}]})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}bestand_klaar")))
        .respond_with(antwoord(json!({"bestand": "B1", "naam": "a.bin", "grootte": 3, "klaar": true})))
        .mount(&server)
        .await;
    let map = tempfile::tempdir().unwrap();
    std::fs::write(map.path().join("a.bin"), b"abc").unwrap();
    let client = Client::new(&server.uri(), "token a:b");
    verstuur_bestand(&client, "OV1", &map.path().join("a.bin"), "a.bin", None, &rem(2), &|_| {}, &|_, _| {}).await.unwrap();
    let paden: Vec<String> = puts(&server.received_requests().await.unwrap()).into_iter().map(|(p, _, _)| p).collect();
    assert_eq!(paden, ["/s3/oud/1", "/s3/nieuw/1"]);
}

#[tokio::test]
async fn een_tijdelijke_fout_van_de_opslag_wordt_opnieuw_geprobeerd() {
    let server = MockServer::start().await;
    Mock::given(method("PUT")).and(path("/s3/B1/1")).respond_with(ResponseTemplate::new(503)).up_to_n_times(1).mount(&server).await;
    opslag_neemt_aan(&server).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}bestand_start")))
        .respond_with(antwoord(json!({"bestand": "B1", "naam": "a.bin", "grootte": 3, "deelgrootte": 8_388_608,
            "delen": [{"nummer": 1, "url": format!("{}/s3/B1/1", server.uri())}]})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}bestand_klaar")))
        .respond_with(antwoord(json!({"bestand": "B1", "naam": "a.bin", "grootte": 3, "klaar": true})))
        .mount(&server)
        .await;
    let map = tempfile::tempdir().unwrap();
    std::fs::write(map.path().join("a.bin"), b"abc").unwrap();
    let client = Client::new(&server.uri(), "token a:b");
    verstuur_bestand(&client, "OV1", &map.path().join("a.bin"), "a.bin", None, &rem(2), &|_| {}, &|_, _| {}).await.unwrap();
    assert_eq!(puts(&server.received_requests().await.unwrap()).len(), 2);
}

#[tokio::test]
async fn voltooien_lijst_en_intrekken() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}voltooi")))
        .and(body_partial_json(json!({"overdracht": "OV1"})))
        .respond_with(antwoord(json!({"overdracht": "OV1", "status": "Actief", "token": "T1", "url": "https://x/overdracht/T1",
            "verloopt_op": "2026-11-02 10:00:00", "aantal_bestanden": 2, "totale_grootte": 10, "mail": ["a@voorbeeld.nl"]})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}mijn")))
        .respond_with(antwoord(json!([{"overdracht": "OV1", "naam": "Tekeningen", "status": "Actief", "verloopt_op": "2026-11-02 10:00:00",
            "totale_grootte": 10, "aantal_bestanden": 2, "aantal_downloads": 3, "laatst_gedownload": null, "ontvangers": ["a@voorbeeld.nl"],
            "beveiligd": true, "url": "https://x/overdracht/T1", "gemaakt": "2026-10-26 10:00:00.000000", "voltooid_op": "2026-10-26 10:05:00"}])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}trek_in")))
        .respond_with(antwoord(json!({"overdracht": "OV1", "status": "Ingetrokken"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}grenzen")))
        .respond_with(antwoord(json!({"max_bytes": 100_000_000_000u64, "standaard_dagen": 7, "max_dagen": 30, "max_ontvangers": 20,
            "concept_uren": 24, "zip_max_bytes": 2_000_000_000u64, "quotum": {"gebruikt_bytes": 5, "limiet_bytes": null}})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}info")))
        .respond_with(antwoord(json!({"overdracht": "OV1", "naam": "T", "status": "Concept", "bericht": null, "ontvangers": [],
            "verloopt_op": "2026-11-02 10:00:00", "beveiligd": false, "url": null, "totale_grootte": 10, "aantal_downloads": 0,
            "concept_geldig_tot": "2026-10-27 10:00:00",
            "bestanden": [{"bestand": "B1", "naam": "a", "grootte": 10, "klaar": false, "aantal_downloads": 0}]})))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), "token a:b");
    let v = client.voltooi_overdracht("OV1").await.unwrap();
    assert_eq!((v.url.as_str(), v.mail.len()), ("https://x/overdracht/T1", 1));
    let lijst = client.mijn_overdrachten().await.unwrap();
    assert_eq!((lijst[0].aantal_downloads, lijst[0].beveiligd), (3, true));
    assert_eq!(client.trek_overdracht_in("OV1").await.unwrap(), "Ingetrokken");
    let g = client.overdracht_grenzen().await.unwrap();
    assert_eq!((g.max_dagen, g.max_ontvangers), (30, 20));
    let info = client.overdracht_info("OV1").await.unwrap();
    assert_eq!((info.bestanden.len(), info.bestanden[0].klaar), (1, false));
}

#[tokio::test]
async fn geen_recht_om_te_versturen_is_een_403() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}grenzen")))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"exc_type": "PermissionError"})))
        .mount(&server)
        .await;
    let fout = Client::new(&server.uri(), "token a:b").overdracht_grenzen().await.unwrap_err();
    assert_eq!(fout.status(), Some(403));
}

#[test]
fn een_map_als_zip_met_paden_en_lege_mappen() {
    let werk = tempfile::tempdir().unwrap();
    let map = werk.path().join("Project");
    std::fs::create_dir_all(map.join("Tekeningen").join("Oud")).unwrap();
    std::fs::create_dir_all(map.join("Leeg")).unwrap();
    std::fs::write(map.join("lees mij.txt"), b"hallo").unwrap();
    std::fs::write(map.join("Tekeningen").join("plan.dwg"), b"dwg").unwrap();
    std::fs::write(map.join("Tekeningen").join("Oud").join("leeg.bin"), b"").unwrap();
    let doel = werk.path().join("Project.zip");
    assert_eq!(pak_map(&map, &doel).unwrap(), 3);
    assert!(!werk.path().join("Project.zip-bezig").exists());

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&doel).unwrap()).unwrap();
    let mut namen: Vec<String> = zip.file_names().map(str::to_string).collect();
    namen.sort();
    assert_eq!(
        namen,
        ["Project/", "Project/Leeg/", "Project/Tekeningen/", "Project/Tekeningen/Oud/", "Project/Tekeningen/Oud/leeg.bin",
         "Project/Tekeningen/plan.dwg", "Project/lees mij.txt"]
    );
    let mut inhoud = String::new();
    zip.by_name("Project/lees mij.txt").unwrap().read_to_string(&mut inhoud).unwrap();
    assert_eq!(inhoud, "hallo");
    assert_eq!(openaec_sync::overdracht::mapomvang(&map).unwrap(), (3, 8));
}

#[tokio::test]
async fn uit_de_opslag_stuurt_bestanden_met_versie_en_mappen() {
    use openaec_sync::overdracht::{methode_onbekend, UitOpslag};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}voeg_toe_van_opslag")))
        .and(body_partial_json(json!({"overdracht": "OV1", "bestanden": [{"bestand": "B1", "versie": 4}], "mappen": ["M1"]})))
        .respond_with(antwoord(json!({"overdracht": "OV1", "bezig": true, "bestanden": [
            {"bestand": "K1", "naam": "plan.pdf", "grootte": 3, "klaar": false},
            {"bestand": "K2", "naam": "a.dwg", "map": "Tekeningen", "grootte": 1, "klaar": false}]})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}maak_van_opslag")))
        .and(body_partial_json(json!({"naam": "Tekeningen", "mappen": ["M1"], "bestanden": [], "verloopt": "2026-11-30"})))
        .respond_with(antwoord(json!({"overdracht": "OV2", "status": "Actief", "bezig": false, "url": "https://x/overdracht/t"})))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::new(&server.uri(), "token a:b");
    let items = [UitOpslag::Bestand { id: "B1".into(), versie: Some(4) }, UitOpslag::Map { id: "M1".into() }];
    let t = client.voeg_toe_van_opslag("OV1", &items).await.unwrap();
    assert!(t.bezig);
    assert_eq!(t.bestanden[1].map.as_deref(), Some("Tekeningen"));
    let nieuw = NieuweOverdracht { naam: "Tekeningen".into(), verloopt_op: Some("2026-11-30".into()), ..Default::default() };
    let v = client.maak_van_opslag(&nieuw, &[UitOpslag::Map { id: "M1".into() }]).await.unwrap();
    assert_eq!((v.overdracht.as_str(), v.bezig, v.url.as_deref()), ("OV2", false, Some("https://x/overdracht/t")));
    // Een oudere server kent de methode niet.
    let oud = openaec_sync::Fout::Server { status: 404, tekst: json!({"exception": "Failed to get method"}).to_string() };
    assert!(methode_onbekend(&oud));
    assert!(!methode_onbekend(&openaec_sync::Fout::Server { status: 417, tekst: json!({"exception": "x"}).to_string() }));
}
