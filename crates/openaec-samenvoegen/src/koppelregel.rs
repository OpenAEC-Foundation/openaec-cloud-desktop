//! De koppelregel (ontwerp §5.6).
//!
//! Per object X en per versie bepalen we de *definities die voor X gelden*: eigenschappensets, typen, materialen,
//! classificaties en documenten. Een definitie geldt voor X als hij aan X hangt via `IfcRelDefines*` of
//! `IfcRelAssociates*`, of via een attribuut van een definitie die voor X geldt (bijv. het type van X en de
//! `HasPropertySets` van dat type, of wat via `IfcRelAssociates*` aan zo'n definitie hangt), ook indirect.
//!
//! Conflict als: D gold in de basis voor X; bij de ene kant geldt D niet meer voor X (koppeling weg, D uit de
//! `HasPropertySets` van het type, ander type, of X verwijderd); bij de andere kant geldt D nog voor X, en die kant
//! wijzigde D. Komen het weghalen en de wijziging van dezelfde kant, dan is het geen conflict.
//!
//! De identiteit van een definitie:
//! - een object (bijv. een IfcPropertySet of IfcTypeObject): `G` + de sleutel;
//! - een knoop waar een koppelrelatie rechtstreeks naar wijst (bijv. een IfcMaterial in IfcRelAssociatesMaterial):
//!   `R` + de sleutel van de relatie, het attribuut en de positie. Zo hangt de identiteit niet af van hoeveel
//!   relaties dezelfde knoop delen;
//! - een knoop dieper in een definitie die zelf aan een koppelrelatie hangt (bijv. de IfcClassification achter een
//!   IfcClassificationReference): `A` + de identiteit van het anker (alle ankers samen).

use std::collections::{HashMap, HashSet};

use crate::model::{attribuut_tekst, op_sleutel, Hash, Model, Soort};
use crate::ontleden::{Token, Tokens};
use crate::samenvoegen::{Conflict, ConflictSoort, Gevonden, Plan, B, H, O};

type DefId = Vec<u8>;

/// Is dit een koppelrelatie met definities (IfcRelDefines* of IfcRelAssociates*)?
pub(crate) fn is_koppelrelatie(m: &Model, i: usize) -> bool {
    m.entiteit(i).is_some_and(|e| e.is_een("IFCRELDEFINES") || e.is_een("IFCRELASSOCIATES"))
}

/// De definities in één versie.
struct Definities<'m, 'a> {
    m: &'m Model<'a>,
    op_sleutel: HashMap<&'m str, usize>,
    /// Per object (sleutel): de definities die er via een koppelrelatie aan hangen, met de sleutel van de relatie.
    aan: HashMap<&'m str, Vec<(DefId, &'m str)>>,
    /// Objecten waar een koppelrelatie naar wijst als definitie (`Relating*`).
    relating: HashSet<&'m str>,
    /// Per knoopdefinitie: de instantie.
    instantie: HashMap<DefId, usize>,
    /// Per definitie: de definities waar hij naar verwijst of die aan hem hangen (onthouden).
    kinderen: HashMap<DefId, Vec<DefId>>,
    /// Ankeridentiteit -> knoop (alleen gevuld als het nodig is).
    op_anker: Option<HashMap<Hash, usize>>,
}

impl<'m, 'a> Definities<'m, 'a> {
    fn nieuw(m: &'m Model<'a>) -> Self {
        let mut d = Definities {
            m,
            op_sleutel: op_sleutel(m),
            aan: HashMap::new(),
            relating: HashSet::new(),
            instantie: HashMap::new(),
            kinderen: HashMap::new(),
            op_anker: None,
        };
        for object in &m.objecten {
            let i = object.inst as usize;
            if !is_koppelrelatie(m, i) {
                continue;
            }
            let Some(e) = m.entiteit(i) else { continue };
            let rel = object.sleutel.as_str();
            let mut gerelateerd = Vec::new();
            let mut definities = Vec::new();
            for (j, a) in e.attributen.iter().enumerate() {
                let related = a.naam.starts_with("Related");
                if !related && !a.naam.starts_with("Relating") {
                    continue;
                }
                let mut p = 0u32;
                for (_, token) in Tokens::nieuw(attribuut_tekst(m, i, j)) {
                    let Token::Verwijzing(n) = token else { continue };
                    let positie = p;
                    p += 1;
                    let Some(&x) = m.bestand.index.get(&n) else { continue };
                    let x = x as usize;
                    match (m.soort[x], related) {
                        (Soort::Object(o), true) => gerelateerd.push(m.objecten[o as usize].sleutel.as_str()),
                        (Soort::Object(o), false) => {
                            let sleutel = m.objecten[o as usize].sleutel.as_str();
                            d.relating.insert(sleutel);
                            definities.push(object_id(sleutel));
                        }
                        (Soort::Knoop, false) => {
                            let mut id = vec![b'R'];
                            id.extend_from_slice(&(rel.len() as u32).to_le_bytes());
                            id.extend_from_slice(rel.as_bytes());
                            id.extend_from_slice(&(j as u32).to_le_bytes());
                            id.extend_from_slice(&positie.to_le_bytes());
                            d.instantie.insert(id.clone(), x);
                            definities.push(id);
                        }
                        (Soort::Knoop, true) => {}
                    }
                }
            }
            for x in gerelateerd {
                let lijst = d.aan.entry(x).or_default();
                for id in &definities {
                    lijst.push((id.clone(), rel));
                }
            }
        }
        d
    }

    /// Is dit object een definitie (IfcPropertyDefinition, IfcTypeObject, of iets waar een koppelrelatie naar wijst)?
    fn is_definitie_object(&self, i: usize, sleutel: &str) -> bool {
        self.relating.contains(sleutel)
            || self.m.entiteit(i).is_some_and(|e| e.is_een("IFCPROPERTYDEFINITION") || e.is_een("IFCTYPEOBJECT"))
    }

    /// Hangt deze knoop rechtstreeks aan een koppelrelatie (als `Relating*`)?
    fn is_definitie_knoop(&self, k: usize) -> bool {
        self.m.ankers(k).iter().any(|a| {
            let i = self.m.objecten[a.object as usize].inst as usize;
            is_koppelrelatie(self.m, i) && self.m.attribuut_naam(i, a.attribuut as usize).starts_with("Relating")
        })
    }

    /// De instantie van een definitie in deze versie.
    fn instantie_van(&mut self, id: &[u8]) -> Option<usize> {
        match id.first()? {
            b'G' => {
                let sleutel = std::str::from_utf8(&id[1..]).ok()?;
                self.op_sleutel.get(sleutel).map(|&o| self.m.objecten[o].inst as usize)
            }
            b'R' => self.instantie.get(id).copied(),
            _ => {
                let a: Hash = id.get(1..)?.try_into().ok()?;
                let m = self.m;
                let kaart = self.op_anker.get_or_insert_with(|| {
                    m.anker_id.iter().enumerate().filter_map(|(k, a)| a.map(|a| (a, k))).collect()
                });
                kaart.get(&a).copied()
            }
        }
    }

    /// De inhoud van een definitie (objecthash of knoophash); None als hij in deze versie niet bestaat.
    fn inhoud(&mut self, id: &[u8]) -> Option<Hash> {
        let i = self.instantie_van(id)?;
        Some(match self.m.soort[i] {
            Soort::Object(o) => self.m.objecten[o as usize].hash,
            Soort::Knoop => self.m.knoop_hash[i],
        })
    }

    /// De definities waar deze definitie naar verwijst, of die via een koppelrelatie aan haar hangen.
    fn kinderen_van(&mut self, id: &[u8]) -> Vec<DefId> {
        if let Some(k) = self.kinderen.get(id) {
            return k.clone();
        }
        let mut uit = Vec::new();
        if let Some(i) = self.instantie_van(id) {
            let m = self.m;
            let eigen_object = match m.soort[i] {
                Soort::Object(o) => {
                    if let Some(extra) = self.aan.get(m.objecten[o as usize].sleutel.as_str()) {
                        uit.extend(extra.iter().map(|(d, _)| d.clone()));
                    }
                    Some(o)
                }
                Soort::Knoop => None,
            };
            // Loop door de inhoud van de definitie: haar eigen knopen en knopen zonder anker.
            let mut stapel = vec![i];
            let mut gezien = HashSet::new();
            while let Some(x) = stapel.pop() {
                if !gezien.insert(x) || gezien.len() > 100_000 {
                    continue;
                }
                for &c in m.bestand.verwijzingen(x) {
                    let c = c as usize;
                    match m.soort[c] {
                        Soort::Object(o) => {
                            let sleutel = m.objecten[o as usize].sleutel.as_str();
                            if Some(o) != eigen_object && self.is_definitie_object(c, sleutel) {
                                uit.push(object_id(sleutel));
                            }
                        }
                        Soort::Knoop => match m.anker_id[c] {
                            None => stapel.push(c),
                            Some(_) if eigen_object.is_some_and(|o| m.ankers(c).iter().any(|a| a.object == o)) => {
                                stapel.push(c)
                            }
                            Some(a) => {
                                if self.is_definitie_knoop(c) {
                                    let mut d = vec![b'A'];
                                    d.extend_from_slice(&a);
                                    uit.push(d);
                                }
                            }
                        },
                    }
                }
            }
        }
        uit.sort();
        uit.dedup();
        self.kinderen.insert(id.to_vec(), uit.clone());
        uit
    }

    /// De definities die voor object X gelden, met per definitie de definities waar hij vandaan bereikt werd (None =
    /// rechtstreeks aan X).
    fn voor(&mut self, x: &str) -> HashMap<DefId, Vec<Option<DefId>>> {
        let mut uit: HashMap<DefId, Vec<Option<DefId>>> = HashMap::new();
        let mut stapel: Vec<DefId> = Vec::new();
        if let Some(lijst) = self.aan.get(x) {
            for (d, _) in lijst {
                if !uit.contains_key(d) {
                    stapel.push(d.clone());
                }
                uit.entry(d.clone()).or_default().push(None);
            }
        }
        while let Some(d) = stapel.pop() {
            if uit.len() > 10_000 {
                break;
            }
            for k in self.kinderen_van(&d) {
                if k == object_id(x) {
                    continue;
                }
                if !uit.contains_key(&k) {
                    stapel.push(k.clone());
                }
                let ouders = uit.entry(k).or_default();
                if !ouders.contains(&Some(d.clone())) {
                    ouders.push(Some(d.clone()));
                }
            }
        }
        uit
    }

    /// Een definitie zoals een mens hem leest.
    fn weergave(&mut self, id: &[u8]) -> String {
        match self.instantie_van(id) {
            Some(i) => match self.m.soort[i] {
                Soort::Object(o) => format!("{} {}", self.m.type_weergave(i), self.m.object_weergave(o as usize)),
                Soort::Knoop => self.m.toon_instantie(i, 120),
            },
            None => "een definitie".to_string(),
        }
    }
}

fn object_id(sleutel: &str) -> DefId {
    [b"G".as_slice(), sleutel.as_bytes()].concat()
}

const KANT: [&str; 3] = ["basis", "ons", "hun"];

/// Past de koppelregel toe.
pub(crate) fn koppelregel(m: &[Model; 3], plan: &Plan) -> Gevonden {
    let mut d = [Definities::nieuw(&m[B]), Definities::nieuw(&m[O]), Definities::nieuw(&m[H])];
    let mut uit = Vec::new();
    let mut sleutels: Vec<&str> = d[B].aan.keys().copied().collect();
    sleutels.sort_unstable();
    let mut inhoud: [HashMap<DefId, Option<Hash>>; 3] = Default::default();
    for x in sleutels {
        let voor_b = d[B].voor(x);
        let mut gewijzigd: Vec<(DefId, [bool; 3])> = Vec::new();
        let mut lijst: Vec<&DefId> = voor_b.keys().collect();
        lijst.sort();
        for def in lijst {
            let mut w = [false; 3];
            for k in [B, O, H] {
                if !inhoud[k].contains_key(def) {
                    let h = d[k].inhoud(def);
                    inhoud[k].insert(def.clone(), h);
                }
            }
            for k in [O, H] {
                w[k] = inhoud[k][def] != inhoud[B][def];
            }
            if w[O] || w[H] {
                gewijzigd.push((def.clone(), w));
            }
        }
        if gewijzigd.is_empty() {
            continue;
        }
        let aanwezig = [true, d[O].op_sleutel.contains_key(x), d[H].op_sleutel.contains_key(x)];
        let in_kant = |k: usize| aanwezig[k];
        let voor_o = if in_kant(O) { d[O].voor(x) } else { HashMap::new() };
        let voor_h = if in_kant(H) { d[H].voor(x) } else { HashMap::new() };
        let voor = [&voor_b, &voor_o, &voor_h];
        for (def, w) in gewijzigd {
            let geldt = |k: usize| voor[k].contains_key(&def);
            let mut paar = None;
            for (weg, wijzig) in [(O, H), (H, O)] {
                if !geldt(weg) && geldt(wijzig) && w[wijzig] {
                    paar = Some((weg, wijzig));
                    break;
                }
            }
            let Some((weg, wijzig)) = paar else { continue };
            let Some(&si) = plan.op_sleutel.get(x) else { continue };
            let naam = d[B].weergave(&def);
            let x_weg = !in_kant(weg);
            let reden = if x_weg { "verwijderde het object" } else { "haalde hem weg" };
            let mut c = Conflict::nieuw(
                ConflictSoort::KoppelingVerdwenen,
                format!(
                    "{naam} gold in de basis voor dit object. {} {reden}, terwijl {} die definitie wijzigde",
                    KANT[weg], KANT[wijzig]
                ),
            )
            .over(m, &plan.sleutels[si]);
            c.attribuut = Some("definitie".into());
            c.basis = Some(naam.clone());
            let status = |k: usize| -> String {
                if !in_kant(k) {
                    "object verwijderd".into()
                } else if !geldt(k) {
                    "geldt niet meer".into()
                } else if w[k] {
                    "geldt, gewijzigd".into()
                } else {
                    "geldt, ongewijzigd".into()
                }
            };
            c.ons = Some(status(O));
            c.hun = Some(status(H));
            uit.push(c);
        }
    }
    uit
}
