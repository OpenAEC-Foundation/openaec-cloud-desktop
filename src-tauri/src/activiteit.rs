//! Wat de sync nu doet, voor het statusvenster, de statusbalk in het hoofdvenster en het pictogram in het
//! systeemvak: de meldingen van de motor (zie `openaec_sync::voortgang`) worden hier bijgehouden en als event
//! "activiteit" naar beide vensters gestuurd.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use openaec_sync::voortgang::{Actie, Gebeurtenis, Richting, Waarnemer};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::schatting::Schatter;
use crate::{systeemvak, AppState, MapRapport};

/// Zoveel regels laatste activiteit worden onthouden.
const RECENT: usize = 30;

/// Venster voor de gemiddelde snelheid van een ronde (seconden).
const SNELHEIDSVENSTER: f64 = 8.0;

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
    /// Grootte van het bestand.
    grootte: u64,
    /// Overgezet van `totaal`: de bytes die echt over de lijn gaan (blokken die er al zijn, gaan niet mee).
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
    /// Bij een upload of download: grootte van het bestand en wat er echt over de lijn ging.
    grootte: Option<u64>,
    verstuurd: Option<u64>,
}

/// Een bestand dat niet lukt (deze ronde mislukt, of wachtend na eerdere mislukte pogingen), voor de details onder de
/// samenvatting "3 bestanden lukken niet".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Misluktregel {
    lokaal: String,
    pad: String,
    reden: String,
    /// Unix-tijd in milliseconden van de volgende poging (alleen bij een bestand dat wacht).
    opnieuw_om: Option<u64>,
}

/// Voortgang van de lopende ronde (over alle gekoppelde mappen samen).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RondeStand {
    /// Bestanden die nog moeten (of nu over de lijn gaan).
    nog: u32,
    /// Bestandsgrootte die al verwerkt is, van het totaal van de ronde.
    bytes_gedaan: u64,
    bytes_totaal: u64,
    /// Bytes per seconde over de laatste seconden (voortschrijdend gemiddelde).
    snelheid: f64,
    /// Geschatte resterende tijd in seconden; None zolang er nog niets te schatten valt (zie `schatting`).
    resterend: Option<u64>,
    /// Wat blokhergebruik deze ronde scheelde (bytes die niet over de lijn hoefden).
    bespaard: u64,
}

/// Tellers van een ronde. Tijden in seconden sinds een vast beginpunt (zie `klok`), zodat het rekenwerk te testen is.
#[derive(Debug, Default)]
struct Ronde {
    bytes: u64,
    bytes_klaar: u64,
    verstuurd_klaar: u64,
    bespaard: u64,
    /// Bestanden in de ronde (uploads en downloads) en hoeveel daarvan af zijn (gelukt of mislukt).
    bestanden: u64,
    bestanden_klaar: u64,
    /// (tijd, totaal verstuurd tot dan) voor de voortschrijdende snelheid.
    metingen: VecDeque<(f64, u64)>,
    /// Resterende tijd, op bytes én bestanden en gedempt.
    schatter: Schatter,
}

/// Gemiddelde snelheid (bytes/s) over het venster: van de oudste meting binnen het venster tot nu.
fn voortschrijdend(metingen: &VecDeque<(f64, u64)>, nu: f64, verstuurd_nu: u64) -> f64 {
    let Some(&(t0, b0)) = metingen.iter().find(|(t, _)| nu - t <= SNELHEIDSVENSTER) else {
        return 0.0;
    };
    let dt = nu - t0;
    if dt < 0.5 {
        return 0.0;
    }
    verstuurd_nu.saturating_sub(b0) as f64 / dt
}

/// Seconden sinds de eerste aanroep: de klok voor de rondetellers.
fn klok() -> f64 {
    static BEGIN: OnceLock<Instant> = OnceLock::new();
    BEGIN.get_or_init(Instant::now).elapsed().as_secs_f64()
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
    /// Alleen tijdens een ronde waarin iets over de lijn gaat.
    ronde: Option<RondeStand>,
    /// De sessie is verlopen: de vensters vragen om opnieuw in te loggen (knop Inloggen).
    sessie_verlopen: bool,
    /// Bestanden die niet lukken, over alle mappen (de details bij de samenvatting).
    mislukt: Vec<Misluktregel>,
}

#[derive(Default)]
struct Binnen {
    bezig: bool,
    uploaden: u32,
    downloaden: u32,
    actief: HashMap<(String, String), Overdracht>,
    recent: VecDeque<Gebeurd>,
    fout: Option<String>,
    /// Per gekoppelde map: de bestanden die niet lukken volgens de laatste ronde van die map.
    mislukt: HashMap<String, Vec<Misluktregel>>,
    ronde: Ronde,
}

impl Binnen {
    /// Gaat er deze ronde echt iets over de lijn? Een controle zonder werk telt niet als "bezig".
    fn werk(&self) -> bool {
        self.uploaden + self.downloaden > 0 || !self.actief.is_empty() || self.ronde.bytes > 0
    }

    /// Verwerk de rapporten van een afgelopen ronde: een fout van een map blijft zichtbaar tot de volgende ronde, en per
    /// map de bestanden die niet lukken (vervangen voor de mappen in deze ronde).
    fn na_ronde(&mut self, rapporten: Result<&[MapRapport], &str>) {
        self.bezig = false;
        self.uploaden = 0;
        self.downloaden = 0;
        self.actief.clear();
        self.fout = match rapporten {
            Err(f) => Some(f.to_string()),
            Ok(r) => r.iter().find_map(|m| m.fout.as_ref().map(|f| format!("{}: {f}", m.ruimte_naam))),
        };
        for m in rapporten.unwrap_or(&[]) {
            let Some(r) = &m.rapport else { continue };
            let regel = |pad: &str, reden: &str, opnieuw_om: Option<u64>| Misluktregel {
                lokaal: m.lokaal.clone(),
                pad: pad.to_string(),
                reden: reden.to_string(),
                opnieuw_om,
            };
            // Een fout in het rapport is "pad: reden" (een pad heeft geen dubbele punt).
            let mut lijst: Vec<Misluktregel> =
                r.fouten.iter().map(|f| f.split_once(": ").map_or_else(|| regel("", f, None), |(p, reden)| regel(p, reden, None))).collect();
            lijst.extend(r.wachtend.iter().map(|w| regel(&w.pad, &w.reden, Some(w.opnieuw_om * 1000))));
            self.mislukt.insert(m.lokaal.clone(), lijst);
        }
    }

    fn alle_mislukt(&self) -> Vec<Misluktregel> {
        let mut uit: Vec<Misluktregel> = self.mislukt.values().flatten().cloned().collect();
        uit.sort_by(|a, b| (&a.lokaal, &a.pad).cmp(&(&b.lokaal, &b.pad)));
        uit
    }

    /// Alles wat deze ronde over de lijn ging, inclusief de lopende bestanden.
    fn verstuurd(&self) -> u64 {
        self.ronde.verstuurd_klaar + self.actief.values().map(|o| o.gedaan).sum::<u64>()
    }

    /// Verwerkte bestandsgrootte: afgeronde bestanden plus het deel van de lopende dat al over is.
    fn bytes_gedaan(&self) -> u64 {
        let lopend: u64 = self
            .actief
            .values()
            .map(|o| if o.totaal == 0 { 0 } else { (o.grootte as f64 * (o.gedaan as f64 / o.totaal as f64)) as u64 })
            .sum();
        (self.ronde.bytes_klaar + lopend).min(self.ronde.bytes)
    }

    fn meet(&mut self, nu: f64) {
        let v = self.verstuurd();
        let m = &mut self.ronde.metingen;
        m.push_back((nu, v));
        while m.len() > 1 && m.front().is_some_and(|(t, _)| nu - t > SNELHEIDSVENSTER) {
            m.pop_front();
        }
        let bytes_gedaan = self.bytes_gedaan();
        let r = &mut self.ronde;
        r.schatter.meet(nu, bytes_gedaan, r.bestanden_klaar, r.bytes, r.bestanden);
    }

    fn ronde_stand(&self, nu: f64) -> Option<RondeStand> {
        let nog = self.uploaden + self.downloaden;
        if !self.bezig || (self.ronde.bytes == 0 && nog == 0) {
            return None;
        }
        let snelheid = voortschrijdend(&self.ronde.metingen, nu, self.verstuurd());
        let bytes_gedaan = self.bytes_gedaan();
        Some(RondeStand {
            nog,
            bytes_gedaan,
            bytes_totaal: self.ronde.bytes,
            snelheid,
            resterend: self.ronde.schatter.resterend(nu),
            bespaard: self.ronde.bespaard,
        })
    }
}

#[derive(Default)]
pub struct Activiteit {
    binnen: Mutex<Binnen>,
    gepauzeerd: AtomicBool,
    /// Opnieuw inloggen nodig (zie `crate::sessie_verlopen`); de sync wacht tot dan.
    sessie_verlopen: AtomicBool,
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

    pub fn sessie_is_verlopen(&self) -> bool {
        self.sessie_verlopen.load(Ordering::SeqCst)
    }

    /// Zet of wis "sessie verlopen"; geeft `true` als dat een verandering is (zo komt er maar één melding).
    pub fn zet_sessie_verlopen(&self, verlopen: bool) -> bool {
        self.sessie_verlopen.swap(verlopen, Ordering::SeqCst) != verlopen
    }

    pub fn stand(&self) -> Stand {
        self.stand_op(klok())
    }

    fn stand_op(&self, nu: f64) -> Stand {
        let b = self.binnen.lock().unwrap();
        let gepauzeerd = self.gepauzeerd();
        let sessie_verlopen = self.sessie_is_verlopen();
        let mislukt = b.alle_mislukt();
        let (toestand, samenvatting) = if gepauzeerd {
            (Toestand::Gepauzeerd, "Gepauzeerd".to_string())
        } else if sessie_verlopen {
            (Toestand::Fout, openaec_sync::SESSIE_VERLOPEN.to_string())
        } else if b.bezig && b.werk() {
            let delen: Vec<String> = [(b.uploaden, "uploaden"), (b.downloaden, "downloaden")]
                .into_iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, w)| format!("{n} {w}"))
                .collect();
            let tekst = if delen.is_empty() { "Bezig met synchroniseren".to_string() } else { format!("Bezig: {}", delen.join(", ")) };
            (Toestand::Bezig, tekst)
        } else if let Some(f) = &b.fout {
            (Toestand::Fout, format!("Fout: {f}"))
        } else if !mislukt.is_empty() {
            let n = mislukt.len();
            (Toestand::Fout, format!("{n} bestand{} lukk{} niet, zie details", if n == 1 { "" } else { "en" }, if n == 1 { "t" } else { "en" }))
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
            ronde: b.ronde_stand(nu),
            sessie_verlopen,
            mislukt,
        }
    }

    fn onthoud(b: &mut Binnen, g: Gebeurd) {
        b.recent.push_front(g);
        b.recent.truncate(RECENT);
    }

    fn verwerk(&self, lokaal: &str, g: Gebeurtenis) {
        self.verwerk_op(lokaal, g, klok());
    }

    fn verwerk_op(&self, lokaal: &str, g: Gebeurtenis, nu: f64) {
        let mut b = self.binnen.lock().unwrap();
        match g {
            Gebeurtenis::Ronde { uploaden, downloaden, bytes } => {
                b.uploaden += uploaden;
                b.downloaden += downloaden;
                b.ronde.bytes += bytes;
                b.ronde.bestanden += u64::from(uploaden + downloaden);
                b.meet(nu);
            }
            Gebeurtenis::Start { pad, richting, totaal } => {
                let o = Overdracht { lokaal: lokaal.to_string(), pad: pad.clone(), richting, grootte: totaal, gedaan: 0, totaal, snelheid: 0.0, begin: Instant::now() };
                b.actief.insert((lokaal.to_string(), pad), o);
            }
            Gebeurtenis::Voortgang { pad, gedaan, totaal, .. } => {
                if let Some(o) = b.actief.get_mut(&(lokaal.to_string(), pad)) {
                    o.gedaan = gedaan;
                    o.totaal = totaal;
                    let sec = o.begin.elapsed().as_secs_f64();
                    o.snelheid = if sec > 0.0 { gedaan as f64 / sec } else { 0.0 };
                }
                b.meet(nu);
            }
            Gebeurtenis::Klaar { pad, actie } => {
                let o = b.actief.remove(&(lokaal.to_string(), pad.clone()));
                if let Some(o) = &o {
                    b.ronde.bytes_klaar += o.grootte;
                    b.ronde.verstuurd_klaar += o.totaal;
                    b.ronde.bespaard += o.grootte.saturating_sub(o.totaal);
                }
                match actie {
                    Actie::Geupload | Actie::Conflict => b.uploaden = b.uploaden.saturating_sub(1),
                    Actie::Gedownload => b.downloaden = b.downloaden.saturating_sub(1),
                    Actie::Verwijderd => {}
                }
                if !matches!(actie, Actie::Verwijderd) {
                    b.ronde.bestanden_klaar += 1;
                }
                b.meet(nu);
                let (grootte, verstuurd) = (o.as_ref().map(|o| o.grootte), o.as_ref().map(|o| o.totaal));
                let g = Gebeurd { lokaal: lokaal.to_string(), pad, actie: actie_tekst(actie), tijd: nu_ms(), fout: None, grootte, verstuurd };
                Self::onthoud(&mut b, g);
            }
            Gebeurtenis::Fout { pad, fout } => {
                if let Some(o) = b.actief.remove(&(lokaal.to_string(), pad.clone())) {
                    b.ronde.bytes_klaar += o.grootte;
                    b.ronde.verstuurd_klaar += o.gedaan;
                    b.ronde.bestanden_klaar += 1;
                    match o.richting {
                        Richting::Upload => b.uploaden = b.uploaden.saturating_sub(1),
                        Richting::Download => b.downloaden = b.downloaden.saturating_sub(1),
                    }
                }
                Self::onthoud(&mut b, Gebeurd { lokaal: lokaal.to_string(), pad, actie: "fout", tijd: nu_ms(), fout: Some(fout), grootte: None, verstuurd: None });
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
        b.ronde = Ronde::default();
    }
    meld(app);
}

/// Einde van een sync-ronde: een fout van een map blijft zichtbaar tot de volgende ronde; bestanden die niet lukken,
/// komen samen in één regel met details (zie [`Binnen::na_ronde`]).
pub fn einde(app: &AppHandle, rapporten: Result<&[MapRapport], &str>) {
    app.state::<AppState>().activiteit.binnen.lock().unwrap().na_ronde(rapporten);
    meld(app);
}

/// Een koppeling is weg: haar bestanden die niet lukten, tellen niet meer mee.
pub fn vergeet_koppeling(app: &AppHandle, lokaal: &str) {
    app.state::<AppState>().activiteit.binnen.lock().unwrap().mislukt.remove(lokaal);
}

/// Pauzeren of hervatten; een lopende ronde stopt na het huidige bestand.
pub fn zet_pauze(app: &AppHandle, aan: bool) {
    app.state::<AppState>().activiteit.gepauzeerd.store(aan, Ordering::SeqCst);
    meld(app);
}

/// Hoogstens zo vaak de stand naar de vensters tijdens een ronde: met veel kleine bestanden tegelijk komen er
/// honderden meldingen per seconde binnen. Het einde van de ronde komt altijd door (zie `einde`).
const MELDINTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// Waarnemer voor één gekoppelde map tijdens een ronde. Meldingen van parallelle overdrachten komen hier door elkaar
/// binnen; de tellers in [`Activiteit`] staan achter een slot en worden per bestand opgeteld.
pub struct Koppelwaarnemer {
    app: AppHandle,
    lokaal: String,
    vorige: Mutex<Option<Instant>>,
}

impl Koppelwaarnemer {
    pub fn new(app: AppHandle, lokaal: String) -> Self {
        Self { app, lokaal, vorige: Mutex::new(None) }
    }
}

impl Waarnemer for Koppelwaarnemer {
    fn melding(&self, g: Gebeurtenis) {
        let altijd = matches!(g, Gebeurtenis::Ronde { .. });
        // De status in de Verkenner meteen mee (sync-pijlen bij het begin, vinkje als het klaar is).
        crate::vinkjes::tijdens(&self.lokaal, &g);
        self.app.state::<AppState>().activiteit.verwerk(&self.lokaal, g);
        let nu = Instant::now();
        {
            let mut vorige = self.vorige.lock().unwrap();
            if !altijd && vorige.is_some_and(|v| nu.duration_since(v) < MELDINTERVAL) {
                return;
            }
            *vorige = Some(nu);
        }
        meld(&self.app);
    }

    fn gepauzeerd(&self) -> bool {
        self.app.state::<AppState>().activiteit.gepauzeerd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    fn ronde(a: &Activiteit, nu: f64) -> RondeStand {
        a.stand_op(nu).ronde.expect("een lopende ronde")
    }

    #[test]
    fn snelheid_voortschrijdend() {
        let mut m = VecDeque::new();
        assert_eq!(voortschrijdend(&m, 10.0, 0), 0.0);
        m.push_back((0.0, 0));
        m.push_back((4.0, 40 * MB));
        // 50 MB in 5 seconden.
        assert_eq!(voortschrijdend(&m, 5.0, 50 * MB), 10.0 * MB as f64);
        // Metingen buiten het venster tellen niet mee: van t=4 (40 MB) tot t=12 (60 MB) is 2,5 MB/s.
        assert_eq!(voortschrijdend(&m, 12.0, 60 * MB), 2.5 * MB as f64);
    }

    #[test]
    fn ronde_telt_bestanden_bytes_en_besparing_op() {
        let a = Activiteit::default();
        a.binnen.lock().unwrap().bezig = true;
        let m = "C:\\Werk";
        // Twee mappen in één ronde: de tellers worden opgeteld.
        a.verwerk_op(m, Gebeurtenis::Ronde { uploaden: 1, downloaden: 1, bytes: 1200 * MB }, 0.0);
        a.verwerk_op("D:\\Ander", Gebeurtenis::Ronde { uploaden: 1, downloaden: 0, bytes: 100 * MB }, 0.0);
        let r = ronde(&a, 0.0);
        assert_eq!((r.nog, r.bytes_gedaan, r.bytes_totaal, r.bespaard), (3, 0, 1300 * MB, 0));

        // Upload van 1000 MB waarvan maar 16 MB over de lijn hoeft; halverwege.
        let start = Gebeurtenis::Start { pad: "groot.ifc".into(), richting: Richting::Upload, totaal: 1000 * MB };
        a.verwerk_op(m, start, 1.0);
        let stap = |gedaan| Gebeurtenis::Voortgang { pad: "groot.ifc".into(), richting: Richting::Upload, gedaan, totaal: 16 * MB };
        a.verwerk_op(m, stap(0), 1.0);
        a.verwerk_op(m, stap(8 * MB), 2.0);
        let r = ronde(&a, 2.0);
        assert_eq!(r.bytes_gedaan, 500 * MB);
        // 8 MB over de lijn in de 2 seconden sinds de eerste meting.
        assert_eq!(r.snelheid, 4.0 * MB as f64);
        // Na 2 seconden en zonder afgeronde bestanden nog geen schatting: "bezig met voorbereiden…".
        assert_eq!(r.resterend, None);
        a.verwerk_op(m, stap(16 * MB), 3.0);
        a.verwerk_op(m, Gebeurtenis::Klaar { pad: "groot.ifc".into(), actie: Actie::Geupload }, 3.0);
        let r = ronde(&a, 3.0);
        assert_eq!((r.nog, r.bytes_gedaan, r.bespaard), (2, 1000 * MB, 984 * MB));
        let g = a.stand_op(3.0).recent[0].clone();
        assert_eq!((g.grootte, g.verstuurd), (Some(1000 * MB), Some(16 * MB)));

        // Een download van 200 MB die mislukt: telt als verwerkt, zonder besparing.
        a.verwerk_op(m, Gebeurtenis::Start { pad: "b.pdf".into(), richting: Richting::Download, totaal: 200 * MB }, 4.0);
        let stap = Gebeurtenis::Voortgang { pad: "b.pdf".into(), richting: Richting::Download, gedaan: 50 * MB, totaal: 200 * MB };
        a.verwerk_op(m, stap, 5.0);
        a.verwerk_op(m, Gebeurtenis::Fout { pad: "b.pdf".into(), fout: "weg".into() }, 5.0);
        let r = ronde(&a, 5.0);
        assert_eq!((r.nog, r.bytes_gedaan, r.bespaard), (1, 1200 * MB, 984 * MB));
    }

    #[test]
    fn verlopen_sessie_geeft_een_melding_tot_er_opnieuw_is_ingelogd() {
        let a = Activiteit::default();
        assert!(!a.stand_op(0.0).sessie_verlopen);
        // De eerste keer is een verandering (één melding); daarna niet meer, hoeveel verzoeken er ook nog mislukken.
        assert!(a.zet_sessie_verlopen(true));
        assert!(!a.zet_sessie_verlopen(true));
        let s = a.stand_op(0.0);
        assert!(s.sessie_verlopen);
        assert_eq!(s.toestand, Toestand::Fout);
        assert_eq!(s.samenvatting, "Je sessie is verlopen. Log opnieuw in.");
        // Opnieuw ingelogd.
        assert!(a.zet_sessie_verlopen(false));
        assert!(!a.zet_sessie_verlopen(false));
        assert_eq!(a.stand_op(0.0).toestand, Toestand::Bijgewerkt);
    }

    fn map(lokaal: &str, rapport: openaec_sync::motor::Rapport) -> MapRapport {
        MapRapport { lokaal: lokaal.into(), ruimte_naam: "Project".into(), rapport: Some(rapport), fout: None }
    }

    #[test]
    fn bestanden_die_niet_lukken_in_een_regel_met_details() {
        use openaec_sync::motor::{Mislukt, Rapport};
        let a = Activiteit::default();
        let mut r = Rapport::default();
        r.fouten = vec!["Keuren/a.pdf: De server weigerde dit verzoek.".into(), "b.txt: geweigerd".into()];
        r.wachtend = vec![Mislukt { pad: "c.txt".into(), reden: "weg".into(), keer: 2, opnieuw_om: 100 }];
        a.binnen.lock().unwrap().na_ronde(Ok(&[map("C:\\Werk", r)]));
        let s = a.stand_op(0.0);
        assert_eq!(s.toestand, Toestand::Fout);
        assert_eq!(s.samenvatting, "3 bestanden lukken niet, zie details");
        assert_eq!(s.mislukt.len(), 3);
        assert_eq!((s.mislukt[0].pad.as_str(), s.mislukt[0].reden.as_str()), ("Keuren/a.pdf", "De server weigerde dit verzoek."));
        assert_eq!(s.mislukt[2].opnieuw_om, Some(100_000));
        // Een ronde van een andere map laat deze lijst staan; een nieuwe ronde van deze map zonder fouten wist hem.
        a.binnen.lock().unwrap().na_ronde(Ok(&[map("D:\\Ander", Rapport::default())]));
        assert_eq!(a.stand_op(0.0).mislukt.len(), 3);
        a.binnen.lock().unwrap().na_ronde(Ok(&[map("C:\\Werk", Rapport::default())]));
        let s = a.stand_op(0.0);
        assert_eq!((s.toestand, s.samenvatting.as_str()), (Toestand::Bijgewerkt, "Alles bijgewerkt"));
    }

    #[test]
    fn controle_zonder_werk_telt_niet_als_bezig() {
        let a = Activiteit::default();
        a.binnen.lock().unwrap().bezig = true;
        a.verwerk_op("x", Gebeurtenis::Ronde { uploaden: 0, downloaden: 0, bytes: 0 }, 0.0);
        let s = a.stand_op(0.0);
        assert_eq!((s.toestand, s.samenvatting.as_str()), (Toestand::Bijgewerkt, "Alles bijgewerkt"));
        a.verwerk_op("x", Gebeurtenis::Ronde { uploaden: 2, downloaden: 0, bytes: 10 }, 0.0);
        let s = a.stand_op(0.0);
        assert_eq!((s.toestand, s.samenvatting.as_str()), (Toestand::Bezig, "Bezig: 2 uploaden"));
    }

    #[test]
    fn geen_ronde_buiten_een_sync_of_zonder_werk() {
        let a = Activiteit::default();
        assert!(a.stand_op(0.0).ronde.is_none());
        a.binnen.lock().unwrap().bezig = true;
        a.verwerk_op("x", Gebeurtenis::Ronde { uploaden: 0, downloaden: 0, bytes: 0 }, 0.0);
        assert!(a.stand_op(1.0).ronde.is_none());
    }
}
