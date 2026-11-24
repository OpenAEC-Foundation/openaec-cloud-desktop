//! Statusvinkjes in de bestandsbeheerder op Linux: een embleem op elk bestand en elke map in een gekoppelde map.
//!
//! Linux heeft geen Cloud Filter API. Wat de bestandsbeheerders wel delen, is de metadata van GVfs: de lijst
//! `metadata::emblems` van een bestand. Nemo (Cinnamon), Caja (MATE), Thunar (Xfce) en Nautilus (GNOME) tekenen de
//! emblemen uit die lijst op het pictogram, zonder extensie en zonder beheerdersrechten. De app zet daar één eigen
//! embleem in; wat de gebruiker zelf koos, blijft staan. Dolphin (KDE) leest die lijst niet: daar is een eigen
//! plugin in C++ voor nodig, en die is er nog niet.
//!
//! | Embleem | Betekenis |
//! |---|---|
//! | [`Embleem::InSync`] | groen vinkje: gelijk aan de server |
//! | [`Embleem::Bezig`] | blauwe pijlen: gewijzigd, of nu onderweg |
//! | [`Embleem::Fout`] | rood uitroepteken: de laatste poging mislukte |
//! | [`Embleem::Overgeslagen`] | grijs streepje: wordt niet gesynchroniseerd (lastige naam, alleen lezen) |
//!
//! Een map toont het geheel (zie `itemstatus::mapstatus`): fout als er iets in fout gaat, anders bezig, anders in sync.
//! De pictogrammen zelf (`emblem-openaec-*.svg`) zet de app in `~/.local/share/icons/hicolor`, het thema waar elk
//! ander thema op terugvalt.
//!
//! Alles wat GVfs raakt, loopt op één eigen thread ([`echt`]); de functies hier zetten alleen een opdracht in de rij.
//! De metadata staat in `~/.local/share/gvfs-metadata`, niet in het bestand zelf: een embleem zetten wijzigt het
//! bestand niet en wekt de bewaking dus niet. Op andere systemen doet dit niets.

use std::collections::{BTreeMap, BTreeSet};

use crate::itemstatus::{mapstatussen, Itemstatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Embleem {
    InSync,
    Bezig,
    Fout,
    Overgeslagen,
}

impl Embleem {
    /// De naam van het pictogram in het thema.
    pub fn naam(self) -> &'static str {
        match self {
            Embleem::InSync => "emblem-openaec-synced",
            Embleem::Bezig => "emblem-openaec-syncing",
            Embleem::Fout => "emblem-openaec-error",
            Embleem::Overgeslagen => "emblem-openaec-skipped",
        }
    }
}

/// De emblemen die van de app zijn; elk ander embleem in de lijst is van de gebruiker en blijft staan.
pub const EIGEN: [Embleem; 4] = [Embleem::InSync, Embleem::Bezig, Embleem::Fout, Embleem::Overgeslagen];

/// Het attribuut van GVfs met de emblemen van een bestand.
pub const ATTRIBUUT: &str = "metadata::emblems";

/// Waar de pictogrammen staan, ten opzichte van `~/.local/share` (ook `linux/postrm` ruimt precies deze op).
pub const PICTOGRAMMEN: [(&str, &[u8]); 4] = [
    ("icons/hicolor/scalable/emblems/emblem-openaec-synced.svg", include_bytes!("../linux/emblemen/emblem-openaec-synced.svg")),
    ("icons/hicolor/scalable/emblems/emblem-openaec-syncing.svg", include_bytes!("../linux/emblemen/emblem-openaec-syncing.svg")),
    ("icons/hicolor/scalable/emblems/emblem-openaec-error.svg", include_bytes!("../linux/emblemen/emblem-openaec-error.svg")),
    ("icons/hicolor/scalable/emblems/emblem-openaec-skipped.svg", include_bytes!("../linux/emblemen/emblem-openaec-skipped.svg")),
];

/// De lijst emblemen met het eigen embleem vervangen door `nieuw` (None: weg). Emblemen van de gebruiker blijven,
/// in hun volgorde; het eigen embleem komt achteraan.
pub fn samengevoegd(bestaand: &[String], nieuw: Option<Embleem>) -> Vec<String> {
    let mut uit: Vec<String> = bestaand.iter().filter(|n| !EIGEN.iter().any(|e| e.naam() == n.as_str())).cloned().collect();
    if let Some(e) = nieuw {
        uit.push(e.naam().to_string());
    }
    uit
}

/// Het embleem per pad (met '/', de gekoppelde map zelf is "") uit wat de sync weet: `in_sync` zijn de bestanden die
/// gelijk zijn aan de server, `bezig` de bestanden die gewijzigd zijn of nu over de lijn gaan. Een fout gaat voor
/// overgeslagen, dat voor bezig, dat voor in sync. Mappen volgen uit de bestanden eronder.
pub fn gewenst(in_sync: &[String], bezig: &BTreeSet<String>, fouten: &BTreeSet<String>, overgeslagen: &BTreeSet<String>) -> BTreeMap<String, Embleem> {
    let mut bestanden: BTreeMap<&str, Embleem> = in_sync.iter().map(|p| (p.as_str(), Embleem::InSync)).collect();
    bestanden.extend(bezig.iter().map(|p| (p.as_str(), Embleem::Bezig)));
    bestanden.extend(overgeslagen.iter().map(|p| (p.as_str(), Embleem::Overgeslagen)));
    bestanden.extend(fouten.iter().map(|p| (p.as_str(), Embleem::Fout)));
    let status = |e: Embleem| match e {
        Embleem::InSync => Itemstatus::Gesynchroniseerd,
        Embleem::Bezig => Itemstatus::Bezig,
        Embleem::Fout => Itemstatus::Fout,
        Embleem::Overgeslagen => Itemstatus::Overgeslagen,
    };
    let mut uit: BTreeMap<String, Embleem> = mapstatussen(bestanden.iter().map(|(p, e)| (*p, status(*e))))
        .into_iter()
        .map(|(map, s)| {
            let e = match s {
                Itemstatus::Fout => Embleem::Fout,
                Itemstatus::Bezig => Embleem::Bezig,
                _ => Embleem::InSync,
            };
            (map, e)
        })
        .collect();
    // Een bestand en een map kunnen niet hetzelfde pad hebben; het bestand wint als de toestand dat toch zegt.
    uit.extend(bestanden.into_iter().map(|(p, e)| (p.to_string(), e)));
    uit
}

/// Wat er aan de emblemen van een map moet veranderen: per pad het nieuwe embleem, of None als het weg moet.
pub fn verschil(oud: &BTreeMap<String, Embleem>, nieuw: &BTreeMap<String, Embleem>) -> Vec<(String, Option<Embleem>)> {
    let mut uit: Vec<(String, Option<Embleem>)> =
        nieuw.iter().filter(|(p, e)| oud.get(*p) != Some(*e)).map(|(p, e)| (p.clone(), Some(*e))).collect();
    uit.extend(oud.keys().filter(|p| !nieuw.contains_key(*p)).map(|p| (p.clone(), None)));
    uit
}

/// Bij koppelen en bij het starten: alles wat in sync is (en de mappen erboven) een vinkje geven.
pub fn koppel(lokaal: &str) {
    echt::stuur(echt::Opdracht::Koppel(lokaal.into()));
}

/// Is deze map in dit proces al gekoppeld?
pub fn gekoppeld(lokaal: &str) -> bool {
    echt::gekoppeld(lokaal)
}

/// De foutpaden van de ronde die net klaar is; [`na_ronde`] verwerkt ze.
pub fn fouten(lokaal: &str, paden: &[String]) {
    echt::stuur(echt::Opdracht::Fouten(lokaal.into(), paden.to_vec()));
}

/// Na een sync-ronde: alle emblemen van de map opnieuw bepalen en zetten wat veranderde.
pub fn na_ronde(lokaal: &str, overgeslagen: &[String]) {
    echt::stuur(echt::Opdracht::Ronde(lokaal.into(), overgeslagen.to_vec()));
}

/// Een bestand is gewijzigd of gaat nu over de lijn: pijlen, ook op de mappen erboven.
pub fn bezig(lokaal: &str, pad: &str) {
    echt::stuur(echt::Opdracht::Bezig(lokaal.into(), pad.to_string()));
}

/// Een bestand is klaar: een vinkje. De mappen erboven volgen na de ronde.
pub fn klaar(lokaal: &str, pad: &str) {
    echt::stuur(echt::Opdracht::Klaar(lokaal.into(), pad.to_string()));
}

/// Bij ontkoppelen: alle eigen emblemen van de map weghalen.
pub fn ontkoppel(lokaal: &str) {
    echt::stuur(echt::Opdracht::Ontkoppel(lokaal.into()));
}

#[cfg(not(target_os = "linux"))]
mod echt {
    use std::path::PathBuf;

    #[allow(dead_code)]
    pub enum Opdracht {
        Koppel(PathBuf),
        Fouten(PathBuf, Vec<String>),
        Ronde(PathBuf, Vec<String>),
        Bezig(PathBuf, String),
        Klaar(PathBuf, String),
        Ontkoppel(PathBuf),
    }

    pub fn stuur(_opdracht: Opdracht) {}

    pub fn gekoppeld(_lokaal: &str) -> bool {
        true
    }
}

#[cfg(target_os = "linux")]
pub mod echt {
    use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock};

    use gio::prelude::*;
    use openaec_sync::motor::{bekende_paden, TOESTANDSMAP};

    use super::{gewenst, samengevoegd, verschil, Embleem, ATTRIBUUT, PICTOGRAMMEN};
    use crate::itemstatus::boven;

    pub enum Opdracht {
        Koppel(PathBuf),
        Fouten(PathBuf, Vec<String>),
        Ronde(PathBuf, Vec<String>),
        Bezig(PathBuf, String),
        Klaar(PathBuf, String),
        Ontkoppel(PathBuf),
        /// Alleen voor de rooktest: meldt dat alles tot hier verwerkt is.
        Klaarmelding(Sender<()>),
    }

    /// De mappen die in dit proces gekoppeld zijn (voor [`gekoppeld`], zonder op de thread te wachten).
    static GEKOPPELD: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

    pub fn gekoppeld(lokaal: &str) -> bool {
        GEKOPPELD.lock().unwrap().as_ref().is_some_and(|m| m.contains(Path::new(lokaal)))
    }

    pub fn stuur(opdracht: Opdracht) {
        match &opdracht {
            Opdracht::Koppel(root) => {
                GEKOPPELD.lock().unwrap().get_or_insert_with(HashSet::new).insert(root.clone());
            }
            Opdracht::Ontkoppel(root) => {
                if let Some(m) = GEKOPPELD.lock().unwrap().as_mut() {
                    m.remove(root);
                }
            }
            _ => {}
        }
        static WERKER: OnceLock<Mutex<Sender<Opdracht>>> = OnceLock::new();
        let werker = WERKER.get_or_init(|| {
            let (tx, rx) = channel::<Opdracht>();
            std::thread::Builder::new()
                .name("emblemen".into())
                .spawn(move || {
                    let mut stand = Stand::default();
                    for opdracht in rx {
                        stand.verwerk(opdracht);
                    }
                })
                .expect("thread voor de emblemen");
            Mutex::new(tx)
        });
        let _ = werker.lock().unwrap().send(opdracht);
    }

    /// Wacht tot alle opdrachten tot hier verwerkt zijn (rooktest).
    pub fn wacht() {
        let (tx, rx) = channel();
        stuur(Opdracht::Klaarmelding(tx));
        let _ = rx.recv();
    }

    fn lokaal_pad(root: &Path, pad: &str) -> PathBuf {
        pad.split('/').filter(|d| !d.is_empty()).fold(root.to_path_buf(), |p, d| p.join(d))
    }

    /// De emblemen van één pad nu, of None als het pad er niet is of GVfs geen metadata kent.
    pub fn lees(pad: &Path) -> Option<Vec<String>> {
        let info = gio::File::for_path(pad).query_info(ATTRIBUUT, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE).ok()?;
        Some(info.attribute_stringv(ATTRIBUUT).iter().map(|s| s.to_string()).collect())
    }

    /// Het eigen embleem van één pad zetten of weghalen; schrijft alleen als de lijst verandert.
    fn zet(pad: &Path, embleem: Option<Embleem>) -> Result<(), String> {
        let Some(nu) = lees(pad) else { return Ok(()) };
        let nieuw = samengevoegd(&nu, embleem);
        if nieuw == nu {
            return Ok(());
        }
        let namen: Vec<&str> = nieuw.iter().map(String::as_str).collect();
        let info = gio::FileInfo::new();
        info.set_attribute_stringv(ATTRIBUUT, &namen[..]);
        gio::File::for_path(pad)
            .set_attributes_from_info(&info, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)
            .map_err(|e| e.to_string())
    }

    /// De pictogrammen in het thema hicolor van de gebruiker; GTK leest de map opnieuw als zijn tijd verandert.
    fn zet_pictogrammen() {
        let Some(basis) = dirs::data_dir() else { return };
        let mut veranderd = false;
        for (pad, inhoud) in PICTOGRAMMEN {
            let doel = basis.join(pad);
            if std::fs::read(&doel).ok().as_deref() == Some(inhoud) {
                continue;
            }
            if let Some(map) = doel.parent() {
                let _ = std::fs::create_dir_all(map);
            }
            veranderd |= std::fs::write(&doel, inhoud).is_ok();
        }
        if veranderd {
            let hicolor = basis.join("icons").join("hicolor");
            let _ = std::fs::File::open(&hicolor).and_then(|f| f.set_modified(std::time::SystemTime::now()));
        }
    }

    #[derive(Default)]
    struct Map {
        /// Wat er nu op staat (pad met '/', de map zelf is "").
        gezet: BTreeMap<String, Embleem>,
        bezig: BTreeSet<String>,
        fouten: BTreeSet<String>,
        overgeslagen: BTreeSet<String>,
    }

    #[derive(Default)]
    struct Stand {
        mappen: HashMap<PathBuf, Map>,
        /// Eén waarschuwing als GVfs geen metadata kan bewaren (geen gvfs, of een bestandssysteem zonder).
        gewaarschuwd: bool,
    }

    impl Stand {
        fn schrijf(&mut self, root: &Path, pad: &str, embleem: Option<Embleem>) {
            if let Err(e) = zet(&lokaal_pad(root, pad), embleem) {
                if !self.gewaarschuwd {
                    self.gewaarschuwd = true;
                    log::warn!("statusvinkjes in de bestandsbeheerder niet beschikbaar ({}): {e}", root.display());
                }
            }
        }

        /// Alles opnieuw bepalen uit de toestand van de sync en zetten wat anders is.
        fn werk_bij(&mut self, root: &Path) {
            let Some(map) = self.mappen.get_mut(root) else { return };
            let in_sync = bekende_paden(root);
            let bekend: BTreeSet<&str> = in_sync.iter().map(String::as_str).collect();
            // Bezig blijft wat nog niet gelijk is aan de server en er nog staat.
            map.bezig.retain(|p| !bekend.contains(p.as_str()) && lokaal_pad(root, p).exists());
            map.fouten.retain(|p| lokaal_pad(root, p).exists());
            map.overgeslagen.retain(|p| lokaal_pad(root, p).exists());
            let nieuw = gewenst(&in_sync, &map.bezig, &map.fouten, &map.overgeslagen);
            let anders = verschil(&map.gezet, &nieuw);
            map.gezet = nieuw;
            for (pad, embleem) in anders {
                self.schrijf(root, &pad, embleem);
            }
        }

        fn verwerk(&mut self, opdracht: Opdracht) {
            match opdracht {
                Opdracht::Koppel(root) => {
                    zet_pictogrammen();
                    self.mappen.entry(root.clone()).or_default();
                    self.werk_bij(&root);
                    let n = self.mappen.get(&root).map_or(0, |m| m.gezet.len());
                    log::info!("statusvinkjes voor {}: {n} bestanden en mappen", root.display());
                }
                Opdracht::Fouten(root, paden) => {
                    if let Some(map) = self.mappen.get_mut(&root) {
                        map.fouten = paden.into_iter().collect();
                    }
                }
                Opdracht::Ronde(root, overgeslagen) => {
                    if let Some(map) = self.mappen.get_mut(&root) {
                        map.overgeslagen = overgeslagen.into_iter().collect();
                    }
                    self.werk_bij(&root);
                }
                Opdracht::Bezig(root, pad) => {
                    // De eigen toestand van de sync en mappen tellen niet: een map volgt uit zijn bestanden.
                    if pad.is_empty() || pad.split('/').next() == Some(TOESTANDSMAP) || !lokaal_pad(&root, &pad).is_file() {
                        return;
                    }
                    let Some(map) = self.mappen.get_mut(&root) else { return };
                    if !map.bezig.insert(pad.clone()) && map.gezet.get(&pad) == Some(&Embleem::Bezig) {
                        return;
                    }
                    // Een fout blijft op de map staan tot de ronde hem opnieuw bepaalt.
                    let mut zetten = vec![pad.clone()];
                    zetten.extend(boven(&pad).into_iter().filter(|m| map.gezet.get(m) != Some(&Embleem::Fout)));
                    zetten.retain(|p| map.gezet.insert(p.clone(), Embleem::Bezig) != Some(Embleem::Bezig));
                    for p in zetten {
                        self.schrijf(&root, &p, Some(Embleem::Bezig));
                    }
                }
                Opdracht::Klaar(root, pad) => {
                    let Some(map) = self.mappen.get_mut(&root) else { return };
                    map.bezig.remove(&pad);
                    map.fouten.remove(&pad);
                    if lokaal_pad(&root, &pad).is_file() && map.gezet.insert(pad.clone(), Embleem::InSync) != Some(Embleem::InSync) {
                        self.schrijf(&root, &pad, Some(Embleem::InSync));
                    }
                }
                Opdracht::Ontkoppel(root) => {
                    // Ook wat een eerdere start zette en deze nog niet kent: alles wat de sync kent, en de mappen erboven.
                    let mut paden: BTreeSet<String> = self.mappen.remove(&root).map(|m| m.gezet.into_keys().collect()).unwrap_or_default();
                    let leeg = BTreeSet::new();
                    paden.extend(gewenst(&bekende_paden(&root), &leeg, &leeg, &leeg).into_keys());
                    paden.insert(String::new());
                    for pad in paden {
                        self.schrijf(&root, &pad, None);
                    }
                }
                Opdracht::Klaarmelding(tx) => {
                    let _ = tx.send(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(paden: &[&str]) -> BTreeSet<String> {
        paden.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn eigen_embleem_naast_dat_van_de_gebruiker() {
        let van_gebruiker = vec!["emblem-favorite".to_string(), "emblem-openaec-syncing".to_string(), "emblem-important".to_string()];
        assert_eq!(samengevoegd(&van_gebruiker, Some(Embleem::InSync)), ["emblem-favorite", "emblem-important", "emblem-openaec-synced"]);
        assert_eq!(samengevoegd(&van_gebruiker, None), ["emblem-favorite", "emblem-important"]);
        assert_eq!(samengevoegd(&[], Some(Embleem::Fout)), ["emblem-openaec-error"]);
        // Al goed: dezelfde lijst, dus niets te schrijven.
        let goed = vec!["emblem-openaec-synced".to_string()];
        assert_eq!(samengevoegd(&goed, Some(Embleem::InSync)), goed);
    }

    #[test]
    fn embleem_per_bestand_en_map() {
        let in_sync: Vec<String> = ["A/plan.pdf", "A/B/detail.dwg", "C/foto.jpg", "los.txt"].iter().map(|p| p.to_string()).collect();
        let g = gewenst(&in_sync, &set(&["A/B/nieuw.ifc"]), &set(&["C/kapot.txt"]), &set(&["A/con.txt"]));
        assert_eq!(g["A/plan.pdf"], Embleem::InSync);
        assert_eq!(g["A/B/nieuw.ifc"], Embleem::Bezig);
        assert_eq!(g["C/kapot.txt"], Embleem::Fout);
        assert_eq!(g["A/con.txt"], Embleem::Overgeslagen);
        // Mappen: bezig als er iets in bezig is, fout als er iets fout gaat; overgeslagen telt niet mee.
        assert_eq!(g["A/B"], Embleem::Bezig);
        assert_eq!(g["A"], Embleem::Bezig);
        assert_eq!(g["C"], Embleem::Fout);
        assert_eq!(g[""], Embleem::Fout);
        // Een fout gaat voor bezig en in sync op hetzelfde pad.
        let g = gewenst(&in_sync, &set(&["los.txt"]), &set(&["los.txt"]), &set(&[]));
        assert_eq!(g["los.txt"], Embleem::Fout);
        // Niets bekend: ook geen embleem op de map zelf.
        assert!(gewenst(&[], &set(&[]), &set(&[]), &set(&[])).is_empty());
    }

    #[test]
    fn alleen_wat_verandert() {
        let oud: BTreeMap<String, Embleem> = [("a".to_string(), Embleem::InSync), ("b".to_string(), Embleem::Bezig), ("weg".to_string(), Embleem::Fout)].into();
        let nieuw: BTreeMap<String, Embleem> = [("a".to_string(), Embleem::InSync), ("b".to_string(), Embleem::InSync), ("c".to_string(), Embleem::Bezig)].into();
        let mut v = verschil(&oud, &nieuw);
        v.sort();
        assert_eq!(v, [("b".to_string(), Some(Embleem::InSync)), ("c".to_string(), Some(Embleem::Bezig)), ("weg".to_string(), None)]);
        assert!(verschil(&nieuw, &nieuw).is_empty());
    }

    #[test]
    fn pictogrammen_voor_elk_embleem() {
        for e in EIGEN {
            let (_, inhoud) = PICTOGRAMMEN.iter().find(|(pad, _)| pad.ends_with(&format!("{}.svg", e.naam()))).expect("pictogram ontbreekt");
            assert!(std::str::from_utf8(inhoud).unwrap().contains("<svg"));
        }
        let postrm = include_str!("../linux/postrm");
        for (pad, _) in PICTOGRAMMEN {
            assert!(postrm.contains(pad.rsplit('/').next().unwrap()), "{pad} ontbreekt in linux/postrm");
        }
    }
}
