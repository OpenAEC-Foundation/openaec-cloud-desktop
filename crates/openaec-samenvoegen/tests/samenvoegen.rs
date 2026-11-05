//! Samenvoegen: de regels uit ontwerp §5.6, de verplichte tegenvoorbeelden uit de reviews, en eigenschappen op echte
//! bestanden. De kleine gevallen draaien in IFC2X3, IFC4 en IFC4X3 (`gebouw`).

mod gemeen;

use gemeen::*;
use openaec_samenvoegen::{verschil, voeg_samen, BijConflict, ConflictSoort, Kant, Opties, Samenvoeging};

fn voeg(b: &str, o: &str, h: &str) -> Samenvoeging {
    voeg_met(b, o, h, BijConflict::Weiger)
}

fn voeg_met(b: &str, o: &str, h: &str, bij_conflict: BijConflict) -> Samenvoeging {
    voeg_samen(b.as_bytes(), o.as_bytes(), h.as_bytes(), &Opties { bij_conflict }).unwrap()
}

/// Het resultaat; de samenvoeging moet zonder conflict lukken. Met `SAMENVOEGEN_UITVOER=<map>` schrijven we elk
/// resultaat ook naar die map, voor `scripts/valideer_met_ifcopenshell.py`.
fn gelukt(b: &str, o: &str, h: &str) -> String {
    let s = voeg(b, o, h);
    assert!(s.gelukt && s.conflicten.is_empty(), "conflicten: {:#?}", s.conflicten);
    let r = String::from_utf8(s.inhoud.unwrap()).unwrap();
    bewaar(&r);
    r
}

fn bewaar(r: &str) {
    static TELLER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if let Some(map) = std::env::var_os("SAMENVOEGEN_UITVOER") {
        let n = TELLER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let test = std::thread::current().name().unwrap_or("test").replace("::", "-");
        std::fs::create_dir_all(&map).unwrap();
        std::fs::write(std::path::Path::new(&map).join(format!("{test}-{n}.ifc")), r).unwrap();
    }
}

/// De soorten conflicten; de samenvoeging moet mislukken.
fn conflicten(b: &str, o: &str, h: &str) -> Vec<ConflictSoort> {
    let s = voeg(b, o, h);
    assert!(!s.gelukt && s.inhoud.is_none(), "geen conflict; statistiek: {:?}", s.statistiek);
    s.conflicten.iter().map(|c| c.soort).collect()
}

/// `a` en `b` zijn inhoudelijk gelijk.
fn zelfde(a: &str, b: &str) {
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    assert!(v.is_leeg(), "verschil: {v:#?}\nresultaat:\n{a}");
}

/// Voegt `regels` toe en past daarna de vervangingen toe.
fn met(tekst: &str, vervangingen: &[(&str, &str)], regels: &str) -> String {
    let mut t = tekst.to_string();
    for (van, naar) in vervangingen {
        t = vervang(&t, van, naar);
    }
    if regels.is_empty() {
        t
    } else {
        voeg_toe(&t, regels)
    }
}

fn bevat(elementen: &str) -> String {
    format!("#50=IFCRELCONTAINEDINSPATIALSTRUCTURE('{}',#5,$,$,({elementen}),#30);", gid("BevatP"))
}

fn in_q(n: u32, naam: &str, elementen: &str) -> String {
    format!("#{n}=IFCRELCONTAINEDINSPATIALSTRUCTURE('{}',#5,$,$,({elementen}),#31);", gid(naam))
}

fn defs(elementen: &str) -> String {
    format!("#62=IFCRELDEFINESBYPROPERTIES('{}',#5,$,$,({elementen}),#61);", gid("Defs"))
}
const BRAND: &str = "IFCINTEGER(30)";

// --- De gewone regels. ---

#[test]
fn wijzigingen_aan_verschillende_objecten() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
        let h = vervang(&b, "#5,'V',$", "#5,'V-hun',$");
        let verwacht = vervang(&o, "#5,'V',$", "#5,'V-hun',$");
        let r = gelukt(&b, &o, &hernummer(&h, 7));
        zelfde(&r, &verwacht);
        // Ons blijft staan zoals het was, op die ene regel na.
        let ons_regels: Vec<&str> = o.lines().collect();
        let anders = r.lines().filter(|l| !ons_regels.contains(l)).count();
        assert_eq!(anders, 1, "{schema}:\n{r}");
    }
}

#[test]
fn hetzelfde_attribuut_anders_is_een_conflict() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
        let h = vervang(&b, "#5,'W',$", "#5,'W-hun',$");
        let s = voeg(&b, &o, &h);
        assert!(!s.gelukt);
        let c = &s.conflicten[0];
        assert_eq!((c.soort, c.attribuut.as_deref()), (ConflictSoort::BeideGewijzigd, Some("Name")));
        assert_eq!(
            (c.basis.as_deref(), c.ons.as_deref(), c.hun.as_deref()),
            (Some("'W'"), Some("'W-ons'"), Some("'W-hun'"))
        );
        assert_eq!(c.globalid.as_deref(), Some(gid("WandW").as_str()));
        // Met een winnaar per conflict.
        for (keuze, naam) in [(BijConflict::Ons, "'W-ons'"), (BijConflict::Hun, "'W-hun'")] {
            let s = voeg_met(&b, &o, &h, keuze);
            assert!(s.gelukt, "{:#?}", s.conflicten);
            assert_eq!(s.conflicten.len(), 1);
            let gekozen = if keuze == BijConflict::Ons { Kant::Ons } else { Kant::Hun };
            assert_eq!(s.conflicten[0].gekozen, Some(gekozen));
            let r = String::from_utf8(s.inhoud.unwrap()).unwrap();
            assert!(r.contains(&format!("#5,{naam},$")), "{r}");
        }
    }
}

#[test]
fn dezelfde_wijziging_en_andere_attributen() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        // Beide dezelfde naam; ons ook een beschrijving, hun een ObjectType.
        let o = vervang(&b, "#5,'W',$,$,#40", "#5,'W2','ons',$,#40");
        let h = vervang(&b, "#5,'W',$,$,#40", "#5,'W2',$,'hun',#40");
        let verwacht = vervang(&b, "#5,'W',$,$,#40", "#5,'W2','ons','hun',#40");
        zelfde(&gelukt(&b, &o, &h), &verwacht);
    }
}

#[test]
fn verzameling_samenvoegen() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let x1 = format!(
            "#70=IFCLOCALPLACEMENT(#20,#71);\n#71=IFCAXIS2PLACEMENT3D(#72,$,$);\n#72=IFCCARTESIANPOINT((5.,0.,0.));\n\
             #73=IFCBUILDINGELEMENTPROXY('{}',#5,'X1',$,$,#70,$,$,$);",
            gid("X1")
        );
        let x2 = format!(
            "#80=IFCLOCALPLACEMENT(#20,#81);\n#81=IFCAXIS2PLACEMENT3D(#82,$,$);\n#82=IFCCARTESIANPOINT((6.,0.,0.));\n\
             #83=IFCBUILDINGELEMENTPROXY('{}',#5,'X2',$,$,#80,$,$,$);",
            gid("X2")
        );
        let o = met(&b, &[(bevat("#43,#44").as_str(), bevat("#43,#44,#73").as_str())], &x1);
        let h = met(&b, &[(bevat("#43,#44").as_str(), bevat("#44,#83").as_str())], &x2);
        // Ons voegt X1 toe, hun voegt X2 toe en haalt W eruit.
        let verwacht = met(&b, &[(bevat("#43,#44").as_str(), bevat("#44,#73,#83").as_str())], &format!("{x1}\n{x2}"));
        // W zit dan nergens meer in; dat mag (hun haalde hem er bewust uit).
        zelfde(&gelukt(&b, &o, &hernummer(&h, 3)), &verwacht);
    }
}

fn met_twee_eigenschappen(schema: &str) -> String {
    met(
        &gebouw(schema),
        &[("'Pset_Test',$,(#60));", "'Pset_Test',$,(#60,#63));")],
        "#63=IFCPROPERTYSINGLEVALUE('Akoestiek',$,IFCINTEGER(40),$);",
    )
}

#[test]
fn eigenschappen_per_naam() {
    for schema in SCHEMAS {
        let b = met_twee_eigenschappen(schema);
        let o = vervang(&b, BRAND, "IFCINTEGER(60)");
        let h = vervang(&b, "IFCINTEGER(40)", "IFCINTEGER(45)");
        let verwacht = vervang(&o, "IFCINTEGER(40)", "IFCINTEGER(45)");
        zelfde(&gelukt(&b, &o, &hernummer(&h, 11)), &verwacht);
        // Twee andere waarden voor dezelfde eigenschap.
        let h = vervang(&b, BRAND, "IFCINTEGER(90)");
        let s = voeg(&b, &o, &h);
        assert!(!s.gelukt);
        assert_eq!(s.conflicten[0].attribuut.as_deref(), Some("HasProperties[Brandwerendheid]"), "{:#?}", s.conflicten);
        // Een eigenschap erbij aan beide kanten, met een andere naam.
        let o =
            met(&b, &[("(#60,#63));", "(#60,#63,#64));")], "#64=IFCPROPERTYSINGLEVALUE('Kleur',$,IFCLABEL('rood'),$);");
        let h =
            met(&b, &[("(#60,#63));", "(#60,#63,#64));")], "#64=IFCPROPERTYSINGLEVALUE('Gewicht',$,IFCINTEGER(7),$);");
        let verwacht = met(
            &b,
            &[("(#60,#63));", "(#60,#63,#64,#65));")],
            "#64=IFCPROPERTYSINGLEVALUE('Kleur',$,IFCLABEL('rood'),$);\n#65=IFCPROPERTYSINGLEVALUE('Gewicht',$,IFCINTEGER(7),$);",
        );
        zelfde(&gelukt(&b, &o, &h), &verwacht);
    }
}

#[test]
fn lijst_is_een_conflict() {
    // IfcRelNests.RelatedObjects is een LIST in IFC4 en IFC4X3 (in IFC2X3 een SET).
    for schema in ["IFC4", "IFC4X3_ADD2"] {
        let b = met(&gebouw(schema), &[], &format!("#90=IFCRELNESTS('{}',#5,$,$,#43,(#44));", gid("Nest")));
        let o = vervang(&b, "#43,(#44));", "#43,(#44,#31));");
        let h = vervang(&b, "#43,(#44));", "#43,(#44,#30));");
        assert!(conflicten(&b, &o, &h).contains(&ConflictSoort::BeideGewijzigd), "{schema}");
    }
}

#[test]
fn verwijderd_en_gewijzigd() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = met(
            &verwijder_regel(&b, "#44="),
            &[(bevat("#43,#44").as_str(), bevat("#43").as_str()), (defs("#43,#44").as_str(), defs("#43").as_str())],
            "",
        );
        let h = vervang(&b, "#5,'V',$", "#5,'V-hun',$");
        assert!(conflicten(&b, &o, &h).contains(&ConflictSoort::VerwijderdEnGewijzigd), "{schema}");
        // Andersom ook.
        assert!(conflicten(&b, &h, &o).contains(&ConflictSoort::VerwijderdEnGewijzigd), "{schema}");
    }
}

#[test]
fn beide_toegevoegd_met_andere_inhoud() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let x = |naam: &str| {
            format!(
                "#70=IFCLOCALPLACEMENT(#20,#71);\n#71=IFCAXIS2PLACEMENT3D(#72,$,$);\n#72=IFCCARTESIANPOINT((5.,0.,0.));\n\
                 #73=IFCBUILDINGELEMENTPROXY('{}',#5,'{naam}',$,$,#70,$,$,$);",
                gid("X")
            )
        };
        let o = voeg_toe(&b, &x("X-ons"));
        let h = voeg_toe(&b, &x("X-hun"));
        assert_eq!(conflicten(&b, &o, &h), vec![ConflictSoort::BeideToegevoegd], "{schema}");
        // Dezelfde inhoud: geen conflict.
        zelfde(&gelukt(&b, &o, &hernummer(&o, 2)), &o);
    }
}

#[test]
fn verwijzing_naar_verwijderd_object() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        // Ons verwijdert verdieping Q; hun zet W in Q.
        let o = met(&verwijder_regel(&b, "#31="), &[("#6,(#30,#31));", "#6,(#30));")], "");
        let h = met(&b, &[(bevat("#43,#44").as_str(), bevat("#44").as_str())], &in_q(51, "BevatQ", "#43"));
        let soorten = conflicten(&b, &o, &h);
        assert!(soorten.contains(&ConflictSoort::VerwijzingNaarVerwijderd), "{schema}: {soorten:?}");
    }
}

// --- De verplichte tegenvoorbeelden uit de reviews. ---

/// Bijsturing 1, punt 1: ons hangt W onder verdieping Q (plaatsing en verdieping); de plaatsingen van P en Q hebben
/// dezelfde inhoud. Hun verplaatst P en geeft P een andere naam. De herplaatsing van ons blijft.
#[test]
fn herplaatsing_door_ons_blijft() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = met(
            &b,
            &[
                ("#40=IFCLOCALPLACEMENT(#20,#41);", "#40=IFCLOCALPLACEMENT(#23,#41);"),
                (bevat("#43,#44").as_str(), bevat("#44").as_str()),
            ],
            &in_q(51, "BevatQ", "#43"),
        );
        let h = met(
            &b,
            &[
                ("#22=IFCCARTESIANPOINT((0.,0.,0.));", "#22=IFCCARTESIANPOINT((0.,0.,3.));"),
                ("#5,'P',$", "#5,'P-hun',$"),
            ],
            "",
        );
        let verwacht = met(
            &o,
            &[
                ("#22=IFCCARTESIANPOINT((0.,0.,0.));", "#22=IFCCARTESIANPOINT((0.,0.,3.));"),
                ("#5,'P',$", "#5,'P-hun',$"),
            ],
            "",
        );
        let r = gelukt(&b, &o, &hernummer(&h, 5));
        zelfde(&r, &verwacht);
        // W hangt in het resultaat onder Q, ook al hebben de plaatsingen van P en Q eerst dezelfde inhoud.
        let v = verschil(b.as_bytes(), r.as_bytes()).unwrap();
        let w = v.gewijzigd.iter().find(|w| w.naam.as_deref() == Some("W")).unwrap();
        assert!(w.attributen[0].nieuw.contains("ObjectPlacement van Q"), "{schema}: {w:?}");
    }
}

/// Bijsturing 1, punt 2a: ons en hun zetten W elk via een nieuwe relatie in een verdieping. Elke kant apart klopt
/// (W zit in één verdieping), samen zit W er in twee: meer dan de INVERSE ContainedInStructure [0:1] toestaat.
#[test]
fn twee_nieuwe_relaties_naar_twee_verdiepingen() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = met(&b, &[(bevat("#43,#44").as_str(), bevat("#44").as_str())], &in_q(51, "BevatQOns", "#43"));
        let h = met(&b, &[(bevat("#43,#44").as_str(), bevat("#44").as_str())], &in_q(51, "BevatQHun", "#43"));
        let s = voeg(&b, &o, &h);
        assert!(!s.gelukt);
        let c = s.conflicten.iter().find(|c| c.soort == ConflictSoort::TeVeelRelaties).expect("TeVeelRelaties");
        assert_eq!(c.attribuut.as_deref(), Some("ContainedInStructure"), "{schema}");
        assert_eq!(c.naam.as_deref(), Some("W"));
    }
}

/// Bijsturing 1, punt 2b: ons verwijdert W met zijn eigen eigenschappenset; hun wijzigt een eigenschap daarin.
#[test]
fn verwijderde_wand_met_eigen_eigenschappenset() {
    for schema in SCHEMAS {
        let b = met(
            &gebouw(schema),
            &[],
            &format!(
                "#63=IFCPROPERTYSINGLEVALUE('Kleur',$,IFCLABEL('rood'),$);\n#64=IFCPROPERTYSET('{}',#5,'Pset_W',$,(#63));\n\
                 #65=IFCRELDEFINESBYPROPERTIES('{}',#5,$,$,(#43),#64);",
                gid("PsetW"),
                gid("DefsW")
            ),
        );
        let o = met(
            &verwijder_regel(&verwijder_regel(&verwijder_regel(&verwijder_regel(&b, "#43="), "#63="), "#64="), "#65="),
            &[(bevat("#43,#44").as_str(), bevat("#44").as_str()), (defs("#43,#44").as_str(), defs("#44").as_str())],
            "",
        );
        let h = vervang(&b, "IFCLABEL('rood')", "IFCLABEL('blauw')");
        let soorten = conflicten(&b, &o, &h);
        assert!(
            soorten.contains(&ConflictSoort::KoppelingVerdwenen)
                || soorten.contains(&ConflictSoort::VerwijderdEnGewijzigd),
            "{schema}: {soorten:?}"
        );
        // Hun verwijdert, ons wijzigt: ook een conflict.
        let soorten = conflicten(&b, &h, &o);
        assert!(!soorten.is_empty(), "{schema}");
    }
}

/// Bijsturing 1, punt 3: hun past in een planning van OPS de auteur aan (FILE_NAME en IFCPERSON); ons wijzigt een
/// taak. De auteur van hun blijft.
#[test]
fn auteur_van_hun_blijft() {
    let b = bestand("13-supermarkt-albert-heijn.ifc");
    let o = vervang(&b, "'Clear out store'", "'Clear out store (ons)'");
    let h =
        vervang(&b, "('Anne Willems'),('Retail Build Express BV')", "('Bram de Vries'),('Retail Build Express BV')");
    let h = vervang(&h, "#1=IFCPERSON($,'Anne Willems',", "#1=IFCPERSON($,'Bram de Vries',");
    let verwacht =
        vervang(&o, "('Anne Willems'),('Retail Build Express BV')", "('Bram de Vries'),('Retail Build Express BV')");
    let verwacht = vervang(&verwacht, "#1=IFCPERSON($,'Anne Willems',", "#1=IFCPERSON($,'Bram de Vries',");
    let r = gelukt(&b, &o, &hernummer(&h, 9));
    zelfde(&r, &verwacht);
    assert!(regel(&r, "FILE_NAME(").contains("'Bram de Vries'"));
    // De tijdstempel blijft die van ons.
    assert_eq!(regel(&r, "FILE_NAME("), regel(&verwacht, "FILE_NAME("));
    // In de drie schema's, met het kleine gebouw.
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
        let h = vervang(&vervang(&b, "('Anne'),", "('Bram'),"), "IFCPERSON($,'Anne',", "IFCPERSON($,'Bram',");
        let verwacht = vervang(&vervang(&o, "('Anne'),", "('Bram'),"), "IFCPERSON($,'Anne',", "IFCPERSON($,'Bram',");
        zelfde(&gelukt(&b, &o, &h), &verwacht);
        // Beide een andere auteur: conflict in de HEADER.
        let o2 = vervang(&o, "('Anne'),", "('Cor'),");
        assert!(conflicten(&b, &o2, &h).contains(&ConflictSoort::Header), "{schema}");
    }
}

/// Bijsturing 2 en ontwerp §5.6: W en V delen eigenschappenset P. Ons verwijdert W; hun zet de brandwerendheid in P
/// van 30 naar 60.
#[test]
fn gedeelde_eigenschappenset_en_verwijderde_wand() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = met(
            &verwijder_regel(&b, "#43="),
            &[(bevat("#43,#44").as_str(), bevat("#44").as_str()), (defs("#43,#44").as_str(), defs("#44").as_str())],
            "",
        );
        let h = vervang(&b, BRAND, "IFCINTEGER(60)");
        let s = voeg(&b, &o, &h);
        assert!(!s.gelukt);
        assert_eq!(s.conflicten.len(), 1, "{schema}: {:#?}", s.conflicten);
        let c = &s.conflicten[0];
        assert_eq!((c.soort, c.naam.as_deref()), (ConflictSoort::KoppelingVerdwenen, Some("W")));
        assert_eq!((c.ons.as_deref(), c.hun.as_deref()), (Some("object verwijderd"), Some("geldt, gewijzigd")));
        // Met hun als winnaar: W blijft, met P = 60. Met ons als winnaar: W is weg, P blijft 30.
        let s = voeg_met(&b, &o, &h, BijConflict::Hun);
        let r = String::from_utf8(s.inhoud.unwrap_or_else(|| panic!("hun wint: {:#?}", s.conflicten))).unwrap();
        assert!(r.contains("'W'") && r.contains("IFCINTEGER(60)"), "{r}");
        let s = voeg_met(&b, &o, &h, BijConflict::Ons);
        let r = String::from_utf8(s.inhoud.expect("ons wint")).unwrap();
        zelfde(&r, &o);
    }
}

/// Bijsturing 2, punt 2: ons haalt W en hun haalt V uit `RelatedElements = {W, V}` (elk zetten ze hem in Q). Samen
/// is de verzameling leeg, terwijl minstens 1 nodig is.
#[test]
fn lege_related_elements() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = met(&b, &[(bevat("#43,#44").as_str(), bevat("#44").as_str())], &in_q(51, "BevatQOns", "#43"));
        let h = met(&b, &[(bevat("#43,#44").as_str(), bevat("#43").as_str())], &in_q(51, "BevatQHun", "#44"));
        let s = voeg(&b, &o, &h);
        assert!(!s.gelukt);
        let c = s.conflicten.iter().find(|c| c.soort == ConflictSoort::BuitenGrenzen).expect("BuitenGrenzen");
        assert_eq!(c.attribuut.as_deref(), Some("RelatedElements"), "{schema}");
        assert!(!s.conflicten.iter().any(|c| c.soort == ConflictSoort::TeVeelRelaties));
    }
}

/// Bijsturing 2: bewust losmaken én wijzigen door één kant is geen conflict. Ons haalt P van W af en zet de
/// brandwerendheid op 60; hun geeft W een andere naam.
#[test]
fn bewust_losmaken_en_wijzigen_door_een_kant() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = met(&b, &[(defs("#43,#44").as_str(), defs("#44").as_str()), (BRAND, "IFCINTEGER(60)")], "");
        let h = vervang(&b, "#5,'W',$", "#5,'W-hun',$");
        let verwacht = vervang(&o, "#5,'W',$", "#5,'W-hun',$");
        zelfde(&gelukt(&b, &o, &h), &verwacht);
    }
}

/// Het kleine gebouw met W en V op één gedeelde plaatsing #40, en Z met een plaatsing relatief aan #40.
fn gedeelde_plaatsing(schema: &str) -> String {
    let b = gebouw(schema);
    let b = vervang(&b, "#5,'V',$,$,#45,", "#5,'V',$,$,#40,");
    let b = verwijder_regel(&verwijder_regel(&verwijder_regel(&b, "#45="), "#46="), "#47=");
    voeg_toe(
        &b,
        &format!(
            "#70=IFCLOCALPLACEMENT(#40,#71);\n#71=IFCAXIS2PLACEMENT3D(#72,$,$);\n#72=IFCCARTESIANPOINT((0.,1.,0.));\n\
             #73=IFCBUILDINGELEMENTPROXY('{}',#5,'Z',$,$,#70,$,$,$);",
            gid("WandZ")
        ),
    )
}

/// Bijsturing 2, punt 3: hun splitst de gedeelde plaatsing van W en V (V krijgt een eigen kopie); ons wijzigt een
/// eigenschap. Geen stil verlies: Z hangt in het resultaat aan de plaatsing van W alleen, zoals bij hun.
#[test]
fn gedeelde_plaatsing_splitsen() {
    for schema in SCHEMAS {
        let b = gedeelde_plaatsing(schema);
        let h = met(&b, &[("#5,'V',$,$,#40,", "#5,'V',$,$,#74,")], "#74=IFCLOCALPLACEMENT(#20,#41);");
        let o = vervang(&b, BRAND, "IFCINTEGER(60)");
        let verwacht = vervang(&h, BRAND, "IFCINTEGER(60)");
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let s = voeg(&b, oo, &hernummer(hh, 4));
            match &s.inhoud {
                Some(r) => zelfde(&String::from_utf8(r.clone()).unwrap(), &verwacht),
                None => assert!(!s.conflicten.is_empty()),
            }
            assert!(s.gelukt, "{schema}: we verwachten dat de splitsing van één kant overkomt: {:#?}", s.conflicten);
        }
        // Wijzigde ons niets, dan is het resultaat hun, byte voor byte.
        assert_eq!(voeg(&b, &b, &h).inhoud.unwrap(), h.as_bytes());
    }
}

/// Bijsturing 2, punt 3: hun laat W en V één plaatsing delen (ontdubbelen) en hangt Z daaraan; ons wijzigt een
/// eigenschap. Geen stil verlies.
#[test]
fn plaatsingen_ontdubbelen() {
    for schema in SCHEMAS {
        // Basis: W op #40, V op #45 met dezelfde inhoud, Z relatief aan #45.
        let b = gebouw(schema);
        let b = vervang(&b, "#45=IFCLOCALPLACEMENT(#20,#46);", "#45=IFCLOCALPLACEMENT(#20,#41);");
        let b = verwijder_regel(&verwijder_regel(&b, "#46="), "#47=");
        let b = voeg_toe(
            &b,
            &format!(
                "#70=IFCLOCALPLACEMENT(#45,#71);\n#71=IFCAXIS2PLACEMENT3D(#72,$,$);\n#72=IFCCARTESIANPOINT((0.,1.,0.));\n\
                 #73=IFCBUILDINGELEMENTPROXY('{}',#5,'Z',$,$,#70,$,$,$);",
                gid("WandZ")
            ),
        );
        let h = met(
            &verwijder_regel(&b, "#45="),
            &[("#5,'V',$,$,#45,", "#5,'V',$,$,#40,"), ("#70=IFCLOCALPLACEMENT(#45,", "#70=IFCLOCALPLACEMENT(#40,")],
            "",
        );
        let o = vervang(&b, BRAND, "IFCINTEGER(60)");
        let verwacht = vervang(&h, BRAND, "IFCINTEGER(60)");
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let s = voeg(&b, oo, &hernummer(hh, 6));
            assert!(s.gelukt, "{schema}: {:#?}", s.conflicten);
            zelfde(&String::from_utf8(s.inhoud.unwrap()).unwrap(), &verwacht);
        }
    }
}

/// Het kleine gebouw met type T voor W en V; T heeft eigenschappenset P_T (`HasPropertySets`) met Geluid = 30.
fn met_type(schema: &str, gerelateerd: &str, sets: &str, extra: &str) -> String {
    met(
        &gebouw(schema),
        &[],
        &format!(
            "#80=IFCBUILDINGELEMENTPROXYTYPE('{t}',#5,'T',$,$,({sets}),$,$,$,.NOTDEFINED.);\n\
             #81=IFCRELDEFINESBYTYPE('{rt}',#5,$,$,({gerelateerd}),#80);\n\
             #82=IFCPROPERTYSET('{pt}',#5,'Pset_T',$,(#83));\n\
             #83=IFCPROPERTYSINGLEVALUE('Geluid',$,IFCINTEGER(30),$);\n{extra}",
            t = gid("TypeT"),
            rt = gid("DefsT"),
            pt = gid("PsetT"),
        ),
    )
}

/// Bijsturing 3 en ontwerp §5.6: W en V hebben type T, en T heeft eigenschappenset P. Ons geeft W type T2; hun
/// wijzigt P.
#[test]
fn ander_type_terwijl_de_set_van_het_type_wijzigt() {
    for schema in SCHEMAS {
        let b = met_type(schema, "#43,#44", "#82", "");
        let o = met(
            &b,
            &[(
                format!("#81=IFCRELDEFINESBYTYPE('{}',#5,$,$,(#43,#44),#80);", gid("DefsT")).as_str(),
                format!("#81=IFCRELDEFINESBYTYPE('{}',#5,$,$,(#44),#80);", gid("DefsT")).as_str(),
            )],
            &format!(
                "#84=IFCBUILDINGELEMENTPROXYTYPE('{}',#5,'T2',$,$,$,$,$,$,.NOTDEFINED.);\n\
                 #85=IFCRELDEFINESBYTYPE('{}',#5,$,$,(#43),#84);",
                gid("TypeT2"),
                gid("DefsT2")
            ),
        );
        let h = vervang(&b, "'Geluid',$,IFCINTEGER(30)", "'Geluid',$,IFCINTEGER(60)");
        let s = voeg(&b, &o, &h);
        assert!(!s.gelukt);
        let c = s.conflicten.iter().find(|c| c.soort == ConflictSoort::KoppelingVerdwenen).expect("koppeling");
        assert_eq!(c.naam.as_deref(), Some("W"), "{schema}");
        assert!(c.basis.as_deref().unwrap().contains("Pset_T"), "{c:?}");
        // Zonder de wijziging van hun: geen conflict.
        let h2 = vervang(&b, "#5,'V',$", "#5,'V-hun',$");
        gelukt(&b, &o, &h2);
    }
}

/// Bijsturing 4 en ontwerp §5.6: W heeft type T met `HasPropertySets = {P, Q}`; P hangt ook rechtstreeks aan V. Ons
/// haalt P uit T (W houdt type T); hun zet de waarde in P van 30 naar 60.
#[test]
fn set_uit_het_type_terwijl_hun_hem_wijzigt() {
    for schema in SCHEMAS {
        let b = met_type(
            schema,
            "#43",
            "#82,#86",
            &format!(
                "#86=IFCPROPERTYSET('{}',#5,'Pset_Q',$,(#87));\n#87=IFCPROPERTYSINGLEVALUE('Kleur',$,IFCLABEL('grijs'),$);\n\
                 #88=IFCRELDEFINESBYPROPERTIES('{}',#5,$,$,(#44),#82);",
                gid("PsetQ"),
                gid("DefsPV")
            ),
        );
        let o = vervang(&b, "'T',$,$,(#82,#86)", "'T',$,$,(#86)");
        let h = vervang(&b, "'Geluid',$,IFCINTEGER(30)", "'Geluid',$,IFCINTEGER(60)");
        let s = voeg(&b, &o, &h);
        assert!(!s.gelukt, "{schema}");
        assert_eq!(s.conflicten.len(), 1, "{schema}: {:#?}", s.conflicten);
        let c = &s.conflicten[0];
        assert_eq!((c.soort, c.naam.as_deref()), (ConflictSoort::KoppelingVerdwenen, Some("W")));
        assert_eq!((c.ons.as_deref(), c.hun.as_deref()), (Some("geldt niet meer"), Some("geldt, gewijzigd")));
        // Ons haalt P weg en wijzigt hem zelf: geen conflict.
        let o2 = vervang(&o, "'Geluid',$,IFCINTEGER(30)", "'Geluid',$,IFCINTEGER(60)");
        let h2 = vervang(&b, "#5,'V',$", "#5,'V-hun',$");
        let verwacht = vervang(&o2, "#5,'V',$", "#5,'V-hun',$");
        zelfde(&gelukt(&b, &o2, &h2), &verwacht);
    }
}

/// Het kleine gebouw met een derde verdieping S onder het project.
fn met_s(schema: &str) -> String {
    met(
        &gebouw(schema),
        &[("#6,(#30,#31));", "#6,(#30,#31,#33));")],
        &format!(
            "#33=IFCBUILDINGSTOREY('{}',#5,'S',$,$,#34,$,$,.ELEMENT.,0.);\n#34=IFCLOCALPLACEMENT($,#24);",
            gid("StoreyS")
        ),
    )
}

fn onder(n: u32, naam: &str, ouder: &str, kind: &str) -> String {
    format!("#{n}=IFCRELAGGREGATES('{}',#5,$,$,{ouder},({kind}));", gid(naam))
}

/// Bijsturing 3 en ontwerp §5.6: het project bevat P, Q en S. Ons hangt P onder Q, hun hangt Q onder P. Elke kant
/// apart is een goede boom; samen is er een kring en hangen P en Q niet meer aan het project.
#[test]
fn kring_in_de_boom() {
    for schema in SCHEMAS {
        let b = met_s(schema);
        let o = met(&b, &[("#6,(#30,#31,#33));", "#6,(#31,#33));")], &onder(35, "PonderQ", "#31", "#30"));
        let h = met(&b, &[("#6,(#30,#31,#33));", "#6,(#30,#33));")], &onder(35, "QonderP", "#30", "#31"));
        let soorten = conflicten(&b, &o, &h);
        assert!(soorten.contains(&ConflictSoort::Boom), "{schema}: {soorten:?}");
        // Een herindeling door één kant gaat goed.
        let h2 = vervang(&b, "#5,'W',$", "#5,'W-hun',$");
        let verwacht = vervang(&o, "#5,'W',$", "#5,'W-hun',$");
        zelfde(&gelukt(&b, &o, &h2), &verwacht);
    }
}

// --- Eigenschappen op echte bestanden. ---

/// `voeg_samen(b, b, b) = b`, `(b, ons, b) = ons` en `(b, b, hun) = hun`, byte voor byte.
#[test]
fn eigenschappen_met_een_ongewijzigde_kant() {
    for (naam, b) in alle_bestanden() {
        let x = hernummer(&b, 21);
        let y = vervang_eerste_naam(&b, " (gewijzigd)");
        assert_eq!(voeg(&b, &b, &b).inhoud.unwrap(), b.as_bytes(), "{naam}");
        assert_eq!(voeg(&b, &x, &b).inhoud.unwrap(), x.as_bytes(), "{naam}");
        assert_eq!(voeg(&b, &b, &x).inhoud.unwrap(), x.as_bytes(), "{naam}");
        assert_eq!(voeg(&b, &y, &b).inhoud.unwrap(), y.as_bytes(), "{naam}");
        assert_eq!(voeg(&b, &b, &y).inhoud.unwrap(), y.as_bytes(), "{naam}");
    }
}

/// Namen van objecten (het derde attribuut, na GlobalId en OwnerHistory) die precies één keer als `'naam'` in het
/// bestand staan.
fn unieke_namen(tekst: &str) -> Vec<String> {
    let mut uit = Vec::new();
    for regel in tekst.lines() {
        let Some(begin) = regel.find("('") else { continue };
        let rest = &regel[begin + 2..];
        if rest.find('\'') != Some(22) {
            continue;
        }
        let delen: Vec<&str> = rest[23..].splitn(3, ',').map(str::trim_start).collect();
        if delen.len() < 3 || !(delen[1].starts_with('#') || delen[1] == "$") {
            continue;
        }
        let Some(naam) = delen[2].strip_prefix('\'').and_then(|r| r.split('\'').next()) else { continue };
        if naam.is_empty() || naam.contains('\\') {
            continue;
        }
        if tekst.matches(&format!("'{naam}'")).count() == 1 && !uit.contains(&naam.to_string()) {
            uit.push(naam.to_string());
        }
    }
    uit
}

fn hernoem(tekst: &str, naam: &str, achter: &str) -> String {
    vervang(tekst, &format!("'{naam}'"), &format!("'{naam}{achter}'"))
}

fn vervang_eerste_naam(tekst: &str, achter: &str) -> String {
    let naam = unieke_namen(tekst).into_iter().next().expect("een unieke naam");
    hernoem(tekst, &naam, achter)
}

/// Wijzigingen aan verschillende objecten blijven allebei, ook als hun alles anders nummert.
#[test]
fn verschillende_objecten_op_echte_bestanden() {
    for (naam, b) in alle_bestanden() {
        let namen = unieke_namen(&b);
        assert!(namen.len() >= 2, "{naam}: {namen:?}");
        let (x, y) = (&namen[0], &namen[namen.len() - 1]);
        let o = hernoem(&b, x, " (ons)");
        let h = hernoem(&b, y, " (hun)");
        let verwacht = hernoem(&o, y, " (hun)");
        let r = gelukt(&b, &o, &hernummer(&h, 13));
        zelfde(&r, &verwacht);
        // Ook andersom: ons hernummerd.
        let r = gelukt(&b, &hernummer(&o, 17), &h);
        zelfde(&r, &verwacht);
    }
}

/// Een taakdatum (in IfcTaskTime, een knoop van de taak) bij ons en een taaknaam bij hun, in een planning van OPS.
#[test]
fn taakdatum_en_taaknaam_in_een_planning() {
    let b = bestand("13-supermarkt-albert-heijn.ifc");
    let tijd = regel(&b, "#51=").to_string();
    let datum = tijd.split('\'').find(|d| d.starts_with("20")).expect("een datum").to_string();
    let nieuw = format!("2031{}", &datum[4..]);
    let o = vervang(&b, &tijd, &tijd.replacen(&datum, &nieuw, 1));
    let h = vervang(&b, "'1. Preparation'", "'1. Voorbereiding'");
    let verwacht = vervang(&o, "'1. Preparation'", "'1. Voorbereiding'");
    zelfde(&gelukt(&b, &o, &hernummer(&h, 19)), &verwacht);
    zelfde(&gelukt(&b, &hernummer(&h, 23), &o), &verwacht);
}

/// Een kleur bij hun (een losse knoop) en een naam bij ons, in een model met geometrie.
#[test]
fn kleur_bij_hun_en_naam_bij_ons() {
    let b = bestand("ifc4-tessellation-with-individual-colors.ifc");
    let h = vervang(&b, "IFCCOLOURRGBLIST(((1.0,0.0,0.0)", "IFCCOLOURRGBLIST(((0.0,0.0,1.0)");
    let o = vervang_eerste_naam(&b, " (ons)");
    let verwacht = vervang(&o, "IFCCOLOURRGBLIST(((1.0,0.0,0.0)", "IFCCOLOURRGBLIST(((0.0,0.0,1.0)");
    zelfde(&gelukt(&b, &o, &hernummer(&h, 29)), &verwacht);
    zelfde(&gelukt(&b, &h, &hernummer(&o, 31)), &verwacht);
}

/// Een gewijzigde wandgeometrie bij hun en een naam bij ons.
#[test]
fn geometrie_bij_hun_en_naam_bij_ons() {
    let b = bestand("ifc4-wall-with-opening-and-window.ifc");
    let h = vervang(
        &b,
        "#47 = IFCAXIS2PLACEMENT3D(#24, $, $);",
        "#47 = IFCAXIS2PLACEMENT3D(#99999, $, $);\n#99999 = IFCCARTESIANPOINT((0., 0., 0.5));",
    );
    let o = vervang_eerste_naam(&b, " (ons)");
    let verwacht = vervang(
        &o,
        "#47 = IFCAXIS2PLACEMENT3D(#24, $, $);",
        "#47 = IFCAXIS2PLACEMENT3D(#99999, $, $);\n#99999 = IFCCARTESIANPOINT((0., 0., 0.5));",
    );
    zelfde(&gelukt(&b, &o, &hernummer(&h, 37)), &verwacht);
    zelfde(&gelukt(&b, &h, &hernummer(&o, 41)), &verwacht);
}

#[test]
fn onbekend_schema_is_een_conflict() {
    let b = gebouw("IFC4").replace("FILE_SCHEMA(('IFC4'))", "FILE_SCHEMA(('IFC4X1'))");
    let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
    let h = vervang(&b, "#5,'V',$", "#5,'V-hun',$");
    let soorten = conflicten(&b, &o, &h);
    assert!(soorten.len() >= 3 && soorten.iter().all(|s| *s == ConflictSoort::NietOndersteund), "{soorten:?}");
    // Verschillende schema's.
    let h = gebouw("IFC2X3");
    assert_eq!(conflicten(&gebouw("IFC4"), &gebouw("IFC4"), &h), vec![ConflictSoort::NietOndersteund]);
}

#[test]
fn rapport_als_json() {
    let b = gebouw("IFC4");
    let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
    let h = vervang(&b, "#5,'W',$", "#5,'W-hun',$");
    let json = serde_json::to_value(voeg(&b, &o, &h)).unwrap();
    let c = &json["conflicten"][0];
    assert_eq!(c["soort"], "beide_gewijzigd");
    assert_eq!(c["globalid"], gid("WandW"));
    assert_eq!(c["type"], "IfcBuildingElementProxy");
    // De naam komt van ons.
    assert_eq!(c["naam"], "W-ons");
    assert_eq!(c["attribuut"], "Name");
    assert_eq!(
        (c["basis"].as_str(), c["ons"].as_str(), c["hun"].as_str()),
        (Some("'W'"), Some("'W-ons'"), Some("'W-hun'"))
    );
    assert_eq!(json["gelukt"], false);
}

#[test]
fn losse_knopen_beide_anders() {
    let b = bestand("ifc4-tessellation-with-individual-colors.ifc");
    let o = vervang(&b, "IFCCOLOURRGBLIST(((1.0,0.0,0.0)", "IFCCOLOURRGBLIST(((0.0,1.0,0.0)");
    let h = vervang(&b, "IFCCOLOURRGBLIST(((1.0,0.0,0.0)", "IFCCOLOURRGBLIST(((0.0,0.0,1.0)");
    assert_eq!(conflicten(&b, &o, &h), vec![ConflictSoort::LosseKnopen]);
    // Dezelfde kleur aan beide kanten: geen conflict.
    zelfde(&gelukt(&b, &o, &hernummer(&o, 3)), &o);
}

/// Hun zet een nieuw element X met een plaatsing relatief aan die van Q; ons verwijdert Q.
#[test]
fn plaatsing_van_een_verwijderd_object() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = met(
            &verwijder_regel(&verwijder_regel(&verwijder_regel(&verwijder_regel(&b, "#31="), "#23="), "#24="), "#25="),
            &[("#6,(#30,#31));", "#6,(#30));")],
            "",
        );
        let h = met(
            &b,
            &[(bevat("#43,#44").as_str(), bevat("#43,#44,#73").as_str())],
            &format!(
                "#70=IFCLOCALPLACEMENT(#23,#71);\n#71=IFCAXIS2PLACEMENT3D(#72,$,$);\n#72=IFCCARTESIANPOINT((5.,0.,0.));\n\
                 #73=IFCBUILDINGELEMENTPROXY('{}',#5,'X',$,$,#70,$,$,$);",
                gid("X")
            ),
        );
        let soorten = conflicten(&b, &o, &h);
        assert!(soorten.contains(&ConflictSoort::VerwijzingNaarVerwijderd), "{schema}: {soorten:?}");
    }
}

/// Wat het schrijven doet: alleen de regels van gewijzigde objecten veranderen; wat van hun komt, staat achteraan
/// met nummers boven het hoogste nummer van ons; knopen waar niets meer naar verwijst, zijn weg.
#[test]
fn schrijven_houdt_ons_en_nummert_hun_achteraan() {
    let b = gebouw("IFC4");
    let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
    // Hun verplaatst V (nieuwe punten) en voegt X toe.
    let h = met(
        &b,
        &[("#47=IFCCARTESIANPOINT((2.,0.,0.));", "#47=IFCCARTESIANPOINT((2.,5.,0.));")],
        &format!(
            "#70=IFCLOCALPLACEMENT(#20,#71);\n#71=IFCAXIS2PLACEMENT3D(#72,$,$);\n#72=IFCCARTESIANPOINT((5.,0.,0.));\n\
             #73=IFCBUILDINGELEMENTPROXY('{}',#5,'X',$,$,#70,$,$,$);",
            gid("X")
        ),
    );
    let s = voeg(&b, &o, &hernummer(&h, 8));
    assert!(s.gelukt, "{:#?}", s.conflicten);
    let r = String::from_utf8(s.inhoud.unwrap()).unwrap();
    // De HEADER en de regels van ons, behalve die van V's plaatsing, staan er nog.
    let ons: Vec<&str> = o.lines().collect();
    let erin: Vec<&str> = r.lines().collect();
    let weg: Vec<&&str> = ons.iter().filter(|l| !erin.contains(l)).collect();
    // De plaatsing van V (#45) krijgt op zijn eigen regel de inhoud van hun; de oude punten zijn weg.
    assert_eq!(
        weg,
        vec![
            &"#45=IFCLOCALPLACEMENT(#20,#46);",
            &"#46=IFCAXIS2PLACEMENT3D(#47,$,$);",
            &"#47=IFCCARTESIANPOINT((2.,0.,0.));"
        ],
        "{r}"
    );
    // Nieuwe regels hebben een nummer boven 62 (het hoogste van ons).
    for l in erin.iter().filter(|l| !ons.contains(l)) {
        let n: u64 = l.trim_start_matches('#').split('=').next().unwrap().parse().unwrap();
        assert!(n == 45 || n > 62, "{l}");
    }
    let st = &s.statistiek;
    // V zelf blijft dezelfde tekst; alleen #45 is herschreven.
    assert_eq!((st.objecten_van_hun, st.instanties_verwijderd, st.instanties_herschreven), (2, 2, 1), "{st:?}");
    assert!(r.contains("#44=IFCBUILDINGELEMENTPROXY('0WandV0000000000000000',#5,'V',$,$,#45,$,$,$);"));
}

/// Sommige programma's geven relaties bij elk opslaan een nieuwe GlobalId. Ons en hun doen dat allebei, elk
/// anders, en wijzigen elk een ander object.
#[test]
fn relaties_met_een_nieuwe_globalid_aan_beide_kanten() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let nieuw = |t: &str, kant: &str| {
            let t = vervang(t, &format!("'{}'", gid("Aggr")), &format!("'{}'", gid(&format!("Aggr{kant}"))));
            vervang(&t, &format!("'{}'", gid("BevatP")), &format!("'{}'", gid(&format!("BevatP{kant}"))))
        };
        let o = nieuw(&vervang(&b, "#5,'W',$", "#5,'W-ons',$"), "Ons");
        let h = nieuw(&vervang(&b, "#5,'V',$", "#5,'V-hun',$"), "Hun");
        let verwacht = vervang(&o, "#5,'V',$", "#5,'V-hun',$");
        let s = voeg(&b, &o, &hernummer(&h, 15));
        assert!(s.gelukt, "{schema}: {:#?}", s.conflicten);
        assert_eq!(s.statistiek.relaties_gekoppeld, 4);
        zelfde(&String::from_utf8(s.inhoud.unwrap()).unwrap(), &verwacht);
    }
}
