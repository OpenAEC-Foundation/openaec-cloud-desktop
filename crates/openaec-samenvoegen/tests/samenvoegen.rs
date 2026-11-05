//! Samenvoegen: de regels uit ontwerp §5.6, de verplichte tegenvoorbeelden uit de reviews, en eigenschappen op echte
//! bestanden. De kleine gevallen draaien in IFC2X3, IFC4 en IFC4X3 (`gebouw`).

mod gemeen;

use gemeen::*;
use openaec_samenvoegen::{verschil, voeg_samen, ConflictSoort, Samenvoeging};

fn voeg(b: &str, o: &str, h: &str) -> Samenvoeging {
    voeg_samen(b.as_bytes(), o.as_bytes(), h.as_bytes()).unwrap()
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
        // Er is geen resultaat: geen kant wint (K4).
        assert!(s.inhoud.is_none());
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

// --- Regressies uit de review van PR #7 (Sol). Elke test gaat het feit na met een losse lezer (`Feiten`), niet met
// de vergelijker van de bibliotheek. ---

const ROOD: [f64; 3] = [1., 0., 0.];
const GROEN: [f64; 3] = [0., 1., 0.];
const BLAUW: [f64; 3] = [0., 0., 1.];

/// Een vorm met een kleur vanaf nummer `n`: punt, box, representatie, productvorm, kleur, schaduw, stijl en het
/// gestileerde item. In IFC2X3 staat er een IfcPresentationStyleAssignment tussen (op `n + 100`).
fn vorm(schema: &str, n: u32, rgb: &str) -> String {
    let ifc2x3 = schema == "IFC2X3";
    let schaduw = if ifc2x3 { format!("#{}", n + 4) } else { format!("#{},$", n + 4) };
    let stijl = if ifc2x3 { n + 100 } else { n + 6 };
    let mut s = format!(
        "#{n}=IFCCARTESIANPOINT((0.,0.,0.));\n#{b}=IFCBOUNDINGBOX(#{n},1.,1.,1.);\n\
         #{r}=IFCSHAPEREPRESENTATION(#7,'Box','BoundingBox',(#{b}));\n#{p}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{r}));\n\
         #{k}=IFCCOLOURRGB($,{rgb});\n#{sch}=IFCSURFACESTYLESHADING({schaduw});\n\
         #{st}=IFCSURFACESTYLE($,.BOTH.,(#{sch}));\n#{i}=IFCSTYLEDITEM(#{b},(#{stijl}),$);",
        b = n + 1,
        r = n + 2,
        p = n + 3,
        k = n + 4,
        sch = n + 5,
        st = n + 6,
        i = n + 7,
    );
    if ifc2x3 {
        s += &format!("\n#{}=IFCPRESENTATIONSTYLEASSIGNMENT((#{}));", n + 100, n + 6);
    }
    s
}

/// Het gebouw met een rode vorm voor W (#63-#70) en, met `v_ook`, een blauwe voor V (#71-#78), zoals in Sol's
/// voorbeelden.
fn met_vormen(schema: &str, v_ook: bool) -> String {
    let b = vervang(&gebouw(schema), "#5,'W',$,$,#40,$,", "#5,'W',$,$,#40,#66,");
    let w = vorm(schema, 63, "1.,0.,0.");
    if !v_ook {
        return voeg_toe(&b, &w);
    }
    let b = vervang(&b, "#5,'V',$,$,#45,$,", "#5,'V',$,$,#45,#74,");
    voeg_toe(&b, &format!("{w}\n{}", vorm(schema, 71, "0.,0.,1.")))
}

/// Wisselt de nummers `#a` en `#b` overal: dezelfde inhoud, andere nummers.
fn wissel(tekst: &str, a: u32, b: u32) -> String {
    let mut uit = String::new();
    let mut rest = tekst;
    while let Some(p) = rest.find('#') {
        uit.push_str(&rest[..p]);
        let cijfers = rest[p + 1..].chars().take_while(|c| c.is_ascii_digit()).count();
        match rest[p + 1..p + 1 + cijfers].parse::<u32>() {
            Ok(n) => uit.push_str(&format!(
                "#{}",
                if n == a {
                    b
                } else if n == b {
                    a
                } else {
                    n
                }
            )),
            Err(_) => uit.push('#'),
        }
        rest = &rest[p + 1 + cijfers..];
    }
    uit + rest
}

/// De vorm (het eerste item van de eerste representatie) van het object met deze GlobalId.
fn vorm_van(f: &Feiten, globalid: &str) -> u64 {
    let pds = f.verwijzing(f.met_globalid(globalid), 6);
    let rep = f.verwijzingen(pds, 2)[0];
    f.verwijzingen(rep, 3)[0]
}

/// De kleuren van de gestileerde items op deze vorm.
fn kleuren_op(f: &Feiten, vorm: u64) -> Vec<[f64; 3]> {
    let mut uit = Vec::new();
    for item in f.naar("IFCSTYLEDITEM", 0, vorm) {
        let mut stijlen = f.verwijzingen(item, 1);
        while let Some(s) = stijlen.pop() {
            match f.type_van(s) {
                "IFCPRESENTATIONSTYLEASSIGNMENT" => stijlen.extend(f.verwijzingen(s, 0)),
                "IFCSURFACESTYLE" => {
                    for e in f.verwijzingen(s, 2) {
                        let kleur = f.verwijzing(e, 0);
                        uit.push([1, 2, 3].map(|j| f.attr(kleur, j).parse::<f64>().unwrap()));
                    }
                }
                t => panic!("onverwachte stijl {t}"),
            }
        }
    }
    uit
}

/// De waarden van eigenschap `naam` bij dit object, via IfcRelDefinesByProperties.
fn eigenschap(f: &Feiten, object: u64, naam: &str) -> Vec<String> {
    let mut uit = Vec::new();
    for rel in f.naar("IFCRELDEFINESBYPROPERTIES", 4, object) {
        for p in f.verwijzingen(f.verwijzing(rel, 5), 4) {
            if f.attr(p, 0) == format!("'{naam}'") {
                uit.push(f.attr(p, 2).to_string());
            }
        }
    }
    uit
}

/// Sol, PR #7 bevinding 1 (stijlverwisseling): hun nummert de vormen van W en V om (dezelfde inhoud, de nummers
/// gewisseld) en maakt V groen; ons hernoemt W. W blijft rood en V wordt groen.
#[test]
fn stijlen_na_omnummeren() {
    for schema in SCHEMAS {
        let b = met_vormen(schema, true);
        let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
        let h = wissel(&vervang(&b, "#75=IFCCOLOURRGB($,0.,0.,1.)", "#75=IFCCOLOURRGB($,0.,1.,0.)"), 64, 72);
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let r = gelukt(&b, oo, hh);
            let f = Feiten::lees(&r);
            let (w, v) = (vorm_van(&f, &gid("WandW")), vorm_van(&f, &gid("WandV")));
            assert_ne!(w, v, "{schema}\n{r}");
            assert_eq!(kleuren_op(&f, w), vec![ROOD], "{schema}: W\n{r}");
            assert_eq!(kleuren_op(&f, v), vec![GROEN], "{schema}: V\n{r}");
            assert_eq!(f.attr(f.met_globalid(&gid("WandW")), 2), "'W-ons'");
        }
    }
}

/// Als hierboven, maar W en V zijn allebei blauw: hun maakt alleen V groen. Hun twee gestileerde items verschillen
/// dan alleen in de vorm waar ze bij horen.
#[test]
fn stijl_van_een_van_twee_gelijke_vormen() {
    for schema in SCHEMAS {
        let b = vervang(&met_vormen(schema, true), "#67=IFCCOLOURRGB($,1.,0.,0.)", "#67=IFCCOLOURRGB($,0.,0.,1.)");
        let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
        let h = vervang(&b, "#75=IFCCOLOURRGB($,0.,0.,1.)", "#75=IFCCOLOURRGB($,0.,1.,0.)");
        for (oo, hh) in [(&o, &h), (&h, &o), (&o, &wissel(&h, 64, 72)), (&o, &hernummer(&h, 3))] {
            let r = gelukt(&b, oo, hh);
            let f = Feiten::lees(&r);
            let (w, v) = (vorm_van(&f, &gid("WandW")), vorm_van(&f, &gid("WandV")));
            assert_ne!(w, v, "{schema}\n{r}");
            assert_eq!(kleuren_op(&f, w), vec![BLAUW], "{schema}: W\n{r}");
            assert_eq!(kleuren_op(&f, v), vec![GROEN], "{schema}: V\n{r}");
        }
        // Elke kant wijzigt de stijl van een andere vorm: geen conflict, want de stijlen horen bij een ander object.
        let o = vervang(&b, "#67=IFCCOLOURRGB($,0.,0.,1.)", "#67=IFCCOLOURRGB($,1.,0.,0.)");
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let r = gelukt(&b, oo, &hernummer(hh, 11));
            let f = Feiten::lees(&r);
            assert_eq!(kleuren_op(&f, vorm_van(&f, &gid("WandW"))), vec![ROOD], "{schema}: W\n{r}");
            assert_eq!(kleuren_op(&f, vorm_van(&f, &gid("WandV"))), vec![GROEN], "{schema}: V\n{r}");
        }
    }
}

/// Sol, PR #7 bevinding 3: alleen W heeft een vorm (rood). Hun geeft V een eigen vorm met dezelfde inhoud en een
/// eigen kleur (blauw); ons hernoemt W. V houdt zijn eigen vorm, en elke vorm heeft één stijl.
#[test]
fn eigen_vorm_met_dezelfde_inhoud() {
    for schema in SCHEMAS {
        let b = met_vormen(schema, false);
        let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
        let h = met_vormen(schema, true);
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let r = gelukt(&b, oo, &hernummer(hh, 5));
            let f = Feiten::lees(&r);
            let (w, v) = (vorm_van(&f, &gid("WandW")), vorm_van(&f, &gid("WandV")));
            assert_ne!(w, v, "{schema}: V deelt de vorm van W\n{r}");
            assert_eq!(kleuren_op(&f, w), vec![ROOD], "{schema}: W\n{r}");
            assert_eq!(kleuren_op(&f, v), vec![BLAUW], "{schema}: V\n{r}");
        }
    }
}

/// Een materiaal en een IfcRelAssociatesMaterial voor W en V.
fn materiaal(schema: &str, naam: &str, rel: &str) -> String {
    let m = if schema == "IFC2X3" { format!("IFCMATERIAL('{naam}')") } else { format!("IFCMATERIAL('{naam}',$,$)") };
    format!("#71={m};\n#72=IFCRELASSOCIATESMATERIAL('{}',#5,$,$,(#43,#44),#71);", gid(rel))
}

/// Sol, PR #7 bevinding 2 (materiaalbotsing): W en V zijn van beton. Ons maakt ze van staal en hun van hout, elk
/// met een nieuwe relatie (een nieuwe GlobalId) in plaats van de oude. Dat is een conflict. Eén kant alleen niet.
#[test]
fn materiaal_aan_beide_kanten_vervangen() {
    for schema in SCHEMAS {
        let beton = materiaal(schema, "Beton", "MatRel");
        let b = voeg_toe(&gebouw(schema), &beton);
        let o = vervang(&vervang(&b, "#5,'W',$", "#5,'W-ons',$"), &beton, &materiaal(schema, "Staal", "MatOns"));
        let h = vervang(&vervang(&b, "#5,'V',$", "#5,'V-hun',$"), &beton, &materiaal(schema, "Hout", "MatHun"));
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let soorten = conflicten(&b, oo, &hernummer(hh, 7));
            assert!(soorten.contains(&ConflictSoort::RelatieVervangen), "{schema}: {soorten:?}");
        }
        // Alleen ons vervangt: één materiaalrelatie, voor W en V, van staal.
        let h = vervang(&b, "#5,'V',$", "#5,'V-hun',$");
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let r = gelukt(&b, oo, &hernummer(hh, 9));
            let f = Feiten::lees(&r);
            let rels = f.van_type("IFCRELASSOCIATESMATERIAL");
            assert_eq!(rels.len(), 1, "{schema}\n{r}");
            let mut doelen = f.verwijzingen(rels[0], 4);
            doelen.sort_unstable();
            let mut wv = vec![f.met_globalid(&gid("WandW")), f.met_globalid(&gid("WandV"))];
            wv.sort_unstable();
            assert_eq!(doelen, wv, "{schema}\n{r}");
            assert_eq!(f.attr(rels[0], 0), format!("'{}'", gid("MatOns")));
            assert_eq!(f.attr(f.verwijzing(rels[0], 5), 0), "'Staal'");
        }
    }
}

/// Sol, PR #7 bevinding 4 (splitsing): W en V delen een plaatsing. Ons geeft V een eigen kopie; hun zet de
/// brandwerendheid op 60. In het resultaat hebben W en V elk een eigen plaatsing met dezelfde inhoud, en is de
/// brandwerendheid 60.
#[test]
fn gedeelde_plaatsing_splitsen_door_een_kant() {
    for schema in SCHEMAS {
        let b = vervang(&gebouw(schema), "#5,'V',$,$,#45,", "#5,'V',$,$,#40,");
        let b = verwijder_regel(&verwijder_regel(&verwijder_regel(&b, "#45="), "#46="), "#47=");
        let o = met(&b, &[("#5,'V',$,$,#40,", "#5,'V',$,$,#71,")], "#71=IFCLOCALPLACEMENT(#20,#41);");
        let h = vervang(&b, BRAND, "IFCINTEGER(60)");
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let r = gelukt(&b, oo, &hernummer(hh, 13));
            let f = Feiten::lees(&r);
            let (w, v) = (f.met_globalid(&gid("WandW")), f.met_globalid(&gid("WandV")));
            let (pw, pv) = (f.verwijzing(w, 5), f.verwijzing(v, 5));
            assert_ne!(pw, pv, "{schema}: W en V delen nog een plaatsing\n{r}");
            let p = f.verwijzing(f.met_globalid(&gid("StoreyP")), 5);
            for x in [pw, pv] {
                assert_eq!(f.verwijzing(x, 0), p, "{schema}\n{r}");
                let punt = f.verwijzing(f.verwijzing(x, 1), 0);
                assert_eq!(f.attr(punt, 0), "(1.,0.,0.)", "{schema}\n{r}");
            }
            for x in [w, v] {
                assert_eq!(eigenschap(&f, x, "Brandwerendheid"), vec!["IFCINTEGER(60)"], "{schema}\n{r}");
            }
        }
    }
}

/// Sol, PR #7 bevinding 5: ons zet de plaatsing van P relatief aan die van Q, hun die van Q relatief aan die van P.
/// Elke kant apart is goed; samen staan de plaatsingen in een kring. Dat is een conflict.
#[test]
fn plaatsingen_in_een_kring() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = vervang(&b, "#20=IFCLOCALPLACEMENT($,", "#20=IFCLOCALPLACEMENT(#23,");
        let h = vervang(&b, "#23=IFCLOCALPLACEMENT($,", "#23=IFCLOCALPLACEMENT(#20,");
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let s = voeg(&b, oo, &hernummer(hh, 17));
            assert!(!s.gelukt && s.inhoud.is_none(), "{schema}");
            let c = s.conflicten.iter().find(|c| c.soort == ConflictSoort::PlaatsingKring);
            let c = c.unwrap_or_else(|| panic!("{schema}: {:#?}", s.conflicten));
            assert_eq!(c.attribuut.as_deref(), Some("ObjectPlacement.PlacementRelTo"));
        }
        // Eén kant alleen: geen kring.
        let h = vervang(&b, "#5,'W',$", "#5,'W-hun',$");
        let r = gelukt(&b, &o, &hernummer(&h, 19));
        let f = Feiten::lees(&r);
        let p = f.verwijzing(f.met_globalid(&gid("StoreyP")), 5);
        let q = f.verwijzing(f.met_globalid(&gid("StoreyQ")), 5);
        assert_eq!((f.attr(p, 0), f.attr(q, 0)), (format!("#{q}").as_str(), "$"), "{schema}\n{r}");
    }
}

/// Het gebouw met een baseline-planning (IfcWorkSchedule) met CreationDate 15 januari.
fn met_planning(schema: &str) -> String {
    voeg_toe(
        &gebouw(schema),
        &format!(
            "#80=IFCWORKSCHEDULE('{}',#5,'Baseline 1',$,$,$,'2027-01-15T09:00:00',$,$,$,$,'2027-03-01T07:00:00',\
             '2027-06-02T07:00:00',.BASELINE.);",
            gid("Planning")
        ),
    )
}

fn creatiedatum(t: &str, datum: &str) -> String {
    vervang(t, "'Baseline 1',$,$,$,'2027-01-15T09:00:00'", &format!("'Baseline 1',$,$,$,'{datum}'"))
}

/// De IfcOwnerHistory gewijzigd op tijdstip `tijd`.
fn gewijzigd_op(t: &str, tijd: u64) -> String {
    vervang(t, "#4,$,.NOCHANGE.,$,$,$,1700000000);", &format!("#4,$,.MODIFIED.,{tijd},$,$,1700000000);"))
}

fn met_tijdstempel(t: &str, tijd: &str) -> String {
    vervang(t, "FILE_NAME('test.ifc','2026-11-05T10:00:00',", &format!("FILE_NAME('test.ifc','{tijd}',"))
}

/// De vluchtige waarden in een resultaat: CreationDate van de planning, ChangeAction en LastModifiedDate van de
/// IfcOwnerHistory van W, en de tijdstempel in FILE_NAME.
fn vluchtig(r: &str) -> (String, String, String, String) {
    let f = Feiten::lees(r);
    let plan = f.met_globalid(&gid("Planning"));
    let eigenaar = f.verwijzing(f.met_globalid(&gid("WandW")), 1);
    let kop = r.lines().find(|l| l.starts_with("FILE_NAME(")).unwrap();
    let tijd = kop.split(',').nth(1).unwrap().to_string();
    (f.attr(plan, 6).into(), f.attr(eigenaar, 3).into(), f.attr(eigenaar, 4).into(), tijd)
}

/// Vluchtige velden geven nooit een conflict. Wijzigde alleen hun zo'n veld, dan komt de waarde van hun; wijzigden
/// beide, dan blijft die van ons. Bijv. de CreationDate van een baseline, die Open Planner Studio niet bij elk
/// opslaan overschrijft.
#[test]
fn vluchtige_velden_samenvoegen() {
    for schema in ["IFC4", "IFC4X3_ADD2"] {
        let b = met_planning(schema);
        // Alleen hun wijzigt de vluchtige velden; ons wijzigt een naam.
        let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
        let h =
            met_tijdstempel(&gewijzigd_op(&creatiedatum(&b, "2027-02-15T09:00:00"), 1800000000), "2026-11-06T08:00:00");
        let hun = (
            "'2027-02-15T09:00:00'".to_string(),
            ".MODIFIED.".to_string(),
            "1800000000".to_string(),
            "'2026-11-06T08:00:00'".to_string(),
        );
        let r = gelukt(&b, &o, &hernummer(&h, 23));
        assert_eq!(vluchtig(&r), hun, "{schema}\n{r}");
        assert!(r.contains("'W-ons'"));
        // Ook als ons verder niets wijzigde.
        assert_eq!(vluchtig(&gelukt(&b, &b, &hernummer(&h, 25))), hun, "{schema}");
        // Beide wijzigen: de waarde van ons blijft, zonder conflict.
        let o2 =
            met_tijdstempel(&gewijzigd_op(&creatiedatum(&o, "2027-03-15T09:00:00"), 1900000000), "2026-11-07T08:00:00");
        let r = gelukt(&b, &o2, &hernummer(&h, 27));
        let ons = (
            "'2027-03-15T09:00:00'".to_string(),
            ".MODIFIED.".to_string(),
            "1900000000".to_string(),
            "'2026-11-07T08:00:00'".to_string(),
        );
        assert_eq!(vluchtig(&r), ons, "{schema}\n{r}");
        // Elke kant een ander vluchtig veld: allebei komen ze over.
        let o3 = gewijzigd_op(&b, 1900000000);
        let h3 = creatiedatum(&b, "2027-02-15T09:00:00");
        let r = gelukt(&b, &o3, &hernummer(&h3, 29));
        let (creatie, actie, tijd, _) = vluchtig(&r);
        assert_eq!(
            (creatie.as_str(), actie.as_str(), tijd.as_str()),
            ("'2027-02-15T09:00:00'", ".MODIFIED.", "1900000000")
        );
    }
}

/// Een ander FILE_SCHEMA bij één kant (bijv. IFC4 naar IFC4_ADD2, dezelfde tabel) is een conflict: we kiezen niet
/// stil een schema.
#[test]
fn ander_file_schema_is_een_conflict() {
    let b = gebouw("IFC4");
    let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
    let h = vervang(&b, "FILE_SCHEMA(('IFC4'))", "FILE_SCHEMA(('IFC4_ADD2'))");
    for (bb, oo, hh) in [(&b, &o, &h), (&b, &h, &o), (&b, &b, &h)] {
        let s = voeg(bb, oo, hh);
        assert!(!s.gelukt && s.inhoud.is_none());
        let c = s.conflicten.iter().find(|c| c.attribuut.as_deref() == Some("FILE_SCHEMA"));
        let c = c.unwrap_or_else(|| panic!("{:#?}", s.conflicten));
        assert_eq!(c.soort, ConflictSoort::NietOndersteund);
        assert!(c.melding.contains("IFC4_ADD2"), "{}", c.melding);
    }
}

/// Windows-programma's schrijven vaak `\r\n` als regeleinde. Samenvoegen werkt met elk regeleinde, en het resultaat
/// houdt dat van ons, ook voor de regels die van hun komen.
#[test]
fn regeleinden_van_windows_bij_samenvoegen() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
        // Hun hernoemt V en voegt X toe (nieuwe regels).
        let x = format!(
            "#70=IFCLOCALPLACEMENT(#20,#71);\n#71=IFCAXIS2PLACEMENT3D(#72,$,$);\n#72=IFCCARTESIANPOINT((5.,0.,0.));\n\
             #73=IFCBUILDINGELEMENTPROXY('{}',#5,'X',$,$,#70,$,$,$);",
            gid("X")
        );
        let wijzig = |t: &str| met(t, &[("#5,'V',$", "#5,'V-hun',$"), (&bevat("#43,#44"), &bevat("#43,#44,#73"))], &x);
        let h = wijzig(&b);
        let verwacht = wijzig(&o);
        let zet = |t: &str, crlf: bool| if crlf { met_crlf(t) } else { t.to_string() };
        for (bc, oc, hc) in [(true, true, true), (false, true, false), (true, false, true), (false, false, true)] {
            let r = gelukt(&zet(&b, bc), &zet(&o, oc), &zet(&h, hc));
            let (crlf, nl) = (r.matches("\r\n").count(), r.matches('\n').count());
            if oc {
                assert_eq!(crlf, nl, "{schema} ({bc}, {oc}, {hc}): niet overal \\r\\n\n{r}");
            } else {
                assert_eq!(crlf, 0, "{schema} ({bc}, {oc}, {hc}): \\r\\n in een resultaat van ons met \\n\n{r}");
            }
            zelfde(&r, &verwacht);
            let f = Feiten::lees(&r);
            assert_eq!(f.attr(f.met_globalid(&gid("X")), 2), "'X'");
            assert_eq!(f.attr(f.met_globalid(&gid("WandV")), 2), "'V-hun'");
        }
    }
}

// --- Uit fixronde 2 van PR #6 (Sol, review van 1456355), bij het samenvoegen. ---

/// Hun wijzigt alleen de Precision van de context (een anker van het project in een SET); ons hernoemt een object.
/// Beide komen over, en alleen het project neemt de context van hun.
#[test]
fn precisie_van_de_context_bij_hun() {
    let b = bestand("ifc4-wall-with-opening-and-window.ifc");
    let h = vervang(&b, "'Model', 3, 1.E-5,", "'Model', 3, 1.E-4,");
    let o = vervang_eerste_naam(&b, " (ons)");
    for (oo, hh) in [(&o, &h), (&h, &o)] {
        let r = gelukt(&b, oo, &hernummer(hh, 33));
        let f = Feiten::lees(&r);
        let contexten = f.van_type("IFCGEOMETRICREPRESENTATIONCONTEXT");
        assert_eq!(contexten.len(), 1, "{r}");
        assert_eq!(f.attr(contexten[0], 3), "1.E-4");
        assert!(r.contains(" (ons)'"));
    }
    // Ons maakt de wand hoger (in zijn vorm): geen conflict met de Precision van hun, want de vorm van de wand
    // verwijst naar de context als anker, niet op inhoud.
    let o = vervang(
        &b,
        "#71 = IFCEXTRUDEDAREASOLID(#72, #79, #27, 2000.);",
        "#71 = IFCEXTRUDEDAREASOLID(#72, #79, #27, 2500.);",
    );
    for (oo, hh) in [(&o, &h), (&h, &o)] {
        let r = gelukt(&b, oo, &hernummer(hh, 35));
        let f = Feiten::lees(&r);
        assert_eq!(f.attr(f.van_type("IFCGEOMETRICREPRESENTATIONCONTEXT")[0], 3), "1.E-4");
        // De wand: zijn Body-vorm is een extrusie van 2500.
        let vorm = f.verwijzing(f.met_globalid("3ZYW59sxj8lei475l7EhLU"), 6);
        let body = f.verwijzingen(vorm, 2).into_iter().find(|&r| f.attr(r, 1) == "'Body'").unwrap();
        let solid = f.verwijzingen(body, 3)[0];
        assert_eq!(f.attr(solid, 3), "2500.", "{r}");
    }
}

/// Hun zet de plaatsingen van W en V relatief aan elkaar (een kring over ankers); ons hernoemt W. Dat is een
/// conflict, geen resultaat.
#[test]
fn kring_over_ankers_is_een_conflict() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let h = vervang(&b, "#40=IFCLOCALPLACEMENT(#20,", "#40=IFCLOCALPLACEMENT(#45,");
        let h = vervang(&h, "#45=IFCLOCALPLACEMENT(#20,", "#45=IFCLOCALPLACEMENT(#40,");
        let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let soorten = conflicten(&b, oo, hh);
            assert!(soorten.contains(&ConflictSoort::NietOndersteund), "{schema}: {soorten:?}");
        }
    }
}

// --- Uit de review van PR #7 op 6a27723 (Sol). ---

/// Sol's basis: W en V delen één rode vorm (#66). In IFC2X3 mag een IfcProductDefinitionShape maar bij één product
/// horen (`ShapeOfProduct` is daar SET [1:1]); daar heeft alleen W de vorm.
fn gedeelde_vorm(schema: &str) -> String {
    let b = met_vormen(schema, false);
    if schema == "IFC2X3" {
        return b;
    }
    vervang(&b, "#5,'V',$,$,#45,$,", "#5,'V',$,$,#45,#66,")
}

/// De IfcOwnerHistory van het object met deze GlobalId: (ChangeAction, LastModifiedDate).
fn eigenaar_van(f: &Feiten, globalid: &str) -> (String, String) {
    let e = f.verwijzing(f.met_globalid(globalid), 1);
    (f.attr(e, 3).to_string(), f.attr(e, 4).to_string())
}

/// Sol, bevinding 1 (gedeelde vormen): W en V delen een rode vorm; ons hernoemt W. Hun voegt X en Y toe, die samen
/// een eigen vorm zonder stijl delen, met dezelfde inhoud als die van W en V. X en Y houden hun eigen vorm zonder
/// stijl; W en V blijven rood. "Allebei bij meer objecten" is geen bewijs dat het dezelfde knoop is. Alleen IFC4 en
/// IFC4X3, net als bij Sol: in IFC2X3 mogen producten geen vorm en geen plaatsing delen.
#[test]
fn gedeelde_vorm_van_andere_objecten() {
    for schema in ["IFC4", "IFC4X3_ADD2"] {
        let b = gedeelde_vorm(schema);
        let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
        let xy = format!(
            "#100=IFCCARTESIANPOINT((0.,0.,0.));\n#101=IFCBOUNDINGBOX(#100,1.,1.,1.);\n\
             #102=IFCSHAPEREPRESENTATION(#7,'Box','BoundingBox',(#101));\n#103=IFCPRODUCTDEFINITIONSHAPE($,$,(#102));\n\
             #104=IFCBUILDINGELEMENTPROXY('{}',#5,'X',$,$,#40,#103,$,$);\n\
             #105=IFCBUILDINGELEMENTPROXY('{}',#5,'Y',$,$,#45,#103,$,$);",
            gid("NieuwX"),
            gid("NieuwY")
        );
        let h = met(&b, &[(&bevat("#43,#44"), &bevat("#43,#44,#104,#105"))], &xy);
        for (oo, hh) in [(&o, &h), (&h, &o), (&o, &hernummer(&h, 37))] {
            let r = gelukt(&b, oo, hh);
            let f = Feiten::lees(&r);
            let [w, v, x, y] = ["WandW", "WandV", "NieuwX", "NieuwY"].map(|n| vorm_van(&f, &gid(n)));
            assert_eq!(w, v, "{schema}: W en V delen hun vorm\n{r}");
            assert_eq!(x, y, "{schema}: X en Y delen hun vorm\n{r}");
            assert_ne!(x, w, "{schema}: X en Y kregen de vorm van W en V\n{r}");
            assert_eq!(kleuren_op(&f, w), vec![ROOD], "{schema}: W en V\n{r}");
            assert_eq!(kleuren_op(&f, x), Vec::<[f64; 3]>::new(), "{schema}: X en Y\n{r}");
            assert_eq!(f.attr(f.met_globalid(&gid("WandW")), 2), "'W-ons'");
        }
    }
}

/// Sol, bevinding 2 (eigen IfcOwnerHistory): W en V delen een IfcOwnerHistory. Ons geeft W een eigen kopie en
/// hernoemt W; hun zet de gedeelde op MODIFIED, tijdstip 1800000000. Beide IfcOwnerHistory's krijgen de waarden van
/// hun, en W houdt zijn eigen.
#[test]
fn vluchtig_bij_een_eigen_ownerhistory() {
    for schema in SCHEMAS {
        let b = gedeelde_vorm(schema);
        let o = met(&b, &[("#5,'W',$", "#90,'W-ons',$")], "#90=IFCOWNERHISTORY(#3,#4,$,.NOCHANGE.,$,$,$,1700000000);");
        let h = gewijzigd_op(&b, 1800000000);
        let hun = (".MODIFIED.".to_string(), "1800000000".to_string());
        for (oo, hh) in [(&o, &h), (&h, &o), (&o, &hernummer(&h, 39)), (&h, &hernummer(&o, 41))] {
            let r = gelukt(&b, oo, hh);
            let f = Feiten::lees(&r);
            let (w, v) = (f.met_globalid(&gid("WandW")), f.met_globalid(&gid("WandV")));
            assert_eq!(eigenaar_van(&f, &gid("WandW")), hun, "{schema}: W\n{r}");
            assert_eq!(eigenaar_van(&f, &gid("WandV")), hun, "{schema}: V\n{r}");
            assert_eq!(eigenaar_van(&f, &gid("Project")), hun, "{schema}: project\n{r}");
            assert_ne!(f.verwijzing(w, 1), f.verwijzing(v, 1), "{schema}: W houdt zijn eigen\n{r}");
            assert_eq!(f.attr(w, 2), "'W-ons'");
        }
    }
}

/// Sol, bevinding 3 (vluchtig veld van een object): ons zet een andere CreationDate in de planning, hun hernoemt
/// hem. Beide komen over, zonder conflict.
#[test]
fn vluchtig_objectveld_en_naam_bij_de_andere_kant() {
    for schema in ["IFC4", "IFC4X3_ADD2"] {
        let b = met_planning(schema);
        let o = creatiedatum(&b, "2027-03-15T09:00:00");
        let h = vervang(&b, "'Baseline 1'", "'Baseline hernoemd'");
        for (oo, hh) in [(&o, &h), (&h, &o), (&o, &hernummer(&h, 43))] {
            let r = gelukt(&b, oo, hh);
            let f = Feiten::lees(&r);
            let plan = f.met_globalid(&gid("Planning"));
            assert_eq!(f.attr(plan, 2), "'Baseline hernoemd'", "{schema}\n{r}");
            assert_eq!(f.attr(plan, 6), "'2027-03-15T09:00:00'", "{schema}\n{r}");
        }
    }
}

/// De andere deling: hun geeft W een eigen IfcOwnerHistory (MODIFIED, tijdstip 1800000000) en wijzigt de
/// Description van W; ons hernoemt W, of laat W met rust. W krijgt zijn eigen IfcOwnerHistory met de waarden van
/// hun, en V houdt de gedeelde zonder wijziging. Geen conflict: elk object krijgt de deling die bij zijn waarde past.
#[test]
fn eigen_ownerhistory_van_een_kant() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let eigen = "#90=IFCOWNERHISTORY(#3,#4,$,.MODIFIED.,1800000000,$,$,1700000000);";
        let h = met(&b, &[("#5,'W',$", "#90,'W',$")], eigen);
        let h_ook = met(&b, &[("#5,'W',$", "#90,'W','hun'")], eigen);
        let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
        let hun = (".MODIFIED.".to_string(), "1800000000".to_string());
        let basis = (".NOCHANGE.".to_string(), "$".to_string());
        for (oo, hh) in [(&o, &h_ook), (&h_ook, &o), (&o, &h), (&h, &o), (&b, &h)] {
            let r = gelukt(&b, oo, &hernummer(hh, 45));
            let f = Feiten::lees(&r);
            let (w, v) = (f.met_globalid(&gid("WandW")), f.met_globalid(&gid("WandV")));
            assert_ne!(f.verwijzing(w, 1), f.verwijzing(v, 1), "{schema}: W heeft een eigen\n{r}");
            assert_eq!(eigenaar_van(&f, &gid("WandW")), hun, "{schema}: W\n{r}");
            assert_eq!(eigenaar_van(&f, &gid("WandV")), basis, "{schema}: V\n{r}");
        }
    }
}

/// Kan een object de deling van de andere kant niet krijgen, dan is een botsing van vluchtige waarden een conflict,
/// geen stille keuze. Ons geeft de gedeelde IfcOwnerHistory een andere OwningApplication; hun geeft W een eigen
/// (MODIFIED) met de oude. W en V delen dan bij ons één knoop, maar vragen een andere ChangeAction.
#[test]
fn botsende_vluchtige_waarden_in_een_gedeelde_knoop() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let o = met(
            &b,
            &[("#5=IFCOWNERHISTORY(#3,#4,", "#5=IFCOWNERHISTORY(#3,#91,")],
            "#91=IFCAPPLICATION(#2,'2.0','Ander','Ander');",
        );
        let h =
            met(&b, &[("#5,'W',$", "#90,'W',$")], "#90=IFCOWNERHISTORY(#3,#4,$,.MODIFIED.,1800000000,$,$,1700000000);");
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let s = voeg(&b, oo, &hernummer(hh, 47));
            assert!(!s.gelukt && s.inhoud.is_none(), "{schema}: {:?}", s.statistiek);
            assert!(
                s.conflicten.iter().any(|c| c.soort == ConflictSoort::GedeeldAnker),
                "{schema}: {:#?}",
                s.conflicten
            );
        }
    }
}

/// Een nieuw object met de gedeelde IfcOwnerHistory volgt die: ons voegt Y toe, hun zet de gedeelde op MODIFIED.
/// Geen conflict, en alle objecten (ook Y) krijgen de waarden van hun.
#[test]
fn nieuw_object_volgt_de_gedeelde_ownerhistory() {
    for schema in SCHEMAS {
        let b = gebouw(schema);
        let y = format!(
            "#70=IFCBUILDINGELEMENTPROXY('{}',#5,'Y',$,$,#71,$,$,$);\n#71=IFCLOCALPLACEMENT(#20,#72);\n\
             #72=IFCAXIS2PLACEMENT3D(#73,$,$);\n#73=IFCCARTESIANPOINT((3.,0.,0.));",
            gid("NieuwY")
        );
        let o = met(&b, &[(&bevat("#43,#44"), &bevat("#43,#44,#70"))], &y);
        let h = gewijzigd_op(&b, 1800000000);
        let hun = (".MODIFIED.".to_string(), "1800000000".to_string());
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let r = gelukt(&b, oo, &hernummer(hh, 49));
            let f = Feiten::lees(&r);
            for naam in ["WandW", "WandV", "NieuwY", "Project"] {
                assert_eq!(eigenaar_van(&f, &gid(naam)), hun, "{schema}: {naam}\n{r}");
            }
        }
    }
}

/// Twee objecten delen bij ons een eigen IfcOwnerHistory (#95), en ons geeft die een andere OwningApplication. Hun
/// geeft W en V elk een eigen, met een ander tijdstip. Ze kunnen de deling van hun niet krijgen, en vragen in de
/// knoop van ons elk een andere waarde: een conflict.
#[test]
fn twee_objecten_vragen_elk_een_andere_waarde() {
    for schema in SCHEMAS {
        let b = met(
            &gebouw(schema),
            &[("#5,'W',$", "#95,'W',$"), ("#5,'V',$", "#95,'V',$")],
            "#95=IFCOWNERHISTORY(#3,#4,$,.NOCHANGE.,$,$,$,1700000000);",
        );
        let o = met(
            &b,
            &[("#95=IFCOWNERHISTORY(#3,#4,", "#95=IFCOWNERHISTORY(#3,#96,")],
            "#96=IFCAPPLICATION(#2,'2.0','Ander','Ander');",
        );
        let h = met(
            &verwijder_regel(&b, "#95="),
            &[("#95,'W',$", "#90,'W',$"), ("#95,'V',$", "#91,'V',$")],
            "#90=IFCOWNERHISTORY(#3,#4,$,.MODIFIED.,1800000000,$,$,1700000000);\n\
             #91=IFCOWNERHISTORY(#3,#4,$,.MODIFIED.,1900000000,$,$,1700000000);",
        );
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let s = voeg(&b, oo, &hernummer(hh, 51));
            assert!(!s.gelukt && s.inhoud.is_none(), "{schema}: {:?}", s.statistiek);
            assert!(
                s.conflicten.iter().any(|c| c.soort == ConflictSoort::GedeeldAnker),
                "{schema}: {:#?}",
                s.conflicten
            );
        }
    }
}

/// Sol ronde 3 op #6 (bevinding 1), bij het samenvoegen. Het project heeft twee contexten met dezelfde inhoud; W
/// hangt aan de ene, V aan de andere. Ons zet W aan de context van V, hun wisselt de volgorde van de contexten.
/// Geen stil verlies: in het resultaat hangt W aan dezelfde context als V, en het project heeft twee contexten.
#[test]
fn wand_naar_een_gelijke_context_en_omwisselen() {
    for schema in SCHEMAS {
        let b = met_contexten(schema, Some(GELIJK));
        let o = vervang(&b, "*,#7,$,.MODEL_VIEW.", "*,#70,$,.MODEL_VIEW.");
        let h = vervang(&b, ",(#7,#70),#8);", ",(#70,#7),#8);");
        // Los nagegaan: in ons hangen W en V aan dezelfde context, in de basis en bij hun niet.
        let f = Feiten::lees(&o);
        assert_eq!(context_van(&f, "WandW"), context_van(&f, "WandV"), "{schema}");
        for f in [Feiten::lees(&b), Feiten::lees(&h)] {
            assert_ne!(context_van(&f, "WandW"), context_van(&f, "WandV"), "{schema}");
        }
        for (oo, hh) in [(&o, &h), (&h, &o)] {
            let r = gelukt(&b, oo, &hernummer(hh, 37));
            let f = Feiten::lees(&r);
            let project = f.met_globalid(&gid("Project"));
            let contexten = f.verwijzingen(project, 7);
            assert_eq!(contexten.len(), 2, "{schema}: {r}");
            let (w, v) = (context_van(&f, "WandW"), context_van(&f, "WandV"));
            assert_eq!(w, v, "{schema}: W hangt niet aan de context van V\n{r}");
            assert!(contexten.contains(&w), "{schema}: {r}");
            assert_eq!(f.van_type("IFCGEOMETRICREPRESENTATIONCONTEXT").len(), 2, "{schema}: {r}");
        }
    }
}
