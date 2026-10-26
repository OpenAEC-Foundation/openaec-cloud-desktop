//! Autorisatie per verzoek. Een [`Client`](crate::Client) vraagt bij elk verzoek de waarde van de Authorization-header
//! op bij een [`Tokenbron`], in plaats van één vaste waarde voor een hele ronde: een OAuth-token leeft maar kort (een
//! kwartier), en een ronde met duizenden bestanden duurt langer.
//!
//! [`Sessie`] is de bron voor een omgeving: een API-sleutel zoals bewaard, of OAuth-tokens die ruim voor het verlopen
//! vernieuwd worden. Weigert de server het token toch (401, bijv. omdat hij het eerder introk), dan vernieuwt de client
//! één keer geforceerd en probeert het verzoek één keer opnieuw ([`Tokenbron::na_weigering`]). Er loopt hoogstens één
//! vernieuwing tegelijk per sessie: een refresh-token is na gebruik ongeldig, dus het nieuwe paar wordt direct bewaard en
//! wie op het slot wachtte, gebruikt dat. Lukt vernieuwen niet meer (refresh-token verlopen of ingetrokken), dan geeft de
//! bron [`Fout::SessieVerlopen`] en vraagt hij de server niet steeds opnieuw met hetzelfde dode refresh-token.

use futures_util::future::BoxFuture;
use tokio::sync::Mutex;

use crate::oauth::{self, Tokens};
use crate::Fout;

/// Levert de Authorization-header voor elk verzoek naar de API.
pub trait Tokenbron: Send + Sync {
    /// De waarde voor het volgende verzoek; vernieuwt zo nodig eerst (ruim voor het verlopen).
    fn geef(&self) -> BoxFuture<'_, Result<String, Fout>>;

    /// De server weigerde `afgewezen` (401): de waarde om het verzoek één keer opnieuw mee te proberen. Heeft een ander
    /// verzoek al vernieuwd, dan is dat die nieuwe waarde, anders wordt er geforceerd vernieuwd. `None`: niet te
    /// vernieuwen (een API-sleutel); de weigering geldt dan.
    fn na_weigering<'a>(&'a self, afgewezen: &'a str) -> BoxFuture<'a, Result<Option<String>, Fout>>;
}

/// Een vaste waarde, bijv. een API-sleutel (`token sleutel:geheim`): werkt zonder vernieuwen.
pub struct VasteSleutel(pub String);

impl Tokenbron for VasteSleutel {
    fn geef(&self) -> BoxFuture<'_, Result<String, Fout>> {
        Box::pin(async move { Ok(self.0.clone()) })
    }

    fn na_weigering<'a>(&'a self, _afgewezen: &'a str) -> BoxFuture<'a, Result<Option<String>, Fout>> {
        Box::pin(async { Ok(None) })
    }
}

/// Waar een sessie haar sleutel of tokens bewaart (de sleutelbos van het systeem, een configuratiebestand, of het
/// geheugen in een test). De waarde is een API-sleutel of de tokens als JSON ([`Tokens`]).
pub trait Bewaring: Send + Sync {
    fn lees(&self) -> Option<String>;
    fn bewaar(&self, waarde: &str) -> Result<(), String>;
}

/// Bewaring in het geheugen (tests, of een sessie die niets hoeft te onthouden).
#[derive(Default)]
pub struct Geheugen(pub std::sync::Mutex<Option<String>>);

impl Geheugen {
    pub fn met(waarde: impl Into<String>) -> Self {
        Self(std::sync::Mutex::new(Some(waarde.into())))
    }
}

impl Bewaring for Geheugen {
    fn lees(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }

    fn bewaar(&self, waarde: &str) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(waarde.to_string());
        Ok(())
    }
}

#[derive(Clone)]
enum Bewaard {
    Sleutel(String),
    Tokens(Tokens),
}

impl Bewaard {
    fn uit(waarde: String) -> Self {
        match serde_json::from_str::<Tokens>(&waarde) {
            Ok(t) => Bewaard::Tokens(t),
            Err(_) => Bewaard::Sleutel(waarde),
        }
    }

    fn autorisatie(&self) -> String {
        match self {
            Bewaard::Sleutel(s) => s.clone(),
            Bewaard::Tokens(t) => t.autorisatie(),
        }
    }
}

#[derive(Default)]
struct Stand {
    /// Wat het laatst gelezen of vernieuwd is; None: nog lezen uit de bewaring.
    waarde: Option<Bewaard>,
    /// Het refresh-token waarmee vernieuwen geweigerd werd: daarmee niet nog eens proberen.
    geweigerd: Option<String>,
}

/// De autorisatie van één omgeving, gedeeld door alle clients (rondes, bewaking, vensters, bestanden op aanvraag).
pub struct Sessie {
    basis: String,
    bewaring: Box<dyn Bewaring>,
    /// Ook het slot voor vernieuwen: hoogstens één vernieuwing tegelijk.
    stand: Mutex<Stand>,
}

impl Sessie {
    pub fn new(basis: &str, bewaring: impl Bewaring + 'static) -> Self {
        Self { basis: basis.trim_end_matches('/').to_string(), bewaring: Box::new(bewaring), stand: Mutex::new(Stand::default()) }
    }

    pub fn basis(&self) -> &str {
        &self.basis
    }

    /// Vergeet wat in het geheugen staat (na opnieuw inloggen of uitloggen): het volgende verzoek leest de bewaring.
    pub async fn vergeet(&self) {
        *self.stand.lock().await = Stand::default();
    }

    /// Als [`Sessie::vergeet`], zonder te wachten (bijv. vanuit een command zonder async runtime). Lukt het niet meteen
    /// (er loopt een vernieuwing), dan merkt die sessie het nieuwe paar alsnog bij de volgende weigering.
    pub fn probeer_te_vergeten(&self) {
        if let Ok(mut stand) = self.stand.try_lock() {
            *stand = Stand::default();
        }
    }

    /// Lees de bewaring opnieuw. Werd vernieuwen eerder geweigerd en staat daar nog hetzelfde refresh-token, dan is de
    /// sessie verlopen (zonder de server te vragen).
    fn lees(&self, stand: &mut Stand) -> Result<Bewaard, Fout> {
        let waarde = self.bewaring.lees().map(Bewaard::uit).ok_or(Fout::SessieVerlopen)?;
        if let (Bewaard::Tokens(t), Some(geweigerd)) = (&waarde, &stand.geweigerd) {
            if &t.refresh_token == geweigerd {
                return Err(Fout::SessieVerlopen);
            }
        }
        stand.geweigerd = None;
        stand.waarde = Some(waarde.clone());
        Ok(waarde)
    }

    /// Vernieuw met het refresh-token van `tokens` en bewaar het nieuwe paar meteen. Weigert de server (4xx), dan is
    /// de sessie verlopen. Een netwerk- of serverfout bij een token dat nog geldig is (alleen `geforceerd` niet): dan
    /// gewoon het huidige token gebruiken.
    async fn vernieuw(&self, stand: &mut Stand, tokens: Tokens, geforceerd: bool) -> Result<String, Fout> {
        match oauth::vernieuw(&self.basis, &tokens).await {
            Ok(nieuw) => {
                // Het oude refresh-token is nu ongeldig: het nieuwe paar moet bewaard blijven, ook als de bewaring even
                // niet lukt (dan in elk geval in het geheugen).
                let _ = self.bewaring.bewaar(&serde_json::to_string(&nieuw).expect("tokens als JSON"));
                let autorisatie = nieuw.autorisatie();
                stand.waarde = Some(Bewaard::Tokens(nieuw));
                stand.geweigerd = None;
                Ok(autorisatie)
            }
            Err(Fout::Server { status: 400..=499, .. }) => {
                stand.geweigerd = Some(tokens.refresh_token);
                stand.waarde = None;
                Err(Fout::SessieVerlopen)
            }
            Err(_) if !geforceerd && !tokens.verloopt_binnen(0) => Ok(tokens.autorisatie()),
            Err(f) => Err(f),
        }
    }
}

impl Tokenbron for Sessie {
    fn geef(&self) -> BoxFuture<'_, Result<String, Fout>> {
        Box::pin(async move {
            let mut stand = self.stand.lock().await;
            let waarde = match stand.waarde.clone() {
                Some(w) => w,
                None => self.lees(&mut stand)?,
            };
            match waarde {
                Bewaard::Sleutel(s) => Ok(s),
                Bewaard::Tokens(t) if !t.verlopen() => Ok(t.autorisatie()),
                Bewaard::Tokens(t) => self.vernieuw(&mut stand, t, false).await,
            }
        })
    }

    fn na_weigering<'a>(&'a self, afgewezen: &'a str) -> BoxFuture<'a, Result<Option<String>, Fout>> {
        Box::pin(async move {
            let mut stand = self.stand.lock().await;
            // Al vernieuwd door een ander verzoek (dat eerder het slot had)?
            if let Some(w) = stand.waarde.as_ref().filter(|w| w.autorisatie() != afgewezen) {
                return Ok(Some(w.autorisatie()));
            }
            // Opnieuw ingelogd sinds dit verzoek begon?
            let waarde = self.lees(&mut stand)?;
            if waarde.autorisatie() != afgewezen {
                return Ok(Some(waarde.autorisatie()));
            }
            match waarde {
                Bewaard::Sleutel(_) => Ok(None),
                Bewaard::Tokens(t) => self.vernieuw(&mut stand, t, true).await.map(Some),
            }
        })
    }
}
