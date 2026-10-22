//! Wat de sync nu doet, voor het statusvenster, de statusbalk in het hoofdvenster en het pictogram in het
//! systeemvak: de meldingen van de motor (zie `openaec_sync::voortgang`) worden hier bijgehouden en als event
//! "activiteit" naar beide vensters gestuurd.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use openaec_sync::voortgang::{Actie, Gebeurtenis, Richting, Waarnemer};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::{systeemvak, AppState, MapRapport};

/// Zoveel regels laatste activiteit worden onthouden.
const RECENT: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Toestand {
    Bijgewerkt,
    Bezig,
    Gepauzeerd,
    Fout,
}

/// Een bestand dat nu over de lijn gaat.
#[derive(Debug, Clone, Serialize)]
pub struct Overdracht {
    /// Gekoppelde map en pad daarin (met '/').
    lokaal: String,
    pad: String,
    richting: Richting,
    gedaan: u64,
    totaal: u64,
    /// Bytes per seconde sinds de start van dit bestand.
    snelheid: f64,
    #[serde(skip)]
    begin: Instant,
}

/// Iets dat net gebeurd is (voor de lijst met laatste activiteit).
#[derive(Debug, Clone, Serialize)]
pub struct Gebeurd {
    lokaal: String,
    pad: String,
    /// "geupload", "gedownload", "verwijderd", "conflict" of "fout".
    actie: &'static str,
    /// Milliseconden sinds 1970 (de vensters tonen de lokale tijd).
    tijd: u64,
    fout: Option<String>,
}

/// Samenvatting zoals de vensters hem tonen.
#[derive(Debug, Clone, Serialize)]
pub struct Stand {
    toestand: Toestand,
    samenvatting: String,
    gepauzeerd: bool,
    uploaden: u32,
    downloaden: u32,
    actief: Vec<Overdracht>,
    recent: Vec<Gebeurd>,
}

#[derive(Default)]
struct Binnen {
    bezig: bool,
    uploaden: u32,
    downloaden: u32,
    actief: HashMap<(String, String), Overdracht>,
    recent: VecDeque<Gebeurd>,
    fout: Option<String>,
}

#[derive(Default)]
pub struct Activiteit {
    binnen: Mutex<Binnen>,
    gepauzeerd: AtomicBool,
    /// Laatst getoonde toestand en tooltip van het systeemvak (alleen bijwerken als er iets verandert).
    vak: Mutex<Option<(Toestand, String)>>,
}

fn nu_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn actie_tekst(actie: Actie) -> &'static str {
    match actie {
        Actie::Geupload => "geupload",
        Actie::Gedownload => "gedownload",
        Actie::Verwijderd => "verwijderd",
        Actie::Conflict => "conflict",
    }
}

impl Activiteit {
    pub fn gepauzeerd(&self) -> bool {
        self.gepauzeerd.load(Ordering::SeqCst)
    }

    pub fn stand(&self) -> Stand {
        let b = self.binnen.lock().unwrap();
        let gepauzeerd = self.gepauzeerd();
        let (toestand, samenvatting) = if gepauzeerd {
            (Toestand::Gepauzeerd, "Gepauzeerd".to_string())
        } else if b.bezig {
            let delen: Vec<String> = [(b.uploaden, "uploaden"), (b.downloaden, "downloaden")]
                .into_iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, w)| format!("{n} {w}"))
                .collect();
            let tekst = if delen.is_empty() { "Bezig: controleren op wijzigingen".to_string() } else { format!("Bezig: {}", delen.join(", ")) };
            (Toestand::Bezig, tekst)
        } else if let Some(f) = &b.fout {
            (Toestand::Fout, format!("Fout: {f}"))
        } else {
            (Toestand::Bijgewerkt, "Alles bijgewerkt".to_string())
        };
        let mut actief: Vec<Overdracht> = b.actief.values().cloned().collect();
        actief.sort_by_key(|o| o.begin);
        Stand {
            toestand,
            samenvatting,
            gepauzeerd,
            uploaden: b.uploaden,
            downloaden: b.downloaden,
            actief,
            recent: b.recent.iter().cloned().collect(),
        }
    }

    fn onthoud(b: &mut Binnen, g: Gebeurd) {
        b.recent.push_front(g);
        b.recent.truncate(RECENT);
    }

    fn verwerk(&self, lokaal: &str, g: Gebeurtenis) {
        let mut b = self.binnen.lock().unwrap();
        match g {
            Gebeurtenis::Ronde { uploaden, downloaden } => {
                b.uploaden += uploaden;
                b.downloaden += downloaden;
            }
            Gebeurtenis::Start { pad, richting, totaal } => {
                let o = Overdracht { lokaal: lokaal.to_string(), pad: pad.clone(), richting, gedaan: 0, totaal, snelheid: 0.0, begin: Instant::now() };
                b.actief.insert((lokaal.to_string(), pad), o);
            }
            Gebeurtenis::Voortgang { pad, gedaan, totaal, .. } => {
                if let Some(o) = b.actief.get_mut(&(lokaal.to_string(), pad)) {
                    o.gedaan = gedaan;
                    o.totaal = totaal;
                    let sec = o.begin.elapsed().as_secs_f64();
                    o.snelheid = if sec > 0.0 { gedaan as f64 / sec } else { 0.0 };
                }
            }
            Gebeurtenis::Klaar { pad, actie } => {
                b.actief.remove(&(lokaal.to_string(), pad.clone()));
                match actie {
                    Actie::Geupload | Actie::Conflict => b.uploaden = b.uploaden.saturating_sub(1),
                    Actie::Gedownload => b.downloaden = b.downloaden.saturating_sub(1),
                    Actie::Verwijderd => {}
                }
                Self::onthoud(&mut b, Gebeurd { lokaal: lokaal.to_string(), pad, actie: actie_tekst(actie), tijd: nu_ms(), fout: None });
            }
            Gebeurtenis::Fout { pad, fout } => {
                if let Some(o) = b.actief.remove(&(lokaal.to_string(), pad.clone())) {
                    match o.richting {
                        Richting::Upload => b.uploaden = b.uploaden.saturating_sub(1),
                        Richting::Download => b.downloaden = b.downloaden.saturating_sub(1),
                    }
                }
                Self::onthoud(&mut b, Gebeurd { lokaal: lokaal.to_string(), pad, actie: "fout", tijd: nu_ms(), fout: Some(fout) });
            }
        }
    }
}

/// Stuur de stand naar de vensters en werk het systeemvak bij.
pub fn meld(app: &AppHandle) {
    let state = app.state::<AppState>();
    let stand = state.activiteit.stand();
    let _ = app.emit("activiteit", &stand);
    let vak = (stand.toestand, format!("OpenAEC Desktop App – {}", stand.samenvatting));
    let veranderd = {
        let mut vorige = state.activiteit.vak.lock().unwrap();
        let veranderd = vorige.as_ref() != Some(&vak);
        *vorige = Some(vak.clone());
        veranderd
    };
    // Buiten het slot: het systeemvak wordt op de hoofdthread bijgewerkt, en daar kan ook een command meld() aanroepen.
    if veranderd {
        systeemvak::zet_toestand(app, vak.0, &vak.1);
    }
}

/// Begin van een sync-ronde (over een of meer koppelingen).
pub fn begin(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut b = state.activiteit.binnen.lock().unwrap();
        b.bezig = true;
        b.uploaden = 0;
        b.downloaden = 0;
        b.actief.clear();
    }
    meld(app);
}

/// Einde van een sync-ronde; de eerste fout (van een map of een bestand) blijft zichtbaar tot de volgende ronde.
pub fn einde(app: &AppHandle, rapporten: Result<&[MapRapport], &str>) {
    {
        let state = app.state::<AppState>();
        let mut b = state.activiteit.binnen.lock().unwrap();
        b.bezig = false;
        b.uploaden = 0;
        b.downloaden = 0;
        b.actief.clear();
        b.fout = match rapporten {
            Err(f) => Some(f.to_string()),
            Ok(r) => r.iter().find_map(|m| {
                m.fout
                    .as_ref()
                    .map(|f| format!("{}: {f}", m.ruimte_naam))
                    .or_else(|| m.rapport.as_ref().and_then(|r| r.fouten.first().cloned()))
            }),
        };
    }
    meld(app);
}

/// Pauzeren of hervatten; een lopende ronde stopt na het huidige bestand.
pub fn zet_pauze(app: &AppHandle, aan: bool) {
    app.state::<AppState>().activiteit.gepauzeerd.store(aan, Ordering::SeqCst);
    meld(app);
}

/// Waarnemer voor één gekoppelde map tijdens een ronde.
pub struct Koppelwaarnemer {
    pub app: AppHandle,
    pub lokaal: String,
}

impl Waarnemer for Koppelwaarnemer {
    fn melding(&self, g: Gebeurtenis) {
        self.app.state::<AppState>().activiteit.verwerk(&self.lokaal, g);
        meld(&self.app);
    }

    fn gepauzeerd(&self) -> bool {
        self.app.state::<AppState>().activiteit.gepauzeerd()
    }
}
