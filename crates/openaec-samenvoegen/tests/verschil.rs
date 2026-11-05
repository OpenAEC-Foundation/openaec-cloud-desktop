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

#[test]
fn gedeelde_plaatsing_splitsen_is_een_wijziging() {
    for schema in SCHEMAS {
        // Basis: W en V delen plaatsing #40; Z hangt eronder.
        let a = gebouw(schema);
        let a = vervang(
            &a,
            "#44=IFCBUILDINGELEMENTPROXY('0WandV0000000000000000',#5,'V',$,$,#45,",
            "#44=IFCBUILDINGELEMENTPROXY('0WandV0000000000000000',#5,'V',$,$,#40,",
        );
        let a = voeg_toe(&a, "#70=IFCLOCALPLACEMENT(#40,#46);\n#71=IFCBUILDINGELEMENTPROXY('0WandZ0000000000000000',#5,'Z',$,$,#70,$,$,$);");
        // Een kant splitst: V krijgt een eigen kopie van de plaatsing.
        let b = vervang(&a, "'V',$,$,#40,", "'V',$,$,#72,");
        let b = voeg_toe(&b, "#72=IFCLOCALPLACEMENT(#20,#41);");
        let v = verschil(a.as_bytes(), b.as_bytes()).unwrap();
        // W en V zelf staan op dezelfde plek; Z hangt nu alleen aan de plaatsing van W.
        let namen: Vec<_> = v.gewijzigd.iter().map(|w| w.naam.clone().unwrap()).collect();
        assert_eq!(namen, vec!["Z"], "{schema}");
        assert!(v.gewijzigd[0].attributen[0].oud.contains("(+1)"));
    }
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
