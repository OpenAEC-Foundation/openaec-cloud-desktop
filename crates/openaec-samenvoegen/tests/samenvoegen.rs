//! Samenvoegen: de regels uit ontwerp §5.6, de verplichte tegenvoorbeelden uit de reviews, en eigenschappen op echte
//! bestanden.

mod gemeen;

use gemeen::*;
use openaec_samenvoegen::{verschil, voeg_samen, BijConflict, ConflictSoort, Kant, Opties, Samenvoeging};

fn voeg(b: &str, o: &str, h: &str) -> Samenvoeging {
    voeg_samen(b.as_bytes(), o.as_bytes(), h.as_bytes(), &Opties::default()).unwrap()
}

fn voeg_met(b: &str, o: &str, h: &str, bij_conflict: BijConflict) -> Samenvoeging {
    voeg_samen(b.as_bytes(), o.as_bytes(), h.as_bytes(), &Opties { bij_conflict }).unwrap()
}

/// Het resultaat; de samenvoeging moet zonder conflict lukken.
fn gelukt(b: &str, o: &str, h: &str) -> String {
    let s = voeg(b, o, h);
    assert!(s.gelukt && s.conflicten.is_empty(), "conflicten: {:#?}", s.conflicten);
    String::from_utf8(s.inhoud.unwrap()).unwrap()
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
    assert!(v.is_leeg(), "verschil: {v:#?}");
}

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
