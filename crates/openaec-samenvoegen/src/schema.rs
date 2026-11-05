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
pub(crate) struct Attribuut {
    pub naam: String,
    pub optioneel: bool,
    pub aggregaat: Option<Aggregaat>,
    /// Het minimum en maximum aantal elementen (alleen voor SET, LIST en BAG); maximum None = onbegrensd.
    pub minimum: u32,
    pub maximum: Option<u32>,
    /// Verandert bij elk opslaan (zie [`VLUCHTIG`]); telt niet mee in een hash.
    pub vluchtig: bool,
}

/// Een INVERSE-attribuut: de instanties van `bron` die in hun attribuut `voor` naar deze entiteit verwijzen.
#[derive(Debug, Clone)]
pub(crate) struct Inverse {
    pub naam: String,
    /// Type-naam in hoofdletters.
    pub bron: String,
    pub voor: String,
    /// Bovengrens; None = onbegrensd.
    pub max: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct Entiteit {
    /// De naam zoals in het schema (bijv. `IfcTask`).
    pub naam: String,
    /// Deze entiteit en haar supertypes, in hoofdletters, van onder naar boven.
    pub voorouders: Vec<String>,
    /// IfcRoot of een subtype ervan: een object met een GlobalId.
    pub root: bool,
    /// Een subtype van IfcRelationship.
    pub relatie: bool,
    /// Een subtype van IfcProperty of IfcPhysicalQuantity: het eerste attribuut is de naam.
    pub benoemd: bool,
    pub attributen: Vec<Attribuut>,
    /// Ook de geërfde.
    pub inverses: Vec<Inverse>,
}

impl Entiteit {
    /// Is deze entiteit `naam` (in hoofdletters) of een subtype ervan?
    pub fn is_een(&self, naam: &str) -> bool {
        self.voorouders.iter().any(|v| v == naam)
    }
}

/// Velden die een programma bij elk opslaan opnieuw invult, per entiteit (met haar subtypes). Ze tellen niet mee
/// bij het vergelijken. Bij samenvoegen komt de waarde van hun als alleen hun het veld wijzigde, anders die van ons.
/// - IfcOwnerHistory: de tijdstempels en wie het laatst wijzigde. OwningUser en OwningApplication tellen wel mee.
/// - IfcWorkControl (IfcWorkPlan, IfcWorkSchedule): Open Planner Studio schrijft bij elk opslaan het huidige
///   tijdstip in CreationDate (`creationDate: now` in `ifcWriter.ts`).
pub(crate) const VLUCHTIG: &[(&str, &[&str])] = &[
    (
        "IFCOWNERHISTORY",
        &["ChangeAction", "LastModifiedDate", "LastModifyingUser", "LastModifyingApplication", "CreationDate"],
    ),
    ("IFCWORKCONTROL", &["CreationDate"]),
];

pub(crate) struct Schema {
    pub naam: &'static str,
    entiteiten: HashMap<String, Entiteit>,
    /// De INVERSE-attributen met een bovengrens: (bron in hoofdletters, FOR-attribuut).
    begrensd: Vec<(String, String)>,
}

impl Schema {
    /// De entiteit bij een type-naam in hoofdletters (zoals in een IFC-bestand).
    pub fn entiteit(&self, type_naam: &str) -> Option<&Entiteit> {
        self.entiteiten.get(type_naam)
    }

    /// Kan attribuut `naam` van entiteit `e` het FOR-attribuut van een INVERSE met een bovengrens zijn?
    pub fn is_inverse_voor(&self, e: &Entiteit, naam: &str) -> bool {
        self.begrensd.iter().any(|(bron, voor)| voor == naam && e.is_een(bron))
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
    struct Ruw<'t> {
        supertype: Option<String>,
        vlaggen: &'t str,
        attributen: &'t str,
        inverses: &'t str,
        naam: &'t str,
    }
    let mut ruw: HashMap<String, Ruw> = HashMap::new();
    for regel in tekst.lines() {
        if regel.starts_with('!') || regel.is_empty() {
            continue;
        }
        let delen: Vec<&str> = regel.split('|').collect();
        let [type_naam, supertype, vlaggen, attributen, inverses] = delen[..] else {
            continue;
        };
        ruw.insert(
            type_naam.to_ascii_uppercase(),
            Ruw {
                supertype: (supertype != "-").then(|| supertype.to_ascii_uppercase()),
                vlaggen,
                attributen,
                inverses,
                naam: type_naam,
            },
        );
    }
    let mut entiteiten = HashMap::new();
    for (sleutel, r) in &ruw {
        let mut voorouders = vec![sleutel.clone()];
        while let Some(boven) = ruw.get(voorouders.last().unwrap()).and_then(|r| r.supertype.clone()) {
            if voorouders.contains(&boven) || voorouders.len() > 64 {
                break;
            }
            voorouders.push(boven);
        }
        let vluchtig: Vec<&str> = VLUCHTIG
            .iter()
            .filter(|(entiteit, _)| voorouders.iter().any(|v| v == entiteit))
            .flat_map(|(_, velden)| velden.iter().copied())
            .collect();
        let attributen = r
            .attributen
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
                let (minimum, maximum) = soort.get(1..).and_then(|g| g.split_once('-')).unwrap_or(("0", "?"));
                let minimum = minimum.parse().unwrap_or(0);
                let maximum = maximum.parse().ok();
                let vluchtig = vluchtig.contains(&naam.as_str());
                Attribuut { naam, optioneel, aggregaat, minimum, maximum, vluchtig }
            })
            .collect();
        // Inverses van boven naar onder; een herdeclaratie vervangt die met dezelfde naam.
        let mut inverses: Vec<Inverse> = Vec::new();
        for voorouder in voorouders.iter().rev() {
            let Some(v) = ruw.get(voorouder) else { continue };
            for inv in v.inverses.split(',').filter(|i| !i.is_empty()) {
                let Some(inverse) = lees_inverse(inv) else { continue };
                match inverses.iter_mut().find(|i| i.naam == inverse.naam) {
                    Some(bestaand) => *bestaand = inverse,
                    None => inverses.push(inverse),
                }
            }
        }
        entiteiten.insert(
            sleutel.clone(),
            Entiteit {
                naam: r.naam.to_string(),
                voorouders,
                root: r.vlaggen.contains('R'),
                relatie: r.vlaggen.contains('Q'),
                benoemd: r.vlaggen.contains('N'),
                attributen,
                inverses,
            },
        );
    }
    let mut begrensd: Vec<(String, String)> = entiteiten
        .values()
        .flat_map(|e| e.inverses.iter().filter(|i| i.max.is_some()).map(|i| (i.bron.clone(), i.voor.clone())))
        .collect();
    begrensd.sort();
    begrensd.dedup();
    Schema { naam, entiteiten, begrensd }
}

/// `Naam=Type.ForAttribuut[min:max]`.
fn lees_inverse(tekst: &str) -> Option<Inverse> {
    let (naam, rest) = tekst.split_once('=')?;
    let (bron, rest) = rest.split_once('.')?;
    let (voor, grenzen) = rest.split_once('[')?;
    let (_, max) = grenzen.trim_end_matches(']').split_once(':')?;
    Some(Inverse {
        naam: naam.to_string(),
        bron: bron.to_ascii_uppercase(),
        voor: voor.to_string(),
        max: max.parse().ok(),
    })
}

/// Is dit een geldige GlobalId: 22 tekens uit `0-9A-Za-z_$`, het eerste teken `0` tot en met `3`?
pub(crate) fn is_globalid(tekst: &[u8]) -> bool {
    tekst.len() == 22
        && matches!(tekst[0], b'0'..=b'3')
        && tekst.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
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
            assert_eq!((elementen.minimum, elementen.maximum), (1, None));
            assert!(schema.entiteit("IFCPROPERTYSINGLEVALUE").unwrap().benoemd);
            assert!(!schema.entiteit("IFCCARTESIANPOINT").unwrap().root);
        }
        assert!(kies("IFC2X2").is_none());
        let ifc4 = kies("IFC4").unwrap();
        let wand = ifc4.entiteit("IFCWALLSTANDARDCASE").unwrap();
        assert!(wand.is_een("IFCELEMENT") && wand.is_een("IFCROOT") && !wand.is_een("IFCRELATIONSHIP"));
        let bevat = wand.inverses.iter().find(|i| i.naam == "ContainedInStructure").unwrap();
        assert_eq!(
            (bevat.bron.as_str(), bevat.voor.as_str(), bevat.max),
            ("IFCRELCONTAINEDINSPATIALSTRUCTURE", "RelatedElements", Some(1))
        );
        let opening = ifc4.entiteit("IFCOPENINGELEMENT").unwrap();
        let vult = opening.inverses.iter().find(|i| i.naam == "VoidsElements").unwrap();
        assert_eq!(vult.max, Some(1));
        let eigenaar = ifc4.entiteit("IFCOWNERHISTORY").unwrap();
        let vluchtig: Vec<&str> = eigenaar.attributen.iter().filter(|a| a.vluchtig).map(|a| a.naam.as_str()).collect();
        assert_eq!(vluchtig.len(), 5);
        assert!(!eigenaar.attributen[0].vluchtig, "OwningUser telt mee");
        let plan = kies("IFC4X3_ADD2").unwrap().entiteit("IFCWORKPLAN").unwrap();
        assert!(plan.attributen.iter().any(|a| a.naam == "CreationDate" && a.vluchtig));
        let punt = ifc4.entiteit("IFCCARTESIANPOINT").unwrap();
        assert_eq!((punt.attributen[0].minimum, punt.attributen[0].maximum), (1, Some(3)));
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
