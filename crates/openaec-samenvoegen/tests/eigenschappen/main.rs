//! Eigenschappen van `verschil` en `voeg_samen` (ontwerp §5.6), getoetst met veel willekeurige bewerkingen op echte
//! startbestanden (proptest). De controle staat los van de bibliotheek: een eigen lezer en schrijver (`stap`), een
//! eigen canonieke vorm en verwachting per attribuut (`vorm`), en een generator van bewerkingen (`bewerk`). Alleen
//! de schematabellen in `src/schema/` lezen we mee, als data.
//!
//! Omgevingsvariabelen:
//! - `EIGENSCHAPPEN_GEVALLEN`: het aantal gevallen per eigenschap (standaard 128). Voor een lange run bijv. 5000.
//! - `EIGENSCHAPPEN_ZAAD`: een getal voor de toevalsgenerator. Zonder is elke run gelijk.
//! - `OPS_VOORBEELDEN`: een map met planningen van Open Planner Studio (`examples/` van open-planner-studio). Die
//!   komen bij de startbestanden.
//! - `EIGENSCHAPPEN_UITVOER`: een map; bij een fout staan daar de basis, ons en hun van het kleinste geval.
//! - `SAMENVOEGEN_UITVOER`: een map; elk resultaat komt daarin, voor `scripts/valideer_met_ifcopenshell.py`.
//! - `EIGENSCHAPPEN_STRENG`: ook een conflict van de zelfcontrole is een fout (om een klein voorbeeld te vinden).

#[path = "../gemeen/mod.rs"]
mod gemeen;

mod bewerk;
mod stap;
mod vorm;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::sync::OnceLock;

use bewerk::{deel_plaatsing, pas_toe, Bewerking};
use gemeen::{alle_bestanden, gebouw, gid, hernummer, vervang, SCHEMAS};
use openaec_samenvoegen::{verschil, voeg_samen, ConflictSoort, Samenvoeging};
use proptest::collection::vec;
use proptest::option;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};
use stap::Stap;
use vorm::{controleer, vorm, Vorm};

// --- Startbestanden en gevallen. ---

fn startbestanden() -> &'static [(String, String)] {
    static S: OnceLock<Vec<(String, String)>> = OnceLock::new();
    S.get_or_init(|| {
        let mut uit: Vec<(String, String)> = SCHEMAS.iter().map(|s| (format!("gebouw-{s}"), gebouw(s))).collect();
        uit.extend(alle_bestanden().into_iter().map(|(n, t)| (n.to_string(), t)));
        if let Some(map) = std::env::var_os("OPS_VOORBEELDEN") {
            let mut paden: Vec<_> = std::fs::read_dir(&map)
                .unwrap_or_else(|e| panic!("OPS_VOORBEELDEN: {e}"))
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifc")))
                .collect();
            paden.sort();
            for p in paden {
                let naam = p.file_name().unwrap().to_string_lossy().to_string();
                if !uit.iter().any(|(n, _)| *n == naam) {
                    uit.push((naam, std::fs::read_to_string(&p).unwrap()));
                }
            }
        }
        uit
    })
}

/// Een doel: vaak een van de eerste vier (zodat ons en hun vaak hetzelfde raken), soms een willekeurig.
fn keuze() -> impl Strategy<Value = u32> {
    prop_oneof![3 => 0u32..4, 1 => any::<u32>()]
}

fn bewerking() -> impl Strategy<Value = Bewerking> {
    let w = || 0u8..3;
    prop_oneof![
        (keuze(), w()).prop_map(|(k, w)| Bewerking::Hernoem(k, w)),
        (keuze(), w()).prop_map(|(k, w)| Bewerking::Omschrijving(k, w)),
        (keuze(), keuze(), w()).prop_map(|(k, l, w)| Bewerking::Knoopwaarde(k, l, w)),
        (keuze(), w()).prop_map(|(k, w)| Bewerking::Eigenschap(k, w)),
        (keuze(), w()).prop_map(|(k, w)| Bewerking::NieuweEigenschap(k, w)),
        keuze().prop_map(Bewerking::Kopie),
        keuze().prop_map(Bewerking::Wis),
        (keuze(), keuze()).prop_map(|(k, d)| Bewerking::Verdieping(k, d)),
        keuze().prop_map(Bewerking::Splits),
        (keuze(), w()).prop_map(|(k, w)| Bewerking::Relatie(k, w)),
        (keuze(), any::<u64>()).prop_map(|(k, z)| Bewerking::SetVolgorde(k, z)),
        (keuze(), w()).prop_map(|(k, w)| Bewerking::Tijd(k, w)),
    ]
}

#[derive(Clone, Debug)]
struct Kant {
    bewerkingen: Vec<Bewerking>,
    /// Nummert de kant na de bewerkingen opnieuw, met dit zaad (zoals OPS bij elk opslaan).
    hernummer: Option<u64>,
}

fn kant() -> impl Strategy<Value = Kant> {
    (vec(bewerking(), 0..5), option::of(any::<u64>())).prop_map(|(bewerkingen, hernummer)| Kant { bewerkingen, hernummer })
}

#[derive(Clone, Debug)]
struct Geval {
    bestand: usize,
    /// Bewerkingen op de basis: object A gebruikt de plaatsing van object B.
    delen: Vec<(u32, u32)>,
    ons: Kant,
    hun: Kant,
}

fn geval() -> impl Strategy<Value = Geval> {
    (any::<usize>(), vec((keuze(), keuze()), 0..2), kant(), kant())
        .prop_map(|(bestand, delen, ons, hun)| Geval { bestand, delen, ons, hun })
}

struct Versies {
    naam: String,
    b: String,
    x: String,
    y: String,
}

fn met_bewerkingen(basis: &Stap, k: &Kant, teken: char) -> String {
    let mut s = basis.clone();
    let mut teller = 0;
    for b in &k.bewerkingen {
        pas_toe(&mut s, b, teken, &mut teller);
    }
    let t = s.schrijf();
    match k.hernummer {
        Some(z) => hernummer(&t, z),
        None => t,
    }
}

fn maak(g: &Geval) -> Versies {
    let bestanden = startbestanden();
    let (naam, start) = &bestanden[g.bestand % bestanden.len()];
    let b = if g.delen.is_empty() {
        start.clone()
    } else {
        let mut s = Stap::lees(start);
        for &(a, c) in &g.delen {
            deel_plaatsing(&mut s, a, c);
        }
        s.schrijf()
    };
    let basis = Stap::lees(&b);
    let x = met_bewerkingen(&basis, &g.ons, 'x');
    let y = met_bewerkingen(&basis, &g.hun, 'y');
    Versies { naam: naam.clone(), b, x, y }
}

/// De vormen van deze teksten, met als universum de objecten die in alle teksten staan.
fn vormen(teksten: &[&str]) -> Vec<Vorm> {
    let stappen: Vec<Stap> = teksten.iter().map(|t| Stap::lees(t)).collect();
    let mut universum: Option<BTreeSet<String>> = None;
    for s in &stappen {
        let g: BTreeSet<String> = s.objecten().into_keys().collect();
        universum = Some(match universum {
            None => g,
            Some(u) => u.intersection(&g).cloned().collect(),
        });
    }
    let u = universum.unwrap();
    stappen.iter().map(|s| vorm(s, &u)).collect()
}

// --- Het draaien. ---

/// Cijfers over een run, voor de lange runs.
#[derive(Default)]
struct Telling {
    gevallen: usize,
    gelukt: usize,
    conflict: usize,
    soorten: BTreeMap<String, usize>,
    per_bestand: BTreeMap<String, usize>,
}

fn draai<S: Strategy>(naam: &str, strategie: S, test: impl Fn(&S::Value) -> Result<(), TestCaseError>)
where
    S::Value: Debug,
{
    let gevallen = std::env::var("EIGENSCHAPPEN_GEVALLEN").ok().and_then(|g| g.parse().ok()).unwrap_or(128);
    let config = Config { cases: gevallen, max_shrink_iters: 512, failure_persistence: None, ..Config::default() };
    let rng = match std::env::var("EIGENSCHAPPEN_ZAAD").ok().and_then(|z| z.parse::<u64>().ok()) {
        Some(z) => {
            let mut zaad = [0u8; 32];
            zaad[..8].copy_from_slice(&z.to_le_bytes());
            TestRng::from_seed(RngAlgorithm::ChaCha, &zaad)
        }
        None => TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    };
    let mut runner = TestRunner::new_with_rng(config, rng);
    if let Err(e) = runner.run(&strategie, |v| test(&v)) {
        panic!("{naam}: {e}");
    }
}

fn fout(e: openaec_samenvoegen::Fout) -> TestCaseError {
    TestCaseError::fail(format!("fout van de bibliotheek: {e}"))
}

fn tekst(s: &Samenvoeging) -> String {
    String::from_utf8(s.inhoud.clone().unwrap()).unwrap()
}

/// Bewaart de versies van een mislukt geval in `EIGENSCHAPPEN_UITVOER`. Bij het verkleinen overschrijft elk kleiner
/// geval het vorige.
fn bewaar_fout(v: &Versies) {
    if let Some(map) = std::env::var_os("EIGENSCHAPPEN_UITVOER") {
        let map = std::path::Path::new(&map);
        std::fs::create_dir_all(map).unwrap();
        for (naam, t) in [("basis", &v.b), ("ons", &v.x), ("hun", &v.y)] {
            std::fs::write(map.join(format!("{naam}.ifc")), t).unwrap();
        }
        std::fs::write(map.join("bestand.txt"), &v.naam).unwrap();
    }
}

fn bewaar_resultaat(t: &str) {
    static TELLER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if let Some(map) = std::env::var_os("SAMENVOEGEN_UITVOER") {
        let n = TELLER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::fs::create_dir_all(&map).unwrap();
        std::fs::write(std::path::Path::new(&map).join(format!("eigenschappen-{n}.ifc")), t).unwrap();
    }
}

/// Het resultaat leest weer in (met de bibliotheek en met de eigen lezer) en verwijst nergens naar iets dat er niet
/// is.
fn leesbaar(t: &str, wat: &str) -> Result<(), TestCaseError> {
    match verschil(t.as_bytes(), t.as_bytes()) {
        Ok(v) => prop_assert!(v.is_leeg(), "{wat}: verschil met zichzelf is niet leeg"),
        Err(e) => return Err(TestCaseError::fail(format!("{wat}: leest niet in: {e}"))),
    }
    let f = Stap::lees(t).losse_verwijzingen();
    prop_assert!(f.is_empty(), "{wat}: {f:?}");
    Ok(())
}

fn conflict_is_netjes(s: &Samenvoeging, wat: &str) -> Result<(), TestCaseError> {
    if !s.gelukt {
        prop_assert!(s.inhoud.is_none(), "{wat}: conflict, maar toch een inhoud");
        prop_assert!(!s.conflicten.is_empty(), "{wat}: niet gelukt, maar geen conflict in het rapport");
    }
    Ok(())
}

fn controleer_geval(g: &Geval, telling: &RefCell<Telling>) -> Result<(), TestCaseError> {
    let v = maak(g);
    let uitkomst = controleer_versies(&v, telling);
    if uitkomst.is_err() {
        bewaar_fout(&v);
    }
    uitkomst
}

fn controleer_versies(v: &Versies, telling: &RefCell<Telling>) -> Result<(), TestCaseError> {
    let (b, x, y) = (v.b.as_bytes(), v.x.as_bytes(), v.y.as_bytes());
    let naam = &v.naam;
    // Een bewerking kan (zelden) knopen in een kring zetten, bijv. twee plaatsingen relatief aan elkaar. Dat kent de
    // bibliotheek niet (README, "Wat het nog niet kan"); zo'n geval slaan we over. Elke andere fout is een vondst.
    for kant in [x, y] {
        match verschil(b, kant) {
            Err(openaec_samenvoegen::Fout::Kring { .. }) => return Err(TestCaseError::reject("kring")),
            Err(e) => return Err(fout(e)),
            Ok(_) => {}
        }
    }

    // Wijzigde een kant niets, dan is het resultaat de andere kant, byte voor byte.
    let r = voeg_samen(b, x, b).map_err(fout)?;
    prop_assert!(r.gelukt, "{naam}: voeg_samen(b, x, b) gaf een conflict: {:#?}", r.conflicten);
    prop_assert!(r.inhoud.as_deref() == Some(x), "{naam}: voeg_samen(b, x, b) is niet x");
    let r = voeg_samen(b, b, y).map_err(fout)?;
    prop_assert!(r.gelukt, "{naam}: voeg_samen(b, b, y) gaf een conflict: {:#?}", r.conflicten);
    prop_assert!(r.inhoud.as_deref() == Some(y), "{naam}: voeg_samen(b, b, y) is niet y");

    // Dezelfde wijziging aan beide kanten.
    let r = voeg_samen(b, x, x).map_err(fout)?;
    prop_assert!(r.gelukt, "{naam}: voeg_samen(b, x, x) gaf een conflict: {:#?}", r.conflicten);
    let rt = tekst(&r);
    leesbaar(&rt, "voeg_samen(b, x, x)")?;
    let vs = vormen(&[&v.x, &rt]);
    prop_assert!(vs[0] == vs[1], "{naam}: voeg_samen(b, x, x) is niet x:\n{}", controleer(&vs[0], &vs[0], &vs[0], &vs[1]).join("\n"));

    // Ons en hun, in beide volgordes.
    let r1 = voeg_samen(b, x, y).map_err(fout)?;
    let r2 = voeg_samen(b, y, x).map_err(fout)?;
    conflict_is_netjes(&r1, "voeg_samen(b, x, y)")?;
    conflict_is_netjes(&r2, "voeg_samen(b, y, x)")?;
    // Streng: een conflict van de zelfcontrole is een fout in de schrijver (het samenvoegen besloot dat het kon, maar
    // het geschreven resultaat klopte niet). Er gaat niets verloren, maar zo krijgen we een klein voorbeeld.
    if std::env::var_os("EIGENSCHAPPEN_STRENG").is_some() {
        for r in [&r1, &r2] {
            let zelf: Vec<_> = r.conflicten.iter().filter(|c| c.soort == ConflictSoort::Zelfcontrole).collect();
            prop_assert!(zelf.is_empty(), "{naam}: zelfcontrole: {zelf:#?}");
        }
    }
    {
        let mut t = telling.borrow_mut();
        t.gevallen += 1;
        *t.per_bestand.entry(naam.clone()).or_default() += 1;
        if r1.gelukt {
            t.gelukt += 1;
        } else {
            t.conflict += 1;
            for c in &r1.conflicten {
                *t.soorten.entry(format!("{:?}", c.soort)).or_default() += 1;
            }
        }
    }
    prop_assert_eq!(
        r1.gelukt,
        r2.gelukt,
        "{}: symmetrie: (b, x, y) gaf {:#?}\n(b, y, x) gaf {:#?}",
        naam,
        r1.conflicten,
        r2.conflicten
    );
    if !r1.gelukt {
        return Ok(());
    }
    let (t1, t2) = (tekst(&r1), tekst(&r2));
    leesbaar(&t1, "voeg_samen(b, x, y)")?;
    leesbaar(&t2, "voeg_samen(b, y, x)")?;
    bewaar_resultaat(&t1);
    let vs = vormen(&[&v.b, &v.x, &v.y, &t1, &t2]);
    let f = controleer(&vs[0], &vs[1], &vs[2], &vs[3]);
    prop_assert!(f.is_empty(), "{naam}: voeg_samen(b, x, y):\n{}", f.join("\n"));
    let f = controleer(&vs[0], &vs[2], &vs[1], &vs[4]);
    prop_assert!(f.is_empty(), "{naam}: voeg_samen(b, y, x):\n{}", f.join("\n"));
    prop_assert!(
        vs[3].zelfde_inhoud(&vs[4]),
        "{naam}: symmetrie: (b, x, y) en (b, y, x) geven een ander model:\n{}",
        controleer(&vs[3], &vs[3], &vs[3], &vs[4]).join("\n")
    );
    Ok(())
}

fn druk_telling(naam: &str, t: &Telling) {
    eprintln!(
        "{naam}: {} gevallen, {} gelukt, {} met conflict; conflictsoorten {:?}; startbestanden {:?}",
        t.gevallen, t.gelukt, t.conflict, t.soorten, t.per_bestand
    );
}

// --- De eigenschappen. ---

/// Willekeurige bewerkingen aan beide kanten: de identiteiten, symmetrie, geen stil verlies, en het resultaat leest
/// weer in.
#[test]
fn samenvoegen_volgens_de_regels() {
    let telling = RefCell::new(Telling::default());
    draai("samenvoegen_volgens_de_regels", geval(), |g| controleer_geval(g, &telling));
    druk_telling("samenvoegen_volgens_de_regels", &telling.borrow());
}

/// Alleen een andere volgorde in een SET en een nieuwe nummering: het verschil is leeg.
#[test]
fn volgorde_en_nummers_geven_geen_verschil() {
    let strategie = (any::<usize>(), vec((keuze(), any::<u64>()), 1..4), any::<u64>());
    draai("volgorde_en_nummers_geven_geen_verschil", strategie, |(bestand, volgordes, zaad)| {
        let bestanden = startbestanden();
        let (naam, b) = &bestanden[bestand % bestanden.len()];
        let mut s = Stap::lees(b);
        let mut teller = 0;
        for &(k, z) in volgordes {
            pas_toe(&mut s, &Bewerking::SetVolgorde(k, z), 'x', &mut teller);
        }
        let x = hernummer(&s.schrijf(), *zaad);
        let v = verschil(b.as_bytes(), x.as_bytes()).map_err(fout)?;
        prop_assert!(v.is_leeg(), "{naam}: verschil na alleen volgorde en nummers: {v:#?}");
        let vs = vormen(&[b, &x]);
        prop_assert!(vs[0] == vs[1], "{naam}: de eigen vorm ziet een verschil");
        Ok(())
    });
}

// --- De controle zelf: ziet ze wat ze moet zien? ---

/// De controle ziet een verloren wijziging: een resultaat zonder de naam van hun.
#[test]
fn controle_ziet_verlies() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let x = vervang(&b, "'W',", "'W-ons',");
        let y = vervang(&b, "'V',", "'V-hun',");
        let r = voeg_samen(b.as_bytes(), x.as_bytes(), y.as_bytes()).unwrap();
        assert!(r.gelukt);
        let vs = vormen(&[&b, &x, &y, &tekst(&r), &x]);
        assert!(controleer(&vs[0], &vs[1], &vs[2], &vs[3]).is_empty());
        let fouten = controleer(&vs[0], &vs[1], &vs[2], &vs[4]);
        assert_eq!(fouten.len(), 1, "{fouten:?}");
        assert!(fouten[0].contains(&gid("WandV")), "{fouten:?}");
    }
}

/// De controle ziet een herplaatsing via een anker: W onder de plaatsing van Q, met dezelfde inhoud als die van P
/// (het voorbeeld uit ontwerp §5.6).
#[test]
fn controle_ziet_ankers() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let x = vervang(&b, "#40=IFCLOCALPLACEMENT(#20,#41);", "#40=IFCLOCALPLACEMENT(#23,#41);");
        let vs = vormen(&[&b, &x]);
        let w = gid("WandW");
        assert_ne!(vs[0].objecten[&w], vs[1].objecten[&w], "{schema}");
        let v = gid("WandV");
        assert_eq!(vs[0].objecten[&v], vs[1].objecten[&v], "{schema}");
    }
}

/// De controle ziet dat een gedeelde plaatsing gesplitst is.
#[test]
fn controle_ziet_delen() {
    for schema in SCHEMAS {
        let b = vervang(&gebouw(schema), ",#45,$,$,$);", ",#40,$,$,$);");
        let x = gemeen::voeg_toe(&vervang(&b, ",#40,$,$,$);\n#45", ",#99,$,$,$);\n#45"), "#99=IFCLOCALPLACEMENT(#20,#41);");
        let vs = vormen(&[&b, &x]);
        for naam in ["WandW", "WandV"] {
            assert_ne!(vs[0].objecten[&gid(naam)], vs[1].objecten[&gid(naam)], "{schema} {naam}");
        }
    }
}
