//! Statusvinkjes in de Verkenner (Windows): elke gekoppelde map is een sync-root over volledige bestanden. Wat de motor
//! gesynchroniseerd heeft, krijgt een groen vinkje; nieuwe en gewijzigde bestanden, bestanden met een fout en de mappen
//! erboven sync-pijlen. Lukt registreren niet (een map die al onder een andere sync-provider valt, geen NTFS), dan
//! alleen een waarschuwing in het logboek: synchroniseren werkt gewoon door. Op andere systemen doet dit niets.
//!
//! Alle functies behalve [`gebeurtenis`] doen bestands-IO en horen op een blocking thread.

use std::path::Path;

use crate::instellingen::Koppeling;

/// Moet [`koppel`] nog voor deze map (in dit proces nog niet geprobeerd, of sindsdien ontkoppeld)?
#[cfg(windows)]
pub fn nog_te_koppelen(lokaal: &str) -> bool {
    echt::nog_te_koppelen(lokaal)
}

/// Bij koppelen en bij het starten: registreren en alle paden die in sync zijn (en hun mappen) markeren.
#[cfg(windows)]
pub fn koppel(k: &Koppeling) {
    echt::koppel(k)
}

/// Na een sync-ronde: `bijgewerkt` in sync, `foutpaden` niet in sync, en de mappen opnieuw.
#[cfg(windows)]
pub fn na_ronde(lokaal: &str, bijgewerkt: &[String], foutpaden: &[String]) {
    echt::na_ronde(lokaal, bijgewerkt, foutpaden)
}

/// Bij ontkoppelen: placeholders terugzetten en de registratie afmelden.
#[cfg(windows)]
pub fn ontkoppel(lokaal: &str) {
    echt::ontkoppel(lokaal)
}

/// Gebeurtenis van de bestandswachter op `pad` onder `root`. False als het om onze eigen markering gaat (telt niet
/// mee voor een sync-ronde). Een echte wijziging zet het bestand en de mappen erboven meteen op "niet in sync":
/// Windows doet dat zelf niet bij schrijven midden in een bestand.
#[cfg(windows)]
pub fn gebeurtenis(root: &Path, pad: &Path) -> bool {
    echt::gebeurtenis(root, pad)
}

#[cfg(not(windows))]
pub fn nog_te_koppelen(_lokaal: &str) -> bool {
    false
}

#[cfg(not(windows))]
pub fn koppel(_k: &Koppeling) {}

#[cfg(not(windows))]
pub fn na_ronde(_lokaal: &str, _bijgewerkt: &[String], _foutpaden: &[String]) {}

#[cfg(not(windows))]
pub fn ontkoppel(_lokaal: &str) {}

#[cfg(not(windows))]
pub fn gebeurtenis(_root: &Path, _pad: &Path) -> bool {
    true
}

/// Mappen boven `pad` (met '/'), van de hoofdmap ("") tot en met de map waar het in staat.
#[cfg(any(windows, test))]
fn boven(pad: &str) -> Vec<String> {
    let delen: Vec<&str> = pad.split('/').filter(|d| !d.is_empty()).collect();
    (0..delen.len()).map(|n| delen[..n].join("/")).collect()
}

#[cfg(any(windows, test))]
fn lokaal_pad(root: &Path, pad: &str) -> std::path::PathBuf {
    pad.split('/').filter(|d| !d.is_empty()).fold(root.to_path_buf(), |p, d| p.join(d))
}

#[cfg(any(windows, test))]
fn relatief(root: &Path, pad: &Path) -> String {
    let rel = pad.strip_prefix(root).unwrap_or(pad);
    rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/")
}

#[cfg(windows)]
mod echt {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{Duration, Instant, SystemTime};

    use openaec_sync::motor::bekende_paden;

    use super::{boven, lokaal_pad, relatief};
    use crate::cloudfilter;
    use crate::instellingen::Koppeling;

    /// Gekoppelde mappen waarvoor registreren in dit proces is geprobeerd (ook als het mislukte: geen herhaalde
    /// waarschuwingen), en de mappen waarvoor het lukte.
    static BEHANDELD: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
    static ACTIEF: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
    /// Grootte, wijzigings- en aanmaaktijd en attributen van een pad, zonder de attributen die het zetten van een
    /// status zelf verandert (reparse-punt, vastgezet/losgemaakt, ophalen bij openen, offline, sparse).
    type Kenmerk = (u64, SystemTime, SystemTime, u32);
    const STATUSBITS: u32 = 0x0000_0200 | 0x0000_0400 | 0x0000_1000 | 0x0004_0000 | 0x0008_0000 | 0x0010_0000 | 0x0040_0000;
    /// Kenmerk van paden vlak voordat we hun status zetten, met het tijdstip. Een statuswijziging verandert attributen
    /// en de bestandswachter ziet dat als wijziging; met hetzelfde kenmerk is die van onszelf.
    static EIGEN: Mutex<BTreeMap<PathBuf, (Kenmerk, Instant)>> = Mutex::new(BTreeMap::new());
    /// Bestanden waar de bestandswachter iets zag; na de ronde weer in sync als ze ongewijzigd blijken. Windows wist
    /// de status ook bij alleen een ander attribuut of tijdstip, wat de motor terecht negeert.
    static VOORLOPIG: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
    /// Laatste keer dat alle bekende bestanden opnieuw zijn nagelopen, per gekoppelde map.
    static VOLLEDIG: Mutex<BTreeMap<PathBuf, Instant>> = Mutex::new(BTreeMap::new());
    const EIGEN_GELDIG: Duration = Duration::from_secs(60);
    /// Vangnet voor wat de bestandswachter mist (zoals de volledige ronde van de bewaking).
    const VOLLEDIG_ELKE: Duration = Duration::from_secs(600);

    fn kenmerk(pad: &Path) -> Option<Kenmerk> {
        use std::os::windows::fs::MetadataExt;
        let meta = std::fs::metadata(pad).ok()?;
        Some((meta.len(), meta.modified().ok()?, meta.created().ok()?, meta.file_attributes() & !STATUSBITS))
    }

    fn is_eigen(pad: &Path) -> bool {
        let Some((k, t)) = EIGEN.lock().unwrap().get(pad).copied() else { return false };
        t.elapsed() < EIGEN_GELDIG && kenmerk(pad) == Some(k)
    }

    fn ruim_eigen_op() {
        EIGEN.lock().unwrap().retain(|_, (_, t)| t.elapsed() < EIGEN_GELDIG);
    }

    fn zet(root: &Path, pad: &Path, in_sync: bool) {
        if let Some(k) = kenmerk(pad) {
            EIGEN.lock().unwrap().insert(pad.to_path_buf(), (k, Instant::now()));
        }
        let r = if in_sync { cloudfilter::markeer_in_sync(root, pad) } else { cloudfilter::markeer_niet_in_sync(root, pad) };
        if let Err(e) = r {
            log::debug!("status van {} niet gezet: {e}", pad.display());
        }
    }

    /// Mappen boven de paden: in sync als er onder hen geen foutpad is, anders niet.
    fn mappen(root: &Path, bekend: &[String], foutpaden: &[String]) {
        let fout: BTreeSet<String> = foutpaden.iter().flat_map(|p| boven(p)).collect();
        let alle: BTreeSet<String> = bekend.iter().chain(foutpaden).flat_map(|p| boven(p)).collect();
        for map in &alle {
            zet(root, &lokaal_pad(root, map), !fout.contains(map));
        }
    }

    fn toestandsmap(root: &Path) {
        if let Err(e) = cloudfilter::sluit_toestandsmap_uit(root) {
            log::debug!("toestandsmap in {} niet uitgesloten: {e}", root.display());
        }
    }

    pub fn nog_te_koppelen(lokaal: &str) -> bool {
        !BEHANDELD.lock().unwrap().contains(Path::new(lokaal))
    }

    pub fn koppel(k: &Koppeling) {
        let root = PathBuf::from(&k.lokaal);
        BEHANDELD.lock().unwrap().insert(root.clone());
        let icoon = std::env::current_exe().map(|p| format!("{},0", p.display())).unwrap_or_default();
        let naam = format!("OpenAEC Storage – {}", k.ruimte_naam);
        if let Err(e) = cloudfilter::registreer_volledig(&k.lokaal, &naam, env!("CARGO_PKG_VERSION"), &icoon) {
            log::warn!("statusvinkjes voor {} niet beschikbaar: {e}", k.lokaal);
            return;
        }
        ACTIEF.lock().unwrap().insert(root.clone());
        ruim_eigen_op();
        let bekend = bekende_paden(&root);
        for pad in &bekend {
            zet(&root, &lokaal_pad(&root, pad), true);
        }
        VOLLEDIG.lock().unwrap().insert(root.clone(), Instant::now());
        mappen(&root, &bekend, &[]);
        toestandsmap(&root);
        log::info!("statusvinkjes voor {}: {} bestanden in sync", k.lokaal, bekend.len());
    }

    pub fn na_ronde(lokaal: &str, bijgewerkt: &[String], foutpaden: &[String]) {
        let root = PathBuf::from(lokaal);
        if !ACTIEF.lock().unwrap().contains(&root) {
            return;
        }
        ruim_eigen_op();
        // Alleen wat nu nog gelijk is aan de toestand: wat tijdens de ronde opnieuw veranderde, wacht op de volgende.
        let bekend = bekende_paden(&root);
        let in_sync: BTreeSet<&str> = bekend.iter().map(String::as_str).collect();
        let fout: BTreeSet<&str> = foutpaden.iter().map(String::as_str).collect();
        let mut voorlopig = Vec::new();
        VOORLOPIG.lock().unwrap().retain(|p| {
            if !p.starts_with(&root) {
                return true;
            }
            let rel = relatief(&root, p);
            let klaar = in_sync.contains(rel.as_str()) || fout.contains(rel.as_str()) || !p.exists();
            if in_sync.contains(rel.as_str()) {
                voorlopig.push(rel);
            }
            !klaar
        });
        let alles = {
            let mut volledig = VOLLEDIG.lock().unwrap();
            let nu_doen = volledig.get(&root).map_or(true, |t| t.elapsed() >= VOLLEDIG_ELKE);
            if nu_doen {
                volledig.insert(root.clone(), Instant::now());
            }
            if nu_doen { bekend.as_slice() } else { &[] }
        };
        for pad in bijgewerkt.iter().chain(&voorlopig).chain(alles) {
            if in_sync.contains(pad.as_str()) && !fout.contains(pad.as_str()) {
                zet(&root, &lokaal_pad(&root, pad), true);
            }
        }
        for pad in foutpaden {
            let p = lokaal_pad(&root, pad);
            if p.exists() {
                zet(&root, &p, false);
            }
        }
        mappen(&root, &bekend, foutpaden);
        toestandsmap(&root);
    }

    pub fn ontkoppel(lokaal: &str) {
        let root = PathBuf::from(lokaal);
        BEHANDELD.lock().unwrap().remove(&root);
        ACTIEF.lock().unwrap().remove(&root);
        VOLLEDIG.lock().unwrap().remove(&root);
        VOORLOPIG.lock().unwrap().retain(|p| !p.starts_with(&root));
        // Nooit terugzetten in een map die niet (meer) als eigen sync-root geregistreerd is.
        if !cloudfilter::eigen_sync_root(lokaal) {
            return;
        }
        match cloudfilter::zet_terug(&root) {
            Ok(n) => log::info!("statusvinkjes in {lokaal} verwijderd ({n} bestanden en mappen)"),
            Err(e) => log::warn!("terugzetten in {lokaal}: {e}"),
        }
        if let Err(e) = cloudfilter::afmelden_volledig(lokaal) {
            log::warn!("{e}");
        }
    }

    pub fn gebeurtenis(root: &Path, pad: &Path) -> bool {
        if is_eigen(pad) {
            return false;
        }
        if ACTIEF.lock().unwrap().contains(root) {
            if pad.is_file() {
                naar_niet_in_sync(root, pad);
                VOORLOPIG.lock().unwrap().insert(pad.to_path_buf());
            }
            let mut map = pad.parent();
            while let Some(m) = map.filter(|m| m.starts_with(root)) {
                naar_niet_in_sync(root, m);
                map = m.parent();
            }
        }
        true
    }

    /// Zet een placeholder die in sync is op "niet in sync". Een nieuw bestand blijft een gewoon bestand (de Verkenner
    /// toont het al als "wacht op upload") tot de ronde het markeert.
    fn naar_niet_in_sync(root: &Path, pad: &Path) {
        if cloudfilter::placeholder_status(pad).is_ok_and(|s| s.is_some_and(|s| s.in_sync && !s.uitgesloten)) {
            zet(root, pad, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mappen_boven_een_pad() {
        assert_eq!(boven("Tekeningen/2026/plan.pdf"), vec!["", "Tekeningen", "Tekeningen/2026"]);
        assert_eq!(boven("a.txt"), vec![""]);
    }

    #[test]
    fn paden_heen_en_terug() {
        let root = Path::new("werk").join("Project A");
        let pad = lokaal_pad(&root, "Tekeningen/plan.pdf");
        assert_eq!(pad, root.join("Tekeningen").join("plan.pdf"));
        assert_eq!(relatief(&root, &pad), "Tekeningen/plan.pdf");
        assert_eq!(lokaal_pad(&root, ""), root);
    }
}
