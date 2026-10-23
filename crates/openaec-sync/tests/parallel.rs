//! Parallelle overdrachten in de sync-motor: begrensd aantal tegelijk, mappen vóór de uploads, verwijderen na de
//! uploads, en meldingen die kloppen.

mod common;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use common::{schrijf, NepOpslag};
use openaec_sync::motor::{sync_eenmalig, sync_met_opties, Opslag, OpslagBestand, Opties, Upload};
use openaec_sync::voortgang::{Actie, Gebeurtenis, Waarnemer};
use openaec_sync::Fout;

/// Opslag die elke overdracht even laat duren en bijhoudt hoeveel er tegelijk lopen en in welke volgorde alles gebeurt.
#[derive(Default)]
struct TrageOpslag {
    nep: NepOpslag,
    nu: AtomicUsize,
    hoogste: AtomicUsize,
    volgorde: Mutex<Vec<String>>,
}

impl TrageOpslag {
    async fn bezig<T>(&self, wat: String, werk: impl FnOnce() -> T) -> T {
        let nu = self.nu.fetch_add(1, Ordering::SeqCst) + 1;
        self.hoogste.fetch_max(nu, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        let uit = werk();
        self.volgorde.lock().unwrap().push(wat);
        self.nu.fetch_sub(1, Ordering::SeqCst);
        uit
    }

    fn volgorde(&self) -> Vec<String> {
        self.volgorde.lock().unwrap().clone()
    }
}

impl Opslag for TrageOpslag {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        self.nep.lijst().await
    }
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.bezig(format!("upload {pad}"), || {
            futures_util::FutureExt::now_or_never(self.nep.upload(pad, lokaal, basis_versie)).unwrap()
        })
        .await
    }
    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.bezig(format!("download {}", bestand.pad), || {
            futures_util::FutureExt::now_or_never(self.nep.download(bestand, lokaal)).unwrap()
        })
        .await
    }
    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.bezig(format!("verwijder {}", bestand.pad), || {
            futures_util::FutureExt::now_or_never(self.nep.verwijder(bestand)).unwrap()
        })
        .await
    }
    async fn bereid_mappen(&self, mappen: &[String]) -> Result<(), Fout> {
        self.volgorde.lock().unwrap().push(format!("mappen {}", mappen.join(",")));
        Ok(())
    }
}

#[derive(Default)]
struct Verzamelaar(Mutex<Vec<Gebeurtenis>>);

impl Waarnemer for Verzamelaar {
    fn melding(&self, g: Gebeurtenis) {
        self.0.lock().unwrap().push(g);
    }
}

#[tokio::test]
async fn overdrachten_lopen_parallel_binnen_de_limiet() {
    let opslag = TrageOpslag::default();
    for i in 0..6 {
        opslag.nep.zet(&format!("server/{i}.txt"), b"s");
    }
    let dir = tempfile::tempdir().unwrap();
    for i in 0..10 {
        schrijf(dir.path(), &format!("hier/{i}.txt"), b"lokaal");
    }
    let w = Verzamelaar::default();
    let begin = std::time::Instant::now();
    let r = sync_met_opties(&opslag, dir.path(), &[], &w, &Opties { gelijktijdig: 4, ..Default::default() }).await.unwrap();
    assert_eq!((r.geupload, r.gedownload), (10, 6));
    assert_eq!(opslag.hoogste.load(Ordering::SeqCst), 4, "precies de limiet, niet meer");
    // 16 overdrachten van 30 ms, 4 tegelijk: ruim onder de 480 ms die het na elkaar zou duren.
    assert!(begin.elapsed() < Duration::from_millis(400), "duurde {:?}", begin.elapsed());

    // Elke overdracht een start en een klaar; de ronde vooraf telt alles.
    let m = w.0.lock().unwrap().clone();
    assert_eq!(m[0], Gebeurtenis::Ronde { uploaden: 10, downloaden: 6, bytes: 66 });
    assert_eq!(m.iter().filter(|g| matches!(g, Gebeurtenis::Start { .. })).count(), 16);
    assert_eq!(m.iter().filter(|g| matches!(g, Gebeurtenis::Klaar { .. })).count(), 16);
    for i in 0..10 {
        let pad = format!("hier/{i}.txt");
        let start = m.iter().position(|g| matches!(g, Gebeurtenis::Start { pad: p, .. } if *p == pad)).unwrap();
        let klaar = m.iter().position(|g| matches!(g, Gebeurtenis::Klaar { pad: p, actie: Actie::Geupload } if *p == pad)).unwrap();
        assert!(start < klaar);
    }
    assert_eq!(sync_eenmalig(&opslag, dir.path()).await.unwrap().bijgewerkt, Vec::<String>::new());
}

#[tokio::test]
async fn een_tegelijk_blijft_mogelijk() {
    let opslag = TrageOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    for i in 0..4 {
        schrijf(dir.path(), &format!("{i}.txt"), b"x");
    }
    let opties = Opties { gelijktijdig: 1, ..Default::default() };
    sync_met_opties(&opslag, dir.path(), &[], &openaec_sync::voortgang::Stil, &opties).await.unwrap();
    assert_eq!(opslag.hoogste.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn mappen_voor_de_uploads_en_verwijderen_na_de_uploads() {
    let opslag = TrageOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "oud/plan.pdf", b"plan");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    opslag.volgorde.lock().unwrap().clear();

    // Lokaal verplaatst (hernoemd naar een andere map) en een nieuw bestand diep in een nieuwe map.
    std::fs::create_dir_all(dir.path().join("nieuw")).unwrap();
    std::fs::rename(dir.path().join("oud/plan.pdf"), dir.path().join("nieuw/plan.pdf")).unwrap();
    schrijf(dir.path(), "A/B/C/diep.txt", b"diep");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.verwijderd), (2, 1));

    let v = opslag.volgorde();
    assert_eq!(v[0], "mappen A/B/C,nieuw", "eerst de mappen: {v:?}");
    let upload = v.iter().position(|s| s == "upload nieuw/plan.pdf").unwrap();
    let weg = v.iter().position(|s| s == "verwijder oud/plan.pdf").unwrap();
    assert!(upload < weg, "verwijderen pas na de upload: {v:?}");
    assert!(v.iter().position(|s| s == "upload A/B/C/diep.txt").unwrap() < weg);
    assert_eq!(opslag.nep.paden(), vec!["A/B/C/diep.txt", "nieuw/plan.pdf"]);
}

#[tokio::test]
async fn fouten_per_bestand_stoppen_de_parallelle_rest_niet() {
    let opslag = TrageOpslag::default();
    opslag.nep.weiger("2.txt");
    opslag.nep.weiger("5.txt");
    let dir = tempfile::tempdir().unwrap();
    for i in 0..8 {
        schrijf(dir.path(), &format!("{i}.txt"), b"x");
    }
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    let mut fout = r.foutpaden.clone();
    fout.sort();
    assert_eq!(fout, vec!["2.txt", "5.txt"]);
    assert_eq!(r.geupload, 6);
    // De mislukte komen de volgende ronde opnieuw aan bod; de geslaagde niet.
    let r2 = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r2.geupload, r2.fouten.len()), (0, 2));
}

#[tokio::test]
async fn conflict_blijft_gelijk_bij_parallel_werken() {
    let opslag = TrageOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    for n in ["a.txt", "b.txt", "c.txt"] {
        schrijf(dir.path(), n, b"een");
    }
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    for n in ["a.txt", "b.txt", "c.txt"] {
        opslag.nep.zet(n, b"van collega");
        schrijf(dir.path(), n, b"van mij");
    }
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.conflicten, 3);
    for n in ["a", "b", "c"] {
        assert_eq!(std::fs::read(dir.path().join(format!("{n}.txt"))).unwrap(), b"van collega");
        assert_eq!(std::fs::read(dir.path().join(format!("{n} (conflict test).txt"))).unwrap(), b"van mij");
    }
    let r2 = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r2.geupload, r2.gedownload, r2.conflicten), (0, 0, 0));
}
