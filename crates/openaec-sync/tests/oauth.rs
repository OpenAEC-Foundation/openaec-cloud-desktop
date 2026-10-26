//! Inloggen met het OpenAEC-account: PKCE, loopback-redirect en tokens (Frappe als OAuth-provider).

use openaec_sync::oauth::{autorisatie_url, challenge, vernieuw, verifier, wacht_op_code, wissel_code, Tokens};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "/api/method/frappe.integrations.oauth2.get_token";

#[test]
fn challenge_volgt_rfc_7636() {
    assert_eq!(
        challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let v = verifier();
    assert!((43..=128).contains(&v.len()));
    assert_ne!(v, verifier());
}

#[test]
fn autorisatie_url_bevat_pkce_en_redirect() {
    let url = autorisatie_url("https://klant.cloud.open-aec.com/", "abc", "xyz");
    assert!(url.starts_with("https://klant.cloud.open-aec.com/api/method/frappe.integrations.oauth2.authorize?"));
    for deel in [
        "client_id=openaec-cloud-desktop",
        "response_type=code",
        "code_challenge=abc",
        "code_challenge_method=S256",
        "state=xyz",
        "redirect_uri=http%3A%2F%2F127.0.0.1%3A47613%2Fcallback",
    ] {
        assert!(url.contains(deel), "{deel} ontbreekt in {url}");
    }
}

async fn browser(poort: u16, pad: &str) -> String {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", poort)).await.unwrap();
    s.write_all(format!("GET {pad} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes()).await.unwrap();
    let mut uit = String::new();
    s.read_to_string(&mut uit).await.unwrap();
    uit
}

#[tokio::test]
async fn loopback_vangt_de_code_en_controleert_state() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let poort = listener.local_addr().unwrap().port();
    let wacht = tokio::spawn(wacht_op_code(listener, "goed".to_string()));
    let pagina = browser(poort, "/callback?code=C123&state=goed").await;
    assert!(pagina.contains("200"));
    assert_eq!(wacht.await.unwrap().unwrap(), "C123");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let poort = listener.local_addr().unwrap().port();
    let wacht = tokio::spawn(wacht_op_code(listener, "goed".to_string()));
    browser(poort, "/callback?code=C123&state=fout").await;
    assert!(wacht.await.unwrap().is_err());
}

#[tokio::test]
async fn code_inwisselen_en_vernieuwen() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code_verifier=geheim"))
        .and(body_string_contains("code=C123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"access_token": "A1", "refresh_token": "R1", "expires_in": 3600, "token_type": "Bearer"}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(TOKEN))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=R1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"access_token": "A2", "refresh_token": "R2", "expires_in": 3600}),
        ))
        .mount(&server)
        .await;

    let t: Tokens = wissel_code(&server.uri(), "C123", "geheim").await.unwrap();
    assert_eq!((t.access_token.as_str(), t.refresh_token.as_str()), ("A1", "R1"));
    assert!(!t.verlopen());
    assert_eq!(t.autorisatie(), "Bearer A1");
    let t2 = vernieuw(&server.uri(), &t).await.unwrap();
    assert_eq!(t2.access_token, "A2");
}

#[test]
fn tools_client_van_de_cloud_vraagt_alleen_ai() {
    use openaec_sync::oauth::{autorisatie_url_voor, TOOLS};
    let url = autorisatie_url_voor(&TOOLS, "https://cloud.open-aec.eu", "abc", "xyz");
    assert!(url.starts_with("https://cloud.open-aec.eu/api/method/frappe.integrations.oauth2.authorize?"));
    for deel in [
        "client_id=tools",
        "scope=openid+email+profile+ai",
        "redirect_uri=http%3A%2F%2Flocalhost%3A8765%2Fterug",
        "code_challenge_method=S256",
    ] {
        assert!(url.contains(deel), "{deel} ontbreekt in {url}");
    }
    // Nooit `all` voor de gateway; de sync-client houdt precies zijn eigen scopes.
    assert!(!TOOLS.scope.split(' ').any(|s| s == "all"));
    assert!(autorisatie_url("https://klant.cloud.open-aec.eu", "a", "b").contains("scope=all+openid&"));
}

#[tokio::test]
async fn loopback_op_een_ander_terugpad() {
    use openaec_sync::oauth::wacht_op_code_op;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let poort = listener.local_addr().unwrap().port();
    let wacht = tokio::spawn(wacht_op_code_op(listener, "s".to_string(), "/terug"));
    // Een ander pad (bijv. /favicon.ico of /callback) wordt genegeerd.
    assert!(browser(poort, "/callback?code=X&state=s").await.contains("404"));
    browser(poort, "/terug?code=C9&state=s").await;
    assert_eq!(wacht.await.unwrap().unwrap(), "C9");
}

#[tokio::test]
async fn sessie_van_de_tools_client_vernieuwt_met_client_tools() {
    use openaec_sync::oauth::TOOLS;
    use openaec_sync::sessie::{Geheugen, Sessie, Tokenbron};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("client_id=tools"))
        .and(body_string_contains("refresh_token=R1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "J2", "refresh_token": "R2", "expires_in": 900})))
        .expect(1)
        .mount(&server)
        .await;
    let verlopen = Tokens { access_token: "J1".into(), refresh_token: "R1".into(), verloopt: 0 };
    let sessie = Sessie::met_aanvrager(&server.uri(), Geheugen::met(serde_json::to_string(&verlopen).unwrap()), TOOLS);
    assert_eq!(sessie.geef().await.unwrap(), "Bearer J2");
}
