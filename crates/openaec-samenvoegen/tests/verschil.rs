//! Tests van `verschil`: echte bestanden, bewerkingen in de test zelf en andere nummering.

mod gemeen;

use gemeen::*;
use openaec_samenvoegen::{verschil, Fout};

fn leeg(a: &str, b: &str) -> bool {
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    if !v.is_leeg() {
        eprintln!("{}", serde_json::to_string_pretty(&v).unwrap());
    }
    v.is_leeg()
}

#[test]
fn hernummerd_is_gelijk() {
    for (naam, tekst) in alle_bestanden() {
        for zaad in 1..=3 {
            assert!(leeg(&tekst, &hernummer(&tekst, zaad)), "{naam}, zaad {zaad}");
        }
    }
    for schema in SCHEMAS {
        let tekst = gebouw(schema);
        assert!(leeg(&tekst, &hernummer(&tekst, 9)), "{schema}");
    }
}

#[test]
fn naam_van_een_taak() {
    let a = bestand("13-supermarkt-albert-heijn.ifc");
    let b = vervang(&a, "'Clear out store',", "'Winkel leeghalen',");
    let v = verschil(a.as_bytes(), hernummer(&b, 4).as_bytes()).unwrap();
    assert_eq!(v.gewijzigd.len(), 1);
    let w = &v.gewijzigd[0];
    assert_eq!((w.globalid.as_str(), w.type_naam.as_str()), ("0LopbjNfzor6Xb2xdRu7Nj", "IfcTask"));
    assert_eq!(w.naam.as_deref(), Some("Winkel leeghalen"));
    assert_eq!(w.attributen.len(), 1);
    assert_eq!(w.attributen[0].attribuut, "Name");
    assert_eq!(
        (w.attributen[0].oud.as_str(), w.attributen[0].nieuw.as_str()),
        ("'Clear out store'", "'Winkel leeghalen'")
    );
    assert!(v.toegevoegd.is_empty() && v.verwijderd.is_empty());
}

#[test]
fn taakdatum_in_iftasktime() {
    let a = bestand("13-supermarkt-albert-heijn.ifc");
    let oud = regel(&a, "#51=").to_string();
    let nieuw = oud.replacen("'2027-03-03T16:00:00'", "'2027-03-04T16:00:00'", 1);
    let b = vervang(&a, &oud, &nieuw);
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    assert_eq!(v.gewijzigd.len(), 1);
    let attr = &v.gewijzigd[0].attributen;
    assert_eq!(attr.len(), 1);
    assert_eq!(attr[0].attribuut, "TaskTime.ScheduleFinish");
    assert_eq!(attr[0].nieuw, "'2027-03-04T16:00:00'");
}

#[test]
fn eigenschapswaarde() {
    let a = bestand("13-supermarkt-albert-heijn.ifc");
    let b = vervang(&a, "'HoursPerDay',$,IFCREAL(8.)", "'HoursPerDay',$,IFCREAL(7.5)");
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    assert_eq!(v.gewijzigd.len(), 1);
    let w = &v.gewijzigd[0];
    assert_eq!(w.type_naam, "IfcPropertySet");
    assert_eq!(w.attributen[0].attribuut, "HasProperties");
    assert_eq!(w.attributen[0].erbij, vec!["IfcPropertySingleValue('HoursPerDay', $, IFCREAL(7.5), $)"]);
    assert_eq!(w.attributen[0].eraf, vec!["IfcPropertySingleValue('HoursPerDay', $, IFCREAL(8.), $)"]);
}

#[test]
fn object_erbij_en_eraf() {
    let a = bestand("13-supermarkt-albert-heijn.ifc");
    let volgorde = regel(&a, "#105=").to_string();
    let b = verwijder_regel(&a, "#105=");
    let b = voeg_toe(
        &b,
        "#900=IFCTASKTIME('Nieuw Time',.PREDICTED.,$,.WORKTIME.,'P0Y0M1D','2027-03-01T07:00:00','2027-03-01T16:00:00',$,$,$,$,$,$,.F.,$,$,$,$,$,$);\n#901=IFCTASK('2Nieuw0000000000000000',#6,'Nieuwe taak',$,$,'9',$,$,$,.F.,$,#900,.CONSTRUCTION.);",
    );
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    assert_eq!(v.toegevoegd.len(), 1);
    assert_eq!(v.toegevoegd[0].naam.as_deref(), Some("Nieuwe taak"));
    assert_eq!(v.verwijderd.len(), 1);
    assert_eq!(v.verwijderd[0].type_naam, "IfcRelSequence");
    assert!(v.gewijzigd.is_empty(), "{:?}", v.gewijzigd);
    assert!(volgorde.contains("2gtUsjV_bwyacvRkg5nkzY"));
}

#[test]
fn relatie_met_nieuwe_globalid() {
    let a = bestand("13-supermarkt-albert-heijn.ifc");
    let b = vervang(&a, "'2gtUsjV_bwyacvRkg5nkzY'", "'3Opnieuw00000000000000'");
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    assert!(v.is_leeg(), "{v:?}");
    assert_eq!(v.statistiek.relaties_gekoppeld, 1);
}

#[test]
fn teksten_en_getallen_op_inhoud() {
    let a = ifc("IFC4", "#1=IFCPROPERTYSINGLEVALUE('caf\\X2\\00E9\\X0\\ ''x''',$,IFCREAL(1.),$);\n#2=IFCPROPERTYSET('0Pset00000000000000000',$,'P',$,(#1));");
    let b = ifc("IFC4", "#8=IFCPROPERTYSINGLEVALUE('café ''x''',$,IFCREAL(1.0E0),$);\n#3=IFCPROPERTYSET('0Pset00000000000000000',$,'P',$,(#8));");
    assert!(leeg(&a, &b));
    let c = ifc("IFC4", "#8=IFCPROPERTYSINGLEVALUE('cafe ''x''',$,IFCREAL(1.0E0),$);\n#3=IFCPROPERTYSET('0Pset00000000000000000',$,'P',$,(#8));");
    assert!(!leeg(&a, &c));
}

#[test]
fn complexe_instantie() {
    let a = ifc(
        "IFC2X3",
        "#1=(IFCNAMEDUNIT(*,.LENGTHUNIT.)IFCSIUNIT(.MILLI.,.METRE.));\n#2=IFCUNITASSIGNMENT((#1));\n#3=IFCPROJECT('0Proj0000000000000000A',$,'P',$,$,$,$,(),#2);",
    );
    assert!(leeg(&a, &hernummer(&a, 2)));
    let b = vervang(&a, ".MILLI.", "$");
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    assert_eq!(v.gewijzigd.len(), 1);
    assert_eq!(v.gewijzigd[0].attributen[0].attribuut, "UnitsInContext.Units");
}

#[test]
fn ongeldige_invoer() {
    let goed = bestand("showcase-groot-terrein-onderaannemer.ifc");
    let kapot = goed.replacen("IFCTASK('", "IFCTASK(''", 1);
    match verschil(goed.as_bytes(), kapot.as_bytes()) {
        Err(Fout::Ongeldig { regel, .. }) => assert!(regel > 7, "regel {regel}"),
        anders => panic!("{anders:?}"),
    }
    assert!(matches!(verschil(b"geen ifc", goed.as_bytes()), Err(Fout::Ongeldig { regel: 1, .. })));
    let afgekapt = &goed[..goed.len() / 2];
    assert!(matches!(verschil(goed.as_bytes(), afgekapt.as_bytes()), Err(Fout::Ongeldig { .. })));
}

#[test]
fn header_auteur_telt_tijdstempel_niet() {
    let a = bestand("13-supermarkt-albert-heijn.ifc");
    let b = vervang(&a, "'2026-10-26T11:31:17',('Anne Willems')", "'2026-11-05T09:00:00',('Anne Willems')");
    assert!(leeg(&a, &b));
    let tijd = verschil(a.as_bytes(), b.as_bytes()).unwrap().vluchtig;
    assert_eq!(tijd.len(), 1);
    assert_eq!((tijd[0].waar.as_str(), tijd[0].attribuut.as_str()), ("HEADER", "FILE_NAME.time_stamp"));
    let c = vervang(&a, "('Anne Willems'),('Retail", "('Bram de Vries'),('Retail");
    let v = verschil(a.as_bytes(), c.as_bytes()).unwrap();
    assert_eq!(v.header.len(), 1);
    assert_eq!(v.header[0].attribuut, "FILE_NAME.author");
    assert_eq!(v.header[0].nieuw, "('Bram de Vries')");
}

#[test]
fn eigenaar_alleen_tijdstempel_is_vluchtig() {
    let a = bestand("13-supermarkt-albert-heijn.ifc");
    // Opnieuw opslaan door OPS: andere tijd in IfcOwnerHistory en in CreationDate van werkplan en planning.
    let b = vervang(&a, ".NOCHANGE.,$,$,$,1793014277)", ".NOCHANGE.,$,$,$,1793099999)");
    let b = b.replace("$,$,'2026-10-26T11:31:17',$", "$,$,'2026-11-05T09:00:00',$");
    assert_ne!(a, b);
    assert!(leeg(&a, &b));
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    let velden: Vec<_> = v.vluchtig.iter().map(|w| (w.waar.as_str(), w.attribuut.as_str())).collect();
    assert!(velden.iter().any(|(_, a)| *a == "OwnerHistory.CreationDate"), "{velden:?}");
    assert_eq!(velden.iter().filter(|(_, a)| *a == "CreationDate").count(), 2, "{velden:?}");
    // Een andere auteur raakt elk object met die IfcOwnerHistory.
    let c = vervang(&a, "IFCPERSON($,'Anne Willems'", "IFCPERSON($,'Bram de Vries'");
    let v = verschil(a.as_bytes(), c.as_bytes()).unwrap();
    assert_eq!(v.gewijzigd.len(), v.statistiek.a.objecten);
    assert!(v.gewijzigd.iter().all(|w| w.attributen.iter().any(|a| a.attribuut.starts_with("OwnerHistory"))));
}

#[test]
fn andere_plaatsing_met_gelijke_inhoud_telt() {
    for schema in SCHEMAS {
        let a = gebouw(schema);
        // W hangt nu onder de plaatsing van Q (#23), met dezelfde inhoud als die van P (#20).
        let b = vervang(&a, "#40=IFCLOCALPLACEMENT(#20,#41);", "#40=IFCLOCALPLACEMENT(#23,#41);");
        let v = verschil(a.as_bytes(), hernummer(&b, 3).as_bytes()).unwrap();
        assert_eq!(v.gewijzigd.len(), 1, "{schema}: {v:?}");
        let w = &v.gewijzigd[0];
        assert_eq!(w.naam.as_deref(), Some("W"));
        assert_eq!(w.attributen[0].attribuut, "ObjectPlacement.PlacementRelTo");
        assert!(w.attributen[0].oud.contains("ObjectPlacement van P"), "{}", w.attributen[0].oud);
        assert!(w.attributen[0].nieuw.contains("ObjectPlacement van Q"), "{}", w.attributen[0].nieuw);
    }
}

/// Sols voorbeeld (#6, bevinding 1): W en V delen één plaatsing; V krijgt een eigen kopie met dezelfde inhoud.
/// Zonder derde gebruiker is dat een wijziging van W en V.
#[test]
fn gedeelde_plaatsing_splitsen_is_een_wijziging() {
    for schema in SCHEMAS {
        // Basis: W en V delen plaatsing #40.
        let a = gebouw(schema);
        let a = vervang(&a, "'V',$,$,#45,", "'V',$,$,#40,");
        // Een kant splitst: V krijgt een eigen kopie van de plaatsing.
        let b = vervang(&a, "'V',$,$,#40,", "'V',$,$,#72,");
        let b = voeg_toe(&b, "#72=IFCLOCALPLACEMENT(#20,#41);");
        let v = verschil(a.as_bytes(), hernummer(&b, 2).as_bytes()).unwrap();
        let namen: Vec<_> = v.gewijzigd.iter().map(|w| w.naam.clone().unwrap()).collect();
        assert_eq!(namen, vec!["V", "W"], "{schema}: {v:#?}");
        for w in &v.gewijzigd {
            assert_eq!(w.attributen.len(), 1, "{schema}: {w:#?}");
            assert_eq!(w.attributen[0].attribuut, "ObjectPlacement (gedeeld)");
            assert!(w.attributen[0].oud.starts_with("gedeeld met "), "{}", w.attributen[0].oud);
            assert_eq!(w.attributen[0].nieuw, "niet gedeeld");
        }
        // Weer samenvoegen tot één plaatsing is ook een wijziging; dezelfde verdeling met andere nummers niet.
        assert!(!verschil(b.as_bytes(), a.as_bytes()).unwrap().is_leeg());
        assert!(leeg(&a, &hernummer(&a, 4)));
        assert!(leeg(&b, &hernummer(&b, 5)));
    }
}

/// Splitst een programma de gedeelde IfcOwnerHistory (een gewijzigd object krijgt een eigen kopie), dan is dat
/// geen wijziging van de andere objecten.
#[test]
fn eigen_kopie_van_de_eigenaar_is_geen_wijziging() {
    let a = gebouw("IFC4");
    let b = vervang(
        &a,
        "#43=IFCBUILDINGELEMENTPROXY('0WandW0000000000000000',#5,",
        "#43=IFCBUILDINGELEMENTPROXY('0WandW0000000000000000',#99,",
    );
    let b = voeg_toe(&b, "#99=IFCOWNERHISTORY(#3,#4,$,.MODIFIED.,1700000500,#3,#4,1700000000);");
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    assert!(v.is_leeg(), "{v:#?}");
    // Wel vluchtig: ChangeAction, LastModifiedDate, LastModifyingUser en LastModifyingApplication van W.
    assert_eq!(v.vluchtig.len(), 4, "{:#?}", v.vluchtig);
    assert!(v.vluchtig.iter().all(|w| w.waar.contains("OwnerHistory van W")), "{:#?}", v.vluchtig);
}

/// Een object dat erbij komt en dezelfde gedeelde knoop gebruikt, verandert de andere objecten niet.
#[test]
fn nieuw_object_aan_een_gedeelde_plaatsing() {
    let a = gebouw("IFC2X3");
    let b = voeg_toe(&a, &format!("#80=IFCBUILDINGELEMENTPROXY('{}',#5,'N',$,$,#40,$,$,$);", gid("WandN")));
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    assert_eq!(v.toegevoegd.len(), 1);
    assert!(v.gewijzigd.is_empty(), "{:#?}", v.gewijzigd);
}

/// Sols voorbeeld (#6, bevinding 2): getallen vergelijken we precies, zonder f64.
#[test]
fn getallen_precies() {
    let met = |waarde: &str| {
        ifc(
            "IFC4",
            &format!(
                "#1=IFCPROPERTYSINGLEVALUE('x',$,IFCREAL({waarde}),$);\n#2=IFCPROPERTYSET('{}',$,'P',$,(#1));",
                gid("Pset")
            ),
        )
    };
    for (a, b) in
        [("9007199254740992.", "9007199254740993."), ("1.E999", "2.E999"), ("1.E-999", "2.E-999"), ("1.E-400", "0.")]
    {
        let v = verschil(met(a).as_bytes(), met(b).as_bytes()).unwrap();
        assert_eq!(v.gewijzigd.len(), 1, "{a} -> {b}");
        assert_eq!(
            v.gewijzigd[0].attributen[0].erbij,
            vec![format!("IfcPropertySingleValue('x', $, IFCREAL({b}), $)")]
        );
    }
    for (a, b) in [("1.", "1.0E0"), ("0.001", "1.E-3"), ("1500.", "1.5E+03"), ("-0.", "0.0"), ("12.50", "0.125E2")] {
        assert!(leeg(&met(a), &met(b)), "{a} = {b}");
    }
    // Een exponent buiten i32 is een duidelijke fout.
    assert!(matches!(
        verschil(met("1.").as_bytes(), met("1.E9999999999").as_bytes()),
        Err(Fout::Ongeldig { regel: 8, .. })
    ));
}

/// Sols voorbeeld (#6, bevinding 3): de codetabel van `\P?\` telt mee, en `\S\'` is een teken.
#[test]
fn codetabellen_in_teksten() {
    let met =
        |naam: &str| ifc("IFC4", &format!("#1=IFCBUILDINGELEMENTPROXY('{}',$,'{naam}',$,$,$,$,$,$);", gid("WandW")));
    assert!(!leeg(&met("\\PA\\\\S\\!"), &met("\\PB\\\\S\\!")));
    assert!(leeg(&met("\\PA\\\\S\\!"), &met("\\X\\A1")));
    let v = verschil(met("x").as_bytes(), met("\\PE\\\\S\\*\\S\\U\\S\\b").as_bytes()).unwrap();
    assert_eq!(v.gewijzigd[0].naam.as_deref(), Some("Њет"));
    // `\S\'` is § (0x27 + 0x80), niet het einde van de tekst.
    let v = verschil(met("x").as_bytes(), met("a\\S\\'b").as_bytes()).unwrap();
    assert_eq!(v.gewijzigd[0].naam.as_deref(), Some("a§b"));
    // Een kapotte code is een fout met het regelnummer.
    assert!(matches!(
        verschil(met("x").as_bytes(), met("\\X2\\00E9").as_bytes()),
        Err(Fout::Ongeldig { regel: 8, .. })
    ));
}

/// Sols voorbeeld (#6, bevinding 4): de volgorde in een SET telt niet, ook niet voor de ankers erin.
#[test]
fn volgorde_in_een_set() {
    let a = ifc(
        "IFC4",
        &format!(
            "#1=IFCPROPERTYSINGLEVALUE('a',$,IFCINTEGER(1),$);\n#2=IFCPROPERTYSINGLEVALUE('b',$,IFCINTEGER(2),$);\n#3=IFCPROPERTYSET('{}',$,'P',$,(#1,#2));",
            gid("Pset")
        ),
    );
    let b = vervang(&a, "(#1,#2)", "(#2,#1)");
    assert!(leeg(&a, &b));
    // Een LIST houdt zijn volgorde: de punten van een lijn.
    let lijn = ifc("IFC4", "#1=IFCCARTESIANPOINT((0.,0.));\n#2=IFCCARTESIANPOINT((1.,0.));\n#3=IFCPOLYLINE((#1,#2));\n#4=IFCSHAPEREPRESENTATION(#9,'Axis','Curve2D',(#3));\n#5=IFCPRODUCTDEFINITIONSHAPE($,$,(#4));\n#6=IFCBUILDINGELEMENTPROXY('0WandW0000000000000000',$,'W',$,$,$,#5,$,$);\n#9=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#11,$);\n#10=IFCCARTESIANPOINT((0.,0.,0.));\n#11=IFCAXIS2PLACEMENT3D(#10,$,$);");
    assert!(!leeg(&lijn, &vervang(&lijn, "IFCPOLYLINE((#1,#2))", "IFCPOLYLINE((#2,#1))")));
    // De contexten van het project (een SET van ankerknopen): een andere volgorde verandert de ankers niet, dus
    // ook de vorm niet die naar de tweede context verwijst.
    let gebouw = gebouw("IFC4");
    let c = vervang(
        &gebouw,
        "#6=IFCPROJECT('0Project00000000000000',#5,'Project',$,$,$,$,(#7),#8);",
        "#6=IFCPROJECT('0Project00000000000000',#5,'Project',$,$,$,$,(#7,#70),#8);",
    );
    let c = vervang(
        &c,
        "#43=IFCBUILDINGELEMENTPROXY('0WandW0000000000000000',#5,'W',$,$,#40,$,",
        "#43=IFCBUILDINGELEMENTPROXY('0WandW0000000000000000',#5,'W',$,$,#40,#73,",
    );
    let c = voeg_toe(&c, "#70=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Plan',2,1.E-05,#11,$);\n#71=IFCPOLYLINE((#10,#22));\n#72=IFCSHAPEREPRESENTATION(#70,'Axis','Curve2D',(#71));\n#73=IFCPRODUCTDEFINITIONSHAPE($,$,(#72));");
    let d = vervang(&c, "(#7,#70),#8);", "(#70,#7),#8);");
    assert!(leeg(&c, &d));
    // De vorm aan de andere context hangen is wel een wijziging.
    let e = vervang(&c, "#72=IFCSHAPEREPRESENTATION(#70,", "#72=IFCSHAPEREPRESENTATION(#7,");
    let v = verschil(c.as_bytes(), e.as_bytes()).unwrap();
    assert_eq!(v.gewijzigd.len(), 1, "{v:#?}");
    assert_eq!(v.gewijzigd[0].naam.as_deref(), Some("W"));
}

/// Sols voorbeeld (#6, bevinding 5): een ander schema in FILE_SCHEMA is een wijziging.
#[test]
fn ander_schema_is_een_wijziging() {
    let a = bestand("13-supermarkt-albert-heijn.ifc");
    let b = vervang(&a, "FILE_SCHEMA(('IFC4X3_ADD2'));", "FILE_SCHEMA(('IFC4'));");
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    assert!(!v.is_leeg());
    let schema: Vec<_> = v.header.iter().filter(|h| h.attribuut == "FILE_SCHEMA.schema_identifiers").collect();
    assert_eq!(schema.len(), 1, "{:#?}", v.header);
    assert_eq!((schema[0].oud.as_str(), schema[0].nieuw.as_str()), ("('IFC4X3_ADD2')", "('IFC4')"));
}

/// Sols voorbeeld (#6, bevinding 5): de CreationDate van een baseline is vluchtig (geen wijziging), maar het verschil
/// noemt hem apart.
#[test]
fn creationdate_van_een_baseline_is_vluchtig() {
    let a = bestand("13-supermarkt-albert-heijn.ifc");
    let a = voeg_toe(&a, "#9000=IFCWORKSCHEDULE('2Baseline0000000000000',#6,'Baseline 1',$,$,$,'2027-01-15T09:00:00',$,$,$,$,'2027-03-01T07:00:00','2027-06-02T07:00:00',.BASELINE.);");
    let b = vervang(&a, "'Baseline 1',$,$,$,'2027-01-15T09:00:00'", "'Baseline 1',$,$,$,'2027-02-15T09:00:00'");
    let v = verschil(a.as_bytes(), hernummer(&b, 6).as_bytes()).unwrap();
    assert!(v.is_leeg(), "{v:#?}");
    assert_eq!(v.vluchtig.len(), 1, "{:#?}", v.vluchtig);
    let w = &v.vluchtig[0];
    assert_eq!(w.globalid.as_deref(), Some("2Baseline0000000000000"));
    assert_eq!(w.attribuut, "CreationDate");
    assert_eq!((w.oud.as_str(), w.nieuw.as_str()), ("'2027-01-15T09:00:00'", "'2027-02-15T09:00:00'"));
}

/// Knopen in een kring: een duidelijke fout, want hun hash zou afhangen van de volgorde in het bestand.
#[test]
fn kring_is_een_fout() {
    let a = ifc("IFC4", "#1=IFCANDERS(#2);\n#2=IFCANDERS(#1);\n#3=IFCSTYLEDITEM(#1,(),'rood');");
    assert!(
        matches!(verschil(a.as_bytes(), a.as_bytes()), Err(Fout::Kring { regel: 8 })),
        "{:?}",
        verschil(a.as_bytes(), a.as_bytes())
    );
}

#[test]
fn losse_knopen_kleuren() {
    let a = bestand("ifc4-tessellation-with-individual-colors.ifc");
    let b = vervang(&a, "IFCCOLOURRGBLIST(((1.0,0.0,0.0)", "IFCCOLOURRGBLIST(((0.0,0.0,1.0)");
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    assert!(v.gewijzigd.is_empty());
    assert_eq!(v.losse_knopen.toegevoegd.len(), 1);
    assert_eq!(v.losse_knopen.verwijderd.len(), 1);
    assert_eq!(v.losse_knopen.toegevoegd[0].type_naam, "IfcIndexedColourMap");
}

#[test]
fn geometrie_van_een_wand() {
    let a = bestand("ifc4-wall-with-opening-and-window.ifc");
    let b = vervang(
        &a,
        "#47 = IFCAXIS2PLACEMENT3D(#24, $, $);",
        "#47 = IFCAXIS2PLACEMENT3D(#99999, $, $);\n#99999 = IFCCARTESIANPOINT((0., 0., 0.5));",
    );
    let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
    let w: Vec<_> = v.gewijzigd.iter().map(|w| w.type_naam.as_str()).collect();
    // De wand zelf; wat aan de plaatsing van de wand hangt (de opening), verwijst naar het anker en blijft gelijk.
    assert_eq!(w, vec!["IfcWall"], "{v:?}");
    assert_eq!(v.gewijzigd[0].attributen[0].attribuut, "ObjectPlacement.RelativePlacement.Location.Coordinates");
    assert_eq!(v.gewijzigd[0].attributen[0].nieuw, "(0., 0., 0.5)");
}

#[test]
fn ifc2x3_model_met_stijlen() {
    let a = bestand("ifc2x3-building-architecture.ifc");
    let v = verschil(a.as_bytes(), hernummer(&a, 5).as_bytes()).unwrap();
    assert!(v.is_leeg());
    assert_eq!(v.statistiek.a.schema_tabel, "IFC2X3");
    assert!(v.statistiek.a.onbekende_typen.is_empty());
    assert_eq!(v.statistiek.a.afwijkend_aantal_attributen, 0);
    assert!(v.statistiek.a.losse_knopen > 0);
}
