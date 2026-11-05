//! Proeven: een IFC-bestand dat hier en op de server tegelijk gewijzigd is, tegen een opslag met vergelijk-en-schrijf
//! (ontwerp §5.6). De opslag in het geheugen houdt per bestand alle versies bij en weigert zoals de server
//! (`_weiger_bij_conflict` met `basis_versie_id`).
mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use common::schrijf;
use openaec_sync::blokken::{hash_bytes, BLOK};
use openaec_sync::motor::{sync_eenmalig, Opslag, OpslagBestand, Rapport, Upload, Versieregel};
use openaec_sync::Fout;
use serde_json::{json, Value};

const IFC: &[u8] = include_bytes!("../../openaec-samenvoegen/tests/bestanden/13-supermarkt-albert-heijn.ifc");

/// Het voorbeeld met een andere naam voor een taak.
fn hernoem(inhoud: &[u8], van: &str, naar: &str) -> Vec<u8> {
    let tekst = String::from_utf8(inhoud.to_vec()).unwrap();
    assert!(tekst.contains(van), "{van} staat niet in het bestand");
    tekst.replacen(van, naar, 1).into_bytes()
}

fn bevat(inhoud: &[u8], tekst: &str) -> bool {
    String::from_utf8_lossy(inhoud).contains(tekst)
}

struct Bestand {
    id: String,
    /// (versie_id, inhoud); het volgnummer is de plek + 1.
    versies: Vec<(String, Vec<u8>)>,
}

#[derive(Default)]
struct Versies {
    bestanden: Mutex<BTreeMap<String, Bestand>>,
    teller: AtomicU32,
    /// Een oudere server: geen versieregels en geen vergelijk-en-schrijf.
    oud: AtomicBool,
    /// Na de eerstvolgende geslaagde schrijfpoging (de gebruiker slaat op terwijl de sync samenvoegt).
    na_schrijven: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// Zoveel keer faalt het lezen van een versie met een tijdelijke fout (503).
    leesfouten: AtomicU32,
    /// `samengevoegd_uit` van elke geslaagde schrijfpoging.
    samengevoegd: Mutex<Vec<Option<(u64, u64)>>>,
}

impl Versies {
    fn id(&self, soort: &str) -> String {
        format!("{soort}{}", self.teller.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Een nieuwe versie van `pad` (een ander slaat op), of een nieuw bestand.
    fn zet(&self, pad: &str, inhoud: &[u8]) {
        let regel = self.id("v");
        let mut b = self.bestanden.lock().unwrap();
        match b.get_mut(pad) {
            Some(f) => f.versies.push((regel, inhoud.to_vec())),
            None => {
                let id = self.id("b");
                b.insert(pad.to_string(), Bestand { id, versies: vec![(regel, inhoud.to_vec())] });
            }
        }
    }

    /// Een ander verwijdert `pad` en maakt een nieuw bestand met dezelfde naam.
    fn maak_opnieuw(&self, pad: &str, inhoud: &[u8]) {
        self.bestanden.lock().unwrap().remove(pad);
        self.zet(pad, inhoud);
    }

    fn huidig(&self, pad: &str) -> Option<Vec<u8>> {
        self.bestanden.lock().unwrap().get(pad).map(|f| f.versies.last().unwrap().1.clone())
    }

    fn aantal_versies(&self, pad: &str) -> usize {
        self.bestanden.lock().unwrap().get(pad).map_or(0, |f| f.versies.len())
    }

    fn paden(&self) -> Vec<String> {
        self.bestanden.lock().unwrap().keys().cloned().collect()
    }

    fn weiger(code: &str, op_plek: Option<&Bestand>) -> Fout {
        let tekst = json!({
            "foutcode": code,
            "huidig_bestand": op_plek.map(|f| f.id.clone()),
            "huidige_versie": op_plek.map(|f| f.versies.len()),
            "huidige_versie_id": op_plek.map(|f| f.versies.last().unwrap().0.clone()),
        });
        Fout::Server { status: 409, tekst: tekst.to_string() }
    }

    fn zoek<T>(&self, bestand: &str, f: impl FnOnce(&Bestand) -> T) -> Option<T> {
        self.bestanden.lock().unwrap().values().find(|b| b.id == bestand).map(f)
    }
}

impl Opslag for Versies {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        let oud = self.oud.load(Ordering::SeqCst);
        Ok(self
            .bestanden
            .lock()
            .unwrap()
            .iter()
            .map(|(pad, f)| {
                let (regel, inhoud) = f.versies.last().unwrap();
                OpslagBestand {
                    pad: pad.clone(),
                    bestand: f.id.clone(),
                    versie: f.versies.len() as u64,
                    grootte: inhoud.len() as u64,
                    versie_id: (!oud).then(|| regel.clone()),
                }
            })
            .collect())
    }

    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        let inhoud = std::fs::read(lokaal)?;
        let huidig = self.bestanden.lock().unwrap().get(pad).map(|f| f.versies.len() as u64);
        let doel = match (huidig, basis_versie) {
            (Some(h), Some(b)) if h == b => pad.to_string(),
            (None, _) => pad.to_string(),
            _ => {
                let (stam, ext) = pad.rsplit_once('.').map(|(s, e)| (s.to_string(), format!(".{e}"))).unwrap_or((pad.to_string(), String::new()));
                format!("{stam} (conflict test){ext}")
            }
        };
        self.zet(&doel, &inhoud);
        let b = self.bestanden.lock().unwrap();
        let f = &b[&doel];
        let versie_id = (!self.oud.load(Ordering::SeqCst)).then(|| f.versies.last().unwrap().0.clone());
        Ok(Upload { pad: doel.clone(), bestand: f.id.clone(), versie: f.versies.len() as u64, conflict: doel != pad, versie_id })
    }

    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        let inhoud = self
            .zoek(&bestand.bestand, |f| match &bestand.versie_id {
                Some(id) => f.versies.iter().find(|(r, _)| r == id).map(|(_, i)| i.clone()),
                None => f.versies.last().map(|(_, i)| i.clone()),
            })
            .flatten()
            .ok_or_else(|| Versies::weiger("onbekende_basis", None))?;
        std::fs::create_dir_all(lokaal.parent().unwrap())?;
        std::fs::write(lokaal, inhoud)?;
        Ok(())
    }

    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.bestanden.lock().unwrap().remove(&bestand.pad);
        Ok(())
    }

    async fn vergelijk_en_schrijf(&self) -> bool {
        !self.oud.load(Ordering::SeqCst)
    }

    async fn schrijf_als(&self, pad: &str, lokaal: &Path, basis: &Versieregel, samengevoegd_uit: Option<(u64, u64)>) -> Result<Upload, Fout> {
        let inhoud = std::fs::read(lokaal)?;
        let upload = {
            let mut b = self.bestanden.lock().unwrap();
            let f = match b.get_mut(pad) {
                None => return Err(Versies::weiger("onbekende_basis", None)),
                Some(f) if f.id != basis.bestand => return Err(Versies::weiger("onbekende_basis", Some(f))),
                Some(f) => f,
            };
            match f.versies.iter().position(|(r, _)| *r == basis.versie_id) {
                None => return Err(Versies::weiger("onbekende_basis", Some(f))),
                Some(i) if i + 1 != f.versies.len() => return Err(Versies::weiger("verouderde_basis", Some(f))),
                Some(_) => {}
            }
            // Zoals de server: `hun` is de basis van de upload, en de basis ligt ervóór.
            if let Some((n, m)) = samengevoegd_uit {
                assert!(m == basis.versie && 1 <= n && n < m, "samengevoegd_uit {n},{m} bij basis {}", basis.versie);
            }
            let regel = format!("v{}", self.teller.fetch_add(1, Ordering::SeqCst) + 1);
            f.versies.push((regel.clone(), inhoud));
            Upload { pad: pad.to_string(), bestand: f.id.clone(), versie: f.versies.len() as u64, conflict: false, versie_id: Some(regel) }
        };
        self.samengevoegd.lock().unwrap().push(samengevoegd_uit);
        if let Some(haak) = self.na_schrijven.lock().unwrap().take() {
            haak();
        }
        Ok(upload)
    }

    async fn lees_versie(&self, versie: &Versieregel) -> Result<Vec<u8>, Fout> {
        if self.leesfouten.load(Ordering::SeqCst) > 0 {
            self.leesfouten.fetch_sub(1, Ordering::SeqCst);
            return Err(Fout::Server { status: 503, tekst: "{}".into() });
        }
        self.zoek(&versie.bestand, |f| {
            let i = f.versies.iter().position(|(r, _)| *r == versie.versie_id)?;
            (i as u64 + 1 == versie.versie).then(|| f.versies[i].1.clone())
        })
        .flatten()
        .ok_or_else(|| Versies::weiger("onbekende_basis", None))
    }

    async fn blokhashes(&self, versie: &Versieregel) -> Result<Option<(u64, Vec<String>)>, Fout> {
        Ok(self
            .zoek(&versie.bestand, |f| f.versies.iter().find(|(r, _)| *r == versie.versie_id).map(|(_, i)| (BLOK, hash_bytes(i, BLOK))))
            .flatten())
    }
}

async fn ronde(opslag: &Versies, map: &Path) -> Rapport {
    sync_eenmalig(opslag, map).await.unwrap()
}

fn toestand(map: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(map.join(".openaec/staat.json")).unwrap()).unwrap()
}

fn eigen(map: &Path) -> Vec<Vec<u8>> {
    let Ok(lijst) = std::fs::read_dir(map.join(".openaec/eigen")) else { return Vec::new() };
    lijst.flatten().map(|i| std::fs::read(i.path()).unwrap()).collect()
}

const PAD: &str = "planning.ifc";
const ANDER: (&str, &str) = ("'Clear out store'", "'Clear out store (ander)'");
const IK: (&str, &str) = ("'Temporary facilities'", "'Temporary facilities (ik)'");
const LATER: (&str, &str) = ("'Demolish existing fit-out'", "'Demolish existing fit-out (later)'");

/// Een gesynchroniseerde map met de planning, en de opslag erbij.
async fn begin() -> (Versies, tempfile::TempDir) {
    let opslag = Versies::default();
    opslag.zet(PAD, IFC);
    let dir = tempfile::tempdir().unwrap();
    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.gedownload, r.fouten.len()), (1, 0), "{:?}", r.fouten);
    (opslag, dir)
}

/// Proef (a): een ander wijzigt taak A, ik taak B. Eén samengevoegde versie op de server, met beide; geen
/// conflictkopie; hier het resultaat, en mijn eigen invoer in `.openaec/eigen/`.
#[tokio::test]
async fn verschillende_taken_worden_een_samengevoegde_versie() {
    let (opslag, dir) = begin().await;
    opslag.zet(PAD, &hernoem(IFC, ANDER.0, ANDER.1));
    let mijn = hernoem(IFC, IK.0, IK.1);
    schrijf(dir.path(), PAD, &mijn);

    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.geupload, r.conflicten, r.fouten.len()), (1, 0, 0), "{:?}", r.fouten);
    let server = opslag.huidig(PAD).unwrap();
    assert!(bevat(&server, ANDER.1) && bevat(&server, IK.1), "beide wijzigingen op de server");
    assert_eq!(opslag.aantal_versies(PAD), 3);
    assert_eq!(*opslag.samengevoegd.lock().unwrap(), vec![Some((1, 2))]);
    assert_eq!(opslag.paden(), vec![PAD.to_string()], "geen conflictkopie");
    assert_eq!(std::fs::read(dir.path().join(PAD)).unwrap(), server, "hier het resultaat");
    assert_eq!(eigen(dir.path()), vec![mijn], "mijn invoer bewaard");
    assert!(toestand(dir.path())["bestanden"][PAD].get("afgeleid").is_none());

    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.geupload, r.gedownload, r.conflicten), (0, 0, 0), "daarna in sync");
}

/// Proef (b): een ander en ik geven dezelfde taak een andere naam. Een conflictkopie met mijn werk, het rapport
/// ernaast; op de plek de versie van de ander. Niets gaat verloren.
#[tokio::test]
async fn dezelfde_taak_anders_geeft_een_conflictkopie_met_rapport() {
    let (opslag, dir) = begin().await;
    let hun = hernoem(IFC, ANDER.0, ANDER.1);
    opslag.zet(PAD, &hun);
    let mijn = hernoem(IFC, ANDER.0, "'Clear out store (ik)'");
    schrijf(dir.path(), PAD, &mijn);

    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.conflicten, r.fouten.len()), (1, 0), "{:?}", r.fouten);
    let kopie = "planning (conflict test).ifc";
    assert_eq!(opslag.huidig(PAD).unwrap(), hun, "op de plek de versie van de ander");
    assert_eq!(opslag.huidig(kopie).unwrap(), mijn, "mijn werk als conflictkopie op de server");
    assert_eq!(std::fs::read(dir.path().join(PAD)).unwrap(), hun);
    assert_eq!(std::fs::read(dir.path().join(kopie)).unwrap(), mijn);
    let rapport: Value = serde_json::from_slice(&std::fs::read(dir.path().join(format!("{kopie}.samenvoegrapport.json"))).unwrap()).unwrap();
    assert_eq!(rapport["reden"], "conflicten");
    assert_eq!((rapport["bestand"].as_str(), rapport["conflictkopie"].as_str()), (Some(PAD), Some(kopie)));
    assert_eq!((rapport["basis"].as_u64(), rapport["huidige"].as_u64()), (Some(1), Some(2)));
    assert!(rapport["samenvoeging"]["conflicten"].as_array().is_some_and(|c| !c.is_empty()), "{rapport}");

    // De volgende ronde neemt het rapport mee naar de server.
    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.geupload, r.conflicten), (1, 0));
    assert!(opslag.paden().contains(&format!("{kopie}.samenvoegrapport.json")));
}

/// Proef (c): ik sla op terwijl de sync samenvoegt (na het schrijven op de server, vóór het resultaat hier staat).
/// Het resultaat vervangt mijn bestand niet; de volgende ronde voegt mijn nieuwe werk samen met de versie op de
/// server. In de versie daarna staan alle drie de wijzigingen.
#[tokio::test]
async fn opslaan_tijdens_het_samenvoegen_komt_in_de_volgende_versie() {
    let (opslag, dir) = begin().await;
    opslag.zet(PAD, &hernoem(IFC, ANDER.0, ANDER.1));
    let mijn = hernoem(IFC, IK.0, IK.1);
    schrijf(dir.path(), PAD, &mijn);
    let later = hernoem(&mijn, LATER.0, LATER.1);
    let (map, l) = (dir.path().to_path_buf(), later.clone());
    *opslag.na_schrijven.lock().unwrap() = Some(Box::new(move || schrijf(&map, PAD, &l)));

    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.geupload, r.conflicten, r.fouten.len()), (1, 0, 0), "{:?}", r.fouten);
    assert_eq!(std::fs::read(dir.path().join(PAD)).unwrap(), later, "mijn opgeslagen werk blijft staan");
    assert!(toestand(dir.path())["bestanden"][PAD]["afgeleid"].is_object(), "afgeleid van mijn invoer");

    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.geupload, r.conflicten, r.fouten.len()), (1, 0, 0), "{:?}", r.fouten);
    let server = opslag.huidig(PAD).unwrap();
    assert!(bevat(&server, ANDER.1) && bevat(&server, IK.1) && bevat(&server, LATER.1), "alle drie op de server");
    assert_eq!(opslag.aantal_versies(PAD), 4);
    assert_eq!(std::fs::read(dir.path().join(PAD)).unwrap(), server);
    assert_eq!(opslag.paden(), vec![PAD.to_string()], "geen conflictkopie");

    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.geupload, r.gedownload, r.conflicten), (0, 0, 0));
}

/// Proef (e): terwijl ik offline werkte, verwijderde een ander het bestand en maakte een nieuw met dezelfde naam. Mijn
/// basis hoort niet bij dat bestand: een conflictkopie met rapport (`onbekende_basis`), niets samengevoegd.
#[tokio::test]
async fn opnieuw_gemaakt_bestand_geeft_een_conflictkopie() {
    let (opslag, dir) = begin().await;
    let nieuw = hernoem(IFC, ANDER.0, ANDER.1);
    opslag.maak_opnieuw(PAD, &nieuw);
    let mijn = hernoem(IFC, IK.0, IK.1);
    schrijf(dir.path(), PAD, &mijn);

    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.conflicten, r.fouten.len()), (1, 0), "{:?}", r.fouten);
    let kopie = "planning (conflict test).ifc";
    assert_eq!(opslag.huidig(PAD).unwrap(), nieuw);
    assert_eq!(opslag.huidig(kopie).unwrap(), mijn);
    let rapport: Value = serde_json::from_slice(&std::fs::read(dir.path().join(format!("{kopie}.samenvoegrapport.json"))).unwrap()).unwrap();
    assert_eq!(rapport["reden"], "onbekende_basis");
    assert!(opslag.samengevoegd.lock().unwrap().is_empty(), "niets samengevoegd");
}

/// Een tijdelijke fout bij het lezen van de basis (de server even niet) is geen conflict: geen kopie, een fout voor de
/// volgende ronde, en alles blijft staan.
#[tokio::test]
async fn tijdelijke_leesfout_geeft_geen_conflictkopie() {
    let (opslag, dir) = begin().await;
    let hun = hernoem(IFC, ANDER.0, ANDER.1);
    opslag.zet(PAD, &hun);
    let mijn = hernoem(IFC, IK.0, IK.1);
    schrijf(dir.path(), PAD, &mijn);
    opslag.leesfouten.store(1, Ordering::SeqCst);

    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.geupload, r.conflicten, r.fouten.len()), (0, 0, 1), "{:?}", r.fouten);
    assert_eq!(opslag.paden(), vec![PAD.to_string()], "geen conflictkopie");
    assert_eq!(opslag.huidig(PAD).unwrap(), hun);
    assert_eq!(std::fs::read(dir.path().join(PAD)).unwrap(), mijn, "mijn werk blijft staan");
}

/// Bestanden van vóór de versieregels (een oudere server). Een ongewijzigd bestand krijgt de versieregel als de inhoud
/// hier precies die versie is; daarna voegt het gewoon samen. Een gewijzigd bestand zonder versieregel wordt een
/// conflictkopie: samenvoegen kan dan niet veilig.
#[tokio::test]
async fn bestand_van_voor_de_versieregels() {
    let opslag = Versies { oud: AtomicBool::new(true), ..Default::default() };
    opslag.zet(PAD, IFC);
    opslag.zet("ander.ifc", IFC);
    let dir = tempfile::tempdir().unwrap();
    let r = ronde(&opslag, dir.path()).await;
    assert_eq!(r.gedownload, 2);
    assert!(toestand(dir.path())["bestanden"][PAD].get("versie_id").is_none());

    opslag.oud.store(false, Ordering::SeqCst);
    let mijn = hernoem(IFC, IK.0, IK.1);
    schrijf(dir.path(), "ander.ifc", &mijn);
    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.conflicten, r.fouten.len()), (1, 0), "{:?}", r.fouten);
    assert_eq!(opslag.huidig("ander (conflict test).ifc").unwrap(), mijn, "gewijzigd zonder versieregel: conflictkopie");
    assert!(toestand(dir.path())["bestanden"][PAD]["versie_id"].is_string(), "ongewijzigd: versieregel overgenomen");

    opslag.zet(PAD, &hernoem(IFC, ANDER.0, ANDER.1));
    schrijf(dir.path(), PAD, &mijn);
    let r = ronde(&opslag, dir.path()).await;
    assert_eq!((r.geupload, r.conflicten, r.fouten.len()), (1, 0, 0), "{:?}", r.fouten);
    let server = opslag.huidig(PAD).unwrap();
    assert!(bevat(&server, ANDER.1) && bevat(&server, IK.1));
}
