//! Selectieve synchronisatie: welke mappen van een ruimte op deze computer komen. Alle regels staan hier, zonder IO,
//! zodat ze los te testen zijn; de motor en de app gebruiken ze.
//!
//! - Een uitgesloten map (pad met '/') en alles eronder blijft op de server maar komt niet (meer) op deze computer
//!   ([`uitgesloten`]). Wat lokaal nieuw of gewijzigd is in zo'n map gaat eerst nog naar de server (zie de motor).
//! - Nieuwe mappen op de server: automatisch meenemen (standaard), of alleen wat gekozen is
//!   ([`NieuweMappen::AlleenGekozen`]). Dan blijft een map die er bij het kiezen nog niet was weg, tot hij gekozen
//!   wordt ([`nieuw_niet_gekozen`]). Een map die hier al bestaat of al bestanden hier heeft, telt nooit als nieuw: zo
//!   blijft eigen werk en het verwijderen ervan gewoon synchroniseren.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Wat er gebeurt met mappen die later op de server verschijnen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NieuweMappen {
    /// Meenemen (standaard).
    #[default]
    Automatisch,
    /// Alleen de mappen die bij het kiezen bestonden en aangevinkt waren.
    AlleenGekozen,
}

fn schoon(map: &str) -> &str {
    map.trim_matches('/')
}

/// Ligt `pad` in `map` (of is het `map` zelf)? Hele mapnamen: `Teken` bevat `Tekeningen/a.pdf` niet.
pub fn onder(pad: &str, map: &str) -> bool {
    let map = schoon(map);
    !map.is_empty() && (pad == map || pad.strip_prefix(map).is_some_and(|rest| rest.starts_with('/')))
}

/// Ligt `pad` in een van de uitgesloten mappen?
pub fn uitgesloten(pad: &str, uitgesloten: &[String]) -> bool {
    uitgesloten.iter().any(|u| onder(pad, u))
}

/// De mappen boven een bestand (met '/'), van ondiep naar diep, zonder de hoofdmap: `A/B/c.txt` geeft `A`, `A/B`.
pub fn mappen_boven(pad: &str) -> Vec<&str> {
    pad.match_indices('/').map(|(i, _)| &pad[..i]).filter(|m| !m.is_empty()).collect()
}

/// Valt een bestand op de server onder een nieuwe map die niet gekozen is ([`NieuweMappen::AlleenGekozen`])? Nieuw is
/// een map die niet in `bekend` staat (de mappen van de server toen er gekozen werd) en hier niet bestaat of
/// bestanden heeft (`hier`).
pub fn nieuw_niet_gekozen(pad: &str, bekend: &BTreeSet<String>, hier: &dyn Fn(&str) -> bool) -> bool {
    mappen_boven(pad).into_iter().any(|m| !bekend.contains(m) && !hier(m))
}

/// Een uitsluitlijst opgeschoond: zonder lege paden en slashes aan de randen, zonder mappen die al onder een andere
/// uitgesloten map vallen, gesorteerd en zonder dubbele.
pub fn opgeschoond(lijst: &[String]) -> Vec<String> {
    let alle: BTreeSet<&str> = lijst.iter().map(|m| schoon(m)).filter(|m| !m.is_empty()).collect();
    alle.iter().filter(|m| !alle.iter().any(|o| o != *m && onder(m, o))).map(|m| m.to_string()).collect()
}

/// Omvang van een map op de server: alles eronder samen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Omvang {
    pub bytes: u64,
    pub bestanden: u64,
}

/// Omvang per map (met '/') uit de bestanden van de server (pad, grootte), alle mappen erboven meegeteld. Een map
/// zonder bestanden komt er niet in.
pub fn mapomvang<'a>(bestanden: impl IntoIterator<Item = (&'a str, u64)>) -> BTreeMap<String, Omvang> {
    let mut uit: BTreeMap<String, Omvang> = BTreeMap::new();
    for (pad, grootte) in bestanden {
        for map in mappen_boven(pad) {
            let o = uit.entry(map.to_string()).or_default();
            o.bytes += grootte;
            o.bestanden += 1;
        }
    }
    uit
}

/// Alle mappen boven een lijst mappen erbij (een map uit de boom van de server heeft zijn bovenliggende mappen ook).
pub fn met_bovenliggende(mappen: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    let mut uit = BTreeSet::new();
    for m in mappen {
        let m = schoon(&m).to_string();
        if m.is_empty() {
            continue;
        }
        for boven in mappen_boven(&format!("{m}/x")) {
            uit.insert(boven.to_string());
        }
    }
    uit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lijst(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn uitgesloten_hele_mapnamen() {
        let u = lijst(&["Tekeningen", "/Archief/2020/"]);
        assert!(uitgesloten("Tekeningen/plan.pdf", &u));
        assert!(uitgesloten("Tekeningen", &u));
        assert!(uitgesloten("Archief/2020/a/b.txt", &u));
        assert!(!uitgesloten("Tekeningen-oud/plan.pdf", &u));
        assert!(!uitgesloten("Teken/plan.pdf", &u));
        assert!(!uitgesloten("Archief/2021/b.txt", &u));
        assert!(!uitgesloten("plan.pdf", &u));
        // Een lege regel sluit niet alles uit.
        assert!(!uitgesloten("plan.pdf", &lijst(&["", "/"])));
    }

    #[test]
    fn mappen_boven_een_bestand() {
        assert_eq!(mappen_boven("A/B/c.txt"), vec!["A", "A/B"]);
        assert!(mappen_boven("c.txt").is_empty());
    }

    #[test]
    fn nieuwe_mappen_alleen_gekozen() {
        let bekend: BTreeSet<String> = ["Tekeningen", "Tekeningen/2026", "Rapporten"].iter().map(|s| s.to_string()).collect();
        let niets = |_: &str| false;
        assert!(!nieuw_niet_gekozen("los.txt", &bekend, &niets));
        assert!(!nieuw_niet_gekozen("Tekeningen/2026/plan.pdf", &bekend, &niets));
        assert!(nieuw_niet_gekozen("Nieuw/plan.pdf", &bekend, &niets));
        // Ook een nieuwe submap in een gekozen map.
        assert!(nieuw_niet_gekozen("Tekeningen/2027/plan.pdf", &bekend, &niets));
        // Een map die hier al bestaat of bestanden hier heeft (eigen werk) is niet nieuw.
        let hier = |m: &str| m == "Eigen" || m == "Eigen/Sub";
        assert!(!nieuw_niet_gekozen("Eigen/Sub/x.txt", &bekend, &hier));
        assert!(nieuw_niet_gekozen("Eigen/Ander/x.txt", &bekend, &hier));
    }

    #[test]
    fn uitsluitlijst_opschonen() {
        let u = opgeschoond(&lijst(&["/A/", "A/B", "C", "", "C", "AB"]));
        assert_eq!(u, lijst(&["A", "AB", "C"]));
    }

    #[test]
    fn omvang_per_map() {
        let m = mapomvang([("A/B/c.txt", 10), ("A/d.txt", 5), ("E/f.txt", 1), ("los.txt", 100)]);
        assert_eq!(m["A"], Omvang { bytes: 15, bestanden: 2 });
        assert_eq!(m["A/B"], Omvang { bytes: 10, bestanden: 1 });
        assert_eq!(m["E"], Omvang { bytes: 1, bestanden: 1 });
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn bovenliggende_mappen_erbij() {
        let s = met_bovenliggende(lijst(&["A/B/C", "D", ""]));
        assert_eq!(s.into_iter().collect::<Vec<_>>(), lijst(&["A", "A/B", "A/B/C", "D"]));
    }
}
