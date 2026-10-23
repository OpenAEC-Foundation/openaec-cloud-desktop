//! Kleine bestanden in één verzoek: de motor groepeert kleine uploads en downloads binnen de grenzen van de opslag,
//! valt terug op losse overdrachten als het verzoek als geheel mislukt, en houdt de regels per bestand gelijk.

mod common;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use common::{schrijf, NepOpslag};
use openaec_sync::motor::{sync_eenmalig, sync_met_voortgang, Batchgrenzen, KleinUpload, Opslag, OpslagBestand, Upload};
use openaec_sync::voortgang::{Actie, Gebeurtenis, Waarnemer};
use openaec_sync::Fout;

const GRENZEN: Batchgrenzen = Batchgrenzen { max_bestand: 100, max_aantal: 64, max_bytes: 1_000_000 };

/// Opslag in het geheugen die kleine bestanden in groepen aanneemt en bijhoudt hoe groot elke groep was.
#[derive(Default)]
struct BatchOpslag {
    nep: NepOpslag,
    grenzen: Option<Batchgrenzen>,
    /// Het verzoek als geheel weigeren (bijv. een netwerkfout): de motor moet elk bestand apart proberen.
    weiger_batch: bool,
    groepen_up: Mutex<Vec<usize>>,
    groepen_down: Mutex<Vec<usize>>,
    los: AtomicUsize,
}

impl Opslag for BatchOpslag {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        self.nep.lijst().await
    }
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.los.fetch_add(1, Ordering::SeqCst);
        self.nep.upload(pad, lokaal, basis_versie).await
    }
    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.los.fetch_add(1, Ordering::SeqCst);
        self.nep.download(bestand, lokaal).await
    }
    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.nep.verwijder(bestand).await
    }
    async fn klein_grenzen(&self) -> Option<Batchgrenzen> {
        self.grenzen
    }
    async fn upload_klein(&self, bestanden: &[KleinUpload<'_>]) -> Result<Vec<Result<Upload, Fout>>, Fout> {
        if self.weiger_batch {
            return Err(Fout::Server { status: 502, tekst: "even niet".into() });
        }
        self.groepen_up.lock().unwrap().push(bestanden.len());
        let mut uit = Vec::new();
        for b in bestanden {
            uit.push(self.nep.upload(b.pad, b.lokaal, b.basis_versie).await);
        }
        Ok(uit)
    }
    async fn download_klein(&self, bestanden: &[(&OpslagBestand, &Path)]) -> Result<Vec<Result<(), Fout>>, Fout> {
        if self.weiger_batch {
            return Err(Fout::Server { status: 502, tekst: "even niet".into() });
        }
        self.groepen_down.lock().unwrap().push(bestanden.len());
        let mut uit = Vec::new();
        for (b, lokaal) in bestanden {
            uit.push(self.nep.download(b, lokaal).await);
        }
        Ok(uit)
    }
}

#[derive(Default)]
struct Verzamelaar(Mutex<Vec<Gebeurtenis>>);

impl Waarnemer for Verzamelaar {
    fn melding(&self, g: Gebeurtenis) {
        self.0.lock().unwrap().push(g);
    }
}

fn met_grenzen(grenzen: Batchgrenzen) -> BatchOpslag {
    BatchOpslag { grenzen: Some(grenzen), ..Default::default() }
}

#[tokio::test]
async fn kleine_uploads_gaan_in_groepen_en_grote_los() {
    let opslag = met_grenzen(GRENZEN);
    let dir = tempfile::tempdir().unwrap();
    for i in 0..150 {
        schrijf(dir.path(), &format!("Map {}/{i}.txt", i % 7), format!("klein {i}").as_bytes());
    }
    schrijf(dir.path(), "groot.bin", &[7u8; 500]);
    let w = Verzamelaar::default();
    let r = sync_met_voortgang(&opslag, dir.path(), &[], &w).await.unwrap();
    assert_eq!(r.geupload, 151);
    let mut groepen = opslag.groepen_up.lock().unwrap().clone();
    groepen.sort();
    assert_eq!(groepen, vec![22, 64, 64]);
    assert_eq!(opslag.los.load(Ordering::SeqCst), 1, "alleen het grote bestand apart");
    assert_eq!(opslag.nep.inhoud("Map 3/10.txt").unwrap(), b"klein 10");

    let m = w.0.lock().unwrap().clone();
    assert_eq!(m.iter().filter(|g| matches!(g, Gebeurtenis::Start { .. })).count(), 151);
    assert_eq!(m.iter().filter(|g| matches!(g, Gebeurtenis::Klaar { actie: Actie::Geupload, .. })).count(), 151);
    // Elk klein bestand eindigt met een volledige voortgang (voor de tellers in de app).
    assert!(m.contains(&Gebeurtenis::Voortgang {
        pad: "Map 3/10.txt".into(),
        richting: openaec_sync::voortgang::Richting::Upload,
        gedaan: 8,
        totaal: 8
    }));
    let r2 = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r2.geupload, r2.gedownload), (0, 0));
}

#[tokio::test]
async fn groepen_blijven_onder_het_aantal_bytes() {
    let opslag = met_grenzen(Batchgrenzen { max_bestand: 100, max_aantal: 64, max_bytes: 250 });
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        schrijf(dir.path(), &format!("{i}.bin"), &[1u8; 100]);
    }
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    // 2 + 2 groepen van 200 bytes; de laatste alleen gaat los.
    assert_eq!(opslag.groepen_up.lock().unwrap().clone(), vec![2, 2]);
    assert_eq!(opslag.los.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn kleine_downloads_gaan_in_groepen() {
    let opslag = met_grenzen(GRENZEN);
    for i in 0..70 {
        opslag.nep.zet(&format!("Server/{i}.txt"), format!("van de server {i}").as_bytes());
    }
    opslag.nep.zet("Server/groot.bin", &[1u8; 300]);
    let dir = tempfile::tempdir().unwrap();
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.gedownload, 71);
    let mut groepen = opslag.groepen_down.lock().unwrap().clone();
    groepen.sort();
    assert_eq!(groepen, vec![6, 64]);
    assert_eq!(opslag.los.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read(dir.path().join("Server/42.txt")).unwrap(), b"van de server 42");
    let r2 = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r2.geupload, r2.gedownload), (0, 0));
}

#[tokio::test]
async fn zonder_grenzen_gaat_alles_los_zoals_bij_een_oude_server() {
    let opslag = BatchOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    for i in 0..10 {
        schrijf(dir.path(), &format!("{i}.txt"), b"x");
    }
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert!(opslag.groepen_up.lock().unwrap().is_empty());
    assert_eq!(opslag.los.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn mislukt_het_verzoek_als_geheel_dan_gaat_elk_bestand_apart() {
    let opslag = BatchOpslag { grenzen: Some(GRENZEN), weiger_batch: true, ..Default::default() };
    opslag.nep.zet("van/server.txt", b"s");
    opslag.nep.zet("van/server2.txt", b"s2");
    let dir = tempfile::tempdir().unwrap();
    for i in 0..10 {
        schrijf(dir.path(), &format!("{i}.txt"), b"x");
    }
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.gedownload, r.fouten.len()), (10, 2, 0));
    assert_eq!(opslag.los.load(Ordering::SeqCst), 12);
}

#[tokio::test]
async fn een_weigering_in_een_groep_raakt_alleen_dat_bestand() {
    let opslag = met_grenzen(GRENZEN);
    opslag.nep.weiger("3.txt");
    let dir = tempfile::tempdir().unwrap();
    for i in 0..6 {
        schrijf(dir.path(), &format!("{i}.txt"), b"x");
    }
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.foutpaden, vec!["3.txt"]);
    assert_eq!(r.geupload, 5);
    let mut bijgewerkt = r.bijgewerkt.clone();
    bijgewerkt.sort();
    assert_eq!(bijgewerkt, vec!["0.txt", "1.txt", "2.txt", "4.txt", "5.txt"]);
}

#[tokio::test]
async fn conflict_in_een_groep_zoals_bij_een_losse_upload() {
    let opslag = met_grenzen(GRENZEN);
    let dir = tempfile::tempdir().unwrap();
    for n in ["a", "b", "c"] {
        schrijf(dir.path(), &format!("{n}.txt"), b"een");
    }
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    for n in ["a", "b", "c"] {
        opslag.nep.zet(&format!("{n}.txt"), b"van collega");
        schrijf(dir.path(), &format!("{n}.txt"), b"van mij");
    }
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.conflicten, 3);
    for n in ["a", "b", "c"] {
        assert_eq!(std::fs::read(dir.path().join(format!("{n}.txt"))).unwrap(), b"van collega");
        assert_eq!(std::fs::read(dir.path().join(format!("{n} (conflict test).txt"))).unwrap(), b"van mij");
    }
    assert_eq!(sync_eenmalig(&opslag, dir.path()).await.unwrap().bijgewerkt, Vec::<String>::new());
}
