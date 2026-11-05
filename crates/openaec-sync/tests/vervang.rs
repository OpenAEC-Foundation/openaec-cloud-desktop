//! Veilig vervangen en verwijderen in een hele ronde (ontwerp samenwerken §5.6, punt 3 en 4): een programma met het
//! bestand open, een geheugenkoppeling, een geweigerde verplaatsing, een bestand dat vlak vóór de plaatsing verschijnt,
//! opslaan vlak vóór een lokale verwijdering, en een wijziging die alleen in een bron gebeurt. De stappen van één
//! vervanging, met onderbrekingen ertussen, staan in `src/vervang.rs`.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use common::{schrijf, NepOpslag};
use openaec_sync::motor::{bekend_bestand, sync_eenmalig, Batchgrenzen, Opslag, OpslagBestand, Upload};
use openaec_sync::Fout;

fn lees(root: &Path, pad: &str) -> Option<String> {
    std::fs::read_to_string(root.join(pad)).ok()
}

fn bronnen(root: &Path) -> usize {
    std::fs::read_dir(root.join(".openaec/bron")).map(|d| d.count()).unwrap_or(0)
}

fn journaal(root: &Path) -> usize {
    std::fs::read_dir(root.join(".openaec/journaal")).map(|d| d.count()).unwrap_or(0)
}

/// a.txt ("een") staat hier en op de server, in sync; daarna zet een collega "van collega" op de server.
async fn gesynct_en_gewijzigd_op_de_server(opslag: &NepOpslag, root: &Path) {
    schrijf(root, "a.txt", b"een");
    sync_eenmalig(opslag, root).await.unwrap();
    opslag.zet("a.txt", b"van collega");
}

#[tokio::test]
async fn download_vervangt_en_laat_niets_achter() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    gesynct_en_gewijzigd_op_de_server(&opslag, dir.path()).await;
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.gedownload, 1);
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("van collega"));
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.gedownload, r.verwijderd), (0, 0, 0));
}

/// Proef: een programma schrijft na de inhoudscontrole via zijn al open verbinding en blijft open. De bron blijft
/// bewaakt zolang het open is; daarna gaat het werk als conflictkopie naar de server.
#[tokio::test]
async fn programma_schrijft_via_open_verbinding_en_blijft_open() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    gesynct_en_gewijzigd_op_de_server(&opslag, dir.path()).await;
    let mut programma = std::fs::OpenOptions::new().append(true).open(dir.path().join("a.txt")).unwrap();

    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("van collega"));
    assert_eq!(bronnen(dir.path()), 1, "het programma heeft de bron open");

    programma.write_all(b" + mijn werk").unwrap();
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.conflicten), (0, 0), "nog open: niets afgerond");
    assert_eq!(bronnen(dir.path()), 1);

    drop(programma);
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.conflicten, 1);
    assert_eq!(lees(dir.path(), "a (conflict test).txt").as_deref(), Some("een + mijn werk"));
    assert_eq!(opslag.inhoud("a (conflict test).txt").unwrap(), b"een + mijn werk");
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("van collega"));
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.gedownload, r.conflicten), (0, 0, 0));
}

/// Dezelfde proef met een echt ander proces dat het bestand open heeft.
#[cfg(unix)]
#[tokio::test]
async fn ander_proces_schrijft_via_open_verbinding() {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    gesynct_en_gewijzigd_op_de_server(&opslag, dir.path()).await;
    // Het proces opent het bestand (fd 3), meldt dat, schrijft na een regel op stdin en stopt na de volgende.
    let mut kind = Command::new("sh")
        .args(["-c", "exec 3>>\"$1\"; echo open; read x; printf ' + werk' >&3; echo geschreven; read y", "sh"])
        .arg(dir.path().join("a.txt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut uit = std::io::BufReader::new(kind.stdout.take().unwrap());
    let mut regel = String::new();
    uit.read_line(&mut regel).unwrap();

    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("van collega"));
    assert_eq!(bronnen(dir.path()), 1);

    let mut invoer = kind.stdin.take().unwrap();
    writeln!(invoer).unwrap();
    uit.read_line(&mut regel).unwrap();
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(bronnen(dir.path()), 1, "het proces is nog open");

    drop(invoer);
    kind.wait().unwrap();
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.conflicten, 1);
    assert_eq!(opslag.inhoud("a (conflict test).txt").unwrap(), b"een + werk");
    assert_eq!(bronnen(dir.path()), 0);
}

/// Opslag met twee haken: na de vergelijking (vóór alle overdrachten en verwijderingen) en bij het verwijderen op de
/// server (na alle downloads, vóór het afronden van de bronnen).
struct MetHaken<A: Fn() + Sync, B: Fn() + Sync> {
    nep: NepOpslag,
    na_vergelijking: A,
    bij_verwijderen: B,
}

impl<A: Fn() + Sync, B: Fn() + Sync> Opslag for MetHaken<A, B> {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        self.nep.lijst().await
    }
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.nep.upload(pad, lokaal, basis_versie).await
    }
    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.nep.download(bestand, lokaal).await
    }
    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        (self.bij_verwijderen)();
        self.nep.verwijder(bestand).await
    }
    async fn klein_grenzen(&self) -> Option<Batchgrenzen> {
        (self.na_vergelijking)();
        None
    }
}

/// Proef: een programma schrijft tussen de inhoudscontrole (bij het verplaatsen) en de procescontrole (bij het
/// afronden) en sluit, in dezelfde ronde. Het werk staat dan in de bron, en de laatste controle van de inhoud ziet het.
#[tokio::test]
async fn programma_schrijft_en_sluit_tussen_de_controles() {
    let programma: Mutex<Option<std::fs::File>> = Mutex::new(None);
    let opslag = MetHaken {
        nep: NepOpslag::default(),
        na_vergelijking: || {},
        bij_verwijderen: || {
            if let Some(mut f) = programma.lock().unwrap().take() {
                f.write_all(b" + mijn werk").unwrap();
            }
        },
    };
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "weg.txt", b"x");
    gesynct_en_gewijzigd_op_de_server(&opslag.nep, dir.path()).await;
    // weg.txt lokaal weg: in de ronde gaat het op de server naar de prullenbak, na de download van a.txt.
    std::fs::remove_file(dir.path().join("weg.txt")).unwrap();
    *programma.lock().unwrap() = Some(std::fs::OpenOptions::new().append(true).open(dir.path().join("a.txt")).unwrap());

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert!(programma.lock().unwrap().is_none(), "het programma schreef en sloot tijdens de ronde");
    assert_eq!((r.gedownload, r.verwijderd, r.conflicten), (1, 1, 1));
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("van collega"));
    assert_eq!(lees(dir.path(), "a (conflict test).txt").as_deref(), Some("een + mijn werk"));
    assert_eq!(opslag.nep.inhoud("a (conflict test).txt").unwrap(), b"een + mijn werk");
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
}

/// Proef: een schrijfbare geheugenkoppeling, ook als het programma het bestand zelf al gesloten heeft.
#[cfg(unix)]
#[tokio::test]
async fn schrijfbare_geheugenkoppeling() {
    use std::os::fd::AsRawFd;
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    gesynct_en_gewijzigd_op_de_server(&opslag, dir.path()).await;
    let f = std::fs::OpenOptions::new().read(true).write(true).open(dir.path().join("a.txt")).unwrap();
    let adres = unsafe { libc::mmap(std::ptr::null_mut(), 3, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, f.as_raw_fd(), 0) };
    assert_ne!(adres, libc::MAP_FAILED);
    drop(f);

    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("van collega"));
    assert_eq!(bronnen(dir.path()), 1, "de koppeling houdt de bron bewaakt");

    unsafe {
        std::ptr::copy_nonoverlapping(b"EEN".as_ptr(), adres.cast::<u8>(), 3);
        libc::msync(adres, 3, libc::MS_SYNC);
        libc::munmap(adres, 3);
    }
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.conflicten, 1);
    assert_eq!(opslag.inhoud("a (conflict test).txt").unwrap(), b"EEN");
    assert_eq!(bronnen(dir.path()), 0);
}

/// Proef: de eerste verplaatsing wordt geweigerd. De lokale inhoud en de toestand blijven, er blijft geen bron of
/// journaal achter, en de ronde meldt een fout voor dit bestand.
#[tokio::test]
async fn geweigerde_eerste_verplaatsing() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "map/a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    opslag.zet("map/a.txt", b"van collega");
    let voor = bekend_bestand(dir.path(), "map/a.txt").unwrap();
    let Some(_weigering) = Weigering::zet(&dir.path().join("map")) else { return };

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.gedownload, 0);
    assert_eq!(r.foutpaden, vec!["map/a.txt".to_string()]);
    assert_eq!(lees(dir.path(), "map/a.txt").as_deref(), Some("een"));
    assert_eq!(bekend_bestand(dir.path(), "map/a.txt"), Some(voor));
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
}

/// Een verplaatsing uit `map` weigeren zolang dit bestaat: Unix een map zonder schrijfrecht, Windows een programma
/// dat het bestand zonder FILE_SHARE_DELETE open heeft.
struct Weigering {
    #[cfg(unix)]
    map: PathBuf,
    #[cfg(windows)]
    _open: std::fs::File,
}

impl Weigering {
    #[cfg(unix)]
    fn zet(map: &Path) -> Option<Self> {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("overgeslagen: root mag altijd schrijven");
            return None;
        }
        std::fs::set_permissions(map, std::fs::Permissions::from_mode(0o555)).unwrap();
        Some(Self { map: map.to_path_buf() })
    }

    #[cfg(windows)]
    fn zet(map: &Path) -> Option<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE, zonder FILE_SHARE_DELETE.
        let open = std::fs::OpenOptions::new().read(true).write(true).share_mode(1 | 2).open(map.join("a.txt")).unwrap();
        Some(Self { _open: open })
    }
}

#[cfg(unix)]
impl Drop for Weigering {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.map, std::fs::Permissions::from_mode(0o755));
    }
}

/// Opslag die bij het downloaden ook een bestand van de gebruiker op het echte pad zet: het verschijnt vlak vóór de
/// plaatsing.
struct GebruikerSlaatOp {
    nep: NepOpslag,
    root: PathBuf,
    gedaan: AtomicBool,
}

impl Opslag for GebruikerSlaatOp {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        self.nep.lijst().await
    }
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.nep.upload(pad, lokaal, basis_versie).await
    }
    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.nep.download(bestand, lokaal).await?;
        if !self.gedaan.swap(true, Ordering::SeqCst) {
            schrijf(&self.root, &bestand.pad, b"van de gebruiker");
        }
        Ok(())
    }
    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.nep.verwijder(bestand).await
    }
}

/// Proef: een bestand dat vlak vóór de plaatsing op het pad verschijnt, blijft staan; de volgende ronde maakt er een
/// conflictkopie van, en niets gaat verloren.
#[tokio::test]
async fn bestand_dat_vlak_voor_de_plaatsing_verschijnt_blijft_staan() {
    let dir = tempfile::tempdir().unwrap();
    let opslag = GebruikerSlaatOp { nep: NepOpslag::default(), root: dir.path().to_path_buf(), gedaan: AtomicBool::new(false) };
    opslag.nep.zet("b.txt", b"van de server");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.gedownload, 0);
    assert_eq!(lees(dir.path(), "b.txt").as_deref(), Some("van de gebruiker"));

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.conflicten, 1);
    assert_eq!(opslag.nep.inhoud("b (conflict test).txt").unwrap(), b"van de gebruiker");
    assert_eq!(lees(dir.path(), "b.txt").as_deref(), Some("van de server"));
    assert_eq!(lees(dir.path(), "b (conflict test).txt").as_deref(), Some("van de gebruiker"));
}

/// Proef: de gebruiker slaat op na de scan en vóór de lokale verwijdering (een collega verwijderde het bestand). Het
/// nieuwe werk blijft staan en gaat daarna als nieuw bestand naar de server.
#[tokio::test]
async fn opslaan_na_de_scan_voor_een_lokale_verwijdering() {
    let dir = tempfile::tempdir().unwrap();
    let opslaan = AtomicBool::new(false);
    let opslag = MetHaken {
        nep: NepOpslag::default(),
        na_vergelijking: || {
            if opslaan.swap(false, Ordering::SeqCst) {
                schrijf(dir.path(), "a.txt", b"een + nieuw werk");
            }
        },
        bij_verwijderen: || {},
    };
    schrijf(dir.path(), "a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    opslag.nep.bestanden.lock().unwrap().remove("a.txt");
    opslaan.store(true, Ordering::SeqCst);

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert!(!opslaan.load(Ordering::SeqCst), "opgeslagen na de vergelijking");
    assert_eq!(r.verwijderd, 0);
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("een + nieuw werk"));
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.geupload, 1);
    assert_eq!(opslag.nep.inhoud("a.txt").unwrap(), b"een + nieuw werk");
}

/// Proef: een wijziging die alleen in een bron gebeurt, zonder gebeurtenis op het oorspronkelijke pad (dat is hier
/// zelfs weg: een collega verwijderde het bestand). De volgende ronde verwerkt haar: het werk gaat als nieuw bestand
/// naar de server en komt terug op het pad.
#[tokio::test]
async fn wijziging_alleen_in_de_bron_wordt_verwerkt() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    let mut programma = std::fs::OpenOptions::new().append(true).open(dir.path().join("a.txt")).unwrap();
    opslag.bestanden.lock().unwrap().remove("a.txt");

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.verwijderd, 1);
    assert_eq!(lees(dir.path(), "a.txt"), None);
    assert_eq!(bronnen(dir.path()), 1);

    programma.write_all(b" + werk").unwrap();
    drop(programma);
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.geupload, 1);
    assert_eq!(opslag.inhoud("a.txt").unwrap(), b"een + werk");
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("een + werk"));
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.gedownload, r.verwijderd), (0, 0, 0));
}

/// Een map die niet meer op deze computer hoort: ook die kopie gaat weg via de veilige routine, en werk dat er net
/// in kwam, gaat naar de server.
#[tokio::test]
async fn uitgevinkte_map_gaat_veilig_weg() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "Oud/a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    let r = openaec_sync::motor::sync_met_selectie(&opslag, dir.path(), &["Oud".to_string()]).await.unwrap();
    assert!(r.fouten.is_empty(), "{:?}", r.fouten);
    assert_eq!(lees(dir.path(), "Oud/a.txt"), None);
    assert_eq!(opslag.inhoud("Oud/a.txt").unwrap(), b"een");
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
}

/// Opslag die tijdens de eerste upload een nieuwe versie van het bestand opslaat (de gebruiker slaat op terwijl de
/// sync uploadt), en die alles bijhoudt wat er naar de server ging.
struct SlaatOpTijdensUpload {
    nep: NepOpslag,
    root: PathBuf,
    gedaan: AtomicBool,
    verstuurd: Mutex<Vec<Vec<u8>>>,
}

impl Opslag for SlaatOpTijdensUpload {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        self.nep.lijst().await
    }
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.verstuurd.lock().unwrap().push(std::fs::read(lokaal)?);
        let up = self.nep.upload(pad, lokaal, basis_versie).await?;
        if !self.gedaan.swap(true, Ordering::SeqCst) {
            schrijf(&self.root, pad, b"B: opgeslagen tijdens de upload");
        }
        Ok(up)
    }
    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.nep.download(bestand, lokaal).await
    }
    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.nep.verwijder(bestand).await
    }
}

/// Proef (review Sol, PR #8, punt 1): upload A; tijdens de upload slaat de gebruiker B op. De toestand mag B niet als
/// "op de server" vastleggen: de volgende ronde uploadt B. Daarna wijzigt een collega het bestand en haalt de sync
/// dat op; B staat dan als versie op de server en gaat niet verloren.
#[tokio::test]
async fn opslaan_tijdens_de_upload_gaat_niet_verloren() {
    let dir = tempfile::tempdir().unwrap();
    let opslag = SlaatOpTijdensUpload {
        nep: NepOpslag::default(),
        root: dir.path().to_path_buf(),
        gedaan: AtomicBool::new(false),
        verstuurd: Mutex::new(Vec::new()),
    };
    schrijf(dir.path(), "a.txt", b"A");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.geupload, 1);
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("B: opgeslagen tijdens de upload"));

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.geupload, 1, "B is een lokale wijziging ten opzichte van wat er geüpload is");
    opslag.nep.zet("a.txt", b"van collega");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();

    assert!(opslag.verstuurd.lock().unwrap().iter().any(|v| v == b"B: opgeslagen tijdens de upload"), "B staat als versie op de server");
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("van collega"));
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
}

/// Een map tijdelijk ontoegankelijk maken (Unix: geen rechten); terug bij `drop`.
#[cfg(unix)]
struct Ontoegankelijk(PathBuf);

#[cfg(unix)]
impl Ontoegankelijk {
    fn zet(map: &Path) -> Option<Self> {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("overgeslagen: root mag altijd lezen");
            return None;
        }
        std::fs::set_permissions(map, std::fs::Permissions::from_mode(0o000)).unwrap();
        Some(Self(map.to_path_buf()))
    }
}

#[cfg(unix)]
impl Drop for Ontoegankelijk {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

/// Proef (review Sol, PR #8, punt 3): een download vervangt een bestand dat een programma open heeft; het programma
/// schrijft nieuw werk via die verbinding en sluit. Dan is de bronmap één ronde niet te bekijken. Het journaal moet
/// blijven (met een fout voor dit bestand); als de map weer te bekijken is, gaat het werk naar de server.
#[cfg(unix)]
#[tokio::test]
async fn bron_die_even_niet_te_bekijken_is_blijft_bewaakt() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    gesynct_en_gewijzigd_op_de_server(&opslag, dir.path()).await;
    let mut programma = std::fs::OpenOptions::new().append(true).open(dir.path().join("a.txt")).unwrap();
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(bronnen(dir.path()), 1);
    programma.write_all(b" + werk").unwrap();
    drop(programma);

    {
        let Some(_dicht) = Ontoegankelijk::zet(&dir.path().join(".openaec/bron")) else { return };
        let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
        assert!(r.foutpaden.contains(&"a.txt".to_string()), "een fout voor dit bestand: {:?}", r.fouten);
        assert_eq!(journaal(dir.path()), 1, "het journaal blijft");
    }

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.conflicten, 1);
    assert_eq!(opslag.inhoud("a (conflict test).txt").unwrap(), b"een + werk");
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
}

/// Proef (review Sol, PR #8, punt 4): een map uitvinken; de sync stopt na de lokale verplaatsing en vóór het bewaren van
/// de toestand. Daarna de map weer aanvinken. Het bestand op de server mag niet naar de prullenbak (het lege pad is geen
/// lokale verwijdering), en komt hier terug.
#[tokio::test]
async fn uitvinken_onderbroken_voor_het_bewaren_verwijdert_niets_op_de_server() {
    use futures_util::FutureExt;
    let stoppen = AtomicBool::new(false);
    let opslag = MetHaken {
        nep: NepOpslag::default(),
        na_vergelijking: || {
            if stoppen.load(Ordering::SeqCst) {
                panic!("de sync stopt hier (proef)");
            }
        },
        bij_verwijderen: || {},
    };
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "Oud/a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();

    stoppen.store(true, Ordering::SeqCst);
    let uit = ["Oud".to_string()];
    let ronde = openaec_sync::motor::sync_met_selectie(&opslag, dir.path(), &uit);
    assert!(std::panic::AssertUnwindSafe(ronde).catch_unwind().await.is_err());
    stoppen.store(false, Ordering::SeqCst);
    assert_eq!(lees(dir.path(), "Oud/a.txt"), None, "verplaatst naar de bron");
    assert_eq!(journaal(dir.path()), 1);

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.verwijderd, 0);
    assert_eq!(opslag.nep.inhoud("Oud/a.txt").unwrap(), b"een", "het bestand staat nog op de server");
    assert_eq!(lees(dir.path(), "Oud/a.txt").as_deref(), Some("een"));
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
}

/// Opslag die offline kan zijn: dan mislukt het eerste verzoek van een ronde (de lijst). Met `stoppen` stopt de ronde
/// (panic) na de vergelijking, vóór alle overdrachten en vóór het bewaren van de toestand.
#[derive(Default)]
struct Offline {
    nep: NepOpslag,
    offline: AtomicBool,
    stoppen: AtomicBool,
}

impl Opslag for Offline {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        if self.offline.load(Ordering::SeqCst) {
            return Err(Fout::Controle("geen verbinding (proef)".into()));
        }
        self.nep.lijst().await
    }
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.nep.upload(pad, lokaal, basis_versie).await
    }
    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.nep.download(bestand, lokaal).await
    }
    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.nep.verwijder(bestand).await
    }
    async fn klein_grenzen(&self) -> Option<Batchgrenzen> {
        if self.stoppen.load(Ordering::SeqCst) {
            panic!("de sync stopt hier (proef)");
        }
        None
    }
}

/// Proef (review Sol ronde 2 op bbdfad1, punt 2): een map uitvinken; de sync stopt na de lokale verplaatsing en vóór het
/// bewaren van de toestand. Het herstel gebeurt terwijl de server offline is (de ronde stopt bij de lijst). Weer online,
/// met de map aangevinkt: niets naar de prullenbak op de server, en het bestand komt hier terug.
#[tokio::test]
async fn uitvinken_onderbroken_en_offline_hersteld_verwijdert_niets_op_de_server() {
    use futures_util::FutureExt;
    let opslag = Offline::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "Oud/a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();

    opslag.stoppen.store(true, Ordering::SeqCst);
    let uit = ["Oud".to_string()];
    let ronde = openaec_sync::motor::sync_met_selectie(&opslag, dir.path(), &uit);
    assert!(std::panic::AssertUnwindSafe(ronde).catch_unwind().await.is_err());
    opslag.stoppen.store(false, Ordering::SeqCst);
    assert_eq!(lees(dir.path(), "Oud/a.txt"), None, "verplaatst naar de bron");
    assert_eq!(journaal(dir.path()), 1);

    opslag.offline.store(true, Ordering::SeqCst);
    assert!(sync_eenmalig(&opslag, dir.path()).await.is_err(), "offline: de ronde stopt bij de lijst");
    opslag.offline.store(false, Ordering::SeqCst);

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.verwijderd, r.gedownload), (0, 1));
    assert_eq!(opslag.nep.inhoud("Oud/a.txt").unwrap(), b"een", "het bestand staat nog op de server");
    assert_eq!(lees(dir.path(), "Oud/a.txt").as_deref(), Some("een"));
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
}

/// Proef (review Sol ronde 2 op bbdfad1, punt 3): een gesynchroniseerd bestand is open in een programma; de map wordt
/// uitgevinkt en weer aangevinkt. Het bestand komt opnieuw binnen en is bekend. De oude bron (nog open) mag het daarna
/// niet nog eens laten vergeten: zonder wijziging van de gebruiker geen conflict, upload of download.
#[tokio::test]
async fn oude_verwijderingsbron_vergeet_een_opnieuw_opgehaald_bestand_niet() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "Oud/a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    let programma = std::fs::File::open(dir.path().join("Oud/a.txt")).unwrap();

    let uit = ["Oud".to_string()];
    openaec_sync::motor::sync_met_selectie(&opslag, dir.path(), &uit).await.unwrap();
    assert_eq!(lees(dir.path(), "Oud/a.txt"), None);
    assert_eq!(bronnen(dir.path()), 1, "het programma heeft de bron open");

    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.gedownload, 1, "weer aangevinkt: opnieuw opgehaald");
    for _ in 0..2 {
        let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
        assert_eq!((r.geupload, r.gedownload, r.conflicten, r.verwijderd), (0, 0, 0, 0), "{r:?}");
    }
    drop(programma);
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.gedownload, r.conflicten, r.verwijderd), (0, 0, 0, 0));
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
    assert_eq!(opslag.paden(), vec!["Oud/a.txt".to_string()]);
}

/// Proef (review Sol, PR #8, punt 4): een collega verwijdert het bestand; de sync verplaatst het hier naar een bron, maar
/// een programma heeft het open en schrijft er daarna nieuw werk in. Dan is de computer offline. Het werk komt toch
/// terug op het pad (zonder de server), en gaat naar de server zodra die er weer is.
#[tokio::test]
async fn werk_in_een_verwijderde_bron_komt_ook_offline_terug() {
    let opslag = Offline::default();
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "a.txt", b"een");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    let mut programma = std::fs::OpenOptions::new().append(true).open(dir.path().join("a.txt")).unwrap();
    opslag.nep.bestanden.lock().unwrap().remove("a.txt");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.verwijderd, 1);
    assert_eq!(bronnen(dir.path()), 1, "het programma heeft de bron open");

    programma.write_all(b" + werk").unwrap();
    drop(programma);
    opslag.offline.store(true, Ordering::SeqCst);
    assert!(sync_eenmalig(&opslag, dir.path()).await.is_err(), "offline: de ronde stopt bij de lijst");
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("een + werk"), "het werk staat weer op het pad");
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));

    opslag.offline.store(false, Ordering::SeqCst);
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.geupload, 1);
    assert_eq!(opslag.nep.inhoud("a.txt").unwrap(), b"een + werk");
}

/// Proef (review Sol ronde 3 op f0d144c, punt 2): een programma houdt versie 1 open. De sync vervangt het bestand door
/// versie 2; de oude bron blijft bewaakt. Daarna komt versie 3 binnen en wordt bewaard. Het oude journaal mag versie 2
/// niet terugboeken: zonder wijziging van de gebruiker geen upload, geen download en geen conflict.
#[tokio::test]
async fn oude_vervangingsbron_boekt_een_nieuwere_versie_niet_terug() {
    let opslag = NepOpslag::default();
    let dir = tempfile::tempdir().unwrap();
    opslag.zet("a.txt", b"versie 1");
    sync_eenmalig(&opslag, dir.path()).await.unwrap();
    let programma = std::fs::File::open(dir.path().join("a.txt")).unwrap();

    opslag.zet("a.txt", b"versie 2");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.gedownload, 1);
    assert_eq!(bronnen(dir.path()), 1, "het programma heeft versie 1 open");

    opslag.zet("a.txt", b"versie 3");
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!(r.gedownload, 1);
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("versie 3"));

    for _ in 0..2 {
        let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
        assert_eq!((r.geupload, r.gedownload, r.conflicten, r.verwijderd), (0, 0, 0, 0), "{r:?}");
    }
    assert_eq!(opslag.paden(), vec!["a.txt".to_string()], "geen conflictkopie");
    assert_eq!(opslag.inhoud("a.txt").unwrap(), b"versie 3");
    drop(programma);
    let r = sync_eenmalig(&opslag, dir.path()).await.unwrap();
    assert_eq!((r.geupload, r.gedownload, r.conflicten, r.verwijderd), (0, 0, 0, 0));
    assert_eq!((bronnen(dir.path()), journaal(dir.path())), (0, 0));
    assert_eq!(lees(dir.path(), "a.txt").as_deref(), Some("versie 3"));
}
