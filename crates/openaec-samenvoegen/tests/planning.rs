//! Samenvoegen van planningen van Open Planner Studio (OPS); zie `docs/samenwerken/planning.md`.
//!
//! De bestanden in `tests/bestanden/planning` zijn gemaakt met de code van OPS zelf (`scripts/ops/maak_planningen.sh`):
//! de supermarktplanning lezen, één wijziging zoals een planner hem maakt, doorrekenen en opslaan. Elke geslaagde
//! samenvoeging gaat door een eigen controle van de taakstructuur ([`Taken::lees`]). Met
//! `SAMENVOEGEN_UITVOER=<map>` komt elk resultaat ook in die map, voor de lezer van OPS
//! (`node scripts/ops/ops.mjs <ops-repo> lees <map>/*.ifc`) en voor IfcOpenShell.

mod gemeen;

use std::collections::HashMap;

use gemeen::*;
use openaec_samenvoegen::{verschil, voeg_samen, Conflict, ConflictSoort, Samenvoeging};

fn planning(naam: &str) -> String {
    bestand(&format!("planning/{naam}.ifc"))
}

fn voeg(b: &str, o: &str, h: &str) -> Samenvoeging {
    voeg_samen(b.as_bytes(), o.as_bytes(), h.as_bytes()).unwrap()
}

/// Het resultaat; de samenvoeging moet lukken en de taakstructuur moet kloppen.
fn gelukt(b: &str, o: &str, h: &str) -> (String, Taken) {
    let s = voeg(b, o, h);
    assert!(s.gelukt && s.conflicten.is_empty(), "conflicten: {:#?}", s.conflicten);
    let r = String::from_utf8(s.inhoud.unwrap()).unwrap();
    if let Some(map) = std::env::var_os("SAMENVOEGEN_UITVOER") {
        static TELLER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = TELLER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let test = std::thread::current().name().unwrap_or("test").replace("::", "-");
        std::fs::create_dir_all(&map).unwrap();
        std::fs::write(std::path::Path::new(&map).join(format!("planning-{test}-{n}.ifc")), &r).unwrap();
    }
    let t = Taken::lees(&r);
    (r, t)
}

/// De conflicten; de samenvoeging moet mislukken.
fn mislukt(b: &str, o: &str, h: &str) -> Vec<Conflict> {
    let s = voeg(b, o, h);
    assert!(!s.gelukt && s.inhoud.is_none(), "geen conflict; statistiek: {:?}", s.statistiek);
    s.conflicten
}

/// Wat een planner in het bestand ziet, gelezen met de losse lezer van de tests (niet met de bibliotheek), en
/// gecontroleerd: elke taak heeft een IfcTaskTime en hangt in precies één IfcRelNests; een samenvatting is een taak
/// of het werkschema; geen kring in de boom; elke IfcRelSequence loopt van taak naar taak en staat er één keer; het
/// werkschema bestuurt precies alle taken (IfcRelAssignsToControl).
struct Taken {
    f: Feiten,
    /// Naam -> IfcTask.
    taak: HashMap<String, u64>,
    /// Taak -> samenvatting (een taak of het werkschema).
    ouder: HashMap<u64, u64>,
    /// (van, naar, SequenceType), op naam.
    relaties: Vec<(String, String, String)>,
}

impl Taken {
    fn lees(r: &str) -> Self {
        let f = Feiten::lees(r);
        let taken = f.van_type("IFCTASK");
        let naam = |n: u64| f.attr(n, 2).trim_matches('\'').to_string();
        let mut taak = HashMap::new();
        for &t in &taken {
            assert_eq!(f.type_van(f.verwijzing(t, 11)), "IFCTASKTIME", "taak {}", naam(t));
            assert!(taak.insert(naam(t), t).is_none(), "twee taken heten {}", naam(t));
        }
        let mut ouder = HashMap::new();
        for nest in f.van_type("IFCRELNESTS") {
            let o = f.verwijzing(nest, 4);
            assert!(matches!(f.type_van(o), "IFCTASK" | "IFCWORKSCHEDULE"), "samenvatting #{o}");
            for kind in f.verwijzingen(nest, 5) {
                assert_eq!(f.type_van(kind), "IFCTASK");
                assert!(ouder.insert(kind, o).is_none(), "taak {} hangt in twee samenvattingen", naam(kind));
            }
        }
        for &t in &taken {
            let mut x = t;
            for _ in 0..=taken.len() {
                match ouder.get(&x) {
                    Some(&o) if f.type_van(o) == "IFCWORKSCHEDULE" => break,
                    Some(&o) => x = o,
                    None => panic!("taak {} hangt niet in de boom", naam(t)),
                }
            }
            assert_eq!(f.type_van(*ouder.get(&x).unwrap()), "IFCWORKSCHEDULE", "kring boven {}", naam(t));
        }
        let mut relaties = Vec::new();
        for s in f.van_type("IFCRELSEQUENCE") {
            let (a, b) = (f.verwijzing(s, 4), f.verwijzing(s, 5));
            assert_eq!((f.type_van(a), f.type_van(b)), ("IFCTASK", "IFCTASK"));
            let rel = (naam(a), naam(b), f.attr(s, 7).to_string());
            assert!(
                !relaties.iter().any(|x: &(String, String, String)| x.0 == rel.0 && x.1 == rel.1),
                "dubbel: {rel:?}"
            );
            relaties.push(rel);
        }
        let mut bestuurd = Vec::new();
        for c in f.van_type("IFCRELASSIGNSTOCONTROL") {
            if f.type_van(f.verwijzing(c, 6)) == "IFCWORKSCHEDULE" {
                bestuurd.extend(f.verwijzingen(c, 4));
            }
        }
        bestuurd.sort_unstable();
        assert_eq!(bestuurd, taken, "het werkschema bestuurt niet precies alle taken");
        Taken { f, taak, ouder, relaties }
    }

    fn nr(&self, naam: &str) -> u64 {
        *self.taak.get(naam).unwrap_or_else(|| panic!("geen taak {naam}"))
    }

    /// Attribuut `j` van de IfcTaskTime van de taak.
    fn tijd(&self, naam: &str, j: usize) -> String {
        self.f.attr(self.f.verwijzing(self.nr(naam), 11), j).to_string()
    }

    fn duur(&self, naam: &str) -> String {
        self.tijd(naam, 4)
    }

    fn ouder(&self, naam: &str) -> String {
        let o = self.ouder[&self.nr(naam)];
        self.f.attr(o, 2).trim_matches('\'').to_string()
    }

    fn wbs(&self, naam: &str) -> String {
        self.f.attr(self.nr(naam), 5).trim_matches('\'').to_string()
    }
}

/// Een conflict op dit attribuut, met deze waarden van ons en hun.
fn heeft(conflicten: &[Conflict], attribuut: &str, ons: &str, hun: &str) -> bool {
    conflicten.iter().any(|c| {
        c.attribuut.as_deref() == Some(attribuut) && c.ons.as_deref() == Some(ons) && c.hun.as_deref() == Some(hun)
    })
}

const STAAL: &str = "Structural steel, extension";
const STAAL_NL: &str = "Staalconstructie uitbouw";

// --- Wat OPS bij elk opslaan herschrijft. ---

/// OPS opnieuw opslaan zonder wijziging verandert alleen vluchtige velden: de tijdstempel in FILE_NAME, die van de
/// IfcOwnerHistory en de CreationDate van het werkplan en het werkschema. Een samenvoeging met zo'n versie geeft
/// het werk van de andere kant.
#[test]
fn opnieuw_opslaan_verandert_alleen_vluchtige_velden() {
    let (b, opnieuw) = (planning("basis"), planning("opnieuw"));
    let v = verschil(b.as_bytes(), opnieuw.as_bytes()).unwrap();
    assert!(v.is_leeg(), "{v:#?}");
    let mut velden: Vec<&str> = v.vluchtig.iter().map(|x| x.attribuut.as_str()).collect();
    velden.sort_unstable();
    velden.dedup();
    assert_eq!(velden, ["CreationDate", "FILE_NAME.time_stamp", "OwnerHistory.CreationDate"], "{:#?}", v.vluchtig);
    let (_, t) = gelukt(&b, &opnieuw, &planning("naam-hun"));
    assert_eq!(t.duur(STAAL_NL), "'P0Y0M5D'");
    let (_, t) = gelukt(&b, &planning("naam-hun"), &opnieuw);
    t.nr(STAAL_NL);
}

// --- Wijzigingen aan dezelfde taak. ---

/// Ons maakt de staalconstructie 20 dagen (OPS rekent de planning door), hun geeft dezelfde taak een andere naam.
/// Allebei blijven: de IfcTaskTime van de taak voegen we per attribuut samen (ons de duur en de datums, hun de naam).
#[test]
fn andere_duur_bij_ons_en_andere_naam_bij_hun() {
    let b = planning("basis");
    let (ons, hun) = (planning("duur-ons"), planning("naam-hun"));
    let (r, t) = gelukt(&b, &ons, &hun);
    assert_eq!(t.duur(STAAL_NL), "'P0Y0M20D'");
    assert_eq!(t.tijd(STAAL_NL, 0), format!("'{STAAL_NL} Time'"));
    assert!(!t.taak.contains_key(STAAL));
    // De doorgerekende datums van ons blijven, ook bij de opvolger.
    let o = Taken::lees(&ons);
    assert_eq!(t.tijd("Roof and facade, extension", 7), o.tijd("Roof and facade, extension", 7));
    assert_eq!(t.tijd(STAAL_NL, 8), o.tijd(STAAL, 8));
    // Andersom geeft inhoudelijk hetzelfde.
    let (r2, _) = gelukt(&b, &hun, &ons);
    assert!(verschil(r.as_bytes(), r2.as_bytes()).unwrap().is_leeg());
    // En met een anders genummerde versie van hun.
    gelukt(&b, &ons, &hernummer(&hun, 29));
}

/// Beide kanten geven dezelfde taak een andere duur: een conflict op het veld zelf, met beide waarden.
#[test]
fn allebei_een_andere_duur_voor_dezelfde_taak() {
    let c = mislukt(&planning("basis"), &planning("duur-ons"), &planning("duur-hun"));
    assert!(heeft(&c, "TaskTime.ScheduleDuration", "'P0Y0M20D'", "'P0Y0M18D'"), "{c:#?}");
}

/// Een andere duur voor twee taken in dezelfde keten. OPS rekent bij beide de datums en de speling van de
/// opvolgers en de projecteinddatum opnieuw uit, elk anders. Die afgeleide velden geven nu een conflict (zie
/// knownbugs.md, punt 30): er gaat niets verloren, maar een planner had beide wijzigingen verwacht.
#[test]
fn andere_duur_voor_twee_taken_in_een_keten() {
    let c = mislukt(&planning("basis"), &planning("duur-ons"), &planning("duur-ander-hun"));
    // De duur van de fase is ook afgeleid (de som van de taken eronder); die van de taken zelf botst niet.
    let duur: Vec<_> = c.iter().filter(|x| x.attribuut.as_deref() == Some("TaskTime.ScheduleDuration")).collect();
    assert!(duur.iter().all(|x| x.naam.as_deref() == Some("3. Extension")), "{duur:#?}");
    assert!(c.iter().any(|x| x.attribuut.as_deref() == Some("FinishTime")), "{c:#?}");
    assert!(c.iter().any(|x| x.attribuut.as_deref() == Some("TaskTime.EarlyStart")), "{c:#?}");
}

// --- Taken en relaties. ---

/// Ons wist een taak (met haar relaties), hun legt een nieuwe relatie naar die taak: conflict, in beide richtingen.
#[test]
fn ons_wist_een_taak_en_hun_legt_er_een_relatie_naar() {
    let (b, wis, rel) = (planning("basis"), planning("wis-ons"), planning("relatie-hun"));
    for (o, h) in [(&wis, &rel), (&rel, &wis)] {
        let c = mislukt(&b, o, h);
        assert!(c.iter().any(|x| x.soort == ConflictSoort::VerwijzingNaarVerwijderd), "{c:#?}");
    }
}

/// Een taak wissen bij ons en een andere taak hernoemen bij hun: allebei.
#[test]
fn wissen_en_hernoemen() {
    let (_, t) = gelukt(&planning("basis"), &planning("wis-ons"), &planning("naam-hun"));
    assert_eq!(t.taak.len(), 24);
    assert!(!t.taak.contains_key("Partially break out floor"));
    assert!(!t.relaties.iter().any(|r| r.0 == "Partially break out floor" || r.1 == "Partially break out floor"));
    t.nr(STAAL_NL);
}

/// Ons en hun leggen allebei dezelfde relatie (eind-start van A naar B), elk met een eigen GlobalId. Dat is één
/// relatie: het resultaat heeft hem één keer, met de GlobalId van ons.
#[test]
fn allebei_dezelfde_nieuwe_relatie() {
    let (b, ons, hun) = (planning("basis"), planning("relatie-ons"), planning("relatie-hun"));
    let relaties = |x: &str| -> Vec<String> {
        let f = Feiten::lees(x);
        f.van_type("IFCRELSEQUENCE").into_iter().map(|s| f.globalid(s).unwrap()).collect()
    };
    let oud = relaties(&b);
    let nieuw = |x: &str| relaties(x).into_iter().find(|g| !oud.contains(g)).unwrap();
    assert_ne!(nieuw(&ons), nieuw(&hun));
    for (o, h) in [(&ons, &hun), (&hun, &ons)] {
        let (r, t) = gelukt(&b, o, h);
        assert_eq!(t.relaties.len(), 21);
        let gids = relaties(&r);
        assert!(gids.contains(&nieuw(o)) && !gids.contains(&nieuw(h)));
        let rel = ("Start of refurbishment".into(), "Partially break out floor".into(), ".FINISH_START.".into());
        assert!(t.relaties.contains(&rel));
    }
}

/// Dezelfde nieuwe relatie, maar ons legt start-start en hun eind-start: conflict.
#[test]
fn dezelfde_nieuwe_relatie_met_een_ander_type() {
    let c = mislukt(&planning("basis"), &planning("relatie-ss-ons"), &planning("relatie-hun"));
    assert!(c.iter().any(|x| x.soort == ConflictSoort::BeideToegevoegd), "{c:#?}");
}

/// Een relatie toevoegen met een eigen regel, zonder doorrekenen (zoals een ander programma dat kan doen).
fn met_relatie(t: &str, naam: &str, van: &str, naar: &str) -> String {
    let x = Taken::lees(t);
    voeg_toe(
        t,
        &format!("#9001=IFCRELSEQUENCE('{}',#6,$,$,#{},#{},$,.FINISH_START.,$);", gid(naam), x.nr(van), x.nr(naar)),
    )
}

/// Het geval uit de review van Sol op het ontwerp (BLOKKEERT): ons legt A -> B, hun B -> A, in een echte planning van
/// OPS. Vóór deze regel gaf dat een resultaat met beide relaties en zonder conflict; OPS kan dat niet doorrekenen
/// (`CPMSolver.ts`: een kring geeft een fout en geen berekende taken).
#[test]
fn nieuwe_kring_ons_a_naar_b_en_hun_b_naar_a() {
    let b = planning("basis");
    let ons = met_relatie(&b, "RelOns", "Start of refurbishment", "Partially break out floor");
    let hun = met_relatie(&b, "RelHun", "Partially break out floor", "Start of refurbishment");
    for (o, h) in [(&ons, &hun), (&hun, &ons)] {
        let c = mislukt(&b, o, h);
        assert_eq!(c.len(), 1, "{c:#?}");
        assert_eq!(c[0].soort, ConflictSoort::VolgordeKring);
    }
}

/// Een langere kring: ons legt A -> B, hun B -> C, en de basis had al C -> A. Zonder kring gaat het goed.
#[test]
fn nieuwe_kring_over_een_bestaande_keten() {
    let b = planning("basis");
    // In de basis: Clear out store -> Temporary facilities -> Start of refurbishment.
    let ons = met_relatie(&b, "RelOns", "Start of refurbishment", "Partially break out floor");
    let hun = met_relatie(&b, "RelHun", "Partially break out floor", "Temporary facilities");
    for (o, h) in [(&ons, &hun), (&hun, &ons)] {
        let c = mislukt(&b, o, h);
        assert_eq!(c.len(), 1, "{c:#?}");
        assert_eq!(c[0].soort, ConflictSoort::VolgordeKring);
    }
    let ander = met_relatie(&b, "RelHun", "Partially break out floor", "Lay floor tiles");
    let (_, t) = gelukt(&b, &ons, &ander);
    assert_eq!(t.relaties.len(), 22);
}

/// Had ons elders al een kring (C -> D en D -> C), dan blijft een nieuwe kring (A -> B bij ons, B -> A bij hun) een
/// conflict. De oude kring melden we niet: die komt niet door het samenvoegen.
#[test]
fn nieuwe_kring_naast_een_oude_kring() {
    let b = planning("basis");
    let oud = met_relatie(&b, "OudCD", "Lay floor tiles", "Ceilings and lighting");
    let oud = voeg_toe(
        &oud,
        &regel(&met_relatie(&b, "OudDC", "Ceilings and lighting", "Lay floor tiles"), "#9001=")
            .replace("#9001=", "#9002="),
    );
    let ons = voeg_toe(
        &oud,
        &regel(&met_relatie(&b, "RelOns", "Start of refurbishment", "Partially break out floor"), "#9001=")
            .replace("#9001=", "#9003="),
    );
    let hun = met_relatie(&b, "RelHun", "Partially break out floor", "Start of refurbishment");
    let c = mislukt(&b, &ons, &hun);
    assert_eq!(c.len(), 1, "{c:#?}");
    assert_eq!(c[0].soort, ConflictSoort::VolgordeKring);
    assert!(c[0].melding.contains("Partially break out floor"), "{}", c[0].melding);
    assert!(!c[0].melding.contains("Lay floor tiles"), "{}", c[0].melding);
}

/// Ons hangt de kassa onder de uitbouw, hun onder de installaties: conflict.
#[test]
fn taak_aan_beide_kanten_naar_een_andere_samenvatting() {
    let c = mislukt(&planning("basis"), &planning("verplaats-ons"), &planning("verplaats-hun"));
    assert!(c.iter().any(|x| x.naam.as_deref() == Some("Checkout island")), "{c:#?}");
}

/// Ons hangt de kassa onder de uitbouw, hun hernoemt de staalconstructie: allebei.
#[test]
fn verplaatsen_en_hernoemen() {
    let (_, t) = gelukt(&planning("basis"), &planning("verplaats-ons"), &planning("naam-hun"));
    assert_eq!(t.ouder("Checkout island"), "3. Extension");
    assert_eq!(t.wbs("Checkout island"), "3.5");
    assert_eq!(t.ouder(STAAL_NL), "3. Extension");
}

/// Ons voegt een taak toe aan de uitbouw, hun aan de installaties: allebei, elk in de eigen fase.
#[test]
fn nieuwe_taken_in_verschillende_fases() {
    let (_, t) = gelukt(&planning("basis"), &planning("nieuw-ons"), &planning("nieuw-hun"));
    assert_eq!(t.taak.len(), 27);
    assert_eq!((t.ouder("Gevelreclame"), t.wbs("Gevelreclame")), ("3. Extension".into(), "3.5".into()));
    assert_eq!((t.ouder("Camerabewaking"), t.wbs("Camerabewaking")), ("4. Building services".into(), "4.5".into()));
}

/// Ons en hun voegen allebei een taak toe aan de uitbouw. De kinderen van een samenvatting zijn een LIST
/// (IfcRelNests.RelatedObjects): nu een conflict (knownbugs.md, punt 31).
#[test]
fn nieuwe_taken_in_dezelfde_fase() {
    let c = mislukt(&planning("basis"), &planning("nieuw-ons"), &planning("nieuw-zelfde-hun"));
    assert!(c.iter().any(|x| x.attribuut.as_deref() == Some("RelatedObjects")), "{c:#?}");
}

// --- Baselines. ---

/// Ons legt een baseline vast, hun hernoemt een taak: allebei.
#[test]
fn baseline_bij_ons_en_naam_bij_hun() {
    let (r, t) = gelukt(&planning("basis"), &planning("baseline-ons"), &planning("naam-hun"));
    t.nr(STAAL_NL);
    let schema = t.f.van_type("IFCWORKSCHEDULE");
    assert!(schema.iter().any(|&w| t.f.attr(w, 2) == "'Ons'" && t.f.attr(w, 13) == ".BASELINE."));
    assert!(r.contains("'OPS_Baselines'"));
}

/// Beide kanten leggen een baseline vast. OPS bewaart alle baselines in één eigenschap (OPS_Baselines): een
/// conflict, geen verlies (knownbugs.md, punt 32).
#[test]
fn allebei_een_baseline() {
    let c = mislukt(&planning("basis"), &planning("baseline-ons"), &planning("baseline-hun"));
    assert!(c.iter().any(|x| x.naam.as_deref() == Some("OPS_Baselines")), "{c:#?}");
}

// --- De regel voor een eigen knoop, los van OPS. ---

/// Een kleine planning in IFC4 of IFC4X3: taken A en B, elk met een eigen IfcTaskTime (of, met `gedeeld`, één
/// gedeelde).
fn kleine_planning(schema: &str, gedeeld: bool) -> String {
    let tijd_b = if gedeeld { "#11" } else { "#13" };
    ifc(
        schema,
        &format!(
            "#1=IFCPERSON($,'Anne',$,$,$,$,$,$);\n#2=IFCORGANIZATION($,'Bouw BV',$,$,$);\n\
             #3=IFCPERSONANDORGANIZATION(#1,#2,$);\n#4=IFCAPPLICATION(#2,'1','test','test');\n\
             #5=IFCOWNERHISTORY(#3,#4,$,.NOCHANGE.,$,$,$,1700000000);\n\
             #11=IFCTASKTIME('A Time',.PREDICTED.,$,.WORKTIME.,'P5D','2027-03-01T07:00:00',$,$,$,$,$,$,$,$,$,$,$,$,$,$);\n\
             #12=IFCTASK('{}',#5,'A',$,$,'1',$,$,$,.F.,$,#11,.CONSTRUCTION.);\n\
             #13=IFCTASKTIME('B Time',.PREDICTED.,$,.WORKTIME.,'P5D','2027-03-01T07:00:00',$,$,$,$,$,$,$,$,$,$,$,$,$,$);\n\
             #14=IFCTASK('{}',#5,'B',$,$,'2',$,$,$,.F.,$,{tijd_b},.CONSTRUCTION.);",
            gid("TaakA"),
            gid("TaakB")
        ),
    )
}

/// De regel werkt in IFC4 en IFC4X3 (IFC2X3 kent geen IfcTaskTime). Is de knoop gedeeld, dan geldt hij niet: de
/// knoop van ons herschrijven zou ook de andere taak raken.
#[test]
fn eigen_knoop_per_attribuut() {
    for schema in ["IFC4", "IFC4X3_ADD2"] {
        for gedeeld in [false, true] {
            let b = kleine_planning(schema, gedeeld);
            let o = vervang(&b, "'A Time',.PREDICTED.,$,.WORKTIME.,'P5D'", "'A Time',.PREDICTED.,$,.WORKTIME.,'P7D'");
            let h = vervang(&b, "'A Time'", "'Taak A Time'");
            let s = voeg(&b, &o, &h);
            if gedeeld {
                assert!(!s.gelukt, "{schema}");
                assert!(s.conflicten.iter().any(|c| c.attribuut.as_deref() == Some("TaskTime")), "{:#?}", s.conflicten);
                continue;
            }
            assert!(s.gelukt, "{schema}: {:#?}", s.conflicten);
            let f = Feiten::lees(std::str::from_utf8(s.inhoud.as_deref().unwrap()).unwrap());
            let tijd = f.verwijzing(f.met_globalid(&gid("TaakA")), 11);
            assert_eq!((f.attr(tijd, 0), f.attr(tijd, 4)), ("'Taak A Time'", "'P7D'"), "{schema}");
            // Beide een andere duur: conflict op het veld.
            let h2 = vervang(
                &h,
                "'Taak A Time',.PREDICTED.,$,.WORKTIME.,'P5D'",
                "'Taak A Time',.PREDICTED.,$,.WORKTIME.,'P9D'",
            );
            let c = voeg(&b, &o, &h2).conflicten;
            assert!(heeft(&c, "TaskTime.ScheduleDuration", "'P7D'", "'P9D'"), "{c:#?}");
        }
    }
}
