//! Client voor de AI-gateway van OpenAEC: de modellen (`GET /v1/models`) en een vraag met een
//! gestreamd antwoord (`POST /v1/chat/completions` met `stream: true`, server-sent events).
//!
//! De gateway wil een toegangstoken van de cloud met scope `ai` (zie [`crate::oauth::TOOLS`]) en de host van de
//! klantomgeving in `X-OpenAEC-Omgeving`. De autorisatie komt per verzoek uit een [`Tokenbron`]: weigert de gateway het
//! token (401), dan vernieuwt de bron één keer en gaat het verzoek één keer opnieuw. Fouten worden een [`AiFout`] met
//! een Nederlandse melding; de inhoud van vragen en antwoorden komt nooit in een melding of log.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sessie::Tokenbron;
use crate::Fout;

/// De gateway van OpenAEC.
pub const STANDAARD_GATEWAY: &str = "https://ai.cloud.open-aec.eu";
pub const OMGEVINGKOP: &str = "X-OpenAEC-Omgeving";
pub const TOOLKOP: &str = "X-OpenAEC-Tool";
pub const VRAAGKOP: &str = "X-OpenAEC-Vraag-Id";
/// Het veld waarin de gateway de denkblokken van het model ondoorzichtig meegeeft; ongewijzigd terugsturen.
pub const DENKBLOKKEN: &str = "openaec_denkblokken";

/// Een model uit `/v1/models`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    #[serde(default)]
    pub naam: Option<String>,
    /// `EU` (gegevens blijven in de EU) of `VS`.
    #[serde(default)]
    pub datalabel: Option<String>,
    /// Het standaardmodel van de gateway.
    #[serde(default)]
    pub standaard: bool,
}

/// Het verbruik van een vraag (`usage` zoals de gateway het meestuurt).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Gebruik {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    /// Het deel van de invoer dat uit de cache kwam.
    #[serde(default)]
    pub cached_tokens: u64,
}

impl Gebruik {
    fn uit(waarde: &Value) -> Option<Self> {
        let getal = |v: &Value, k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
        if !waarde.is_object() {
            return None;
        }
        let (invoer, uitvoer) = (getal(waarde, "prompt_tokens"), getal(waarde, "completion_tokens"));
        Some(Self {
            prompt_tokens: invoer,
            completion_tokens: uitvoer,
            total_tokens: waarde.get("total_tokens").and_then(Value::as_u64).unwrap_or(invoer + uitvoer),
            cached_tokens: waarde.get("prompt_tokens_details").map(|d| getal(d, "cached_tokens")).unwrap_or(0),
        })
    }
}

/// Een stukje van een gestreamd antwoord.
#[derive(Debug, Clone, PartialEq)]
pub enum Stuk {
    Tekst(String),
    /// De ondoorzichtige denkblokken van het model, voor de volgende beurt (zie [`DENKBLOKKEN`]).
    Denkblokken(String),
    /// Het model is klaar (`finish_reason`: `stop`, `length`, `tool_calls`, …).
    Einde(String),
    Gebruik(Gebruik),
}

/// Wat er mis ging, met een melding voor mensen ([`std::fmt::Display`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(tag = "soort", rename_all = "snake_case")]
pub enum AiFout {
    #[error("Het AI-tegoed van je omgeving is op. Een beheerder kan tegoed bijkopen via Beheer → Abonnement.")]
    GeenTegoed,
    #[error("Je hebt je AI-plafond bereikt. Je kunt {} weer vragen.", over(*retry_na))]
    Plafond { retry_na: Option<u64> },
    #[error("Te veel vragen achter elkaar. Probeer het {} opnieuw.", over(*retry_na))]
    TeVeelVragen { retry_na: Option<u64> },
    #[error("Dit AI-model is in je omgeving niet toegestaan. Kies een ander model.")]
    ModelNietToegestaan,
    #[error("De assistent van de Desktop App staat in je omgeving niet aan. Vraag je beheerder om hem aan te zetten.")]
    ToolNietToegestaan,
    #[error("Je bent geen actief lid van deze omgeving, of de omgeving heeft geen AI. Vraag je beheerder om toegang.")]
    GeenLid,
    #[error("Als gast in deze omgeving kun je de assistent niet gebruiken.")]
    Gast,
    #[error("Je sessie voor de assistent is verlopen. Log opnieuw in.")]
    SessieVerlopen,
    #[error("De AI-dienst is op dit moment niet beschikbaar. Probeer het {} opnieuw.", over(*retry_na))]
    Onbereikbaar { retry_na: Option<u64> },
    #[error("{melding}")]
    Ongeldig { melding: String },
}

/// "over 5 minuten", of "later" bij een onbekende wachttijd.
fn over(retry_na: Option<u64>) -> String {
    retry_na.map_or_else(|| "later".to_string(), |s| format!("over {}", wachttijd(s)))
}

/// Een wachttijd in seconden als "45 seconden", "1 minuut", "12 minuten", "2 uur" of "1 uur en 5 minuten" (naar boven
/// afgerond op hele minuten vanaf een minuut).
pub fn wachttijd(seconden: u64) -> String {
    if seconden < 60 {
        return if seconden == 1 { "1 seconde".into() } else { format!("{seconden} seconden") };
    }
    let minuten = seconden.div_ceil(60);
    let (uren, rest) = (minuten / 60, minuten % 60);
    let min = |m: u64| if m == 1 { "1 minuut".to_string() } else { format!("{m} minuten") };
    match (uren, rest) {
        (0, m) => min(m),
        (u, 0) => format!("{u} uur"),
        (u, m) => format!("{u} uur en {}", min(m)),
    }
}

impl AiFout {
    /// De fout bij een foutantwoord van de gateway (`{"error": {"code", "message"}}`), met `Retry-After` in seconden.
    pub fn uit_antwoord(status: u16, tekst: &str, retry_na: Option<u64>) -> Self {
        let fout: Value = serde_json::from_str(tekst).unwrap_or(Value::Null);
        let code = fout.pointer("/error/code").and_then(Value::as_str).unwrap_or("");
        let melding = fout.pointer("/error/message").and_then(Value::as_str).unwrap_or("").trim().to_string();
        Self::uit_code(status, code, &melding, retry_na)
    }

    fn uit_code(status: u16, code: &str, melding: &str, retry_na: Option<u64>) -> Self {
        match code {
            "geen_tegoed" => return AiFout::GeenTegoed,
            "plafond_bereikt" => return AiFout::Plafond { retry_na },
            "te_veel_vragen" => return AiFout::TeVeelVragen { retry_na },
            "model_niet_toegestaan" => return AiFout::ModelNietToegestaan,
            "tool_niet_toegestaan" => return AiFout::ToolNietToegestaan,
            "geen_lid" => return AiFout::GeenLid,
            "gast" => return AiFout::Gast,
            // Een token zonder scope `ai` (bijv. van voor de scope bestond): opnieuw inloggen geeft er een met `ai`.
            "scope_ontbreekt" | "ongeldig_token" => return AiFout::SessieVerlopen,
            "beleid_onbereikbaar" | "dienst_onbereikbaar" | "aanbieder_onbereikbaar" | "aanbieder_tijdslimiet"
            | "model_niet_beschikbaar" | "aanbieder_fout" | "aanbieder_geweigerd" | "interne_fout" => {
                return AiFout::Onbereikbaar { retry_na }
            }
            "model_not_found" => {
                return AiFout::Ongeldig { melding: "Dit AI-model bestaat niet (meer). Kies een ander model.".into() }
            }
            "denkblokken_ongeldig" => {
                return AiFout::Ongeldig { melding: "Het gesprek is beschadigd. Begin een nieuw gesprek.".into() }
            }
            "vraag_te_groot" => {
                return AiFout::Ongeldig {
                    melding: "De vraag met de bijlagen is te groot voor de AI-dienst (hoogstens 8 MB). Haal een bijlage weg of begin een nieuw gesprek.".into(),
                }
            }
            "afbeelding_te_groot" | "te_veel_afbeeldingen" | "afbeelding_ongeldig" => {
                return AiFout::Ongeldig {
                    melding: "De AI-dienst weigert een afbeelding (te groot, te veel in het gesprek of geen png, jpg of webp).".into(),
                }
            }
            _ => {}
        }
        match status {
            401 => AiFout::SessieVerlopen,
            402 => AiFout::GeenTegoed,
            403 => AiFout::GeenLid,
            429 => AiFout::TeVeelVragen { retry_na },
            500.. => AiFout::Onbereikbaar { retry_na },
            _ if melding.is_empty() => AiFout::Ongeldig { melding: "De AI-dienst kon deze vraag niet verwerken.".into() },
            _ => AiFout::Ongeldig { melding: format!("De AI-dienst kon deze vraag niet verwerken: {melding}") },
        }
    }

    fn uit_fout(f: Fout) -> Self {
        match f {
            Fout::SessieVerlopen => AiFout::SessieVerlopen,
            _ => AiFout::Onbereikbaar { retry_na: None },
        }
    }
}

/// Leest server-sent events uit losse stukken bytes (een stuk kan midden in een regel of een teken eindigen) en geeft
/// per compleet event de inhoud van de `data:`-regels.
#[derive(Default)]
pub struct SseLezer {
    buffer: Vec<u8>,
}

impl SseLezer {
    pub fn voeg_toe(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend(bytes.iter().copied().filter(|b| *b != b'\r'));
        let mut events = Vec::new();
        while let Some(einde) = self.buffer.windows(2).position(|w| w == b"\n\n") {
            let event: Vec<u8> = self.buffer.drain(..einde + 2).collect();
            if let Some(data) = Self::data(&event) {
                events.push(data);
            }
        }
        events
    }

    /// Wat er nog in de buffer staat als de verbinding sluit (een laatste event zonder lege regel erna).
    pub fn rest(&mut self) -> Option<String> {
        let event = std::mem::take(&mut self.buffer);
        Self::data(&event)
    }

    fn data(event: &[u8]) -> Option<String> {
        let tekst = String::from_utf8_lossy(event);
        let regels: Vec<&str> = tekst
            .lines()
            .filter_map(|r| r.strip_prefix("data:"))
            .map(|r| r.strip_prefix(' ').unwrap_or(r))
            .collect();
        (!regels.is_empty()).then(|| regels.join("\n"))
    }
}

/// Wat een event van de stream betekent.
enum Event {
    Stukken(Vec<Stuk>),
    Klaar,
    Fout(AiFout),
}

fn lees_event(data: &str) -> Event {
    if data.trim() == "[DONE]" {
        return Event::Klaar;
    }
    let Ok(waarde) = serde_json::from_str::<Value>(data) else { return Event::Stukken(Vec::new()) };
    if waarde.get("error").is_some() {
        return Event::Fout(AiFout::uit_antwoord(0, data, None)).corrigeer_onbekend();
    }
    let mut stukken = Vec::new();
    if let Some(keuze) = waarde.pointer("/choices/0") {
        if let Some(delta) = keuze.get("delta") {
            if let Some(t) = delta.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                stukken.push(Stuk::Tekst(t.to_string()));
            }
            if let Some(d) = delta.get(DENKBLOKKEN).and_then(Value::as_str) {
                stukken.push(Stuk::Denkblokken(d.to_string()));
            }
        }
        if let Some(r) = keuze.get("finish_reason").and_then(Value::as_str) {
            stukken.push(Stuk::Einde(r.to_string()));
        }
    }
    if let Some(g) = waarde.get("usage").and_then(Gebruik::uit) {
        stukken.push(Stuk::Gebruik(g));
    }
    Event::Stukken(stukken)
}

impl Event {
    /// Een fout midden in de stream zonder bekende code: de dienst viel weg.
    fn corrigeer_onbekend(self) -> Self {
        match self {
            Event::Fout(AiFout::Ongeldig { .. }) | Event::Fout(AiFout::SessieVerlopen) => {
                Event::Fout(AiFout::Onbereikbaar { retry_na: None })
            }
            e => e,
        }
    }
}

/// Hoe een vraag afliep.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Afloop {
    /// `X-OpenAEC-Vraag-Id` van de gateway (voor de beheerder bij een vraag over het verbruik).
    pub vraag_id: Option<String>,
}

/// Een client voor de gateway, namens één omgeving.
#[derive(Clone)]
pub struct AiClient {
    gateway: String,
    omgeving: String,
    tool: Option<String>,
    bron: Arc<dyn Tokenbron>,
    http: reqwest::Client,
}

impl AiClient {
    /// `omgeving`: de host van de klantomgeving (zonder schema en poort), zoals de gateway hem verwacht.
    pub fn new(gateway: &str, omgeving: &str, tool: Option<&str>, bron: Arc<dyn Tokenbron>, http: reqwest::Client) -> Self {
        Self {
            gateway: gateway.trim_end_matches('/').to_string(),
            omgeving: omgeving.to_string(),
            tool: tool.map(str::to_string),
            bron,
            http,
        }
    }

    /// Verstuur een verzoek met de autorisatie van de bron; bij 401 één keer vernieuwen en opnieuw.
    async fn verstuur(&self, maak: impl Fn(&str) -> reqwest::RequestBuilder) -> Result<reqwest::Response, AiFout> {
        let mut autorisatie = self.bron.geef().await.map_err(AiFout::uit_fout)?;
        let mut opnieuw = true;
        loop {
            let mut verzoek = maak(&self.gateway).header(reqwest::header::AUTHORIZATION, &autorisatie).header(OMGEVINGKOP, &self.omgeving);
            if let Some(tool) = &self.tool {
                verzoek = verzoek.header(TOOLKOP, tool);
            }
            let antwoord = verzoek.send().await.map_err(|_| AiFout::Onbereikbaar { retry_na: None })?;
            let status = antwoord.status().as_u16();
            if status == 401 && opnieuw {
                opnieuw = false;
                match self.bron.na_weigering(&autorisatie).await.map_err(AiFout::uit_fout)? {
                    Some(nieuw) => {
                        autorisatie = nieuw;
                        continue;
                    }
                    None => return Err(AiFout::SessieVerlopen),
                }
            }
            if !(200..300).contains(&status) {
                let retry_na = antwoord
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok());
                let tekst = antwoord.text().await.unwrap_or_default();
                return Err(AiFout::uit_antwoord(status, &tekst, retry_na));
            }
            return Ok(antwoord);
        }
    }

    /// De modellen die de gateway aanbiedt (met datalabel en het standaardmodel).
    pub async fn modellen(&self) -> Result<Vec<Model>, AiFout> {
        #[derive(Deserialize)]
        struct Lijst {
            data: Vec<Model>,
        }
        let antwoord = self.verstuur(|g| self.http.get(format!("{g}/v1/models"))).await?;
        let lijst: Lijst = antwoord.json().await.map_err(|_| AiFout::Onbereikbaar { retry_na: None })?;
        Ok(lijst.data)
    }

    /// Stel een vraag (`body` in de chatvorm van de gateway; `stream` wordt aangezet) en geef elk stukje van het antwoord aan
    /// `bij`. Stoppen: de future laten vallen (de gateway telt de vraag dan als afgebroken).
    pub async fn vraag(&self, body: &Value, mut bij: impl FnMut(Stuk) + Send) -> Result<Afloop, AiFout> {
        let mut body = body.clone();
        body["stream"] = Value::Bool(true);
        body["stream_options"] = serde_json::json!({ "include_usage": true });
        let mut antwoord = self.verstuur(|g| self.http.post(format!("{g}/v1/chat/completions")).json(&body)).await?;
        let afloop = Afloop {
            vraag_id: antwoord.headers().get(VRAAGKOP).and_then(|v| v.to_str().ok()).map(str::to_string),
        };
        let mut lezer = SseLezer::default();
        let mut klaar = false;
        let verwerk = |events: Vec<String>, bij: &mut dyn FnMut(Stuk)| -> Result<bool, AiFout> {
            for data in events {
                match lees_event(&data) {
                    Event::Stukken(s) => s.into_iter().for_each(&mut *bij),
                    Event::Klaar => return Ok(true),
                    Event::Fout(f) => return Err(f),
                }
            }
            Ok(false)
        };
        while !klaar {
            match antwoord.chunk().await {
                Ok(Some(bytes)) => klaar = verwerk(lezer.voeg_toe(&bytes), &mut bij)?,
                Ok(None) => {
                    klaar = verwerk(lezer.rest().into_iter().collect(), &mut bij)?;
                    if !klaar {
                        // De verbinding sloot zonder [DONE]: het antwoord is onvolledig.
                        return Err(AiFout::Onbereikbaar { retry_na: None });
                    }
                }
                Err(_) => return Err(AiFout::Onbereikbaar { retry_na: None }),
            }
        }
        Ok(afloop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_over_willekeurige_grenzen() {
        let stroom = "data: {\"a\":1}\r\n\r\n: commentaar\n\ndata: één\ndata: twee\n\ndata: [DONE]\n\n";
        for stap in 1..stroom.len() {
            let mut lezer = SseLezer::default();
            let mut events = Vec::new();
            for deel in stroom.as_bytes().chunks(stap) {
                events.extend(lezer.voeg_toe(deel));
            }
            assert_eq!(events, ["{\"a\":1}", "één\ntwee", "[DONE]"], "stap {stap}");
        }
        let mut lezer = SseLezer::default();
        assert!(lezer.voeg_toe(b"data: [DONE]").is_empty());
        assert_eq!(lezer.rest().as_deref(), Some("[DONE]"));
    }

    #[test]
    fn wachttijden() {
        assert_eq!(wachttijd(1), "1 seconde");
        assert_eq!(wachttijd(45), "45 seconden");
        assert_eq!(wachttijd(60), "1 minuut");
        assert_eq!(wachttijd(61), "2 minuten");
        assert_eq!(wachttijd(3600), "1 uur");
        assert_eq!(wachttijd(3600 * 2 + 300), "2 uur en 5 minuten");
        assert_eq!(AiFout::Plafond { retry_na: Some(600) }.to_string(), "Je hebt je AI-plafond bereikt. Je kunt over 10 minuten weer vragen.");
        assert_eq!(AiFout::Plafond { retry_na: None }.to_string(), "Je hebt je AI-plafond bereikt. Je kunt later weer vragen.");
        assert_eq!(
            AiFout::TeVeelVragen { retry_na: Some(30) }.to_string(),
            "Te veel vragen achter elkaar. Probeer het over 30 seconden opnieuw."
        );
        assert_eq!(AiFout::Onbereikbaar { retry_na: None }.to_string(), "De AI-dienst is op dit moment niet beschikbaar. Probeer het later opnieuw.");
    }

    #[test]
    fn foutcodes_van_de_gateway() {
        let f = |status, code: &str| AiFout::uit_antwoord(status, &format!(r#"{{"error":{{"code":"{code}","message":"x"}}}}"#), Some(5));
        assert_eq!(f(402, "geen_tegoed"), AiFout::GeenTegoed);
        assert_eq!(f(429, "plafond_bereikt"), AiFout::Plafond { retry_na: Some(5) });
        assert_eq!(f(429, "te_veel_vragen"), AiFout::TeVeelVragen { retry_na: Some(5) });
        assert_eq!(f(403, "model_niet_toegestaan"), AiFout::ModelNietToegestaan);
        assert_eq!(f(403, "geen_lid"), AiFout::GeenLid);
        assert_eq!(f(403, "gast"), AiFout::Gast);
        assert_eq!(f(403, "scope_ontbreekt"), AiFout::SessieVerlopen);
        assert_eq!(f(503, "beleid_onbereikbaar"), AiFout::Onbereikbaar { retry_na: Some(5) });
        assert_eq!(AiFout::uit_antwoord(502, "<html>", None), AiFout::Onbereikbaar { retry_na: None });
        assert!(matches!(f(400, "ongeldige_vraag"), AiFout::Ongeldig { melding } if melding.ends_with(": x")));
        // In JSON voor het venster: de soort en de wachttijd.
        assert_eq!(
            serde_json::to_value(AiFout::Plafond { retry_na: Some(5) }).unwrap(),
            serde_json::json!({"soort": "plafond", "retry_na": 5})
        );
    }

    #[test]
    fn stukken_uit_een_event() {
        let Event::Stukken(s) = lees_event(r#"{"choices":[{"index":0,"delta":{"content":"Hoi"},"finish_reason":null}]}"#) else { panic!() };
        assert_eq!(s, [Stuk::Tekst("Hoi".into())]);
        let Event::Stukken(s) = lees_event(r#"{"choices":[{"delta":{"openaec_denkblokken":"b1.x"},"finish_reason":"stop"}]}"#) else { panic!() };
        assert_eq!(s, [Stuk::Denkblokken("b1.x".into()), Stuk::Einde("stop".into())]);
        let Event::Stukken(s) =
            lees_event(r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_tokens_details":{"cached_tokens":4}}}"#)
        else {
            panic!()
        };
        assert_eq!(s, [Stuk::Gebruik(Gebruik { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15, cached_tokens: 4 })]);
        assert!(matches!(lees_event("[DONE]"), Event::Klaar));
        assert!(matches!(lees_event(r#"{"error":{"code":"aanbieder_fout"}}"#), Event::Fout(AiFout::Onbereikbaar { .. })));
        assert!(matches!(lees_event(r#"{"error":{"code":"geen_tegoed"}}"#), Event::Fout(AiFout::GeenTegoed)));
    }
}
