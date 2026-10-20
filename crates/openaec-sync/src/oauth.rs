//! Inloggen met het OpenAEC-account (ontwerp §8): OAuth 2 authorization code met PKCE tegen de klantomgeving
//! (Frappe als provider; de klantomgeving zelf laat de gebruiker inloggen via het centrale account).
//! De app opent de browser, vangt de code op via een loopback-adres en wisselt die in voor tokens.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::Fout;

/// Vaste client, door `openaec_storage` op elke klantomgeving geregistreerd.
pub const CLIENT_ID: &str = "openaec-cloud-desktop";
pub const POORT: u16 = 47613;
pub const REDIRECT: &str = "http://127.0.0.1:47613/callback";
const TOKEN: &str = "/api/method/frappe.integrations.oauth2.get_token";
const AUTORISEER: &str = "/api/method/frappe.integrations.oauth2.authorize";
/// Zoveel seconden voor het verlopen wordt het token al vernieuwd.
const MARGE: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix-tijd in seconden.
    pub verloopt: u64,
}

#[derive(Deserialize)]
struct TokenAntwoord {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

fn nu() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Tokens {
    pub fn verlopen(&self) -> bool {
        nu() + MARGE >= self.verloopt
    }

    /// Waarde voor de Authorization-header.
    pub fn autorisatie(&self) -> String {
        format!("Bearer {}", self.access_token)
    }
}

pub fn verifier() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn autorisatie_url(basis: &str, challenge: &str, state: &str) -> String {
    let mut url = reqwest::Url::parse(&format!("{}{AUTORISEER}", basis.trim_end_matches('/'))).expect("geldige basis-url");
    url.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("scope", "all openid")
        .append_pair("redirect_uri", REDIRECT)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);
    url.to_string()
}

fn pagina(tekst: &str) -> String {
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>OpenAEC Cloud</title>\
         <body style=\"font-family:system-ui;margin:3rem\"><h2>OpenAEC Cloud</h2><p>{tekst}</p></body>"
    );
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    )
}

/// Wacht op de redirect van de browser en geeft de code terug als `state` klopt.
pub async fn wacht_op_code(listener: TcpListener, state: String) -> Result<String, Fout> {
    loop {
        let (mut verbinding, _) = listener.accept().await?;
        let mut buffer = vec![0u8; 8192];
        let n = verbinding.read(&mut buffer).await?;
        let verzoek = String::from_utf8_lossy(&buffer[..n]);
        let doel = verzoek.lines().next().and_then(|r| r.split_whitespace().nth(1)).unwrap_or("");
        if !doel.starts_with("/callback") {
            // bijv. /favicon.ico: negeren en blijven wachten
            let _ = verbinding.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
            continue;
        }
        let url = reqwest::Url::parse(&format!("http://127.0.0.1{doel}")).map_err(|e| Fout::Controle(e.to_string()))?;
        let waarde = |sleutel: &str| url.query_pairs().find(|(k, _)| k == sleutel).map(|(_, v)| v.to_string());
        let uitkomst = match (waarde("code"), waarde("state"), waarde("error")) {
            (_, _, Some(fout)) => Err(Fout::Controle(format!("inloggen geweigerd: {fout}"))),
            (Some(code), Some(s), _) if s == state => Ok(code),
            _ => Err(Fout::Controle("ongeldig antwoord van de inlogpagina".into())),
        };
        let tekst = if uitkomst.is_ok() {
            "Je bent ingelogd. Je kunt dit venster sluiten en teruggaan naar de app."
        } else {
            "Inloggen is niet gelukt. Probeer het opnieuw vanuit de app."
        };
        let _ = verbinding.write_all(pagina(tekst).as_bytes()).await;
        let _ = verbinding.shutdown().await;
        return uitkomst;
    }
}

async fn token_verzoek(basis: &str, velden: &[(&str, &str)], oud_refresh: Option<&str>) -> Result<Tokens, Fout> {
    let antwoord = reqwest::Client::new()
        .post(format!("{}{TOKEN}", basis.trim_end_matches('/')))
        .form(velden)
        .send()
        .await?;
    let status = antwoord.status();
    if !status.is_success() {
        let tekst = antwoord.text().await.unwrap_or_default();
        return Err(Fout::Server { status: status.as_u16(), tekst });
    }
    let t: TokenAntwoord = antwoord.json().await?;
    let refresh_token = t
        .refresh_token
        .or_else(|| oud_refresh.map(str::to_string))
        .ok_or_else(|| Fout::Controle("geen refresh-token ontvangen".into()))?;
    Ok(Tokens { access_token: t.access_token, refresh_token, verloopt: nu() + t.expires_in.unwrap_or(3600) })
}

pub async fn wissel_code(basis: &str, code: &str, verifier: &str) -> Result<Tokens, Fout> {
    token_verzoek(
        basis,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier),
        ],
        None,
    )
    .await
}

pub async fn vernieuw(basis: &str, tokens: &Tokens) -> Result<Tokens, Fout> {
    token_verzoek(
        basis,
        &[("grant_type", "refresh_token"), ("refresh_token", &tokens.refresh_token), ("client_id", CLIENT_ID)],
        Some(&tokens.refresh_token),
    )
    .await
}
