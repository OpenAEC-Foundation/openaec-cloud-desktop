//! Overstappen: een bestaande map in één richting naar een ruimte uploaden, hervatbaar.

mod common;

use common::{schrijf, NepOpslag};
use openaec_sync::importeer::{importeer, telt_mee_voor_import};

#[tokio::test]
async fn hele_map_met_submappen_gaat_naar_de_ruimte() {
    let opslag = NepOpslag::default();
    let bron = tempfile::tempdir().unwrap();
    schrijf(bron.path(), "Projecten/2026/plan.pdf", b"pdf");
    schrijf(bron.path(), "Projecten/2026/model.ifc", b"ifc");
    schrijf(bron.path(), "notities.txt", b"txt");
    let r = importeer(&opslag, bron.path(), "", 4, |_| {}).await.unwrap();
    assert_eq!((r.geupload, r.overgeslagen, r.fouten.len()), (3, 0, 0));
    assert_eq!(r.bytes, 9);
    assert_eq!(opslag.inhoud("Projecten/2026/model.ifc").unwrap(), b"ifc");
}

#[tokio::test]
async fn in_een_doelmap_en_systeembestanden_blijven_achter() {
    let opslag = NepOpslag::default();
    let bron = tempfile::tempdir().unwrap();
    schrijf(bron.path(), "a.txt", b"a");
    schrijf(bron.path(), "Thumbs.db", b"x");
    schrijf(bron.path(), "sub/desktop.ini", b"x");
    schrijf(bron.path(), "sub/~$rapport.docx", b"x");
    schrijf(bron.path(), "sub/werk.tmp", b"x");
    schrijf(bron.path(), ".cache/iets.bin", b"x");
    schrijf(bron.path(), "sub/.verborgen", b"x");
    let r = importeer(&opslag, bron.path(), "Oud archief", 2, |_| {}).await.unwrap();
    assert_eq!(r.geupload, 1);
    assert_eq!(opslag.paden(), vec!["Oud archief/a.txt".to_string()]);
}

#[tokio::test]
async fn opnieuw_starten_slaat_over_en_gewijzigd_wordt_nieuwe_versie() {
    let opslag = NepOpslag::default();
    let bron = tempfile::tempdir().unwrap();
    schrijf(bron.path(), "a.txt", b"een");
    schrijf(bron.path(), "b.txt", b"twee");
    importeer(&opslag, bron.path(), "", 2, |_| {}).await.unwrap();
    schrijf(bron.path(), "b.txt", b"twee, langer");
    let r = importeer(&opslag, bron.path(), "", 2, |_| {}).await.unwrap();
    assert_eq!((r.geupload, r.overgeslagen), (1, 1));
    assert_eq!(opslag.inhoud("b.txt").unwrap(), b"twee, langer");
    assert_eq!(opslag.paden(), vec!["a.txt".to_string(), "b.txt".to_string()]);
    assert_eq!(opslag.bestanden.lock().unwrap()["b.txt"].1, 2);
}

#[tokio::test]
async fn voortgang_meldt_elk_bestand() {
    let opslag = NepOpslag::default();
    let bron = tempfile::tempdir().unwrap();
    for i in 0..5 {
        schrijf(bron.path(), &format!("f{i}.txt"), b"x");
    }
    let mut gezien = 0;
    importeer(&opslag, bron.path(), "", 3, |_| gezien += 1).await.unwrap();
    assert_eq!(gezien, 5);
}

#[test]
fn filter_voor_systeem_en_tijdelijke_bestanden() {
    for naam in ["Thumbs.db", "desktop.ini", ".DS_Store", "~$brief.docx", "x.tmp", ".verborgen"] {
        assert!(!telt_mee_voor_import(naam), "{naam} moet overgeslagen worden");
    }
    for naam in ["plan.pdf", "model.ifc", "Notities 2026.txt", "a.tmp.pdf"] {
        assert!(telt_mee_voor_import(naam), "{naam} moet mee");
    }
}
