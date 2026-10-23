//! Lastige namen in de sync-motor: overslaan zonder foutlus, opruimen, onder een veilige naam uploaden, een naam van
//! de server hier onder een veilige naam zetten (en terug), en de negeerlijst. De bestanden worden via
//! `namen::lang_pad` gemaakt en gelezen: op Windows kan een bestand als `nul` alleen zo.

mod common;

use std::path::{Path, PathBuf};

use common::{schrijf, NepOpslag};
use openaec_sync::motor::{
    bekend_bestand, bekende_paden, overgeslagen, ruim_op, sync_eenmalig, sync_met_opties, Gereserveerd, Opties, Rapport,
};
use openaec_sync::namen::lang_pad;
use openaec_sync::voortgang::Stil;

fn pad(dir: &Path, rel: &str) -> PathBuf {
    lang_pad(&rel.split('/').fold(dir.to_path_buf(), |p, d| p.join(d)))
}

/// Een bestand met een (mogelijk lastige) naam, met een wijzigingstijd die zichtbaar verschilt van de vorige.
fn maak(dir: &Path, rel: &str, inhoud: &[u8]) {
    let p = pad(dir, rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, inhoud).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    std::fs::File::options().write(true).open(&p).unwrap().set_modified(later).unwrap();
}

fn lees(dir: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(pad(dir, rel)).unwrap()
}

fn bestaat(dir: &Path, rel: &str) -> bool {
    pad(dir, rel).exists()
}

fn opties(gereserveerd: Gereserveerd) -> Opties {
    Opties { gereserveerd, lokale_naamregels: true, ..Default::default() }
}

async fn ronde(opslag: &NepOpslag, dir: &Path, opties: &Opties) -> Rapport {
    sync_met_opties(opslag, dir, &[], &Stil, opties).await.unwrap()
}

fn overgeslagen_paden(r: &Rapport) -> Vec<String> {
    let mut p: Vec<String> = r.overgeslagen.iter().map(|o| o.pad.clone()).collect();
    p.sort();
    p
}

#[tokio::test]
async fn lastige_namen_worden_overgeslagen_zonder_fout_en_zonder_opnieuw_proberen() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    maak(dir.path(), "nul", b"");
    maak(dir.path(), "Map/con.txt", b"log");
    maak(dir.path(), "rapport.", b"punt");
    maak(dir.path(), "aux/binnen.txt", b"in een map met een lastige naam");
    maak(dir.path(), "goed.txt", b"goed");
    let o = opties(Gereserveerd::Overslaan);
    for _ in 0..2 {
        let r = ronde(&opslag, dir.path(), &o).await;
        assert!(r.fouten.is_empty(), "{:?}", r.fouten);
        assert_eq!(overgeslagen_paden(&r), vec!["Map/con.txt", "aux/binnen.txt", "nul", "rapport."]);
        let nul = r.overgeslagen.iter().find(|x| x.pad == "nul").unwrap();
        assert!(nul.reden.contains("gereserveerde naam") && nul.opruimbaar);
        assert!(r.overgeslagen.iter().find(|x| x.pad == "rapport.").unwrap().reden.contains("punt of spatie"));
    }
    assert_eq!(opslag.paden(), vec!["goed.txt"]);
    assert!(bestaat(dir.path(), "nul"), "overslaan laat het bestand staan");
    assert_eq!(overgeslagen(dir.path()).len(), 4);
}

#[tokio::test]
async fn opruimen_haalt_alleen_overgeslagen_bestanden_met_een_lastige_naam_weg() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    maak(dir.path(), "nul", b"");
    maak(dir.path(), "goed.txt", b"goed");
    ronde(&opslag, dir.path(), &opties(Gereserveerd::Overslaan)).await;
    let uit = ruim_op(dir.path(), &["nul".into(), "goed.txt".into()], None);
    assert!(uit[0].1.is_ok());
    assert!(uit[1].1.is_err(), "een gewoon bestand ruimt Opruimen niet op");
    assert!(!bestaat(dir.path(), "nul"));
    assert!(bestaat(dir.path(), "goed.txt"));
    assert!(overgeslagen(dir.path()).is_empty());
}

#[tokio::test]
async fn automatisch_opruimen_alleen_onder_de_grens() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    maak(dir.path(), "nul", b"");
    maak(dir.path(), "Map/con.txt", b"0123456789");
    maak(dir.path(), "aux.dat", &vec![1u8; 70 * 1024]);
    let o = Opties { prullenbak: Some(|_| false), ..opties(Gereserveerd::Verwijderen) };
    let r = ronde(&opslag, dir.path(), &o).await;
    assert!(!bestaat(dir.path(), "nul") && !bestaat(dir.path(), "Map/con.txt"));
    assert!(bestaat(dir.path(), "aux.dat"));
    assert_eq!(overgeslagen_paden(&r), vec!["aux.dat"]);
    assert!(r.overgeslagen[0].reden.contains("te groot"));
    assert_eq!(r.meldingen.len(), 2);
    assert!(r.meldingen.iter().all(|m| m.contains("opgeruimd")));
    assert!(r.fouten.is_empty() && opslag.paden().is_empty());
}

#[tokio::test]
async fn onder_een_veilige_naam_uploaden_en_terug() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    maak(dir.path(), "nul", b"uitvoer");
    maak(dir.path(), "Map/con.log", b"log");
    let o = opties(Gereserveerd::VeiligeNaam);
    let r = ronde(&opslag, dir.path(), &o).await;
    assert_eq!((r.geupload, r.fouten.len()), (2, 0));
    assert_eq!(opslag.paden(), vec!["Map/con_.log", "nul_"]);
    assert_eq!(opslag.inhoud("nul_").unwrap(), b"uitvoer");
    assert!(r.meldingen.iter().any(|m| m.contains("nul_")));
    let mut bijgewerkt = r.bijgewerkt.clone();
    bijgewerkt.sort();
    assert_eq!(bijgewerkt, vec!["Map/con.log", "nul"], "het rapport noemt de paden hier");
    // Tweede ronde: niets, en geen dubbele upload.
    let r = ronde(&opslag, dir.path(), &o).await;
    assert_eq!((r.geupload, r.gedownload, r.meldingen.len()), (0, 0, 0));
    // Lokaal gewijzigd: een nieuwe versie van nul_.
    maak(dir.path(), "nul", b"nieuwe uitvoer");
    assert_eq!(ronde(&opslag, dir.path(), &o).await.geupload, 1);
    assert_eq!(opslag.inhoud("nul_").unwrap(), b"nieuwe uitvoer");
    assert_eq!(opslag.paden().len(), 2);
    // Op de server gewijzigd: komt hier in nul.
    opslag.zet("nul_", b"van de server");
    assert_eq!(ronde(&opslag, dir.path(), &o).await.gedownload, 1);
    assert_eq!(lees(dir.path(), "nul"), b"van de server");
    // Lokaal weg: op de server weg.
    std::fs::remove_file(pad(dir.path(), "nul")).unwrap();
    assert_eq!(ronde(&opslag, dir.path(), &o).await.verwijderd, 1);
    assert_eq!(opslag.paden(), vec!["Map/con_.log"]);
}

#[tokio::test]
async fn naam_van_de_server_komt_hier_onder_een_veilige_naam_en_gaat_terug_naar_het_origineel() {
    let opslag = NepOpslag::default();
    opslag.zet("nul", b"van linux");
    opslag.zet("Map/aux.txt", b"aux");
    opslag.zet("rapport.", b"punt");
    let dir = tempfile::tempdir().unwrap();
    let o = opties(Gereserveerd::Overslaan);
    let r = ronde(&opslag, dir.path(), &o).await;
    assert_eq!((r.gedownload, r.fouten.len()), (3, 0), "{:?}", r.fouten);
    assert_eq!(lees(dir.path(), "nul_"), b"van linux");
    assert_eq!(lees(dir.path(), "Map/aux_.txt"), b"aux");
    assert_eq!(lees(dir.path(), "rapport_"), b"punt");
    assert!(!bestaat(dir.path(), "nul"));
    let mut bekend = bekende_paden(dir.path());
    bekend.sort();
    assert_eq!(bekend, vec!["Map/aux_.txt", "nul_", "rapport_"]);
    let id = opslag.bestanden.lock().unwrap()["nul"].0.clone();
    assert_eq!(bekend_bestand(dir.path(), "nul_").unwrap().0, id);

    // Tweede ronde: niets te doen, geen dubbele uploads.
    let r = ronde(&opslag, dir.path(), &o).await;
    assert_eq!((r.geupload, r.gedownload, r.verwijderd), (0, 0, 0));
    assert_eq!(opslag.paden().len(), 3);
    // Hier gewijzigd: een nieuwe versie van het origineel.
    maak(dir.path(), "nul_", b"van windows");
    assert_eq!(ronde(&opslag, dir.path(), &o).await.geupload, 1);
    assert_eq!(opslag.inhoud("nul").unwrap(), b"van windows");
    assert_eq!(opslag.bestanden.lock().unwrap()["nul"].1, 2);
    assert_eq!(opslag.paden().len(), 3);
    // Hier verwijderd: het origineel naar de prullenbak.
    std::fs::remove_file(pad(dir.path(), "Map/aux_.txt")).unwrap();
    assert_eq!(ronde(&opslag, dir.path(), &o).await.verwijderd, 1);
    assert_eq!(opslag.paden(), vec!["nul", "rapport."]);
}

#[tokio::test]
async fn bezette_veilige_naam_slaat_het_serverbestand_over() {
    let opslag = NepOpslag::default();
    opslag.zet("con", b"van de server");
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "con_", b"van mij");
    let r = ronde(&opslag, dir.path(), &opties(Gereserveerd::Overslaan)).await;
    assert_eq!(overgeslagen_paden(&r), vec!["con"]);
    assert!(!r.overgeslagen[0].opruimbaar, "staat op de server, niet hier");
    assert_eq!(lees(dir.path(), "con_"), b"van mij");
    assert_eq!(opslag.inhoud("con").unwrap(), b"van de server");
    assert_eq!(opslag.inhoud("con_").unwrap(), b"van mij");
}

#[tokio::test]
async fn waar_de_naam_hier_wel_kan_blijft_een_bestaand_serverbestand_gewoon_meedoen() {
    // Zoals op Linux: `nul` kan daar, en staat al op de server (van vóór de naamregels).
    let opslag = NepOpslag::default();
    opslag.zet("nul", b"oud");
    let dir = tempfile::tempdir().unwrap();
    let o = Opties { lokale_naamregels: false, ..Default::default() };
    assert_eq!(ronde(&opslag, dir.path(), &o).await.gedownload, 1);
    assert_eq!(lees(dir.path(), "nul"), b"oud");
    maak(dir.path(), "nul", b"nieuw");
    // Een nieuw bestand met een lastige naam gaat ook hier niet naar de server (die zou het weigeren).
    maak(dir.path(), "aux", b"nieuw bestand");
    let r = ronde(&opslag, dir.path(), &o).await;
    assert_eq!((r.geupload, r.fouten.len()), (1, 0));
    assert_eq!(overgeslagen_paden(&r), vec!["aux"]);
    assert_eq!(opslag.inhoud("nul").unwrap(), b"nieuw");
}

#[tokio::test]
async fn negeerlijst_standaard_en_eigen_patronen() {
    let opslag = NepOpslag::default();
    opslag.zet("Thumbs.db", b"server");
    opslag.zet("oud.bak", b"server");
    let dir = tempfile::tempdir().unwrap();
    for rel in ["~$rapport.docx", "werk.tmp", "Fotos/Thumbs.db", "Map/.DS_Store", "desktop.ini", "b.bak", "build/uit.o"] {
        schrijf(dir.path(), rel, b"tijdelijk");
    }
    schrijf(dir.path(), ".openaecignore", b"# eigen regels\n*.bak\nbuild/\n");
    schrijf(dir.path(), "plan.pdf", b"plan");
    schrijf(dir.path(), "notities.log", b"log");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.geupload, 3);
    assert_eq!(opslag.paden(), vec![".openaecignore", "Thumbs.db", "notities.log", "oud.bak", "plan.pdf"]);
    assert!(!dir.path().join("Thumbs.db").exists() && !dir.path().join("oud.bak").exists(), "niet gedownload");
    assert!(r.overgeslagen.is_empty(), "genegeerd is iets anders dan overgeslagen");

    // Een bestand dat al gesynchroniseerd was en nu genegeerd wordt: lokaal weg betekent niet weg op de server.
    schrijf(dir.path(), ".openaecignore", b"*.bak\nbuild/\n*.log\n");
    std::fs::remove_file(dir.path().join("notities.log")).unwrap();
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.verwijderd, 0);
    assert!(opslag.inhoud("notities.log").is_some());
}
