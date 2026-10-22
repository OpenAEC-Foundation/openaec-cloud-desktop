//! Scenario's van de sync-motor tegen een nagebootste opslag in het geheugen.

mod common;

use common::{schrijf, NepOpslag};
use openaec_sync::motor::sync_eenmalig;

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

// --- mapselectie (selectieve sync) ---------------------------------------------------------------

use openaec_sync::motor::sync_met_selectie;

#[tokio::test]
async fn uitgesloten_map_wordt_niet_gedownload() {
    let opslag = NepOpslag::default();
    opslag.zet("Archief/oud.pdf", b"oud");
    opslag.zet("Actueel/plan.pdf", b"plan");
    let dir = tempfile::tempdir().unwrap();
    let r = sync_met_selectie(&opslag, dir.path(), &["Archief".to_string()]).await.unwrap();
    assert_eq!(r.gedownload, 1);
    assert!(dir.path().join("Actueel/plan.pdf").exists());
    assert!(!dir.path().join("Archief").exists());
}

#[tokio::test]
async fn uitvinken_haalt_de_lokale_kopie_weg_maar_niet_die_op_de_server() {
    let opslag = NepOpslag::default();
    opslag.zet("Archief/2024/oud.pdf", b"oud");
    let dir = tempfile::tempdir().unwrap();
    sync_met_selectie(&opslag, dir.path(), &[]).await.unwrap();
    assert!(dir.path().join("Archief/2024/oud.pdf").exists());
    sync_met_selectie(&opslag, dir.path(), &["Archief".to_string()]).await.unwrap();
    assert!(!dir.path().join("Archief").exists(), "lege map moet ook weg");
    assert_eq!(opslag.inhoud("Archief/2024/oud.pdf").unwrap(), b"oud");
    // weer aanvinken: komt terug
    let r = sync_met_selectie(&opslag, dir.path(), &[]).await.unwrap();
    assert_eq!(r.gedownload, 1);
    assert!(dir.path().join("Archief/2024/oud.pdf").exists());
}

#[tokio::test]
async fn lokaal_werk_in_een_uitgesloten_map_gaat_niet_verloren() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "Archief/nieuw.txt", b"belangrijk");
    let r = sync_met_selectie(&opslag, dir.path(), &["Archief".to_string()]).await.unwrap();
    assert_eq!(r.geupload, 1);
    assert_eq!(opslag.inhoud("Archief/nieuw.txt").unwrap(), b"belangrijk");
}

#[tokio::test]
async fn alleen_hele_mapnamen_tellen() {
    let opslag = NepOpslag::default();
    opslag.zet("Archief 2/a.txt", b"a");
    let dir = tempfile::tempdir().unwrap();
    let r = sync_met_selectie(&opslag, dir.path(), &["Archief".to_string()]).await.unwrap();
    assert_eq!(r.gedownload, 1, "'Archief 2' is een andere map dan 'Archief'");
}

// --- paden per ronde (statusvinkjes in de Verkenner) ---------------------------------------------

use openaec_sync::motor::bekende_paden;

fn gesorteerd(mut paden: Vec<String>) -> Vec<String> {
    paden.sort();
    paden
}

#[tokio::test]
async fn rapport_noemt_de_geuploade_en_gedownloade_paden() {
    let opslag = NepOpslag::default();
    opslag.zet("Model/gebouw.ifc", b"ifc");
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "Tekeningen/plan.pdf", b"pdf");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(gesorteerd(r.bijgewerkt), vec!["Model/gebouw.ifc", "Tekeningen/plan.pdf"]);
    assert!(r.foutpaden.is_empty());
    // niets veranderd: niets bijgewerkt
    let r2 = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert!(r2.bijgewerkt.is_empty());
    // alleen het gewijzigde bestand
    schrijf(dir.path(), "Tekeningen/plan.pdf", b"pdf v2");
    let r3 = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r3.bijgewerkt, vec!["Tekeningen/plan.pdf"]);
}

#[tokio::test]
async fn rapport_noemt_bij_een_conflict_beide_versies() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    opslag.zet("a.txt", b"van collega");
    schrijf(dir.path(), "a.txt", b"van mij");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(gesorteerd(r.bijgewerkt), vec!["a (conflict test).txt", "a.txt"]);
}

#[tokio::test]
async fn fout_bij_een_bestand_komt_in_de_foutpaden_en_de_rest_gaat_door() {
    let opslag = NepOpslag::default();
    opslag.weiger("kapot.txt");
    opslag.weiger("Server/kapot.ifc");
    opslag.zet("Server/kapot.ifc", b"ifc");
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "kapot.txt", b"x");
    schrijf(dir.path(), "goed.txt", b"y");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(gesorteerd(r.foutpaden), vec!["Server/kapot.ifc", "kapot.txt"]);
    assert_eq!(r.fouten.len(), 2);
    assert_eq!(r.bijgewerkt, vec!["goed.txt"]);
}

#[tokio::test]
async fn bekende_paden_zijn_alle_bestanden_die_in_sync_zijn() {
    let opslag = NepOpslag::default();
    opslag.weiger("kapot.txt");
    let dir = tempfile::tempdir().unwrap();
    assert!(bekende_paden(dir.path()).is_empty(), "nog nooit gesynchroniseerd");
    schrijf(dir.path(), "a.txt", b"a");
    schrijf(dir.path(), "Map/b.txt", b"b");
    schrijf(dir.path(), "kapot.txt", b"x");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(gesorteerd(bekende_paden(dir.path())), vec!["Map/b.txt", "a.txt"]);
}
