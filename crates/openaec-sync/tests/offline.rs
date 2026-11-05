//! Proeven: offline werken en daarna netjes afspelen (docs/samenwerken/offline.md). De opslag in het geheugen houdt per
//! bestand alle versies bij, weigert zoals de server (vergelijk-en-schrijf met de versieregel) en kan uitvallen: helemaal
//! offline, een antwoord dat na het vastleggen verloren gaat, of het programma dat vlak na het vastleggen stopt.
mod common;

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use common::schrijf;
use futures_util::FutureExt;
use openaec_sync::blokken::{blokgrootte, hash_bytes, BLOK};
use openaec_sync::motor::{sync_eenmalig, Opslag, OpslagBestand, Rapport, Upload, Versieregel};
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::{Client, Fout};
use serde_json::{json, Value};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const IFC: &[u8] = include_bytes!("../../openaec-samenvoegen/tests/bestanden/13-supermarkt-albert-heijn.ifc");
const PAD: &str = "planning.ifc";
const ANDER: (&str, &str) = ("'Clear out store'", "'Clear out store (ander)'");
const IK: &str = "'Temporary facilities'";

fn hernoem(inhoud: &[u8], van: &str, naar: &str) -> Vec<u8> {
    let tekst = String::from_utf8(inhoud.to_vec()).unwrap();
    assert!(tekst.contains(van), "{van} staat niet in het bestand");
    tekst.replacen(van, naar, 1).into_bytes()
}

fn bevat(inhoud: &[u8], tekst: &str) -> bool {
    String::from_utf8_lossy(inhoud).contains(tekst)
}

/// Mijn werk: taak "Temporary facilities" met een eigen naam.
fn mijn(naam: &str) -> Vec<u8> {
    hernoem(IFC, IK, &format!("'Temporary facilities ({naam})'"))
}

fn netwerkfout() -> Fout {
    Fout::Bestand(std::io::Error::new(ErrorKind::ConnectionReset, "verbinding verbroken (test)"))
}

struct Bestand {
    id: String,
    /// (versie_id, inhoud); het volgnummer is de plek + 1.
    versies: Vec<(String, Vec<u8>)>,
}

#[derive(Default)]
struct Server {
    bestanden: Mutex<BTreeMap<String, Bestand>>,
    teller: AtomicU32,
    /// Geen verbinding: elk verzoek faalt.
    offline: AtomicBool,
    /// Zoveel keer gaat het antwoord verloren nadat de server een upload vastlegde.
    antwoord_kwijt: AtomicU32,
    /// Het programma stopt (een panic) vlak nadat de server de volgende upload vastlegde.
    stop_na_vastleggen: AtomicBool,
    /// Zoveel uploads kwamen er binnen (ook geweigerde).
    uploads: AtomicU32,
    /// Een ander slaat dit op, vlak nadat de volgende lijst gemaakt is (pad, inhoud).
    na_lijst: Mutex<Option<(String, Vec<u8>)>>,
}

impl Server {
    fn nieuw_id(&self, soort: &str) -> String {
        format!("{soort}{}", self.teller.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Een ander slaat op: een nieuwe versie, of een nieuw bestand.
    fn zet(&self, pad: &str, inhoud: &[u8]) -> String {
        let regel = self.nieuw_id("v");
        let mut b = self.bestanden.lock().unwrap();
        if let Some(f) = b.get_mut(pad) {
            f.versies.push((regel, inhoud.to_vec()));
            return f.id.clone();
        }
        let id = self.nieuw_id("b");
        b.insert(pad.to_string(), Bestand { id: id.clone(), versies: vec![(regel, inhoud.to_vec())] });
        id
    }

    /// Een andere versieregel met hetzelfde volgnummer (zoals na verplaatsen met overschrijven, herstellen en opnieuw
    /// uploaden; ontwerp §5.5).
    fn vervang_laatste(&self, pad: &str, inhoud: &[u8]) {
        let regel = self.nieuw_id("v");
        *self.bestanden.lock().unwrap().get_mut(pad).unwrap().versies.last_mut().unwrap() = (regel, inhoud.to_vec());
    }

    fn wis(&self, pad: &str) {
        self.bestanden.lock().unwrap().remove(pad);
    }

    fn huidig(&self, pad: &str) -> Option<Vec<u8>> {
        self.bestanden.lock().unwrap().get(pad).map(|f| f.versies.last().unwrap().1.clone())
    }

    fn id(&self, pad: &str) -> Option<String> {
        self.bestanden.lock().unwrap().get(pad).map(|f| f.id.clone())
    }

    fn aantal_versies(&self, pad: &str) -> usize {
        self.bestanden.lock().unwrap().get(pad).map_or(0, |f| f.versies.len())
    }

    fn paden(&self) -> Vec<String> {
        self.bestanden.lock().unwrap().keys().cloned().collect()
    }

    fn verbinding(&self) -> Result<(), Fout> {
        if self.offline.load(Ordering::SeqCst) {
            return Err(netwerkfout());
        }
        Ok(())
    }

    fn weiger(code: &str, op_plek: Option<&Bestand>) -> Fout {
        let tekst = json!({
            "foutcode": code,
            "huidig_bestand": op_plek.map(|f| f.id.clone()),
            "huidige_versie": op_plek.map(|f| f.versies.len()),
            "huidige_versie_id": op_plek.map(|f| f.versies.last().unwrap().0.clone()),
        });
        Fout::Server { status: 409, tekst: tekst.to_string() }
    }

    /// Na het vastleggen: het antwoord gaat verloren, of het programma stopt.
    fn na_vastleggen(&self, upload: Upload) -> Result<Upload, Fout> {
        if self.stop_na_vastleggen.swap(false, Ordering::SeqCst) {
            panic!("het programma stopt vlak na de upload (test)");
        }
        if self.antwoord_kwijt.load(Ordering::SeqCst) > 0 {
            self.antwoord_kwijt.fetch_sub(1, Ordering::SeqCst);
            return Err(netwerkfout());
        }
        Ok(upload)
    }

    fn zoek<T>(&self, bestand: &str, f: impl FnOnce(&Bestand) -> T) -> Option<T> {
        self.bestanden.lock().unwrap().values().find(|b| b.id == bestand).map(f)
    }
}

impl Opslag for Server {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        self.verbinding()?;
        let lijst = self
            .bestanden
            .lock()
            .unwrap()
            .iter()
            .map(|(pad, f)| {
                let (regel, inhoud) = f.versies.last().unwrap();
                OpslagBestand {
                    pad: pad.clone(),
                    bestand: f.id.clone(),
                    versie: f.versies.len() as u64,
                    grootte: inhoud.len() as u64,
                    versie_id: Some(regel.clone()),
                }
            })
            .collect();
        if let Some((pad, inhoud)) = self.na_lijst.lock().unwrap().take() {
            self.zet(&pad, &inhoud);
        }
        Ok(lijst)
    }

    /// Zoals de server zonder vergelijk-en-schrijf: een nieuwe versie als de basis de huidige is, anders een
    /// conflictkopie; een nieuw bestand als er niets staat.
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.verbinding()?;
        self.uploads.fetch_add(1, Ordering::SeqCst);
        let inhoud = std::fs::read(lokaal)?;
        let huidig = self.bestanden.lock().unwrap().get(pad).map(|f| f.versies.len() as u64);
        let doel = match (huidig, basis_versie) {
            (Some(h), Some(b)) if h == b => pad.to_string(),
            (None, _) => pad.to_string(),
            _ => {
                let (stam, ext) = pad.rsplit_once('.').map(|(s, e)| (s.to_string(), format!(".{e}"))).unwrap_or((pad.to_string(), String::new()));
                format!("{stam} (conflict test){ext}")
            }
        };
        let id = self.zet(&doel, &inhoud);
        let b = self.bestanden.lock().unwrap();
        let f = &b[&doel];
        let up = Upload { pad: doel.clone(), bestand: id, versie: f.versies.len() as u64, conflict: doel != pad, versie_id: Some(f.versies.last().unwrap().0.clone()) };
        drop(b);
        self.na_vastleggen(up)
    }

    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.verbinding()?;
        let inhoud = self
            .zoek(&bestand.bestand, |f| f.versies.iter().find(|(r, _)| Some(r) == bestand.versie_id.as_ref()).map(|(_, i)| i.clone()))
            .flatten()
            .ok_or_else(|| Server::weiger("onbekende_basis", None))?;
        std::fs::create_dir_all(lokaal.parent().unwrap())?;
        std::fs::write(lokaal, inhoud)?;
        Ok(())
    }

    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.verbinding()?;
        self.wis(&bestand.pad);
        Ok(())
    }

    async fn vergelijk_en_schrijf(&self) -> bool {
        true
    }

    async fn schrijf_als(&self, pad: &str, lokaal: &Path, basis: &Versieregel, _samengevoegd_uit: Option<(u64, u64)>) -> Result<Upload, Fout> {
        self.verbinding()?;
        self.uploads.fetch_add(1, Ordering::SeqCst);
        let inhoud = std::fs::read(lokaal)?;
        let up = {
            let mut b = self.bestanden.lock().unwrap();
            let f = match b.get_mut(pad) {
                None => return Err(Server::weiger("onbekende_basis", None)),
                Some(f) if f.id != basis.bestand => return Err(Server::weiger("onbekende_basis", Some(f))),
                Some(f) => f,
            };
            match f.versies.iter().position(|(r, _)| *r == basis.versie_id) {
                None => return Err(Server::weiger("onbekende_basis", Some(f))),
                Some(i) if i + 1 != f.versies.len() => return Err(Server::weiger("verouderde_basis", Some(f))),
                Some(_) => {}
            }
            let regel = format!("v{}", self.teller.fetch_add(1, Ordering::SeqCst) + 1);
            f.versies.push((regel.clone(), inhoud));
            Upload { pad: pad.to_string(), bestand: f.id.clone(), versie: f.versies.len() as u64, conflict: false, versie_id: Some(regel) }
        };
        self.na_vastleggen(up)
    }

    async fn lees_versie(&self, versie: &Versieregel) -> Result<Vec<u8>, Fout> {
        self.verbinding()?;
        self.zoek(&versie.bestand, |f| f.versies.iter().find(|(r, _)| *r == versie.versie_id).map(|(_, i)| i.clone()))
            .flatten()
            .ok_or_else(|| Server::weiger("onbekende_basis", None))
    }

    async fn blokhashes(&self, versie: &Versieregel) -> Result<Option<(u64, Vec<String>)>, Fout> {
        self.verbinding()?;
        Ok(self
            .zoek(&versie.bestand, |f| {
                let (_, i) = f.versies.iter().find(|(r, _)| *r == versie.versie_id)?;
                let blok = blokgrootte(i.len() as u64);
                Some((blok, hash_bytes(i, blok)))
            })
            .flatten())
    }

    /// Zoals de server: hetzelfde doel is goed; een ander bestand op het doel, of een bestand dat weg is, niet.
    async fn verplaats(&self, bestand: &str, naar: &str) -> Result<(), Fout> {
        self.verbinding()?;
        let mut b = self.bestanden.lock().unwrap();
        let Some(van) = b.iter().find(|(_, f)| f.id == bestand).map(|(p, _)| p.clone()) else {
            return Err(Fout::Server { status: 404, tekst: "{}".into() });
        };
        match b.get(naar) {
            Some(f) if f.id == bestand => return Ok(()),
            Some(_) => return Err(Fout::Server { status: 417, tekst: "{}".into() }),
            None => {}
        }
        let f = b.remove(&van).unwrap();
        b.insert(naar.to_string(), f);
        Ok(())
    }
}

/// Eén ronde; `Err` bij een fout van de hele ronde (bijv. offline), en ook als het programma onderweg stopte.
async fn ronde(server: &Server, map: &Path) -> Result<Rapport, String> {
    match std::panic::AssertUnwindSafe(sync_eenmalig(server, map)).catch_unwind().await {
        Ok(r) => r.map_err(|f| f.to_string()),
        Err(_) => Err("het programma stopte".into()),
    }
}

async fn in_sync(server: &Server, map: &Path) {
    let r = ronde(server, map).await.unwrap();
    assert_eq!((r.geupload, r.gedownload, r.conflicten, r.fouten.len()), (0, 0, 0, 0), "daarna in sync: {r:?}");
}

fn toestand(map: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(map.join(".openaec/staat.json")).unwrap()).unwrap()
}

fn onderweg(map: &Path) -> usize {
    std::fs::read_dir(map.join(".openaec/onderweg")).map_or(0, |l| l.count())
}

/// Een gesynchroniseerde map met `pad`, en de opslag erbij.
async fn begin(pad: &str, inhoud: &[u8]) -> (Server, tempfile::TempDir) {
    let server = Server::default();
    server.zet(pad, inhoud);
    let dir = tempfile::tempdir().unwrap();
    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.gedownload, r.fouten.len()), (1, 0), "{:?}", r.fouten);
    (server, dir)
}

/// Offline tien keer opslaan, met een ronde na elke keer (zoals de app doet): elke ronde stopt bij de server.
async fn tien_keer_offline(server: &Server, map: &Path) -> Vec<u8> {
    server.offline.store(true, Ordering::SeqCst);
    let mut laatste = Vec::new();
    for i in 1..=10 {
        laatste = mijn(&format!("ik {i:02}"));
        schrijf(map, PAD, &laatste);
        assert!(ronde(server, map).await.is_err(), "offline");
    }
    server.offline.store(false, Ordering::SeqCst);
    laatste
}

/// Tien keer offline opgeslagen, en op de server veranderde niets: één nieuwe versie met het laatste werk.
#[tokio::test]
async fn tien_keer_offline_opslaan_wordt_een_versie() {
    let (server, dir) = begin(PAD, IFC).await;
    let laatste = tien_keer_offline(&server, dir.path()).await;

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.conflicten, r.fouten.len()), (1, 0, 0), "{:?}", r.fouten);
    assert_eq!(server.aantal_versies(PAD), 2, "één nieuwe versie");
    assert_eq!(server.huidig(PAD).unwrap(), laatste);
    assert_eq!(onderweg(dir.path()), 0);
    in_sync(&server, dir.path()).await;
}

/// Tien keer offline opgeslagen, en intussen werkte een ander door: één samengevoegde versie met beide.
#[tokio::test]
async fn tien_keer_offline_opslaan_terwijl_een_ander_doorwerkt() {
    let (server, dir) = begin(PAD, IFC).await;
    tien_keer_offline(&server, dir.path()).await;
    server.zet(PAD, &hernoem(IFC, ANDER.0, ANDER.1));

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.conflicten, r.fouten.len()), (1, 0, 0), "{:?}", r.fouten);
    let op_server = server.huidig(PAD).unwrap();
    assert!(bevat(&op_server, ANDER.1) && bevat(&op_server, "(ik 10)"), "beide wijzigingen");
    assert_eq!(server.aantal_versies(PAD), 3, "die van de ander en de samenvoeging");
    assert_eq!(server.paden(), vec![PAD.to_string()], "geen conflictkopie");
    assert_eq!(std::fs::read(dir.path().join(PAD)).unwrap(), op_server, "hier het resultaat");
    assert!(!bevat(&op_server, "(ik 09)"));
    in_sync(&server, dir.path()).await;
}

/// Offline hernoemd en verplaatst naar een nieuwe map, daarna gewijzigd; intussen wijzigde een ander het bestand op de
/// server. Bij weer online: eerst de hernoeming, dan de wijziging, samengevoegd met die van de ander. Hetzelfde bestand
/// (met zijn geschiedenis), geen nieuw bestand en geen conflictkopie.
#[tokio::test]
async fn offline_hernoemen_en_wijzigen_terwijl_een_ander_wijzigt() {
    let (server, dir) = begin(PAD, IFC).await;
    let id = server.id(PAD).unwrap();
    let nieuw = "fase 2/planning-v2.ifc";
    server.offline.store(true, Ordering::SeqCst);
    std::fs::create_dir_all(dir.path().join("fase 2")).unwrap();
    std::fs::rename(dir.path().join(PAD), dir.path().join(nieuw)).unwrap();
    assert!(ronde(&server, dir.path()).await.is_err(), "offline");
    assert_eq!(toestand(dir.path())["verplaatst"][nieuw], PAD, "de hernoeming staat vast");
    schrijf(dir.path(), nieuw, &mijn("ik"));
    assert!(ronde(&server, dir.path()).await.is_err(), "offline");
    server.zet(PAD, &hernoem(IFC, ANDER.0, ANDER.1));
    server.offline.store(false, Ordering::SeqCst);

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.conflicten, r.verwijderd, r.fouten.len()), (1, 0, 0, 0), "{r:?}");
    assert_eq!(server.paden(), vec![nieuw.to_string()], "alleen op het nieuwe pad, geen conflictkopie");
    assert_eq!(server.id(nieuw).unwrap(), id, "hetzelfde bestand");
    assert_eq!(server.aantal_versies(nieuw), 3);
    let op_server = server.huidig(nieuw).unwrap();
    assert!(bevat(&op_server, ANDER.1) && bevat(&op_server, "(ik)"), "beide wijzigingen");
    assert_eq!(std::fs::read(dir.path().join(nieuw)).unwrap(), op_server);
    assert!(!dir.path().join(PAD).exists());
    assert!(toestand(dir.path()).get("verplaatst").is_none());
    in_sync(&server, dir.path()).await;
}

/// Hernoemd, maar op de server staat op het nieuwe pad al een ander bestand: zoals altijd een conflictkopie op het nieuwe
/// pad, en het oude pad naar de prullenbak. Niets gaat verloren.
#[tokio::test]
async fn hernoemen_naar_een_bezet_pad_wordt_een_nieuw_bestand() {
    let (server, dir) = begin("a.txt", b"eerste").await;
    server.offline.store(true, Ordering::SeqCst);
    std::fs::rename(dir.path().join("a.txt"), dir.path().join("c.txt")).unwrap();
    assert!(ronde(&server, dir.path()).await.is_err());
    server.offline.store(false, Ordering::SeqCst);
    server.zet("c.txt", b"van een ander");

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!(r.fouten.len(), 0, "{:?}", r.fouten);
    assert_eq!(server.huidig("c (conflict test).txt").unwrap(), b"eerste", "mijn bestand als conflictkopie");
    assert_eq!(server.huidig("c.txt").unwrap(), b"van een ander");
    assert_eq!(std::fs::read(dir.path().join("c (conflict test).txt")).unwrap(), b"eerste");
    assert!(server.huidig("a.txt").is_none(), "het oude pad is weg (prullenbak)");
}

/// Het programma stopt vlak na de upload, vóór de toestand: de volgende start herkent de eigen upload. Geen tweede
/// versie, geen conflictkopie.
#[tokio::test]
async fn onderbreking_na_de_upload_voor_de_toestand() {
    let (server, dir) = begin("notities.txt", b"eerste").await;
    schrijf(dir.path(), "notities.txt", b"tweede");
    server.stop_na_vastleggen.store(true, Ordering::SeqCst);
    assert!(ronde(&server, dir.path()).await.is_err(), "het programma stopte");
    assert_eq!(server.aantal_versies("notities.txt"), 2, "de upload is aangekomen");
    assert_eq!(onderweg(dir.path()), 1, "en staat als onderweg vast");

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.conflicten, r.fouten.len()), (0, 0, 0), "{r:?}");
    assert_eq!(server.aantal_versies("notities.txt"), 2, "geen tweede versie");
    assert_eq!(server.paden(), vec!["notities.txt".to_string()], "geen conflictkopie");
    assert_eq!(onderweg(dir.path()), 0);
    in_sync(&server, dir.path()).await;
}

/// Als hierboven, en daarna sloeg de gebruiker nog eens op vóór de volgende ronde: dat is een gewone wijziging op de
/// versie die al aankwam (geen conflictkopie van het eigen werk).
#[tokio::test]
async fn onderbreking_en_daarna_opnieuw_opgeslagen() {
    let (server, dir) = begin("notities.txt", b"eerste").await;
    schrijf(dir.path(), "notities.txt", b"tweede");
    server.stop_na_vastleggen.store(true, Ordering::SeqCst);
    assert!(ronde(&server, dir.path()).await.is_err());
    schrijf(dir.path(), "notities.txt", b"tweede en derde");

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.conflicten, r.fouten.len()), (1, 0, 0), "{r:?}");
    assert_eq!(server.aantal_versies("notities.txt"), 3);
    assert_eq!(server.huidig("notities.txt").unwrap(), b"tweede en derde");
    assert_eq!(server.paden(), vec!["notities.txt".to_string()], "geen conflictkopie");
    in_sync(&server, dir.path()).await;
}

/// Het programma stopt vlak nadat de server een samenvoeging vastlegde: de volgende start herkent haar, haalt het
/// resultaat hierheen, en schrijft niets opnieuw.
#[tokio::test]
async fn onderbreking_na_een_samenvoeging() {
    let (server, dir) = begin(PAD, IFC).await;
    server.zet(PAD, &hernoem(IFC, ANDER.0, ANDER.1));
    let ik = mijn("ik");
    schrijf(dir.path(), PAD, &ik);
    server.stop_na_vastleggen.store(true, Ordering::SeqCst);
    assert!(ronde(&server, dir.path()).await.is_err(), "het programma stopte");
    assert_eq!(server.aantal_versies(PAD), 3, "de samenvoeging is aangekomen");
    let samengevoegd = server.huidig(PAD).unwrap();

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.gedownload, r.conflicten, r.fouten.len()), (0, 1, 0, 0), "{r:?}");
    assert_eq!(server.aantal_versies(PAD), 3, "niets opnieuw geschreven");
    assert_eq!(std::fs::read(dir.path().join(PAD)).unwrap(), samengevoegd, "hier het resultaat");
    let eigen: Vec<Vec<u8>> = std::fs::read_dir(dir.path().join(".openaec/eigen")).unwrap().flatten().map(|i| std::fs::read(i.path()).unwrap()).collect();
    assert_eq!(eigen, vec![ik], "mijn invoer bewaard");
    in_sync(&server, dir.path()).await;
}

/// Het antwoord op de upload gaat verloren (bijv. de verbinding valt weg, of een herhaalde afronding wordt geweigerd),
/// terwijl de server de upload wel vastlegde. De volgende ronde herkent hem: geen tweede versie, ook niet bij een nieuw
/// bestand (dan zou het een conflictkopie worden).
#[tokio::test]
async fn verloren_antwoord_na_de_upload() {
    let (server, dir) = begin("notities.txt", b"eerste").await;
    schrijf(dir.path(), "notities.txt", b"tweede");
    schrijf(dir.path(), "nieuw.txt", b"nieuw bestand");
    server.antwoord_kwijt.store(2, Ordering::SeqCst);
    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!(r.fouten.len(), 2, "voor de motor mislukt: {r:?}");
    assert_eq!(server.uploads.load(Ordering::SeqCst), 2);

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.conflicten, r.fouten.len()), (0, 0, 0), "{r:?}");
    assert_eq!(server.uploads.load(Ordering::SeqCst), 2, "niets opnieuw verstuurd");
    assert_eq!(server.aantal_versies("notities.txt"), 2);
    assert_eq!(server.paden(), vec!["nieuw.txt".to_string(), "notities.txt".to_string()], "geen conflictkopie");
    in_sync(&server, dir.path()).await;
}

/// De upload kwam níet aan (de verbinding viel weg vóór het vastleggen): de volgende ronde verstuurt hem gewoon opnieuw.
#[tokio::test]
async fn upload_die_niet_aankwam_gaat_opnieuw() {
    let (server, dir) = begin("notities.txt", b"eerste").await;
    schrijf(dir.path(), "notities.txt", b"tweede");
    server.offline.store(true, Ordering::SeqCst);
    assert!(ronde(&server, dir.path()).await.is_err());
    server.offline.store(false, Ordering::SeqCst);

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.fouten.len()), (1, 0), "{r:?}");
    assert_eq!(server.huidig("notities.txt").unwrap(), b"tweede");
    assert_eq!(server.aantal_versies("notities.txt"), 2);
}

/// Offline gewijzigd, en intussen verwijderde een ander het bestand op de server: mijn werk komt terug als nieuw
/// bestand. Niets gaat verloren.
#[tokio::test]
async fn de_server_wiste_het_bestand_intussen() {
    let (server, dir) = begin("notities.txt", b"eerste").await;
    let oud = server.id("notities.txt").unwrap();
    server.offline.store(true, Ordering::SeqCst);
    for werk in [&b"tweede"[..], b"tweede en derde"] {
        schrijf(dir.path(), "notities.txt", werk);
        assert!(ronde(&server, dir.path()).await.is_err());
    }
    server.wis("notities.txt");
    server.offline.store(false, Ordering::SeqCst);

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.verwijderd, r.fouten.len()), (1, 0, 0), "{r:?}");
    assert_eq!(server.huidig("notities.txt").unwrap(), b"tweede en derde");
    assert_ne!(server.id("notities.txt").unwrap(), oud, "een nieuw bestand");
    in_sync(&server, dir.path()).await;
}

/// Offline gewijzigd, en intussen verwijderde een ander het bestand en maakte een nieuw met dezelfde naam. Mijn basis
/// hoort niet bij dat bestand (`onbekende_basis`): een conflictkopie met rapport, niets samengevoegd, en hier het nieuwe
/// bestand van de ander.
#[tokio::test]
async fn dezelfde_naam_opnieuw_gemaakt_op_de_server() {
    let (server, dir) = begin(PAD, IFC).await;
    let laatste = tien_keer_offline(&server, dir.path()).await;
    server.wis(PAD);
    let ander = hernoem(IFC, ANDER.0, ANDER.1);
    server.zet(PAD, &ander);

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.conflicten, r.fouten.len()), (1, 0), "{:?}", r.fouten);
    let kopie = "planning (conflict test).ifc";
    assert_eq!(server.huidig(PAD).unwrap(), ander);
    assert_eq!(server.huidig(kopie).unwrap(), laatste, "mijn werk als conflictkopie");
    assert_eq!(std::fs::read(dir.path().join(PAD)).unwrap(), ander);
    assert_eq!(std::fs::read(dir.path().join(kopie)).unwrap(), laatste);
    let rapport: Value = serde_json::from_slice(&std::fs::read(dir.path().join(format!("{kopie}.samenvoegrapport.json"))).unwrap()).unwrap();
    assert_eq!(rapport["reden"], "onbekende_basis");
}

/// Met de echte client tegen een nagebootste API: `upload_klaar` legt de upload vast, maar het antwoord komt niet goed
/// terug. De volgende ronde ziet het bestand in de lijst, vraagt de blokhashes van die versie, herkent de eigen inhoud
/// en verstuurt niets opnieuw (`upload_start` precies één keer).
async fn upload_zonder_goed_antwoord_wordt_herkend(klaar: ResponseTemplate) {
    const API: &str = "/api/method/openaec_storage.api.";
    let antwoord = |message: Value| ResponseTemplate::new(200).set_body_json(json!({ "message": message }));
    let server = MockServer::start().await;
    let inhoud = b"offline geschreven";
    let mogelijk = json!({"bij_conflict_weiger": true, "samengevoegd_uit": true, "basis_bestand": true, "basis_versie_id": true});
    Mock::given(method("POST")).and(path(format!("{API}mogelijkheden"))).respond_with(antwoord(mogelijk)).mount(&server).await;
    // Eerste ronde: nog niets op de server; daarna het bestand, met zijn versieregel.
    Mock::given(method("POST"))
        .and(path(format!("{API}bladeren")))
        .respond_with(antwoord(json!({"mappen": [], "bestanden": []})))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    let bestand = json!({"name": "B1", "naam": "notities.txt", "grootte": inhoud.len(), "huidige_versie": 1, "versie_id": "V1"});
    Mock::given(method("POST"))
        .and(path(format!("{API}bladeren")))
        .respond_with(antwoord(json!({"mappen": [], "bestanden": [bestand]})))
        .mount(&server)
        .await;
    let deel = format!("{}/s3/deel1", server.uri());
    Mock::given(method("POST"))
        .and(path(format!("{API}upload_start")))
        .respond_with(antwoord(json!({"upload": "U1", "deelgrootte": BLOK, "hergebruik": [], "delen": [{"nummer": 1, "url": deel}]})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT")).and(path("/s3/deel1")).respond_with(ResponseTemplate::new(200).insert_header("etag", "\"e1\"")).mount(&server).await;
    Mock::given(method("POST")).and(path(format!("{API}upload_klaar"))).respond_with(klaar).expect(1).mount(&server).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}blokken")))
        .and(body_partial_json(json!({"bestand": "B1", "versie": 1})))
        .respond_with(antwoord(json!({"versie": 1, "grootte": inhoud.len(), "blokgrootte": BLOK, "hashes": hash_bytes(inhoud, BLOK)})))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "notities.txt", inhoud);
    let opslag = ClientOpslag::new(Client::new(&server.uri(), "token a:b"), "R1");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.fouten.len()), (0, 1), "voor de motor mislukt: {r:?}");

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.conflicten, r.fouten.len()), (0, 0, 0), "{r:?}");
    assert_eq!(toestand(dir.path())["bestanden"]["notities.txt"]["versie_id"], "V1", "de versie van de eigen upload");
    assert_eq!(onderweg(dir.path()), 0);
    server.verify().await;
}

/// Het antwoord van `upload_klaar` is kapot of afgebroken (de verbinding viel weg).
#[tokio::test]
async fn verloren_antwoord_op_upload_klaar() {
    upload_zonder_goed_antwoord_wordt_herkend(ResponseTemplate::new(200).set_body_string("{\"message\": {\"bestand\"")).await;
}

/// `upload_klaar` geeft 403 "Upload onbekend of verlopen", zoals een herhaalde afronding op storage-PR #9 doet nadat de
/// eerste lukte.
#[tokio::test]
async fn geweigerde_afronding_na_vastleggen() {
    let tekst = json!({"exc_type": "PermissionError", "_server_messages": "[\"{\\\"message\\\": \\\"Upload onbekend of verlopen\\\"}\"]"});
    upload_zonder_goed_antwoord_wordt_herkend(ResponseTemplate::new(403).set_body_json(tekst)).await;
}

/// Het scenario van Sol (ontwerp §5.5, "Ook verwijderen is voorwaardelijk"), offline: B wist F1 terwijl zijn toestand
/// versie 1 kent; daarna slaat A versie 2 op; dan komt B online. De verwijdering ging uit van versie 1, dus F1 met
/// versie 2 blijft staan, en komt bij B terug.
#[tokio::test]
async fn offline_gewist_terwijl_een_ander_opsloeg() {
    let (server, dir) = begin("f1.txt", b"versie 1").await;
    server.offline.store(true, Ordering::SeqCst);
    std::fs::remove_file(dir.path().join("f1.txt")).unwrap();
    assert!(ronde(&server, dir.path()).await.is_err(), "offline");
    assert_eq!(toestand(dir.path())["bestanden"]["f1.txt"]["versie"], 1, "de basis van de verwijdering blijft bewaard");
    server.zet("f1.txt", b"versie 2 van A");
    server.offline.store(false, Ordering::SeqCst);

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.verwijderd, r.gedownload, r.fouten.len()), (0, 1, 0), "{r:?}");
    assert_eq!(server.huidig("f1.txt").unwrap(), b"versie 2 van A", "F1 blijft staan");
    assert_eq!(std::fs::read(dir.path().join("f1.txt")).unwrap(), b"versie 2 van A");
    in_sync(&server, dir.path()).await;
}

/// Als hierboven, maar de nieuwe inhoud kreeg hetzelfde volgnummer met een andere versieregel. De basis van de
/// verwijdering is de versieregel, niet het nummer: F1 blijft staan.
#[tokio::test]
async fn offline_gewist_en_dezelfde_versie_met_een_andere_regel() {
    let (server, dir) = begin("f1.txt", b"versie 1").await;
    server.offline.store(true, Ordering::SeqCst);
    std::fs::remove_file(dir.path().join("f1.txt")).unwrap();
    assert!(ronde(&server, dir.path()).await.is_err(), "offline");
    server.vervang_laatste("f1.txt", b"andere inhoud, ook nummer 1");
    server.offline.store(false, Ordering::SeqCst);

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.verwijderd, r.fouten.len()), (0, 0), "{r:?}");
    assert_eq!(server.huidig("f1.txt").unwrap(), b"andere inhoud, ook nummer 1", "F1 blijft staan");
}

/// Als hierboven, maar A slaat op tussen de lijst van B en zijn verwijdering. Dicht pas als de sync vlak voor het
/// verwijderen de huidige versie opnieuw ophaalt (stap 5b) of de server de basis controleert (ontwerp §5.5).
#[tokio::test]
#[ignore = "wacht op de controle vlak voor het verwijderen uit stap 5b (knownbugs 28)"]
async fn gewist_en_een_ander_slaat_op_vlak_voor_het_verwijderen() {
    let (server, dir) = begin("f1.txt", b"versie 1").await;
    std::fs::remove_file(dir.path().join("f1.txt")).unwrap();
    *server.na_lijst.lock().unwrap() = Some(("f1.txt".into(), b"versie 2 van A".to_vec()));

    let r = ronde(&server, dir.path()).await.unwrap();
    assert_eq!((r.verwijderd, r.fouten.len()), (0, 0), "{r:?}");
    assert_eq!(server.huidig("f1.txt").unwrap(), b"versie 2 van A", "F1 blijft staan");
}
