//! Status van een bestand of map in de Verkenner, los van Windows: uit wat de sync weet (in sync, bezig, fout,
//! overgeslagen) en wat het bestandssysteem zegt (lokaal of alleen online, vastgezet) volgt één status, en daaruit
//! wat de app via de Cloud Files API zet: de in-sync-status (groen vinkje of sync-pijlen) en een eigen kolom bij een
//! fout of een overgeslagen bestand. Het wolkje en de vinkjes voor lokaal en vastgezet tekent Windows zelf uit de
//! placeholderstatus.
//!
//! | Status | Wat de Verkenner toont |
//! |---|---|
//! | [`Itemstatus::AlleenOnline`] | wolkje: staat op de server, wordt opgehaald bij openen |
//! | [`Itemstatus::LokaalBeschikbaar`] | groen vinkje (open): op deze computer, mag weer vrijgemaakt worden |
//! | [`Itemstatus::AltijdHier`] | groen vinkje (dicht): altijd op dit apparaat |
//! | [`Itemstatus::Gesynchroniseerd`] | groen vinkje: een map (of een losse gekoppelde map) waarin alles in sync is |
//! | [`Itemstatus::Bezig`] | sync-pijlen: nieuw, gewijzigd of nu onderweg |
//! | [`Itemstatus::Fout`] | sync-pijlen en in de kolom "Status van OpenAEC" de fout |
//! | [`Itemstatus::Overgeslagen`] | sync-pijlen en in de kolom de reden (bijv. een naam die niet kan) |

/// Wat de app en het bestandssysteem van één bestand weten.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Bestandsfeiten {
    /// Gelijk aan de server volgens de laatste ronde (en sindsdien ongewijzigd).
    pub in_sync: bool,
    /// Gaat nu over de lijn, of is gewijzigd en wacht op de volgende ronde.
    pub bezig: bool,
    /// De laatste poging gaf een fout.
    pub fout: bool,
    /// Wordt niet gesynchroniseerd (lastige naam, alleen lezen).
    pub overgeslagen: bool,
    /// De inhoud staat op deze computer (geen placeholder die alleen online staat).
    pub lokaal: bool,
    /// Vastgezet: altijd op dit apparaat.
    pub vastgezet: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Itemstatus {
    Gesynchroniseerd,
    AlleenOnline,
    LokaalBeschikbaar,
    AltijdHier,
    Overgeslagen,
    Bezig,
    Fout,
}

/// Eigen kolom naast de standaardstatus van Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EigenStaat {
    Fout,
    Overgeslagen,
}

impl Itemstatus {
    /// Wat de app zet: in sync (groen vinkje, wolkje) of niet (sync-pijlen), en een eventuele eigen kolom.
    pub fn weergave(self) -> (bool, Option<EigenStaat>) {
        match self {
            Itemstatus::Fout => (false, Some(EigenStaat::Fout)),
            Itemstatus::Overgeslagen => (false, Some(EigenStaat::Overgeslagen)),
            Itemstatus::Bezig => (false, None),
            _ => (true, None),
        }
    }
}

/// Status van een bestand. Een fout gaat voor alles; dan overgeslagen, dan bezig (ook wat niet in sync is). In sync:
/// vastgezet, alleen online of lokaal beschikbaar.
pub fn bestandsstatus(f: Bestandsfeiten) -> Itemstatus {
    if f.fout {
        Itemstatus::Fout
    } else if f.overgeslagen {
        Itemstatus::Overgeslagen
    } else if f.bezig || !f.in_sync {
        Itemstatus::Bezig
    } else if f.vastgezet && f.lokaal {
        Itemstatus::AltijdHier
    } else if !f.lokaal {
        Itemstatus::AlleenOnline
    } else {
        Itemstatus::LokaalBeschikbaar
    }
}

/// Status van een map uit die van alles erin (bestanden en submappen): fout als er iets fout gaat, anders bezig als
/// er iets bezig is, anders gesynchroniseerd. Een overgeslagen bestand maakt de map niet "niet in sync": het komt
/// elke ronde terug zonder dat er iets te doen is.
pub fn mapstatus(inhoud: impl IntoIterator<Item = Itemstatus>) -> Itemstatus {
    let mut uit = Itemstatus::Gesynchroniseerd;
    for s in inhoud {
        match s {
            Itemstatus::Fout => return Itemstatus::Fout,
            Itemstatus::Bezig => uit = Itemstatus::Bezig,
            _ => {}
        }
    }
    uit
}

/// Mappen boven `pad` (met '/'), van de hoofdmap ("") tot en met de map waar het in staat.
pub fn boven(pad: &str) -> Vec<String> {
    let delen: Vec<&str> = pad.split('/').filter(|d| !d.is_empty()).collect();
    (0..delen.len()).map(|n| delen[..n].join("/")).collect()
}

/// Status per map voor alle mappen boven de gegeven bestanden (pad met '/', status), van diep naar ondiep
/// opgeteld. Mappen zonder bestanden met een status komen er niet in.
pub fn mapstatussen<'a>(bestanden: impl IntoIterator<Item = (&'a str, Itemstatus)>) -> std::collections::BTreeMap<String, Itemstatus> {
    let mut uit: std::collections::BTreeMap<String, Itemstatus> = std::collections::BTreeMap::new();
    for (pad, status) in bestanden {
        for map in boven(pad) {
            let s = uit.entry(map).or_insert(Itemstatus::Gesynchroniseerd);
            *s = mapstatus([*s, status]);
        }
    }
    uit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feiten(in_sync: bool, lokaal: bool, vastgezet: bool) -> Bestandsfeiten {
        Bestandsfeiten { in_sync, lokaal, vastgezet, ..Default::default() }
    }

    #[test]
    fn bestand_in_sync_naar_icoon() {
        assert_eq!(bestandsstatus(feiten(true, true, false)), Itemstatus::LokaalBeschikbaar);
        assert_eq!(bestandsstatus(feiten(true, false, false)), Itemstatus::AlleenOnline);
        assert_eq!(bestandsstatus(feiten(true, true, true)), Itemstatus::AltijdHier);
        // Vastgezet maar nog niet opgehaald (net aangezet): wordt opgehaald, tot dan alleen online.
        assert_eq!(bestandsstatus(feiten(true, false, true)), Itemstatus::AlleenOnline);
    }

    #[test]
    fn fout_overgeslagen_en_bezig_gaan_voor() {
        let mut f = feiten(true, true, true);
        f.bezig = true;
        assert_eq!(bestandsstatus(f), Itemstatus::Bezig);
        f.overgeslagen = true;
        assert_eq!(bestandsstatus(f), Itemstatus::Overgeslagen);
        f.fout = true;
        assert_eq!(bestandsstatus(f), Itemstatus::Fout);
        // Niet in sync zonder dat er iets loopt (gewijzigd, wacht op de ronde): ook bezig.
        assert_eq!(bestandsstatus(feiten(false, true, false)), Itemstatus::Bezig);
    }

    #[test]
    fn weergave_per_status() {
        assert_eq!(Itemstatus::AlleenOnline.weergave(), (true, None));
        assert_eq!(Itemstatus::AltijdHier.weergave(), (true, None));
        assert_eq!(Itemstatus::Gesynchroniseerd.weergave(), (true, None));
        assert_eq!(Itemstatus::Bezig.weergave(), (false, None));
        assert_eq!(Itemstatus::Fout.weergave(), (false, Some(EigenStaat::Fout)));
        assert_eq!(Itemstatus::Overgeslagen.weergave(), (false, Some(EigenStaat::Overgeslagen)));
    }

    #[test]
    fn map_aggregatie() {
        use Itemstatus::*;
        assert_eq!(mapstatus([]), Gesynchroniseerd);
        assert_eq!(mapstatus([AlleenOnline, LokaalBeschikbaar, AltijdHier]), Gesynchroniseerd);
        assert_eq!(mapstatus([AlleenOnline, Overgeslagen]), Gesynchroniseerd);
        assert_eq!(mapstatus([AlleenOnline, Bezig, LokaalBeschikbaar]), Bezig);
        assert_eq!(mapstatus([Bezig, Fout, AlleenOnline]), Fout);
        assert_eq!(mapstatus([Fout, Bezig]), Fout);
        // Een submap telt mee zoals een bestand.
        assert_eq!(mapstatus([Gesynchroniseerd, mapstatus([Bezig])]), Bezig);
    }

    #[test]
    fn mapstatussen_over_de_boom() {
        use Itemstatus::*;
        let m = mapstatussen([("A/B/plan.pdf", Bezig), ("A/C/x.txt", Fout), ("A/C/y.txt", AlleenOnline), ("D/z.txt", LokaalBeschikbaar), ("los.txt", AlleenOnline)]);
        assert_eq!(m[""], Fout);
        assert_eq!(m["A"], Fout);
        assert_eq!(m["A/B"], Bezig);
        assert_eq!(m["A/C"], Fout);
        assert_eq!(m["D"], Gesynchroniseerd);
        assert_eq!(m.len(), 5);
    }

    #[test]
    fn mappen_boven_een_pad() {
        assert_eq!(boven("Tekeningen/2026/plan.pdf"), vec!["", "Tekeningen", "Tekeningen/2026"]);
        assert_eq!(boven("a.txt"), vec![""]);
    }
}
