//! De IFC-schema's: per entiteit het supertype, of het een IfcRoot is, en de expliciete attributen in volgorde.
//!
//! De tabellen in `src/schema/*.txt` komen uit de EXPRESS-schema's van buildingSMART, gemaakt met
//! `scripts/genereer_schema.py` (zie daar voor de bron en de opmaak). We lezen een tabel één keer per proces.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Soort aggregaat van een attribuut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Aggregaat {
    Set,
    List,
    Bag,
    Array,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // optioneel, aggregaat en minimum gebruikt het samenvoegen.
pub(crate) struct Attribuut {
    pub naam: String,
    pub optioneel: bool,
    pub aggregaat: Option<Aggregaat>,
    /// Het minimum aantal elementen (alleen voor SET, LIST en BAG).
    pub minimum: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct Entiteit {
    /// De naam zoals in het schema (bijv. `IfcTask`).
    pub naam: String,
    /// IfcRoot of een subtype ervan: een object met een GlobalId.
    pub root: bool,
    /// Een subtype van IfcRelationship.
    pub relatie: bool,
    /// Een subtype van IfcProperty of IfcPhysicalQuantity: het eerste attribuut is de naam.
    #[allow(dead_code)] // gebruikt het samenvoegen.
    pub benoemd: bool,
    pub attributen: Vec<Attribuut>,
}

pub(crate) struct Schema {
    pub naam: &'static str,
    entiteiten: HashMap<String, Entiteit>,
}

impl Schema {
    /// De entiteit bij een type-naam in hoofdletters (zoals in een IFC-bestand).
    pub fn entiteit(&self, type_naam: &str) -> Option<&Entiteit> {
        self.entiteiten.get(type_naam)
    }
}

const IFC2X3: &str = include_str!("schema/ifc2x3.txt");
const IFC4: &str = include_str!("schema/ifc4.txt");
const IFC4X3: &str = include_str!("schema/ifc4x3_add2.txt");

/// Het schema bij de naam uit FILE_SCHEMA, of None als we het niet kennen.
pub(crate) fn kies(file_schema: &str) -> Option<&'static Schema> {
    static TABELLEN: [OnceLock<Schema>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let naam = file_schema.trim().to_ascii_uppercase();
    let (plek, tekst, schema) = match naam.as_str() {
        "IFC2X3" | "IFC2X3_TC1" | "IFC2X3_FINAL" => (0, IFC2X3, "IFC2X3"),
        "IFC4" | "IFC4_ADD1" | "IFC4_ADD2" | "IFC4_ADD2_TC1" => (1, IFC4, "IFC4"),
        n if n == "IFC4X3" || n.starts_with("IFC4X3_") => (2, IFC4X3, "IFC4X3_ADD2"),
        _ => return None,
    };
    Some(TABELLEN[plek].get_or_init(|| lees_tabel(schema, tekst)))
}

fn lees_tabel(naam: &'static str, tekst: &str) -> Schema {
    let mut entiteiten = HashMap::new();
    for regel in tekst.lines() {
        if regel.starts_with('!') || regel.is_empty() {
            continue;
        }
        let mut delen = regel.split('|');
        let (Some(type_naam), Some(_supertype), Some(vlaggen), Some(attrs)) =
            (delen.next(), delen.next(), delen.next(), delen.next())
        else {
            continue;
        };
        let attributen = attrs
            .split(',')
            .filter(|a| !a.is_empty())
            .map(|a| {
                let (links, soort) = a.split_once(':').unwrap_or((a, ""));
                let optioneel = links.ends_with('?');
                let naam = links.trim_end_matches('?').to_string();
                let aggregaat = match soort.as_bytes().first() {
                    Some(b'S') => Some(Aggregaat::Set),
                    Some(b'L') => Some(Aggregaat::List),
                    Some(b'B') => Some(Aggregaat::Bag),
                    Some(b'A') => Some(Aggregaat::Array),
                    _ => None,
                };
                let minimum = soort.get(1..).and_then(|m| m.parse().ok()).unwrap_or(0);
                Attribuut {
                    naam,
                    optioneel,
                    aggregaat,
                    minimum,
                }
            })
            .collect();
        entiteiten.insert(
            type_naam.to_ascii_uppercase(),
            Entiteit {
                naam: type_naam.to_string(),
                root: vlaggen.contains('R'),
                relatie: vlaggen.contains('Q'),
                benoemd: vlaggen.contains('N'),
                attributen,
            },
        );
    }
    Schema { naam, entiteiten }
}

/// Is dit een geldige GlobalId: 22 tekens uit `0-9A-Za-z_$`, het eerste teken `0` tot en met `3`?
pub(crate) fn is_globalid(tekst: &[u8]) -> bool {
    tekst.len() == 22
        && matches!(tekst[0], b'0'..=b'3')
        && tekst
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_laden() {
        for naam in ["IFC2X3", "IFC4", "IFC4X3_ADD2", "ifc4x3"] {
            let schema = kies(naam).unwrap();
            let wand = schema.entiteit("IFCWALL").unwrap();
            assert!(wand.root && !wand.relatie);
            assert_eq!(wand.attributen[0].naam, "GlobalId");
            let rel = schema.entiteit("IFCRELCONTAINEDINSPATIALSTRUCTURE").unwrap();
            assert!(rel.root && rel.relatie);
            let elementen = rel.attributen.iter().find(|a| a.naam == "RelatedElements").unwrap();
            assert_eq!(elementen.aggregaat, Some(Aggregaat::Set));
            assert_eq!(elementen.minimum, 1);
            assert!(schema.entiteit("IFCPROPERTYSINGLEVALUE").unwrap().benoemd);
            assert!(!schema.entiteit("IFCCARTESIANPOINT").unwrap().root);
        }
        assert!(kies("IFC2X2").is_none());
        let taaktijd = kies("IFC4X3_ADD2").unwrap().entiteit("IFCTASKTIME").unwrap();
        assert_eq!(taaktijd.attributen.len(), 20);
        assert_eq!(kies("IFC4").unwrap().entiteit("IFCSIUNIT").unwrap().attributen.len(), 4);
    }

    #[test]
    fn globalid() {
        assert!(is_globalid(b"3ScNjsTf6weF542amHNyMB"));
        assert!(is_globalid(b"0$_aaaaaaaaaaaaaaaaaaa"));
        assert!(!is_globalid(b"4ScNjsTf6weF542amHNyMB"));
        assert!(!is_globalid(b"3ScNjsTf6weF542amHNyM"));
        assert!(!is_globalid(b"3ScNjsTf6weF542amHNy-B"));
    }
}
