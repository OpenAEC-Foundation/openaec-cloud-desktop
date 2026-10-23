//! Delen tegen een nagebootste API: met een gebruiker of als link, bestaande delingen ophalen en intrekken, de
//! leden van een ruimte, en de foutcode en melding uit een weigering van de server.

use openaec_sync::{Client, Fout};
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const API: &str = "/api/method/openaec_storage.api.";

fn antwoord(message: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "message": message }))
}

#[tokio::test]
async fn delen_met_een_gebruiker_stuurt_soort_en_rechten_mee() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}deel")))
        .and(body_partial_json(json!({
            "doel_doctype": "Opslagmap", "doel": "M1", "soort": "gebruiker", "gebruiker": "p.jansen@voorbeeld.nl",
            "rechten": "schrijven", "verloopt": "2026-12-31 23:59:59"
        })))
        // Een deling met een gebruiker heeft geen token.
        .respond_with(antwoord(json!({"deling": "D1", "token": null, "rechten": "schrijven", "url": null})))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), "token a:b");
    let d = client.deel_met_gebruiker("Opslagmap", "M1", "p.jansen@voorbeeld.nl", "schrijven", Some("2026-12-31")).await.unwrap();
    assert_eq!((d.deling.as_str(), d.rechten.as_str(), d.token), ("D1", "schrijven", None));
}

#[tokio::test]
async fn link_met_rechten_en_wachtwoord() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}deel")))
        .and(body_partial_json(json!({"soort": "link", "rechten": "uploaden", "wachtwoord": "geheim", "verloopt": null})))
        .respond_with(antwoord(json!({"deling": "D2", "token": "T2", "rechten": "uploaden", "url": "https://x/delen/T2"})))
        .expect(1)
        .mount(&server)
        .await;

    let d = Client::new(&server.uri(), "token a:b").deel_link("Opslagmap", "M1", "uploaden", Some("geheim"), None).await.unwrap();
    assert_eq!(d.token.as_deref(), Some("T2"));
    assert_eq!(d.url.as_deref(), Some("https://x/delen/T2"));
}

#[tokio::test]
async fn delingen_ophalen_en_intrekken() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}delingen")))
        .and(body_partial_json(json!({"doel_doctype": "Opslagbestand", "doel": "B1"})))
        .respond_with(antwoord(json!([
            {"name": "D1", "soort": "link", "gebruiker": null, "rechten": "lezen", "verloopt": null, "token": "T1",
             "creation": "2026-10-20 09:00:00.000000"},
            {"name": "D2", "soort": "gebruiker", "gebruiker": "p.jansen@voorbeeld.nl", "rechten": "schrijven",
             "verloopt": "2026-11-01 23:59:59", "token": null, "creation": "2026-10-21 10:00:00.000000"}
        ])))
        .mount(&server)
        .await;
    // Frappe laat `message` weg bij een methode zonder antwoord.
    Mock::given(method("POST"))
        .and(path(format!("{API}trek_in")))
        .and(body_partial_json(json!({"deling": "D2"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), "token a:b");
    let lijst = client.delingen("Opslagbestand", "B1").await.unwrap();
    assert_eq!(lijst.len(), 2);
    assert_eq!((lijst[0].soort.as_str(), lijst[0].token.as_deref()), ("link", Some("T1")));
    assert_eq!(lijst[1].gebruiker.as_deref(), Some("p.jansen@voorbeeld.nl"));
    client.trek_in("D2").await.unwrap();
}

#[tokio::test]
async fn ruimteleden_uit_het_document_van_de_ruimte() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/resource/Ruimte/R%201"))
        .and(header("Authorization", "token a:b"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
            "name": "R 1", "naam": "Havenkwartier",
            "leden": [
                {"name": "x1", "gebruiker": "a.bakker@voorbeeld.nl", "rol": "beheerder"},
                {"name": "x2", "gebruiker": "p.jansen@voorbeeld.nl", "rol": "lezer"}
            ]
        }})))
        .mount(&server)
        .await;

    let leden = Client::new(&format!("{}/", server.uri()), "token a:b").ruimteleden("R 1").await.unwrap();
    assert_eq!(leden.iter().map(|l| (l.gebruiker.as_str(), l.rol.as_str())).collect::<Vec<_>>(), vec![
        ("a.bakker@voorbeeld.nl", "beheerder"),
        ("p.jansen@voorbeeld.nl", "lezer"),
    ]);
}

#[tokio::test]
async fn weigering_geeft_foutcode_en_melding() {
    let server = MockServer::start().await;
    let melding = json!({"message": "Deze ruimte is gearchiveerd en alleen te lezen."}).to_string();
    Mock::given(method("POST"))
        .and(path(format!("{API}deel")))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "exc_type": "PermissionError",
            "foutcode": "ruimte_gearchiveerd",
            "_server_messages": json!([melding]).to_string(),
        })))
        .mount(&server)
        .await;

    let fout = Client::new(&server.uri(), "token a:b").deel("Opslagbestand", "B1", None, None).await.unwrap_err();
    assert_eq!(fout.status(), Some(403));
    assert_eq!(fout.foutcode().as_deref(), Some("ruimte_gearchiveerd"));
    assert_eq!(fout.servermelding().as_deref(), Some("Deze ruimte is gearchiveerd en alleen te lezen."));
}

#[test]
fn melding_uit_exception_en_geen_json() {
    let f = Fout::Server { status: 417, tekst: json!({"exception": "frappe.exceptions.ValidationError: Kies een bestaande gebruiker om mee te delen"}).to_string() };
    assert_eq!(f.servermelding().as_deref(), Some("Kies een bestaande gebruiker om mee te delen"));
    assert_eq!(f.foutcode(), None);
    let f = Fout::Server { status: 502, tekst: "<html>Bad gateway</html>".into() };
    assert_eq!((f.status(), f.foutcode(), f.servermelding()), (Some(502), None, None));
    assert_eq!(Fout::Controle("x".into()).status(), None);
}
