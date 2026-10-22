//! Meldingen van de sync-motor per bestand (start, voortgang, klaar, fout) en pauzeren.

mod common;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use common::{schrijf, NepOpslag};
use openaec_sync::motor::{sync_met_voortgang, Opslag, OpslagBestand, Upload, Voortgang};
use openaec_sync::voortgang::{Actie, Gebeurtenis, Richting, Waarnemer};
use openaec_sync::Fout;

/// Verzamelt alle meldingen; pauzeert na `pauze_na` afgeronde bestanden (als gezet).
#[derive(Default)]
struct Verzamelaar {
    meldingen: Mutex<Vec<Gebeurtenis>>,
    pauze_na: Option<usize>,
    gepauzeerd: AtomicBool,
}

impl Waarnemer for Verzamelaar {
    fn melding(&self, g: Gebeurtenis) {
        let mut m = self.meldingen.lock().unwrap();
        m.push(g);
        let klaar = m.iter().filter(|g| matches!(g, Gebeurtenis::Klaar { .. })).count();
        if self.pauze_na.is_some_and(|n| klaar >= n) {
            self.gepauzeerd.store(true, Ordering::SeqCst);
        }
    }

    fn gepauzeerd(&self) -> bool {
        self.gepauzeerd.load(Ordering::SeqCst)
    }
}

impl Verzamelaar {
    fn alle(&self) -> Vec<Gebeurtenis> {
        self.meldingen.lock().unwrap().clone()
    }
}

/// Opslag die bij elke overdracht 1000 tussenstanden meldt, heel snel achter elkaar.
#[derive(Default)]
struct DruktemakerOpslag(NepOpslag);

impl Opslag for DruktemakerOpslag {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        self.0.lijst().await
    }
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.0.upload(pad, lokaal, basis_versie).await
    }
    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.0.download(bestand, lokaal).await
    }
    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.0.verwijder(bestand).await
    }
    async fn upload_met_voortgang(
        &self,
        pad: &str,
        lokaal: &Path,
        basis_versie: Option<u64>,
        voortgang: Voortgang<'_>,
    ) -> Result<Upload, Fout> {
        for i in 1..=1000 {
            voortgang(i, 1000);
        }
        self.0.upload(pad, lokaal, basis_versie).await
    }
}

#[tokio::test]
async fn upload_en_download_geven_start_en_klaar() {
    let opslag = NepOpslag::default();
    opslag.zet("van/server.txt", b"server");
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "van/hier.txt", b"hier!");
    let w = Verzamelaar::default();
    sync_met_voortgang(&opslag, dir.path(), &[], &w).await.unwrap();
    let m = w.alle();
    assert_eq!(m[0], Gebeurtenis::Ronde { uploaden: 1, downloaden: 1 });
    assert!(m.contains(&Gebeurtenis::Start { pad: "van/hier.txt".into(), richting: Richting::Upload, totaal: 5 }));
    assert!(m.contains(&Gebeurtenis::Klaar { pad: "van/hier.txt".into(), actie: Actie::Geupload }));
    assert!(m.contains(&Gebeurtenis::Start { pad: "van/server.txt".into(), richting: Richting::Download, totaal: 6 }));
    assert!(m.contains(&Gebeurtenis::Klaar { pad: "van/server.txt".into(), actie: Actie::Gedownload }));
    // Start komt voor klaar.
    let start = m.iter().position(|g| matches!(g, Gebeurtenis::Start { pad, .. } if pad == "van/hier.txt")).unwrap();
    let klaar = m.iter().position(|g| matches!(g, Gebeurtenis::Klaar { pad, .. } if pad == "van/hier.txt")).unwrap();
    assert!(start < klaar);

    // Tweede ronde: niets te doen, alleen de (lege) ronde.
    let w2 = Verzamelaar::default();
    sync_met_voortgang(&opslag, dir.path(), &[], &w2).await.unwrap();
    assert_eq!(w2.alle(), vec![Gebeurtenis::Ronde { uploaden: 0, downloaden: 0 }]);
}

#[tokio::test]
async fn verwijderen_en_conflict_worden_gemeld() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "weg.txt", b"x");
    schrijf(dir.path(), "a.txt", b"een");
    sync_met_voortgang(&opslag, dir.path(), &[], &Verzamelaar::default()).await.unwrap();
    std::fs::remove_file(dir.path().join("weg.txt")).unwrap();
    opslag.zet("a.txt", b"van collega");
    schrijf(dir.path(), "a.txt", b"van mij");
    let w = Verzamelaar::default();
    sync_met_voortgang(&opslag, dir.path(), &[], &w).await.unwrap();
    let m = w.alle();
    assert!(m.contains(&Gebeurtenis::Klaar { pad: "weg.txt".into(), actie: Actie::Verwijderd }));
    assert!(m.contains(&Gebeurtenis::Klaar { pad: "a.txt".into(), actie: Actie::Conflict }));
}

#[tokio::test]
async fn fout_per_bestand_wordt_gemeld() {
    let opslag = NepOpslag::default();
    opslag.weiger("kapot.txt");
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "kapot.txt", b"x");
    let w = Verzamelaar::default();
    let r = sync_met_voortgang(&opslag, dir.path(), &[], &w).await.unwrap();
    assert_eq!(r.fouten.len(), 1);
    let fout = w.alle().into_iter().find_map(|g| match g {
        Gebeurtenis::Fout { pad, fout } => Some((pad, fout)),
        _ => None,
    });
    let (pad, tekst) = fout.expect("een foutmelding");
    assert_eq!(pad, "kapot.txt");
    assert!(tekst.contains("geweigerd"));
}

#[tokio::test]
async fn bytevoortgang_wordt_afgeremd_maar_de_eindstand_komt_door() {
    let opslag = DruktemakerOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "groot.bin", b"0123456789");
    let w = Verzamelaar::default();
    sync_met_voortgang(&opslag, dir.path(), &[], &w).await.unwrap();
    let standen: Vec<(u64, u64)> = w
        .alle()
        .into_iter()
        .filter_map(|g| match g {
            Gebeurtenis::Voortgang { richting: Richting::Upload, gedaan, totaal, .. } => Some((gedaan, totaal)),
            _ => None,
        })
        .collect();
    // 1000 tussenstanden binnen een paar milliseconden: alleen de eerste en de laatste.
    assert!(standen.len() <= 3, "te veel voortgangsmeldingen: {}", standen.len());
    assert_eq!(standen.first(), Some(&(1, 1000)));
    assert_eq!(standen.last(), Some(&(1000, 1000)));
}

#[tokio::test]
async fn pauzeren_stopt_na_het_lopende_bestand_en_de_rest_volgt_later() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    for n in ["a.txt", "b.txt", "c.txt"] {
        schrijf(dir.path(), n, n.as_bytes());
    }
    let w = Verzamelaar { pauze_na: Some(1), ..Default::default() };
    let r = sync_met_voortgang(&opslag, dir.path(), &[], &w).await.unwrap();
    assert_eq!(r.geupload, 1);
    assert_eq!(opslag.paden().len(), 1);

    let w2 = Verzamelaar::default();
    let r2 = sync_met_voortgang(&opslag, dir.path(), &[], &w2).await.unwrap();
    assert_eq!(r2.geupload, 2);
    assert_eq!(w2.alle()[0], Gebeurtenis::Ronde { uploaden: 2, downloaden: 0 });
}
