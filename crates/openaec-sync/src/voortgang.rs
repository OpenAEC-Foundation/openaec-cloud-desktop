//! Voortgang van een sync-ronde: de motor meldt per bestand wanneer hij begint, hoeveel bytes er zijn overgezet,
//! en of het gelukt is. Een [`Waarnemer`] (de desktop-app) zet die meldingen door naar het scherm en kan de ronde
//! tussen twee bestanden laten stoppen (pauzeren). Voortgang in bytes komt hoogstens een paar keer per seconde per
//! bestand door ([`Teller`]).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

/// Minimale tijd tussen twee voortgangsmeldingen voor hetzelfde bestand.
pub const INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Richting {
    Upload,
    Download,
}

/// Wat er met een bestand is gebeurd.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Actie {
    Geupload,
    Gedownload,
    /// Lokaal of op de server verwijderd (op de server naar de prullenbak).
    Verwijderd,
    /// Beide kanten gewijzigd: er staat nu een conflictkopie naast het bestand.
    Conflict,
}

/// Eén melding van de motor; paden met '/' ten opzichte van de gekoppelde map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "soort", rename_all = "lowercase")]
pub enum Gebeurtenis {
    /// Begin van een ronde: zoveel bestanden gaan naar de server en zoveel komen ervan af; `bytes` is hun grootte
    /// samen (wat er echt over de lijn gaat kan door blokhergebruik minder zijn).
    Ronde { uploaden: u32, downloaden: u32, bytes: u64 },
    /// Een bestand gaat over de lijn; `totaal` is de grootte van het bestand.
    Start { pad: String, richting: Richting, totaal: u64 },
    /// Bytes die al zijn overgezet van `totaal` (alleen de blokken die echt over de lijn gaan). De eerste melding
    /// is (0, totaal), zodra bekend is hoeveel er over de lijn moet.
    Voortgang { pad: String, richting: Richting, gedaan: u64, totaal: u64 },
    Klaar { pad: String, actie: Actie },
    Fout { pad: String, fout: String },
}

/// Ontvanger van de meldingen. Beide methoden hebben een standaard die niets doet.
pub trait Waarnemer: Send + Sync {
    fn melding(&self, _gebeurtenis: Gebeurtenis) {}

    /// Wordt tussen twee bestanden gevraagd; `true` beëindigt de ronde (de rest volgt in een volgende ronde).
    fn gepauzeerd(&self) -> bool {
        false
    }
}

/// Waarnemer die niets doet.
pub struct Stil;

impl Waarnemer for Stil {}

/// Zet bytevoortgang om in [`Gebeurtenis::Voortgang`], hoogstens eens per [`INTERVAL`]; de laatste stap (alles
/// overgezet) komt altijd door.
pub struct Teller<'a> {
    waarnemer: &'a dyn Waarnemer,
    pad: String,
    richting: Richting,
    vorige: Mutex<Option<Instant>>,
}

impl<'a> Teller<'a> {
    pub fn new(waarnemer: &'a dyn Waarnemer, pad: &str, richting: Richting) -> Self {
        Self { waarnemer, pad: pad.to_string(), richting, vorige: Mutex::new(None) }
    }

    pub fn stand(&self, gedaan: u64, totaal: u64) {
        {
            let mut vorige = self.vorige.lock().unwrap();
            let nu = Instant::now();
            let klaar = gedaan >= totaal;
            if !klaar && vorige.is_some_and(|v| nu.duration_since(v) < INTERVAL) {
                return;
            }
            *vorige = Some(nu);
        }
        self.waarnemer.melding(Gebeurtenis::Voortgang { pad: self.pad.clone(), richting: self.richting, gedaan, totaal });
    }
}
