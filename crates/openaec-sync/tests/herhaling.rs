//! Rondes die niet steeds opnieuw werk vinden: een naam met een spatie vooraan (de server haalt die spatie weg), een
//! tweede ronde zonder wijzigingen, en een bestand dat steeds mislukt (wacht een tijd in plaats van elke ronde een
//! nieuwe poging en een nieuwe fout).

mod common;

use std::path::Path;
use std::sync::Mutex;

use common::{schrijf, NepOpslag};
use openaec_sync::motor::{sync_met_opties, wachttijd, Opslag, OpslagBestand, Opties, Upload};
use openaec_sync::voortgang::{Gebeurtenis, Stil, Waarnemer};
use openaec_sync::Fout;

/// Als de echte server: een naam krijgt bij het uploaden geen witruimte vooraan of achteraan (per deel van het pad).
#[derive(Default)]
struct TrimOpslag {
    nep: NepOpslag,
}

fn zonder_witruimte(pad: &str) -> String {
    pad.split('/').map(str::trim).collect::<Vec<_>>().join("/")
}

impl Opslag for TrimOpslag {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        self.nep.lijst().await
    }
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.nep.upload(&zonder_witruimte(pad), lokaal, basis_versie).await
    }
    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.nep.download(bestand, lokaal).await
    }
    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.nep.verwijder(bestand).await
    }
}

/// Onthoudt wat de motor vooraf meldt over een ronde.
#[derive(Default)]
struct Teller(Mutex<Vec<(u32, u32)>>);

impl Waarnemer for Teller {
    fn melding(&self, g: Gebeurtenis) {
        if let Gebeurtenis::Ronde { uploaden, downloaden, .. } = g {
            self.0.lock().unwrap().push((uploaden, downloaden));
        }
    }
}

#[tokio::test]
async fn naam_met_een_spatie_vooraan_lukt_en_komt_niet_elke_ronde_terug() {
    let dir = tempfile::tempdir().unwrap();
    // Een echt bestand met een spatie vooraan, in een map, zoals uit een backup.
    schrijf(dir.path(), "Keuren 2023_V1/ EH-0210.pdf", b"keuring");
    let opslag = TrimOpslag::default();
    let opties = Opties::default();

    let r = sync_met_opties(&opslag, dir.path(), &[], &Stil, &opties).await.unwrap();
    assert!(r.fouten.is_empty(), "fouten: {:?}", r.fouten);
    assert_eq!(r.geupload, 1);
    // Op de server zonder spatie; hier blijft de naam zoals hij was.
    assert_eq!(opslag.nep.paden(), vec!["Keuren 2023_V1/EH-0210.pdf".to_string()]);
    assert!(dir.path().join("Keuren 2023_V1").join(" EH-0210.pdf").is_file());

    let teller = Teller::default();
    let r2 = sync_met_opties(&opslag, dir.path(), &[], &teller, &opties).await.unwrap();
    assert!(r2.fouten.is_empty(), "fouten: {:?}", r2.fouten);
    assert_eq!((r2.geupload, r2.gedownload), (0, 0));
    assert_eq!(*teller.0.lock().unwrap(), vec![(0, 0)]);
    // Geen kopie zonder spatie hier.
    assert!(!dir.path().join("Keuren 2023_V1").join("EH-0210.pdf").exists());
    assert_eq!(openaec_sync::motor::bekend_bestand(dir.path(), "Keuren 2023_V1/ EH-0210.pdf").map(|b| b.1), Some(1));

    // Een wijziging hier gaat als nieuwe versie naar hetzelfde bestand op de server.
    schrijf(dir.path(), "Keuren 2023_V1/ EH-0210.pdf", b"keuring 2");
    let r3 = sync_met_opties(&opslag, dir.path(), &[], &Stil, &opties).await.unwrap();
    assert!(r3.fouten.is_empty(), "fouten: {:?}", r3.fouten);
    assert_eq!(r3.geupload, 1);
    assert_eq!(opslag.nep.inhoud("Keuren 2023_V1/EH-0210.pdf").unwrap(), b"keuring 2");
    assert_eq!(opslag.nep.paden().len(), 1);
}

#[tokio::test]
async fn spatie_vooraan_naast_dezelfde_naam_zonder_spatie_wordt_overgeslagen() {
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), " a.txt", b"met spatie");
    schrijf(dir.path(), "a.txt", b"zonder");
    let opslag = TrimOpslag::default();
    for _ in 0..2 {
        let r = sync_met_opties(&opslag, dir.path(), &[], &Stil, &Opties::default()).await.unwrap();
        assert!(r.fouten.is_empty(), "fouten: {:?}", r.fouten);
        assert!(r.overgeslagen.iter().any(|o| o.pad == " a.txt" && o.reden.contains("a.txt")), "{:?}", r.overgeslagen);
    }
    assert_eq!(opslag.nep.inhoud("a.txt").unwrap(), b"zonder");
}

/// Een spatie achteraan kan op Windows niet: overgeslagen, elke ronde zonder fout.
#[cfg(windows)]
#[tokio::test]
async fn spatie_achteraan_wordt_overgeslagen_zonder_foutlus() {
    let dir = tempfile::tempdir().unwrap();
    let pad = openaec_sync::namen::lang_pad(&dir.path().join("rapport.pdf "));
    std::fs::write(&pad, b"x").unwrap();
    let opslag = TrimOpslag::default();
    for _ in 0..2 {
        let r = sync_met_opties(&opslag, dir.path(), &[], &Stil, &Opties::default()).await.unwrap();
        assert!(r.fouten.is_empty(), "fouten: {:?}", r.fouten);
        assert_eq!(r.geupload, 0);
        assert!(r.overgeslagen.iter().any(|o| o.pad == "rapport.pdf "));
    }
    assert!(opslag.nep.paden().is_empty());
}

#[tokio::test]
async fn tweede_ronde_zonder_wijzigingen_doet_niets() {
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "a.txt", b"a");
    schrijf(dir.path(), "Tekeningen/plan.pdf", b"plan");
    schrijf(dir.path(), "Tekeningen/Oud/schets.dwg", b"schets");
    let opslag = NepOpslag::default();
    opslag.zet("Van de server/notitie.txt", b"server");
    opslag.zet("leeg.txt", b"");

    let r = sync_met_opties(&opslag, dir.path(), &[], &Stil, &Opties::default()).await.unwrap();
    assert!(r.fouten.is_empty(), "fouten: {:?}", r.fouten);
    assert_eq!((r.geupload, r.gedownload), (3, 2));

    for _ in 0..2 {
        let teller = Teller::default();
        let r = sync_met_opties(&opslag, dir.path(), &[], &teller, &Opties::default()).await.unwrap();
        assert_eq!((r.geupload, r.gedownload, r.verwijderd), (0, 0, 0));
        assert!(r.fouten.is_empty() && r.wachtend.is_empty());
        assert_eq!(*teller.0.lock().unwrap(), vec![(0, 0)], "een controle zonder werk meldt geen werk");
    }
}

#[tokio::test]
async fn bestand_dat_steeds_mislukt_wacht_en_komt_niet_elke_ronde_als_fout_terug() {
    let dir = tempfile::tempdir().unwrap();
    schrijf(dir.path(), "goed.txt", b"goed");
    schrijf(dir.path(), "kapot.txt", b"kapot");
    let opslag = NepOpslag::default();
    opslag.weiger("kapot.txt");

    let r = sync_met_opties(&opslag, dir.path(), &[], &Stil, &Opties::default()).await.unwrap();
    assert_eq!(r.geupload, 1);
    assert_eq!(r.fouten.len(), 1);

    // De volgende ronde: geen nieuwe poging en geen nieuwe fout, wel in de lijst die wacht.
    let teller = Teller::default();
    let r2 = sync_met_opties(&opslag, dir.path(), &[], &teller, &Opties::default()).await.unwrap();
    assert!(r2.fouten.is_empty(), "fouten: {:?}", r2.fouten);
    assert_eq!(r2.wachtend.len(), 1);
    assert_eq!((r2.wachtend[0].pad.as_str(), r2.wachtend[0].keer), ("kapot.txt", 1));
    assert!(r2.wachtend[0].reden.contains("geweigerd"));
    assert_eq!(*teller.0.lock().unwrap(), vec![(0, 0)]);

    // Verandert het bestand, dan meteen een nieuwe poging; lukt die, dan is het klaar.
    opslag.weigeren.lock().unwrap().clear();
    schrijf(dir.path(), "kapot.txt", b"gerepareerd");
    let r3 = sync_met_opties(&opslag, dir.path(), &[], &Stil, &Opties::default()).await.unwrap();
    assert!(r3.fouten.is_empty() && r3.wachtend.is_empty());
    assert_eq!(r3.geupload, 1);
    assert_eq!(opslag.inhoud("kapot.txt").unwrap(), b"gerepareerd");
}

#[test]
fn wachttijd_groeit_tot_een_grens() {
    assert_eq!(wachttijd(1), 120);
    assert_eq!(wachttijd(2), 240);
    assert_eq!(wachttijd(3), 480);
    assert_eq!(wachttijd(30), 6 * 3600);
}
