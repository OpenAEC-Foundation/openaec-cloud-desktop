//! Schatting van de resterende tijd van een sync-ronde: rustig en geloofwaardig in plaats van per melding opnieuw.
//!
//! Rekenregels (alles puur, met de tijd als parameter, zodat het te testen is):
//! - **Werk** telt bytes én bestanden: elk bestand kost naast zijn bytes een vaste overhead ([`OVERHEAD_PER_BESTAND`],
//!   in bytes uitgedrukt). Bij duizenden kleine bestanden (bijv. `node_modules`) bepaalt die overhead de duur, niet
//!   het aantal bytes.
//! - **Tempo**: het werk dat in de laatste [`VENSTER`] seconden af kwam, gedeeld door die tijd (voortschrijdend
//!   gemiddelde).
//! - **Pas schatten na genoeg meetpunten**: minstens [`MIN_SECONDEN`] seconden bezig, en minstens [`MIN_BESTANDEN`]
//!   bestanden af (of [`MIN_WERK`] aan werk, voor één groot bestand). Daarvoor: geen schatting ("bezig met
//!   voorbereiden…").
//! - **Dempen**: de getoonde tijd loopt gewoon af met de klok en schuift maar langzaam (tijdconstante
//!   [`DEMPING_SCHATTING`] seconden) naar de nieuwe ruwe schatting. Zo springt hij niet heen en weer.
//!
//! De grove afronding voor mensen ("nog ongeveer 3 minuten") staat in `src/lib/opmaak.ts` (`resterendTekst`).

use std::collections::VecDeque;

/// Overhead per bestand, uitgedrukt in bytes: een verzoek, de controle en de administratie kosten ongeveer zoveel tijd
/// als het overzetten van een halve MiB.
pub const OVERHEAD_PER_BESTAND: u64 = 512 * 1024;
/// Venster voor het voortschrijdende tempo (seconden).
pub const VENSTER: f64 = 20.0;
/// Tijdconstante voor het dempen van de getoonde tijd (seconden).
pub const DEMPING_SCHATTING: f64 = 12.0;
/// Pas na zoveel seconden een schatting.
pub const MIN_SECONDEN: f64 = 5.0;
/// ... en na zoveel afgeronde bestanden,
pub const MIN_BESTANDEN: u64 = 3;
/// ... of na zoveel werk (bytes), voor een ronde met één of enkele grote bestanden.
pub const MIN_WERK: u64 = 16 * 1024 * 1024;

/// Hoeveel werk (in bytes) een stand voorstelt: de bytes plus de overhead per bestand.
pub fn werk(bytes: u64, bestanden: u64) -> u64 {
    bytes.saturating_add(bestanden.saturating_mul(OVERHEAD_PER_BESTAND))
}

/// Houdt de metingen van één ronde bij en geeft een gedempte schatting van de resterende tijd.
#[derive(Debug, Default)]
pub struct Schatter {
    /// (tijd, werk af) binnen het venster.
    metingen: VecDeque<(f64, u64)>,
    /// Tijd van de eerste meting.
    begin: Option<f64>,
    /// Getoonde schatting (seconden) en de tijd waarop die gold.
    schatting: Option<(f64, f64)>,
}

impl Schatter {
    /// Nieuwe meting op tijd `nu`: zoveel bytes en bestanden af, van zoveel in totaal.
    pub fn meet(&mut self, nu: f64, bytes_af: u64, bestanden_af: u64, bytes_totaal: u64, bestanden_totaal: u64) {
        let af = werk(bytes_af, bestanden_af);
        let totaal = werk(bytes_totaal, bestanden_totaal).max(af);
        let begin = *self.begin.get_or_insert(nu);
        self.metingen.push_back((nu, af));
        while self.metingen.len() > 2 && self.metingen.front().is_some_and(|(t, _)| nu - t > VENSTER) {
            self.metingen.pop_front();
        }

        if totaal == af {
            self.schatting = self.schatting.map(|_| (0.0, nu));
            return;
        }
        // Voortschrijdend tempo over het venster.
        let &(t0, w0) = self.metingen.front().unwrap();
        let dt = nu - t0;
        let tempo = if dt >= 1.0 { af.saturating_sub(w0) as f64 / dt } else { 0.0 };
        let genoeg = nu - begin >= MIN_SECONDEN && (bestanden_af >= MIN_BESTANDEN || af >= MIN_WERK);
        if !genoeg || tempo <= 0.0 {
            return;
        }
        let ruw = (totaal - af) as f64 / tempo;
        self.schatting = Some(match self.schatting {
            None => (ruw, nu),
            Some((oud, t)) => {
                // De vorige schatting loopt af met de klok, en schuift langzaam naar de nieuwe.
                let verwacht = (oud - (nu - t)).max(0.0);
                (dempen(verwacht, ruw, nu - t, DEMPING_SCHATTING), nu)
            }
        });
    }

    /// Resterende tijd in hele seconden op tijd `nu` (loopt tussen metingen door af), of None zolang er niets te
    /// schatten valt.
    pub fn resterend(&self, nu: f64) -> Option<u64> {
        let (s, t) = self.schatting?;
        Some((s - (nu - t).max(0.0)).max(0.0).round() as u64)
    }
}

/// Exponentieel dempen: na `dt` seconden is `oud` voor een deel `1 - e^(-dt/tau)` naar `nieuw` geschoven.
fn dempen(oud: f64, nieuw: f64, dt: f64, tau: f64) -> f64 {
    let a = 1.0 - (-dt.max(0.0) / tau).exp();
    oud + a * (nieuw - oud)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn werk_telt_bytes_en_bestanden() {
        assert_eq!(werk(0, 0), 0);
        assert_eq!(werk(10, 2), 10 + 2 * OVERHEAD_PER_BESTAND);
    }

    #[test]
    fn eerst_niets_tot_er_genoeg_gemeten_is() {
        let mut s = Schatter::default();
        // Snel veel bestanden, maar nog geen 5 seconden.
        for i in 0..5 {
            s.meet(i as f64, 0, i * 10, 0, 1000);
            assert_eq!(s.resterend(i as f64), None, "na {i} s nog geen schatting");
        }
        // Wel 5 seconden, maar nog geen 3 bestanden en weinig werk.
        let mut s = Schatter::default();
        for i in 0..=6 {
            s.meet(i as f64, 0, (i / 3) as u64, 0, 1000);
        }
        assert_eq!(s.resterend(6.0), None);
        // Eén groot bestand: na 5 s en genoeg bytes wel.
        let mut s = Schatter::default();
        for i in 0..=6 {
            s.meet(i as f64, i as u64 * 10 * MB, 0, 1000 * MB, 1);
        }
        assert!(s.resterend(6.0).is_some());
    }

    #[test]
    fn veel_kleine_bestanden_overhead_bepaalt_de_duur() {
        // 10 000 lege bestanden, 100 per seconde: dat duurt nog ~95 s, ook al gaan er nul bytes over.
        let mut s = Schatter::default();
        for i in 0..=5 {
            s.meet(i as f64, 0, i as u64 * 100, 0, 10_000);
        }
        let r = s.resterend(5.0).unwrap();
        assert!((90..=100).contains(&r), "resterend {r}");
    }

    #[test]
    fn constant_tempo_geeft_de_echte_tijd() {
        // 100 MB in 100 bestanden van 1 MB, één per seconde.
        let mut s = Schatter::default();
        for i in 0..=30u64 {
            s.meet(i as f64, i * MB, i, 100 * MB, 100);
        }
        let r = s.resterend(30.0).unwrap();
        assert!((65..=75).contains(&r), "resterend {r}");
        // Tussen metingen door loopt hij af met de klok.
        assert_eq!(s.resterend(32.0).unwrap(), r - 2);
    }

    #[test]
    fn springt_niet_heen_en_weer() {
        // Afwisselend een seconde met 10 bestanden en een seconde met 0 (bijv. een groter bestand ertussen).
        let mut s = Schatter::default();
        let totaal = 2_000u64;
        let mut af = 0;
        let mut getoond = Vec::new();
        for i in 0..=120u64 {
            if i % 2 == 0 {
                af += 10;
            }
            s.meet(i as f64, 0, af, 0, totaal);
            // Na een halve minuut is hij ingeschoten.
            if i >= 30 {
                getoond.push(s.resterend(i as f64).unwrap() as i64);
            }
        }
        // Elke seconde hoogstens een paar seconden anders dan "één seconde minder".
        for w in getoond.windows(2) {
            let sprong = (w[1] - (w[0] - 1)).abs();
            assert!(sprong <= 3, "sprong van {sprong} s: {:?}", w);
        }
        // En in de buurt van de echte waarde (5 bestanden per seconde): (2000 - 600) / 5 = 280 s.
        let laatste = *getoond.last().unwrap();
        assert!((250..=310).contains(&laatste), "laatste {laatste}");
    }

    #[test]
    fn pieken_worden_gedempt() {
        // Rustig tempo, dan één seconde met een enorme sprong (veel bestanden uit een batch tegelijk klaar).
        let mut s = Schatter::default();
        for i in 0..=30u64 {
            s.meet(i as f64, 0, i * 10, 0, 3_000);
        }
        let voor = s.resterend(30.0).unwrap() as f64;
        s.meet(31.0, 0, 310 + 200, 0, 3_000);
        let na = s.resterend(31.0).unwrap() as f64;
        assert!(na < voor, "minder werk over, dus korter");
        assert!(na > voor * 0.75, "maar geen sprong: voor {voor}, na {na}");
    }

    #[test]
    fn klaar_is_nul() {
        let mut s = Schatter::default();
        for i in 0..=10u64 {
            s.meet(i as f64, i * MB, i, 10 * MB, 10);
        }
        assert_eq!(s.resterend(10.0), Some(0));
    }
}
