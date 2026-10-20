//! Scenario's van de sync-motor tegen een nagebootste opslag in het geheugen.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use openaec_sync::motor::{sync_eenmalig, Opslag, OpslagBestand, Upload};
use openaec_sync::Fout;

type Inhoud = BTreeMap<String, (String, u64, Vec<u8>)>;

/// Opslag in het geheugen: pad (met '/') -> (bestand-id, versie, inhoud). Conflictregel zoals de server.
#[derive(Default)]
struct NepOpslag {
    bestanden: Mutex<Inhoud>,
    teller: Mutex<u64>,
}

impl NepOpslag {
    fn zet(&self, pad: &str, inhoud: &[u8]) {
        let mut b = self.bestanden.lock().unwrap();
        let mut t = self.teller.lock().unwrap();
        let versie = b.get(pad).map(|x| x.1 + 1).unwrap_or(1);
        let id = b.get(pad).map(|x| x.0.clone()).unwrap_or_else(|| {
            *t += 1;
            format!("b{}", *t)
        });
        b.insert(pad.to_string(), (id, versie, inhoud.to_vec()));
    }
    fn inhoud(&self, pad: &str) -> Option<Vec<u8>> {
        self.bestanden.lock().unwrap().get(pad).map(|x| x.2.clone())
    }
    fn paden(&self) -> Vec<String> {
        self.bestanden.lock().unwrap().keys().cloned().collect()
    }
}

impl Opslag for NepOpslag {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        Ok(self
            .bestanden
            .lock()
            .unwrap()
            .iter()
            .map(|(pad, (id, versie, inhoud))| OpslagBestand {
                pad: pad.clone(),
                bestand: id.clone(),
                versie: *versie,
                grootte: inhoud.len() as u64,
            })
            .collect())
    }

    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        let inhoud = std::fs::read(lokaal)?;
        let huidig = self.bestanden.lock().unwrap().get(pad).map(|x| x.1);
        let doel = match (huidig, basis_versie) {
            (Some(h), Some(b)) if h == b => pad.to_string(),
            (None, _) => pad.to_string(),
            _ => {
                let (stam, ext) = pad.rsplit_once('.').map(|(s, e)| (s.to_string(), format!(".{e}"))).unwrap_or((pad.to_string(), String::new()));
                format!("{stam} (conflict test){ext}")
            }
        };
        self.zet(&doel, &inhoud);
        let (id, versie, _) = self.bestanden.lock().unwrap()[&doel].clone();
        Ok(Upload { pad: doel.clone(), bestand: id, versie, conflict: doel != pad })
    }

    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        let inhoud = self.inhoud(&bestand.pad).unwrap();
        if let Some(map) = lokaal.parent() {
            std::fs::create_dir_all(map)?;
        }
        std::fs::write(lokaal, inhoud)?;
        Ok(())
    }

    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.bestanden.lock().unwrap().remove(&bestand.pad);
        Ok(())
    }
}

fn schrijf(map: &Path, pad: &str, inhoud: &[u8]) {
    let p = map.join(pad);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, inhoud).unwrap();
    // mtime moet zichtbaar veranderen voor de wijzigingsdetectie
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    std::fs::File::options().write(true).open(map.join(pad)).unwrap().set_modified(later).unwrap();
}

#[tokio::test]
async fn nieuw_lokaal_bestand_gaat_naar_de_server_en_tweede_ronde_doet_niets() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "Tekeningen/plan.pdf", b"pdf");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.geupload, 1);
    assert_eq!(opslag.inhoud("Tekeningen/plan.pdf").unwrap(), b"pdf");
    let r2 = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r2.geupload, r2.gedownload, r2.verwijderd), (0, 0, 0));
}

#[tokio::test]
async fn nieuw_bestand_op_server_komt_lokaal() {
    let opslag = NepOpslag::default();
    opslag.zet("Model/gebouw.ifc", b"ifc");
    let dir = tempfile::tempdir().unwrap();
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.gedownload, 1);
    assert_eq!(std::fs::read(dir.path().join("Model/gebouw.ifc")).unwrap(), b"ifc");
}

#[tokio::test]
async fn lokale_wijziging_wordt_nieuwe_versie() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    schrijf(dir.path(), "a.txt", b"twee");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.geupload, 1);
    assert_eq!(opslag.inhoud("a.txt").unwrap(), b"twee");
    assert_eq!(opslag.paden(), vec!["a.txt".to_string()]);
}

#[tokio::test]
async fn wijziging_op_server_wordt_lokaal_bijgewerkt() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    opslag.zet("a.txt", b"van collega");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.gedownload, 1);
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"van collega");
}

#[tokio::test]
async fn beide_gewijzigd_geeft_conflictkopie_en_niets_gaat_verloren() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    opslag.zet("a.txt", b"van collega");
    schrijf(dir.path(), "a.txt", b"van mij");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.conflicten, 1);
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"van collega");
    assert_eq!(std::fs::read(dir.path().join("a (conflict test).txt")).unwrap(), b"van mij");
    assert_eq!(opslag.inhoud("a (conflict test).txt").unwrap(), b"van mij");
}

#[tokio::test]
async fn lokaal_verwijderd_gaat_op_server_naar_de_prullenbak() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "weg.txt", b"x");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    std::fs::remove_file(dir.path().join("weg.txt")).unwrap();
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.verwijderd, 1);
    assert!(opslag.inhoud("weg.txt").is_none());
}

#[tokio::test]
async fn op_server_verwijderd_verdwijnt_lokaal_als_het_ongewijzigd_was() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "weg.txt", b"x");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    opslag.bestanden.lock().unwrap().remove("weg.txt");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert!(!dir.path().join("weg.txt").exists());
}

#[tokio::test]
async fn op_server_verwijderd_maar_lokaal_gewijzigd_wordt_opnieuw_geupload() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "werk.txt", b"x");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    opslag.bestanden.lock().unwrap().remove("werk.txt");
    schrijf(dir.path(), "werk.txt", b"nog bezig");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.geupload, 1);
    assert_eq!(opslag.inhoud("werk.txt").unwrap(), b"nog bezig");
}

#[tokio::test]
async fn interne_toestandsmap_wordt_niet_gesynchroniseerd() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "a.txt", b"a");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(opslag.paden(), vec!["a.txt".to_string()]);
}
