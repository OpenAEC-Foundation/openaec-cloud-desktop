//! De opslaan-lus met een opslag in het geheugen: acht klanten tegelijk, een slot, een conflict, en de weigeringen
//! "onbekende basis" en "basis niet te lezen".

mod gemeen;

use std::collections::HashMap;
use std::sync::{Arc, Barrier, Mutex};

use gemeen::*;
use openaec_samenvoegen::{sla_op, Basis, LusFout, LusOpties, Opslag, Reden, Uitkomst, Weigering};

/// Een opslag in het geheugen, zoals openaec-storage of Nextcloud. Op de plek staat één bestand met een eigen
/// identiteit; elke versie heeft een id dat nooit terugkomt (één teller voor alle bestanden, als tekst zoals een
/// ETag). Een bestand dat opnieuw wordt gemaakt, krijgt een nieuwe identiteit.
#[derive(Default)]
struct Geheugen {
    teller: u64,
    /// Het bestand dat nu op de plek staat, en zijn versies (oud naar nieuw).
    bestand: String,
    versies: Vec<String>,
    /// De inhoud per versie-id, met het bestand waar hij bij hoort; ook van verwijderde bestanden.
    inhoud: HashMap<String, (String, Vec<u8>)>,
    slot: Option<String>,
    /// De opslag weet niet zeker of een oude basis bij dit bestand hoort (openaec-storage: `onbekende_basis`).
    onzeker: bool,
    /// Een opslag die bij een ander bestand toch "verouderd" zegt (de lus moet dat zelf zien).
    verouderd_bij_ander_bestand: bool,
    gelezen: usize,
    geschreven: usize,
}

impl Geheugen {
    fn id(&mut self, soort: &str) -> String {
        self.teller += 1;
        // Geen oplopend getal: een id zegt niets over de volgorde.
        format!("{soort}-{:016x}", self.teller.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    fn nieuwe_versie(&mut self, inhoud: &[u8]) -> Basis<String, String> {
        let versie = self.id("etag");
        self.inhoud.insert(versie.clone(), (self.bestand.clone(), inhoud.to_vec()));
        self.versies.push(versie.clone());
        Basis { bestand: self.bestand.clone(), versie }
    }

    fn huidige(&self) -> Basis<String, String> {
        Basis { bestand: self.bestand.clone(), versie: self.versies.last().unwrap().clone() }
    }

    /// Verwijdert het bestand en maakt op dezelfde plek een nieuw bestand met deze inhoud.
    fn maak_opnieuw(&mut self, inhoud: &[u8]) -> Basis<String, String> {
        self.bestand = self.id("bestand");
        self.versies.clear();
        self.nieuwe_versie(inhoud)
    }
}

#[derive(Clone)]
struct Klant(Arc<Mutex<Geheugen>>);

impl Klant {
    fn nieuw(inhoud: &str) -> Self {
        let mut g = Geheugen::default();
        g.maak_opnieuw(inhoud.as_bytes());
        Klant(Arc::new(Mutex::new(g)))
    }

    fn basis(&self) -> Basis<String, String> {
        self.0.lock().unwrap().huidige()
    }
}

impl Opslag for Klant {
    type Bestand = String;
    type Versie = String;

    fn huidige(&mut self) -> Result<Basis<String, String>, Weigering<String, String>> {
        Ok(self.0.lock().unwrap().huidige())
    }

    fn lees(&mut self, basis: &Basis<String, String>) -> Result<Vec<u8>, Weigering<String, String>> {
        let mut g = self.0.lock().unwrap();
        g.gelezen += 1;
        match g.inhoud.get(&basis.versie) {
            Some((bestand, inhoud)) if *bestand == basis.bestand => Ok(inhoud.clone()),
            _ => Err(Weigering::Fout(format!("geen versie {} van {}", basis.versie, basis.bestand))),
        }
    }

    fn schrijf_als(
        &mut self,
        basis: &Basis<String, String>,
        inhoud: &[u8],
    ) -> Result<Basis<String, String>, Weigering<String, String>> {
        let mut g = self.0.lock().unwrap();
        if let Some(door) = &g.slot {
            return Err(Weigering::Vergrendeld { door: Some(door.clone()) });
        }
        let huidige = g.huidige();
        if *basis == huidige {
            g.geschreven += 1;
            return Ok(g.nieuwe_versie(inhoud));
        }
        let van_dit_bestand = basis.bestand == g.bestand && g.versies.contains(&basis.versie);
        if van_dit_bestand && !g.onzeker || g.verouderd_bij_ander_bestand {
            Err(Weigering::Verouderd { huidige })
        } else {
            Err(Weigering::OnbekendeBasis)
        }
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
                let begin = klant.huidige().unwrap();
                start.wait();
                let opties = LusOpties { max_pogingen: 20 };
                match sla_op(&mut klant, begin, ons.as_bytes(), &opties).unwrap() {
                    Uitkomst::Opgeslagen { eigen, pogingen, samenvoegingen, .. } => {
                        assert_eq!(eigen, ons.as_bytes());
                        assert_eq!(samenvoegingen.len(), pogingen - 1);
                        pogingen
                    }
                    Uitkomst::Conflict { reden, .. } => panic!("conflict: {reden:#?}"),
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
    let laatste = &g.inhoud[g.versies.last().unwrap()].1;
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
    let basis_nu = opslag.basis();
    let fout = sla_op(&mut opslag, basis_nu, ons.as_bytes(), &LusOpties::default()).unwrap_err();
    assert_eq!(fout, LusFout::Vergrendeld { door: Some("Anne".into()) });
    let g = opslag.0.lock().unwrap();
    assert_eq!((g.gelezen, g.geschreven, g.versies.len()), (0, 0, 1));
}

/// Twee klanten wijzigen dezelfde naam anders: de tweede krijgt een conflict, en er wordt niets opgeslagen.
#[test]
fn conflict_in_de_lus() {
    let basis = gebouw("IFC4X3_ADD2");
    let mut opslag = Klant::nieuw(&basis);
    let begin = opslag.basis();
    let a = vervang(&basis, "#5,'W',$", "#5,'W-a',$");
    let b = vervang(&basis, "#5,'W',$", "#5,'W-b',$");
    let na_a = match sla_op(&mut opslag, begin.clone(), a.as_bytes(), &LusOpties::default()).unwrap() {
        Uitkomst::Opgeslagen { nieuw, pogingen: 1, .. } => nieuw,
        u => panic!("{u:?}"),
    };
    assert_eq!(na_a.bestand, begin.bestand);
    assert_ne!(na_a.versie, begin.versie);
    match sla_op(&mut opslag, begin, b.as_bytes(), &LusOpties::default()).unwrap() {
        Uitkomst::Conflict { huidige, reden: Reden::Samenvoegen(s), eigen } => {
            assert_eq!(huidige, Some(na_a));
            assert_eq!(eigen, b.as_bytes());
            assert_eq!(s.conflicten[0].attribuut.as_deref(), Some("Name"));
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
    let begin = opslag.basis();
    let a = vervang(&basis, "#5,'W',$", "#5,'W-a',$");
    sla_op(&mut opslag, begin.clone(), a.as_bytes(), &LusOpties::default()).unwrap();
    let na_a = opslag.basis();
    let b = vervang(&basis, "#5,'V',$", "#5,'V-b',$");
    let een = LusOpties { max_pogingen: 1 };
    let fout = sla_op(&mut opslag, begin.clone(), b.as_bytes(), &een).unwrap_err();
    assert_eq!(fout, LusFout::TeVaakVerouderd { pogingen: 1, huidige: na_a });
    // Met twee pogingen lukt het: samenvoegen en opnieuw schrijven.
    match sla_op(&mut opslag, begin, b.as_bytes(), &LusOpties { max_pogingen: 2 }).unwrap() {
        Uitkomst::Opgeslagen { nieuw, inhoud, pogingen, .. } => {
            assert_eq!(pogingen, 2);
            assert_eq!(nieuw, opslag.basis());
            let verwacht = vervang(&a, "#5,'V',$", "#5,'V-b',$");
            assert!(verschil(&inhoud, verwacht.as_bytes()).unwrap().is_leeg());
        }
        u => panic!("{u:?}"),
    }
}

/// Een ander bestand op dezelfde plek (verwijderd en opnieuw gemaakt): de oude basis hoort er niet bij. Geen
/// samenvoeging, niets gelezen en niets geschreven; de client maakt een conflictkopie.
#[test]
fn ander_bestand_op_dezelfde_plek() {
    let basis = gebouw("IFC4");
    let mut opslag = Klant::nieuw(&basis);
    let oud = opslag.basis();
    let ander = vervang(&basis, "#5,'V',$", "#5,'V-ander',$");
    let nieuw = opslag.0.lock().unwrap().maak_opnieuw(ander.as_bytes());
    assert_ne!(nieuw.bestand, oud.bestand);
    assert_ne!(nieuw.versie, oud.versie);
    let ons = vervang(&basis, "#5,'W',$", "#5,'W-ons',$");
    match sla_op(&mut opslag, oud, ons.as_bytes(), &LusOpties::default()).unwrap() {
        Uitkomst::Conflict { huidige: None, reden: Reden::OnbekendeBasis, eigen } => assert_eq!(eigen, ons.as_bytes()),
        u => panic!("verwachtte OnbekendeBasis: {u:?}"),
    }
    let g = opslag.0.lock().unwrap();
    assert_eq!((g.gelezen, g.geschreven, g.versies.len()), (0, 0, 1));
}

/// De opslag weet niet zeker of de basis bij dit bestand hoort (openaec-storage: `onbekende_basis`): niet lezen,
/// niet samenvoegen, niets schrijven.
#[test]
fn onbekende_basis() {
    let basis = gebouw("IFC4X3_ADD2");
    let mut opslag = Klant::nieuw(&basis);
    let begin = opslag.basis();
    let a = vervang(&basis, "#5,'V',$", "#5,'V-a',$");
    sla_op(&mut opslag, begin.clone(), a.as_bytes(), &LusOpties::default()).unwrap();
    let mut g = opslag.0.lock().unwrap();
    g.onzeker = true;
    g.gelezen = 0;
    drop(g);
    let ons = vervang(&basis, "#5,'W',$", "#5,'W-ons',$");
    match sla_op(&mut opslag, begin, ons.as_bytes(), &LusOpties::default()).unwrap() {
        Uitkomst::Conflict { reden: Reden::OnbekendeBasis, .. } => {}
        u => panic!("verwachtte OnbekendeBasis: {u:?}"),
    }
    let g = opslag.0.lock().unwrap();
    assert_eq!((g.gelezen, g.geschreven, g.versies.len()), (0, 1, 2));
}

/// Een opslag die bij een ander bestand toch "verouderd" zegt: de lus ziet zelf dat het bestand anders is, en
/// leest en schrijft niets.
#[test]
fn verouderd_maar_een_ander_bestand() {
    let basis = gebouw("IFC2X3");
    let mut opslag = Klant::nieuw(&basis);
    let oud = opslag.basis();
    let mut g = opslag.0.lock().unwrap();
    let nieuw = g.maak_opnieuw(basis.as_bytes());
    g.verouderd_bij_ander_bestand = true;
    drop(g);
    let ons = vervang(&basis, "#5,'W',$", "#5,'W-ons',$");
    match sla_op(&mut opslag, oud, ons.as_bytes(), &LusOpties::default()).unwrap() {
        Uitkomst::Conflict { huidige, reden: Reden::OnbekendeBasis, .. } => assert_eq!(huidige, Some(nieuw)),
        u => panic!("verwachtte OnbekendeBasis: {u:?}"),
    }
    let g = opslag.0.lock().unwrap();
    assert_eq!((g.gelezen, g.geschreven), (0, 0));
}

/// De basis is verouderd, maar zijn inhoud is er niet meer (bijv. de versie is opgeruimd): geen samenvoeging en
/// niets geschreven; het conflict zegt waarom.
#[test]
fn basis_niet_te_lezen() {
    let basis = gebouw("IFC4");
    let mut opslag = Klant::nieuw(&basis);
    let begin = opslag.basis();
    let a = vervang(&basis, "#5,'V',$", "#5,'V-a',$");
    sla_op(&mut opslag, begin.clone(), a.as_bytes(), &LusOpties::default()).unwrap();
    let na_a = opslag.basis();
    opslag.0.lock().unwrap().inhoud.remove(&begin.versie);
    let ons = vervang(&basis, "#5,'W',$", "#5,'W-ons',$");
    match sla_op(&mut opslag, begin.clone(), ons.as_bytes(), &LusOpties::default()).unwrap() {
        Uitkomst::Conflict { huidige, reden: Reden::BasisNietTeLezen(melding), eigen } => {
            assert_eq!(huidige, Some(na_a));
            assert!(melding.contains(&begin.versie), "{melding}");
            assert_eq!(eigen, ons.as_bytes());
        }
        u => panic!("verwachtte BasisNietTeLezen: {u:?}"),
    }
    let g = opslag.0.lock().unwrap();
    assert_eq!((g.geschreven, g.versies.len()), (1, 2));
}
