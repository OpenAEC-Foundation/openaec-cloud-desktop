//! Status in de Verkenner (Windows). Twee soorten gekoppelde mappen:
//!
//! - **een losse gekoppelde map** is zelf een sync-root over volledige bestanden: alles blijft op deze computer
//!   (vastgezet), met een groen vinkje als het gesynchroniseerd is;
//! - **een ruimte in de hoofdmap** valt onder de sync-root van de hoofdmap (zie `hoofdmap`): bestanden op aanvraag.
//!   De pin-status is daar van de gebruiker ("Altijd op dit apparaat behouden", "Ruimte vrijmaken"); de app volgt
//!   hem ([`gebeurtenis`]).
//!
//! In beide gevallen: wat de motor gesynchroniseerd heeft, staat in sync; nieuwe en gewijzigde bestanden, bestanden die
//! nu over de lijn gaan ([`tijdens`]) en de mappen erboven tonen sync-pijlen; een fout of een overgeslagen bestand
//! krijgt daarnaast een eigen kolom "Status van OpenAEC" met de reden, en een vergrendeld bestand een kolom
//! "Vergrendeld door" ([`eigen_staten`], [`zet_slot`]). Welke status wat betekent, staat in `itemstatus`.
//!
//! Lukt registreren niet (een map die al onder een andere sync-provider valt, geen NTFS), dan alleen een waarschuwing
//! in het logboek: synchroniseren werkt gewoon door. Op andere systemen doet dit niets.
//!
//! Alle functies behalve [`gebeurtenis`] en [`tijdens`] doen bestands-IO en horen op een blocking thread.

use std::path::Path;

use crate::instellingen::Koppeling;
#[cfg(any(windows, test))]
use crate::itemstatus::boven;

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

/// Als [`koppel`], voor een ruimte in de hoofdmap: niet zelf registreren (de hoofdmap is de sync-root) en niets
/// vastzetten. De hoofdmap moet al geregistreerd zijn.
#[cfg(windows)]
pub fn koppel_op_aanvraag(k: &Koppeling) {
    echt::koppel_op_aanvraag(k)
}

/// Na een sync-ronde: `bijgewerkt` in sync, `foutpaden` niet in sync, en de mappen opnieuw.
#[cfg(windows)]
pub fn na_ronde(lokaal: &str, bijgewerkt: &[String], foutpaden: &[String]) {
    echt::na_ronde(lokaal, bijgewerkt, foutpaden)
}

/// Na een sync-ronde: de eigen kolom "Status van OpenAEC" bij fouten en overgeslagen bestanden (pad, reden), en weg
/// waar hij niet meer klopt.
#[cfg(windows)]
pub fn eigen_staten(lokaal: &str, fouten: &[(String, String)], overgeslagen: &[(String, String)]) {
    echt::eigen_staten(lokaal, fouten, overgeslagen)
}

/// Wie een bestand vergrendeld heeft (None: niet meer vergrendeld), in de kolom "Vergrendeld door".
#[cfg(windows)]
pub fn zet_slot(lokaal: &str, pad: &str, wie: Option<String>) {
    echt::zet_slot(lokaal, pad, wie)
}

/// Paden met een vergrendeling in de Verkenner (om af en toe na te kijken of hij nog geldt).
#[cfg(windows)]
pub fn sloten(lokaal: &str) -> Vec<String> {
    echt::sloten(lokaal)
}

/// Bij ontkoppelen: placeholders terugzetten en de registratie afmelden (een ruimte in de hoofdmap: alleen de
/// bestanden die alleen online stonden weghalen, die staan toch op de server).
#[cfg(windows)]
pub fn ontkoppel(lokaal: &str) {
    echt::ontkoppel(lokaal)
}

/// Gebeurtenis van de bestandswachter op `pad` onder `root`. False als het om onze eigen markering of alleen een
/// statuswijziging gaat (telt niet mee voor een sync-ronde). Een echte wijziging zet het bestand en de mappen erboven
/// meteen op "niet in sync": Windows doet dat zelf niet bij schrijven midden in een bestand. In de hoofdmap volgt de
/// app hier ook de pin-status (ophalen of ruimte vrijmaken).
#[cfg(windows)]
pub fn gebeurtenis(root: &Path, pad: &Path) -> bool {
    echt::gebeurtenis(root, pad)
}

/// Melding van de motor tijdens een ronde: een bestand dat begint krijgt sync-pijlen (en de mappen erboven), een
/// bestand dat klaar is meteen weer zijn vinkje. Gaat via een eigen thread, dus houdt de ronde niet op.
#[cfg(windows)]
pub fn tijdens(lokaal: &str, g: &openaec_sync::voortgang::Gebeurtenis) {
    echt::tijdens(lokaal, g)
}

#[cfg(not(windows))]
pub fn nog_te_koppelen(_lokaal: &str) -> bool {
    false
}

#[cfg(not(windows))]
pub fn koppel(_k: &Koppeling) {}

#[cfg(not(windows))]
pub fn koppel_op_aanvraag(_k: &Koppeling) {}

#[cfg(not(windows))]
pub fn na_ronde(_lokaal: &str, _bijgewerkt: &[String], _foutpaden: &[String]) {}

#[cfg(not(windows))]
pub fn eigen_staten(_lokaal: &str, _fouten: &[(String, String)], _overgeslagen: &[(String, String)]) {}

#[cfg(not(windows))]
pub fn zet_slot(_lokaal: &str, _pad: &str, _wie: Option<String>) {}

#[cfg(not(windows))]
pub fn sloten(_lokaal: &str) -> Vec<String> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn ontkoppel(_lokaal: &str) {}

#[cfg(not(windows))]
pub fn gebeurtenis(_root: &Path, _pad: &Path) -> bool {
    true
}

#[cfg(not(windows))]
pub fn tijdens(_lokaal: &str, _g: &openaec_sync::voortgang::Gebeurtenis) {}

#[cfg(any(windows, test))]
fn lokaal_pad(root: &Path, pad: &str) -> std::path::PathBuf {
    pad.split('/').filter(|d| !d.is_empty()).fold(root.to_path_buf(), |p, d| p.join(d))
}

#[cfg(any(windows, test))]
fn relatief(root: &Path, pad: &Path) -> String {
    let rel = pad.strip_prefix(root).unwrap_or(pad);
    rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/")
}

/// Eigen kolommen van één pad: de status bij een fout of overgeslagen bestand, en wie het vergrendeld heeft.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Kolommen {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fout: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    slot: Option<String>,
}

#[cfg(any(windows, test))]
impl Kolommen {
    fn leeg(&self) -> bool {
        self.status.is_none() && self.slot.is_none()
    }
}

/// Nieuwe statuskolommen na een ronde: alleen paden met een fout of overgeslagen; een vergrendeling blijft staan.
/// Geeft per pad dat verandert de nieuwe kolommen (leeg: weghalen).
#[cfg(any(windows, test))]
fn nieuwe_kolommen(
    oud: &std::collections::BTreeMap<String, Kolommen>,
    fouten: &[(String, String)],
    overgeslagen: &[(String, String)],
) -> std::collections::BTreeMap<String, Kolommen> {
    use std::collections::BTreeMap;
    let mut gewenst: BTreeMap<String, Kolommen> =
        oud.iter().map(|(p, k)| (p.clone(), Kolommen { status: None, fout: None, slot: k.slot.clone() })).collect();
    for (pad, reden) in overgeslagen {
        let k = gewenst.entry(pad.clone()).or_default();
        k.status = Some(format!("Overgeslagen: {reden}"));
        k.fout = Some(false);
    }
    for (pad, reden) in fouten {
        let k = gewenst.entry(pad.clone()).or_default();
        k.status = Some(format!("Fout: {reden}"));
        k.fout = Some(true);
    }
    gewenst.into_iter().filter(|(p, k)| oud.get(p).cloned().unwrap_or_default() != *k).collect()
}

/// (pad, reden) uit een regel van `Rapport::fouten` ("pad: reden"); het langste passende foutpad wint.
pub fn splits_fout(regel: &str, foutpaden: &[String]) -> Option<(String, String)> {
    foutpaden
        .iter()
        .filter(|p| regel.starts_with(p.as_str()) && regel[p.len()..].starts_with(": "))
        .max_by_key(|p| p.len())
        .map(|p| (p.clone(), regel[p.len() + 2..].to_string()))
}

#[cfg(windows)]
mod echt {
    use std::collections::{BTreeMap, BTreeSet, HashMap};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use openaec_sync::motor::{bekende_bestanden, bekende_paden, TOESTANDSMAP};
    use openaec_sync::voortgang::{Actie, Gebeurtenis};

    use super::{boven, lokaal_pad, nieuwe_kolommen, relatief, Kolommen};
    use crate::cloudfilter;
    use crate::instellingen::Koppeling;
    use crate::itemstatus::{bestandsstatus, mapstatussen, Bestandsfeiten, Itemstatus};

    /// Gekoppelde mappen waarvoor registreren in dit proces is geprobeerd (ook als het mislukte: geen herhaalde
    /// waarschuwingen), en de mappen waarvoor het lukte.
    static BEHANDELD: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
    static ACTIEF: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
    /// Ruimtes in de hoofdmap (bestanden op aanvraag).
    static OP_AANVRAAG: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
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
    /// Eigen kolommen per gekoppelde map (pad met '/' -> kolommen), zoals ze in de Verkenner staan; ook bewaard in
    /// `.openaec/verkenner.json`, want Windows onthoudt ze in het bestand zelf.
    static KOLOMMEN: Mutex<BTreeMap<PathBuf, BTreeMap<String, Kolommen>>> = Mutex::new(BTreeMap::new());
    /// Grootte en wijzigingstijd per pad volgens de toestand van de laatste ronde, met de wijzigingstijd van het
    /// toestandsbestand (opnieuw lezen als dat veranderd is).
    type Toestandskenmerken = (SystemTime, HashMap<String, (u64, u128)>);
    static TOESTAND: Mutex<BTreeMap<PathBuf, Toestandskenmerken>> = Mutex::new(BTreeMap::new());
    /// Bestanden waarvoor nu opgehaald of vrijgemaakt wordt (niet twee keer tegelijk).
    static PINWERK: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
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

    fn op_aanvraag(root: &Path) -> bool {
        OP_AANVRAAG.lock().unwrap().contains(root)
    }

    fn zet(root: &Path, pad: &Path, in_sync: bool) {
        if let Some(k) = kenmerk(pad) {
            EIGEN.lock().unwrap().insert(pad.to_path_buf(), (k, Instant::now()));
        }
        let r = if op_aanvraag(root) {
            cloudfilter::markeer_op_aanvraag(root, pad, in_sync)
        } else if in_sync {
            cloudfilter::markeer_in_sync(root, pad)
        } else {
            cloudfilter::markeer_niet_in_sync(root, pad)
        };
        if let Err(e) = r {
            log::debug!("status van {} niet gezet: {e}", pad.display());
        }
    }

    /// Mappen boven de paden, met de status uit wat erin zit (zie `itemstatus::mapstatus`): niet in sync als er onder
    /// hen een foutpad is, anders in sync.
    fn mappen(root: &Path, bekend: &[String], foutpaden: &[String]) {
        let fout = Bestandsfeiten { fout: true, ..Default::default() };
        let goed = Bestandsfeiten { in_sync: true, lokaal: true, ..Default::default() };
        let bestanden = bekend.iter().map(|p| (p.as_str(), bestandsstatus(goed))).chain(foutpaden.iter().map(|p| (p.as_str(), bestandsstatus(fout))));
        for (map, s) in &mapstatussen(bestanden) {
            zet(root, &lokaal_pad(root, map), s.weergave().0);
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

    fn markeer_alles(root: &Path) -> usize {
        ruim_eigen_op();
        let bekend = bekende_paden(root);
        for pad in &bekend {
            zet(root, &lokaal_pad(root, pad), true);
        }
        VOLLEDIG.lock().unwrap().insert(root.to_path_buf(), Instant::now());
        mappen(root, &bekend, &[]);
        toestandsmap(root);
        laad_kolommen(root);
        bekend.len()
    }

    pub fn koppel(k: &Koppeling) {
        let root = PathBuf::from(&k.lokaal);
        BEHANDELD.lock().unwrap().insert(root.clone());
        OP_AANVRAAG.lock().unwrap().remove(&root);
        let icoon = std::env::current_exe().map(|p| format!("{},0", p.display())).unwrap_or_default();
        let naam = format!("OpenAEC Storage – {}", k.ruimte_naam);
        if let Err(e) = cloudfilter::registreer_volledig(&k.lokaal, &naam, env!("CARGO_PKG_VERSION"), &icoon) {
            log::warn!("statusvinkjes voor {} niet beschikbaar: {e}", k.lokaal);
            return;
        }
        ACTIEF.lock().unwrap().insert(root.clone());
        let n = markeer_alles(&root);
        log::info!("statusvinkjes voor {}: {n} bestanden in sync", k.lokaal);
    }

    pub fn koppel_op_aanvraag(k: &Koppeling) {
        let root = PathBuf::from(&k.lokaal);
        BEHANDELD.lock().unwrap().insert(root.clone());
        let onder_hoofdmap = root.parent().is_some_and(|h| cloudfilter::eigen_sync_root(&h.to_string_lossy()));
        if !onder_hoofdmap {
            log::warn!("statusvinkjes voor {} niet beschikbaar: de hoofdmap is niet geregistreerd", k.lokaal);
            return;
        }
        OP_AANVRAAG.lock().unwrap().insert(root.clone());
        ACTIEF.lock().unwrap().insert(root.clone());
        let n = markeer_alles(&root);
        log::info!("statusvinkjes voor {} (hoofdmap): {n} bestanden in sync", k.lokaal);
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
            let nu_doen = volledig.get(&root).is_none_or(|t| t.elapsed() >= VOLLEDIG_ELKE);
            if nu_doen {
                volledig.insert(root.clone(), Instant::now());
            }
            if nu_doen { bekend.as_slice() } else { &[] }
        };
        // Per bestand de status uit wat de ronde weet (zie `itemstatus::bestandsstatus`).
        let feiten = |pad: &str| {
            let a = cloudfilter::attributen(&lokaal_pad(&root, pad)).unwrap_or(0);
            Bestandsfeiten {
                in_sync: in_sync.contains(pad),
                fout: fout.contains(pad),
                lokaal: !cloudfilter::is_ontwaterd(a),
                vastgezet: a & cloudfilter::ATTR_VASTGEZET != 0,
                ..Default::default()
            }
        };
        for pad in bijgewerkt.iter().chain(&voorlopig).chain(alles) {
            if bestandsstatus(feiten(pad)).weergave().0 {
                zet(&root, &lokaal_pad(&root, pad), true);
            }
        }
        for pad in foutpaden {
            let p = lokaal_pad(&root, pad);
            if p.exists() {
                zet(&root, &p, bestandsstatus(feiten(pad)).weergave().0);
            }
        }
        mappen(&root, &bekend, foutpaden);
        toestandsmap(&root);
    }

    // ---- eigen kolommen ----

    fn kolommenbestand(root: &Path) -> PathBuf {
        root.join(TOESTANDSMAP).join("verkenner.json")
    }

    fn laad_kolommen(root: &Path) {
        let mut alle = KOLOMMEN.lock().unwrap();
        if alle.contains_key(root) {
            return;
        }
        let bewaard: BTreeMap<String, Kolommen> =
            std::fs::read(kolommenbestand(root)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        alle.insert(root.to_path_buf(), bewaard);
    }

    fn bewaar_kolommen(root: &Path, kolommen: &BTreeMap<String, Kolommen>) {
        let pad = kolommenbestand(root);
        if kolommen.is_empty() {
            let _ = std::fs::remove_file(pad);
        } else if pad.parent().is_some_and(|m| m.is_dir()) {
            let _ = std::fs::write(pad, serde_json::to_vec_pretty(kolommen).unwrap_or_default());
        }
    }

    /// Kolommen van één pad in de Verkenner zetten; geeft terug of het lukte.
    fn schrijf_kolommen(root: &Path, rel: &str, k: &Kolommen) -> bool {
        let pad = lokaal_pad(root, rel);
        if !pad.exists() {
            return true; // weg: dan ook niets meer te tonen
        }
        let mut lijst = Vec::new();
        if let Some(s) = &k.status {
            let icoon = if k.fout == Some(true) { cloudfilter::ICOON_FOUT } else { cloudfilter::ICOON_OVERGESLAGEN };
            lijst.push((cloudfilter::EIGENSCHAP_STATUS, s.clone(), icoon));
        }
        if let Some(s) = &k.slot {
            lijst.push((cloudfilter::EIGENSCHAP_SLOT, s.clone(), cloudfilter::ICOON_SLOT));
        }
        if let Some(km) = kenmerk(&pad) {
            EIGEN.lock().unwrap().insert(pad.clone(), (km, Instant::now()));
        }
        match cloudfilter::zet_eigenschappen(&pad, &lijst) {
            Ok(()) => true,
            Err(e) => {
                log::debug!("eigen kolommen van {} niet gezet: {e}", pad.display());
                false
            }
        }
    }

    fn werk_kolommen_bij(root: &Path, wijzigingen: BTreeMap<String, Kolommen>) {
        if wijzigingen.is_empty() {
            return;
        }
        laad_kolommen(root);
        let mut geschreven = Vec::new();
        for (rel, k) in wijzigingen {
            if schrijf_kolommen(root, &rel, &k) {
                geschreven.push((rel, k));
            }
        }
        let mut alle = KOLOMMEN.lock().unwrap();
        let kolommen = alle.entry(root.to_path_buf()).or_default();
        for (rel, k) in geschreven {
            if k.leeg() {
                kolommen.remove(&rel);
            } else {
                kolommen.insert(rel, k);
            }
        }
        bewaar_kolommen(root, kolommen);
    }

    pub fn eigen_staten(lokaal: &str, fouten: &[(String, String)], overgeslagen: &[(String, String)]) {
        let root = PathBuf::from(lokaal);
        if !ACTIEF.lock().unwrap().contains(&root) {
            return;
        }
        laad_kolommen(&root);
        let oud = KOLOMMEN.lock().unwrap().get(&root).cloned().unwrap_or_default();
        let wijzigingen = nieuwe_kolommen(&oud, fouten, overgeslagen);
        // Een overgeslagen bestand gaat niet naar de server: sync-pijlen, met de reden in de kolom.
        for (pad, _) in overgeslagen {
            let p = lokaal_pad(&root, pad);
            if p.is_file() {
                zet(&root, &p, Itemstatus::Overgeslagen.weergave().0);
            }
        }
        werk_kolommen_bij(&root, wijzigingen);
    }

    pub fn zet_slot(lokaal: &str, pad: &str, wie: Option<String>) {
        let root = PathBuf::from(lokaal);
        if !ACTIEF.lock().unwrap().contains(&root) {
            return;
        }
        laad_kolommen(&root);
        let mut k = KOLOMMEN.lock().unwrap().get(&root).and_then(|m| m.get(pad).cloned()).unwrap_or_default();
        if k.slot == wie {
            return;
        }
        k.slot = wie;
        werk_kolommen_bij(&root, BTreeMap::from([(pad.to_string(), k)]));
    }

    pub fn sloten(lokaal: &str) -> Vec<String> {
        let root = PathBuf::from(lokaal);
        laad_kolommen(&root);
        KOLOMMEN
            .lock()
            .unwrap()
            .get(&root)
            .map(|m| m.iter().filter(|(_, k)| k.slot.is_some()).map(|(p, _)| p.clone()).collect())
            .unwrap_or_default()
    }

    // ---- ontkoppelen ----

    fn ruim_alleen_online_op(root: &Path) -> u32 {
        let mut n = 0;
        let mut te_doen = vec![root.to_path_buf()];
        while let Some(map) = te_doen.pop() {
            let Ok(items) = std::fs::read_dir(&map) else { continue };
            for item in items.flatten() {
                let pad = item.path();
                match item.file_type() {
                    Ok(t) if t.is_dir() => te_doen.push(pad),
                    Ok(t) if t.is_file() => {
                        if cloudfilter::attributen(&pad).is_some_and(cloudfilter::is_ontwaterd) && std::fs::remove_file(&pad).is_ok() {
                            n += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        n
    }

    pub fn ontkoppel(lokaal: &str) {
        let root = PathBuf::from(lokaal);
        BEHANDELD.lock().unwrap().remove(&root);
        ACTIEF.lock().unwrap().remove(&root);
        VOLLEDIG.lock().unwrap().remove(&root);
        KOLOMMEN.lock().unwrap().remove(&root);
        VOORLOPIG.lock().unwrap().retain(|p| !p.starts_with(&root));
        if OP_AANVRAAG.lock().unwrap().remove(&root) {
            // In de hoofdmap: wat alleen online stond, heeft hier geen inhoud en staat op de server.
            let n = ruim_alleen_online_op(&root);
            log::info!("{lokaal} ontkoppeld: {n} bestanden die alleen online stonden, weggehaald");
            return;
        }
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

    // ---- bestandswachter ----

    fn mtime_ns(t: SystemTime) -> u128 {
        t.duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    }

    /// Is het bestand gelijk aan de toestand van de laatste ronde (grootte en wijzigingstijd, zoals de motor kijkt)?
    fn als_in_toestand(root: &Path, pad: &Path) -> bool {
        let Ok(meta) = std::fs::metadata(pad) else { return false };
        let Ok(staat) = std::fs::metadata(root.join(TOESTANDSMAP).join("staat.json")).and_then(|m| m.modified()) else {
            return false;
        };
        let mut alle = TOESTAND.lock().unwrap();
        if alle.get(root).is_none_or(|(t, _)| *t != staat) {
            let kenmerken = bekende_bestanden(root).into_iter().map(|b| (b.pad, (b.grootte, b.mtime_ns))).collect();
            alle.insert(root.to_path_buf(), (staat, kenmerken));
        }
        let rel = relatief(root, pad);
        let nu = (meta.len(), meta.modified().map(mtime_ns).unwrap_or(0));
        alle.get(root).and_then(|(_, k)| k.get(&rel)).is_some_and(|k| *k == nu)
    }

    /// Op de achtergrond ophalen of vrijmaken als de pin-status daarom vraagt (dat kan lang duren).
    fn volg_pin(pad: &Path) {
        let Some(a) = cloudfilter::attributen(pad) else { return };
        let vast = a & cloudfilter::ATTR_VASTGEZET != 0;
        let los = a & cloudfilter::ATTR_LOSGEMAAKT != 0;
        let werk = (vast && cloudfilter::is_ontwaterd(a)) || (los && !cloudfilter::is_ontwaterd(a));
        if !werk || !PINWERK.lock().unwrap().insert(pad.to_path_buf()) {
            return;
        }
        let pad = pad.to_path_buf();
        std::thread::spawn(move || {
            match cloudfilter::volg_pinstatus(&pad) {
                Ok(cloudfilter::Pinactie::Niets) => {}
                Ok(actie) => log::info!("{}: {actie:?}", pad.display()),
                Err(e) => log::warn!("{}: {e}", pad.display()),
            }
            if let Some(k) = kenmerk(&pad) {
                EIGEN.lock().unwrap().insert(pad.clone(), (k, Instant::now()));
            }
            PINWERK.lock().unwrap().remove(&pad);
        });
    }

    pub fn gebeurtenis(root: &Path, pad: &Path) -> bool {
        if is_eigen(pad) {
            return false;
        }
        if !ACTIEF.lock().unwrap().contains(root) {
            return true;
        }
        if op_aanvraag(root) && pad.is_file() {
            volg_pin(pad);
            // Alleen online, ophalen of vrijmaken: de inhoud is niet veranderd.
            if cloudfilter::attributen(pad).is_some_and(cloudfilter::is_ontwaterd) || PINWERK.lock().unwrap().contains(pad) {
                return false;
            }
        }
        if pad.is_file() && als_in_toestand(root, pad) {
            // Alleen een attribuut of status anders (bijv. na ophalen): niets te synchroniseren. Heeft Windows hem op
            // niet in sync gezet, dan weer in sync.
            if cloudfilter::placeholder_status(pad).is_ok_and(|s| s.is_some_and(|s| !s.in_sync && !s.uitgesloten)) {
                zet(root, pad, true);
            }
            return false;
        }
        if pad.is_file() {
            naar_niet_in_sync(root, pad);
            VOORLOPIG.lock().unwrap().insert(pad.to_path_buf());
        }
        let mut map = pad.parent();
        while let Some(m) = map.filter(|m| m.starts_with(root)) {
            naar_niet_in_sync(root, m);
            map = m.parent();
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

    // ---- tijdens een ronde ----

    enum Opdracht {
        Bezig(PathBuf, String),
        Klaar(PathBuf, String),
    }

    fn werker() -> &'static Mutex<Sender<Opdracht>> {
        static WERKER: OnceLock<Mutex<Sender<Opdracht>>> = OnceLock::new();
        WERKER.get_or_init(|| {
            let (tx, rx) = channel::<Opdracht>();
            std::thread::spawn(move || {
                for opdracht in rx {
                    match opdracht {
                        Opdracht::Bezig(root, pad) => {
                            let p = lokaal_pad(&root, &pad);
                            if p.exists() {
                                naar_niet_in_sync(&root, &p);
                            }
                            for map in boven(&pad) {
                                naar_niet_in_sync(&root, &lokaal_pad(&root, &map));
                            }
                        }
                        Opdracht::Klaar(root, pad) => {
                            let p = lokaal_pad(&root, &pad);
                            if p.is_file() {
                                zet(&root, &p, true);
                            }
                        }
                    }
                }
            });
            Mutex::new(tx)
        })
    }

    pub fn tijdens(lokaal: &str, g: &Gebeurtenis) {
        let opdracht = match g {
            Gebeurtenis::Start { pad, .. } => Opdracht::Bezig(PathBuf::from(lokaal), pad.clone()),
            Gebeurtenis::Klaar { pad, actie } if *actie != Actie::Verwijderd => Opdracht::Klaar(PathBuf::from(lokaal), pad.clone()),
            _ => return,
        };
        if !ACTIEF.lock().unwrap().contains(Path::new(lokaal)) {
            return;
        }
        let _ = werker().lock().unwrap().send(opdracht);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn paden_heen_en_terug() {
        let root = Path::new("werk").join("Project A");
        let pad = lokaal_pad(&root, "Tekeningen/plan.pdf");
        assert_eq!(pad, root.join("Tekeningen").join("plan.pdf"));
        assert_eq!(relatief(&root, &pad), "Tekeningen/plan.pdf");
        assert_eq!(lokaal_pad(&root, ""), root);
    }

    #[test]
    fn fout_regel_splitsen() {
        let foutpaden = vec!["a: b.txt".to_string(), "a".to_string()];
        assert_eq!(splits_fout("a: b.txt: geweigerd", &foutpaden), Some(("a: b.txt".into(), "geweigerd".into())));
        assert_eq!(splits_fout("a: netwerkfout", &foutpaden), Some(("a".into(), "netwerkfout".into())));
        assert_eq!(splits_fout("x: y", &foutpaden), None);
    }

    #[test]
    fn kolommen_na_een_ronde() {
        let mut oud = BTreeMap::new();
        oud.insert("oud.txt".to_string(), Kolommen { status: Some("Fout: kapot".into()), fout: Some(true), slot: None });
        oud.insert("slot.dwg".to_string(), Kolommen { status: None, fout: None, slot: Some("Vergrendeld door jan".into()) });
        let fouten = vec![("nieuw.txt".to_string(), "geweigerd".to_string())];
        let overgeslagen =
            vec![("nul".to_string(), "gereserveerde naam".to_string()), ("slot.dwg".to_string(), "alleen lezen".to_string())];
        let w = nieuwe_kolommen(&oud, &fouten, &overgeslagen);
        // De oude fout is opgelost: de kolom gaat weg.
        assert!(w["oud.txt"].leeg());
        assert_eq!(w["nieuw.txt"].status.as_deref(), Some("Fout: geweigerd"));
        assert_eq!(w["nieuw.txt"].fout, Some(true));
        assert_eq!(w["nul"].status.as_deref(), Some("Overgeslagen: gereserveerde naam"));
        // De vergrendeling blijft naast de nieuwe status staan.
        assert_eq!(w["slot.dwg"].slot.as_deref(), Some("Vergrendeld door jan"));
        assert_eq!(w["slot.dwg"].fout, Some(false));
        // Een tweede keer met dezelfde uitkomst verandert niets.
        let mut nu = oud.clone();
        for (p, k) in w {
            if k.leeg() {
                nu.remove(&p);
            } else {
                nu.insert(p, k);
            }
        }
        assert!(nieuwe_kolommen(&nu, &fouten, &overgeslagen).is_empty());
    }
}
