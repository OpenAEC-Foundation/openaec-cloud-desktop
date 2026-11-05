//! De opslaan-lus met een opslag in het geheugen: acht klanten tegelijk, een slot, een conflict.

mod gemeen;

use std::sync::{Arc, Barrier, Mutex};

use gemeen::*;
use openaec_samenvoegen::{sla_op, verschil, LusFout, LusOpties, Opslag, Uitkomst, Weigering};

#[derive(Default)]
struct Geheugen {
    versies: Vec<Vec<u8>>,
    slot: Option<String>,
    gelezen: usize,
    geschreven: usize,
}

#[derive(Clone)]
struct Klant(Arc<Mutex<Geheugen>>);

impl Klant {
    fn nieuw(inhoud: &str) -> Self {
        Klant(Arc::new(Mutex::new(Geheugen { versies: vec![inhoud.as_bytes().to_vec()], ..Default::default() })))
    }
}

impl Opslag for Klant {
    type Versie = usize;

    fn huidige(&mut self) -> Result<usize, Weigering<usize>> {
        Ok(self.0.lock().unwrap().versies.len() - 1)
    }

    fn lees(&mut self, versie: &usize) -> Result<Vec<u8>, Weigering<usize>> {
        let mut g = self.0.lock().unwrap();
        g.gelezen += 1;
        g.versies.get(*versie).cloned().ok_or_else(|| Weigering::Fout(format!("geen versie {versie}")))
    }

    fn schrijf_als(&mut self, basis: &usize, inhoud: &[u8]) -> Result<usize, Weigering<usize>> {
        let mut g = self.0.lock().unwrap();
        if let Some(door) = &g.slot {
            return Err(Weigering::Vergrendeld { door: Some(door.clone()) });
        }
        let huidige = g.versies.len() - 1;
        if *basis != huidige {
            return Err(Weigering::Verouderd { huidige });
        }
        g.geschreven += 1;
        g.versies.push(inhoud.to_vec());
        Ok(huidige + 1)
    }
}

/// De namen van de eerste `n` taken die precies één keer in het bestand staan.
fn taaknamen(tekst: &str, n: usize) -> Vec<String> {
    let mut uit = Vec::new();
    for regel in tekst.lines().filter(|r| r.contains("=IFCTASK(")) {
        let Some(naam) = regel.split('\'').nth(3) else { continue };
        if tekst.matches(&format!("'{naam}'")).count() == 1 {
            uit.push(naam.to_string());
        }
        if uit.len() == n {
            break;
        }
    }
    assert_eq!(uit.len(), n);
    uit
}

fn hernoem(tekst: &str, naam: &str, achter: &str) -> String {
    vervang(tekst, &format!("'{naam}'"), &format!("'{naam}{achter}'"))
}

/// Acht klanten beginnen tegelijk bij dezelfde versie, en wijzigen elk een andere taak. Alle acht wijzigingen
/// komen in het eindresultaat.
#[test]
fn acht_klanten_tegelijk() {
    let basis = bestand("13-supermarkt-albert-heijn.ifc");
    let namen = taaknamen(&basis, 8);
    let opslag = Klant::nieuw(&basis);
    let start = Arc::new(Barrier::new(namen.len()));
    let draden: Vec<_> = namen
        .iter()
        .enumerate()
        .map(|(k, naam)| {
            let mut klant = opslag.clone();
            let start = start.clone();
            // Elke klant nummert zijn versie anders, zoals OPS bij elk opslaan.
            let ons = hernummer(&hernoem(&basis, naam, &format!(" (klant {k})")), k as u64 + 1);
            std::thread::spawn(move || {
                let versie = klant.huidige().unwrap();
                start.wait();
                let opties = LusOpties { max_pogingen: 20, ..Default::default() };
                match sla_op(&mut klant, versie, ons.as_bytes(), &opties).unwrap() {
                    Uitkomst::Opgeslagen { eigen, pogingen, samenvoegingen, .. } => {
                        assert_eq!(eigen, ons.as_bytes());
                        assert_eq!(samenvoegingen.len(), pogingen - 1);
                        pogingen
                    }
                    Uitkomst::Conflict { samenvoeging, .. } => panic!("conflict: {:#?}", samenvoeging.conflicten),
                }
            })
        })
        .collect();
    let pogingen: Vec<usize> = draden.into_iter().map(|d| d.join().unwrap()).collect();
    let g = opslag.0.lock().unwrap();
    assert_eq!(g.versies.len(), namen.len() + 1);
    let mut verwacht = basis.clone();
    for (k, naam) in namen.iter().enumerate() {
        verwacht = hernoem(&verwacht, naam, &format!(" (klant {k})"));
    }
    let laatste = g.versies.last().unwrap();
    let v = verschil(laatste, verwacht.as_bytes()).unwrap();
    assert!(v.is_leeg(), "{v:#?}");
    // Minstens één klant moest samenvoegen.
    assert!(pogingen.iter().any(|&p| p > 1), "{pogingen:?}");
}

/// Een slot van een ander: geen poging tot samenvoegen, en niets gelezen.
#[test]
fn vergrendeld_door_een_ander() {
    let basis = gebouw("IFC4");
    let mut opslag = Klant::nieuw(&basis);
    opslag.0.lock().unwrap().slot = Some("Anne".into());
    let ons = vervang(&basis, "#5,'W',$", "#5,'W-ons',$");
    let fout = sla_op(&mut opslag, 0, ons.as_bytes(), &LusOpties::default()).unwrap_err();
    assert_eq!(fout, LusFout::Vergrendeld { door: Some("Anne".into()) });
    let g = opslag.0.lock().unwrap();
    assert_eq!((g.gelezen, g.geschreven, g.versies.len()), (0, 0, 1));
}

/// Twee klanten wijzigen dezelfde naam anders: de tweede krijgt een conflict, en er wordt niets opgeslagen.
#[test]
fn conflict_in_de_lus() {
    let basis = gebouw("IFC4X3_ADD2");
    let mut opslag = Klant::nieuw(&basis);
    let a = vervang(&basis, "#5,'W',$", "#5,'W-a',$");
    let b = vervang(&basis, "#5,'W',$", "#5,'W-b',$");
    assert!(matches!(
        sla_op(&mut opslag, 0, a.as_bytes(), &LusOpties::default()),
        Ok(Uitkomst::Opgeslagen { versie: 1, pogingen: 1, .. })
    ));
    match sla_op(&mut opslag, 0, b.as_bytes(), &LusOpties::default()).unwrap() {
        Uitkomst::Conflict { huidige, samenvoeging, eigen } => {
            assert_eq!(huidige, 1);
            assert_eq!(eigen, b.as_bytes());
            assert_eq!(samenvoeging.conflicten[0].attribuut.as_deref(), Some("Name"));
        }
        u => panic!("verwachtte een conflict: {u:?}"),
    }
    assert_eq!(opslag.0.lock().unwrap().versies.len(), 2);
}

/// Na `max_pogingen` keer verouderd geven we op.
#[test]
fn te_vaak_verouderd() {
    let basis = gebouw("IFC2X3");
    let mut opslag = Klant::nieuw(&basis);
    let a = vervang(&basis, "#5,'W',$", "#5,'W-a',$");
    sla_op(&mut opslag, 0, a.as_bytes(), &LusOpties::default()).unwrap();
    let b = vervang(&basis, "#5,'V',$", "#5,'V-b',$");
    let fout = sla_op(&mut opslag, 0, b.as_bytes(), &LusOpties { max_pogingen: 1, ..Default::default() }).unwrap_err();
    assert_eq!(fout, LusFout::TeVaakVerouderd { pogingen: 1, huidige: 1 });
    // Met twee pogingen lukt het: samenvoegen en opnieuw schrijven.
    match sla_op(&mut opslag, 0, b.as_bytes(), &LusOpties { max_pogingen: 2, ..Default::default() }).unwrap() {
        Uitkomst::Opgeslagen { versie, inhoud, pogingen, .. } => {
            assert_eq!((versie, pogingen), (2, 2));
            let verwacht = vervang(&a, "#5,'V',$", "#5,'V-b',$");
            assert!(verschil(&inhoud, verwacht.as_bytes()).unwrap().is_leeg());
        }
        u => panic!("{u:?}"),
    }
}
