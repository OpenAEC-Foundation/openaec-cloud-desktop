//! Grenzen voor diepte en grootte (ontwerp §5.14): de bibliotheek leest onbetrouwbare invoer en mag niet vastlopen of
//! crashen. Een stack overflow stopt het hele testprogramma; daarom staan deze tests in een eigen bestand.

mod gemeen;

use gemeen::*;
use openaec_samenvoegen::{verschil, voeg_samen};

const N: usize = 50_000;

/// Een gebouw waarin wand W aan het eind van een keten van `N` plaatsingen staat (elk relatief aan de vorige).
fn met_keten(schema: &str) -> String {
    let mut regels = String::from("#1000=IFCCARTESIANPOINT((0.,0.,0.));\n#1001=IFCAXIS2PLACEMENT3D(#1000,$,$);\n");
    regels.push_str("#2000=IFCLOCALPLACEMENT($,#1001);\n");
    for i in 1..N {
        regels.push_str(&format!("#{}=IFCLOCALPLACEMENT(#{},#1001);\n", 2000 + i, 1999 + i));
    }
    let b = vervang(&gebouw(schema), "'W',$,$,#40,", &format!("'W',$,$,#{},", 1999 + N));
    voeg_toe(&b, &regels)
}

/// Een keten van 50 000 plaatsingen: verschil en samenvoegen werken, zonder stack overflow.
#[test]
fn lange_keten_van_plaatsingen() {
    for schema in SCHEMAS {
        let b = met_keten(schema);
        let x = vervang(&b, "'V',", "'V-ons',");
        let y = vervang(&b, "#1000=IFCCARTESIANPOINT((0.,0.,0.));", "#1000=IFCCARTESIANPOINT((1.,0.,0.));");
        let v = verschil(b.as_bytes(), y.as_bytes()).unwrap();
        assert_eq!(v.gewijzigd.len(), 1, "{schema}: {:?}", v.gewijzigd.iter().map(|w| &w.naam).collect::<Vec<_>>());
        let s = voeg_samen(b.as_bytes(), x.as_bytes(), y.as_bytes()).unwrap();
        assert!(s.gelukt, "{schema}: {:#?}", s.conflicten);
    }
}

/// Een waarde met 100 000 geneste haakjes: een fout of een uitkomst, geen crash.
#[test]
fn diep_geneste_lijsten() {
    let diep = format!("{}0.{}", "(".repeat(100_000), ")".repeat(100_000));
    let b = gebouw("IFC4");
    let x = voeg_toe(&b, &format!("#900=IFCCARTESIANPOINT({diep});"));
    let _ = verschil(b.as_bytes(), x.as_bytes());
    let _ = verschil(x.as_bytes(), x.as_bytes());
    let _ = voeg_samen(b.as_bytes(), x.as_bytes(), b.as_bytes());
    let _ = voeg_samen(b.as_bytes(), x.as_bytes(), x.as_bytes());
    // Ook ongesloten: geen haakje terug.
    let open = voeg_toe(&b, &format!("#900=IFCCARTESIANPOINT({});", "(".repeat(100_000)));
    assert!(verschil(open.as_bytes(), open.as_bytes()).is_err());
}

/// Een tekst van 8 MB in een naam, en een vorm met 50 000 elementen in een SET: het werkt, en de wijziging komt
/// over.
#[test]
fn grote_waarden() {
    let b = gebouw("IFC4");
    let lang = "a".repeat(8 << 20);
    let x = vervang(&b, "'W',", &format!("'{lang}',"));
    let s = voeg_samen(b.as_bytes(), x.as_bytes(), b.as_bytes()).unwrap();
    assert!(s.gelukt && s.inhoud.as_deref() == Some(x.as_bytes()));
    let y = vervang(&b, "'V',", "'V-hun',");
    let s = voeg_samen(b.as_bytes(), x.as_bytes(), y.as_bytes()).unwrap();
    assert!(s.gelukt, "{:#?}", s.conflicten);
    let r = String::from_utf8(s.inhoud.unwrap()).unwrap();
    assert!(r.contains(&lang) && r.contains("'V-hun'"));

    // 50 000 punten in één SET van een vorm (IfcShapeRepresentation.Items).
    let mut regels = String::new();
    let mut items = Vec::new();
    for i in 0..50_000u32 {
        regels.push_str(&format!("#{}=IFCCARTESIANPOINT(({i}.,0.,0.));\n", 10_000 + i));
        items.push(format!("#{}", 10_000 + i));
    }
    regels.push_str(&format!("#9000=IFCSHAPEREPRESENTATION(#7,'Body','PointCloud',({}));\n", items.join(",")));
    regels.push_str("#9001=IFCPRODUCTDEFINITIONSHAPE($,$,(#9000));\n");
    let groot = voeg_toe(&vervang(&b, "'W',$,$,#40,$,", "'W',$,$,#40,#9001,"), &regels);
    let x = vervang(&groot, "'V',", "'V-ons',");
    let y = vervang(&groot, "#10000=IFCCARTESIANPOINT((0.,0.,0.));", "#10000=IFCCARTESIANPOINT((0.,0.,5.));");
    let s = voeg_samen(groot.as_bytes(), x.as_bytes(), y.as_bytes()).unwrap();
    assert!(s.gelukt, "{:#?}", s.conflicten);
}
