//! De client van de AI-gateway tegen een nagebootste gateway (en cloud): een gestreamd antwoord met denkblokken en
//! verbruik, de modellen, de foutcodes met wachttijd, een fout midden in de stream, en een verlopen token dat één keer
//! vernieuwd wordt (met de client `tools` van de cloud).

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use openaec_sync::ai::{AiClient, AiFout, Gebruik, Model, Stuk};
use openaec_sync::oauth::{Tokens, TOOLS};
use openaec_sync::sessie::{Bewaring, Geheugen, Sessie, Tokenbron, VasteSleutel};
use serde_json::json;
use wiremock::matchers::{body_partial_json, body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CHAT: &str = "/v1/chat/completions";
const TOKEN: &str = "/api/method/frappe.integrations.oauth2.get_token";

fn stroom(delen: &[serde_json::Value]) -> String {
    let mut s: String = delen.iter().map(|d| format!("data: {d}\n\n")).collect();
    s.push_str("data: [DONE]\n\n");
    s
}

fn stuk(delta: serde_json::Value, einde: Option<&str>) -> serde_json::Value {
    json!({"id": "chatcmpl-v1", "object": "chat.completion.chunk", "choices": [{"index": 0, "delta": delta, "finish_reason": einde}]})
}

fn sse(tekst: String) -> ResponseTemplate {
    ResponseTemplate::new(200).insert_header("content-type", "text/event-stream").insert_header("X-OpenAEC-Vraag-Id", "v1").set_body_string(tekst)
}

fn client(gateway: &MockServer, bron: Arc<dyn Tokenbron>) -> AiClient {
    AiClient::new(&gateway.uri(), "klant0.cloud.open-aec.eu", Some("openaec-desktop"), bron, reqwest::Client::new())
}

fn vast(waarde: &str) -> Arc<dyn Tokenbron> {
    Arc::new(VasteSleutel(waarde.to_string()))
}

async fn vraag(c: &AiClient) -> (Result<openaec_sync::ai::Afloop, AiFout>, Vec<Stuk>) {
    let stukken = Arc::new(Mutex::new(Vec::new()));
    let s = stukken.clone();
    let body = json!({"model": "claude-haiku-5-5", "messages": [{"role": "user", "content": "Hoi"}]});
    let uitkomst = c.vraag(&body, move |stuk| s.lock().unwrap().push(stuk)).await;
    let stukken = stukken.lock().unwrap().clone();
    (uitkomst, stukken)
}

#[tokio::test]
async fn gestreamd_antwoord_met_denkblokken_en_verbruik() {
    let gateway = MockServer::start().await;
    let tekst = stroom(&[
        stuk(json!({"role": "assistant", "content": ""}), None),
        stuk(json!({"content": "Een **staal**"}), None),
        stuk(json!({"content": "fundering…"}), None),
        stuk(json!({"openaec_denkblokken": "b1.eyJ"}), None),
        stuk(json!({}), Some("stop")),
        json!({"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 7, "total_tokens": 19, "prompt_tokens_details": {"cached_tokens": 0}}}),
    ]);
    Mock::given(method("POST"))
        .and(path(CHAT))
        .and(header("authorization", "Bearer J1"))
        .and(header("x-openaec-omgeving", "klant0.cloud.open-aec.eu"))
        .and(header("x-openaec-tool", "openaec-desktop"))
        .and(body_partial_json(json!({"stream": true, "stream_options": {"include_usage": true}, "model": "claude-haiku-5-5"})))
        .respond_with(sse(tekst))
        .expect(1)
        .mount(&gateway)
        .await;

    let (uitkomst, stukken) = vraag(&client(&gateway, vast("Bearer J1"))).await;
    assert_eq!(uitkomst.unwrap().vraag_id.as_deref(), Some("v1"));
    assert_eq!(
        stukken,
        [
            Stuk::Tekst("Een **staal**".into()),
            Stuk::Tekst("fundering…".into()),
            Stuk::Denkblokken("b1.eyJ".into()),
            Stuk::Einde("stop".into()),
            Stuk::Gebruik(Gebruik { prompt_tokens: 12, completion_tokens: 7, total_tokens: 19, cached_tokens: 0 }),
        ]
    );
}

#[tokio::test]
async fn modellen_met_datalabel_en_standaard() {
    let gateway = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-openaec-omgeving", "klant0.cloud.open-aec.eu"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"object": "list", "data": [
            {"id": "claude-haiku-5-5", "object": "model", "naam": "Claude Haiku 5.5", "datalabel": "EU", "standaard": true},
            {"id": "claude-sonnet-5-5", "object": "model", "naam": "Claude Sonnet 5.5", "datalabel": "EU", "standaard": false},
        ]})))
        .mount(&gateway)
        .await;
    let modellen = client(&gateway, vast("Bearer J1")).modellen().await.unwrap();
    assert_eq!(modellen.len(), 2);
    assert_eq!(
        modellen[0],
        Model { id: "claude-haiku-5-5".into(), naam: Some("Claude Haiku 5.5".into()), datalabel: Some("EU".into()), standaard: true }
    );
    assert!(!modellen[1].standaard);
}

#[tokio::test]
async fn foutcodes_worden_nette_meldingen() {
    let gateway = MockServer::start().await;
    let fout = |status: u16, code: &str| {
        ResponseTemplate::new(status).set_body_json(json!({"error": {"message": "x", "type": "t", "param": null, "code": code}}))
    };
    let gevallen: Vec<(&str, ResponseTemplate, AiFout)> = vec![
        ("Bearer tegoed", fout(402, "geen_tegoed"), AiFout::GeenTegoed),
        ("Bearer plafond", fout(429, "plafond_bereikt").insert_header("Retry-After", "5400"), AiFout::Plafond { retry_na: Some(5400) }),
        ("Bearer model", fout(403, "model_niet_toegestaan"), AiFout::ModelNietToegestaan),
        ("Bearer lid", fout(403, "geen_lid"), AiFout::GeenLid),
        ("Bearer beleid", fout(503, "beleid_onbereikbaar").insert_header("Retry-After", "30"), AiFout::Onbereikbaar { retry_na: Some(30) }),
    ];
    for (sleutel, antwoord, _) in &gevallen {
        Mock::given(method("POST")).and(path(CHAT)).and(header("authorization", *sleutel)).respond_with(antwoord.clone()).mount(&gateway).await;
    }
    for (sleutel, _, verwacht) in gevallen {
        let (uitkomst, stukken) = vraag(&client(&gateway, vast(sleutel))).await;
        assert_eq!(uitkomst.unwrap_err(), verwacht, "{sleutel}");
        assert!(stukken.is_empty());
    }
    assert_eq!(
        AiFout::Plafond { retry_na: Some(5400) }.to_string(),
        "Je hebt je AI-plafond bereikt. Je kunt over 1 uur en 30 minuten weer vragen."
    );
}

#[tokio::test]
async fn een_onbereikbare_gateway() {
    // Een poort waar niets luistert.
    let poort = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let c = AiClient::new(&format!("http://127.0.0.1:{poort}"), "klant0", None, vast("Bearer J1"), reqwest::Client::new());
    let (uitkomst, _) = vraag(&c).await;
    assert_eq!(uitkomst.unwrap_err(), AiFout::Onbereikbaar { retry_na: None });
}

#[tokio::test]
async fn fout_midden_in_de_stream_en_een_afgebroken_stream() {
    let gateway = MockServer::start().await;
    let mut tekst: String = format!("data: {}\n\n", stuk(json!({"content": "Half"}), None));
    tekst.push_str("data: {\"error\": {\"message\": \"x\", \"type\": \"api_error\", \"code\": \"aanbieder_fout\"}}\n\n");
    Mock::given(method("POST")).and(path(CHAT)).and(header("authorization", "Bearer A")).respond_with(sse(tekst)).mount(&gateway).await;
    // Zonder [DONE]: de verbinding viel weg.
    let half = format!("data: {}\n\n", stuk(json!({"content": "Half"}), None));
    Mock::given(method("POST")).and(path(CHAT)).and(header("authorization", "Bearer B")).respond_with(sse(half)).mount(&gateway).await;

    for sleutel in ["Bearer A", "Bearer B"] {
        let (uitkomst, stukken) = vraag(&client(&gateway, vast(sleutel))).await;
        assert_eq!(uitkomst.unwrap_err(), AiFout::Onbereikbaar { retry_na: None });
        assert_eq!(stukken, [Stuk::Tekst("Half".into())], "{sleutel}: wat er al was, blijft");
    }
}

fn nu() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

#[tokio::test]
async fn verlopen_token_wordt_een_keer_vernieuwd() {
    let gateway = MockServer::start().await;
    let cloud = MockServer::start().await;
    // De gateway kent J1 niet meer (ingetrokken of verlopen), J2 wel.
    Mock::given(method("POST"))
        .and(path(CHAT))
        .and(header("authorization", "Bearer J1"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": {"code": "ongeldig_token", "message": "x"}})))
        .expect(1)
        .mount(&gateway)
        .await;
    Mock::given(method("POST"))
        .and(path(CHAT))
        .and(header("authorization", "Bearer J2"))
        .respond_with(sse(stroom(&[stuk(json!({"content": "Gelukt"}), Some("stop"))])))
        .expect(1)
        .mount(&gateway)
        .await;
    Mock::given(method("POST"))
        .and(path(TOKEN))
        .and(body_string_contains("client_id=tools"))
        .and(body_string_contains("refresh_token=R1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "J2", "refresh_token": "R2", "expires_in": 900})))
        .expect(1)
        .mount(&cloud)
        .await;

    let geldig = Tokens { access_token: "J1".into(), refresh_token: "R1".into(), verloopt: nu() + 900 };
    let bewaard = Arc::new(Geheugen::met(serde_json::to_string(&geldig).unwrap()));
    let sessie = Arc::new(Sessie::met_aanvrager(&cloud.uri(), Gedeeld(bewaard.clone()), TOOLS));
    let (uitkomst, stukken) = vraag(&client(&gateway, sessie)).await;
    uitkomst.unwrap();
    assert_eq!(stukken, [Stuk::Tekst("Gelukt".into()), Stuk::Einde("stop".into())]);
    // Het nieuwe paar is meteen bewaard (het oude refresh-token is na gebruik ongeldig).
    let nieuw: Tokens = serde_json::from_str(&bewaard.0.lock().unwrap().clone().unwrap()).unwrap();
    assert_eq!((nieuw.access_token.as_str(), nieuw.refresh_token.as_str()), ("J2", "R2"));
}

#[tokio::test]
async fn sessie_verlopen_als_vernieuwen_geweigerd_wordt() {
    let gateway = MockServer::start().await;
    let cloud = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(CHAT))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": {"code": "ongeldig_token", "message": "x"}})))
        .expect(1)
        .mount(&gateway)
        .await;
    Mock::given(method("POST"))
        .and(path(TOKEN))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"})))
        .expect(1)
        .mount(&cloud)
        .await;
    let geldig = Tokens { access_token: "J1".into(), refresh_token: "R1".into(), verloopt: nu() + 900 };
    let sessie = Arc::new(Sessie::met_aanvrager(&cloud.uri(), Geheugen::met(serde_json::to_string(&geldig).unwrap()), TOOLS));
    let (uitkomst, _) = vraag(&client(&gateway, sessie.clone())).await;
    assert_eq!(uitkomst.unwrap_err(), AiFout::SessieVerlopen);
    // Niet nog eens met hetzelfde dode refresh-token: meteen verlopen, zonder de cloud te vragen.
    assert_eq!(client(&gateway, sessie).modellen().await.unwrap_err(), AiFout::SessieVerlopen);

    // Nooit ingelogd voor de assistent: meteen verlopen.
    let leeg = Arc::new(Sessie::met_aanvrager(&cloud.uri(), Geheugen::default(), TOOLS));
    assert_eq!(client(&gateway, leeg).modellen().await.unwrap_err(), AiFout::SessieVerlopen);
}

/// Een bewaring die de test daarna nog kan lezen.
struct Gedeeld(Arc<Geheugen>);

impl Bewaring for Gedeeld {
    fn lees(&self) -> Option<String> {
        self.0.lees()
    }
    fn bewaar(&self, waarde: &str) -> Result<(), String> {
        self.0.bewaar(waarde)
    }
}
