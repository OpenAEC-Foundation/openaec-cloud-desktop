//! Het verschil tussen twee versies van een IFC-bestand, per object.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::model::{
    attribuut_tekst, enkele_verwijzing, header_vluchtig, koppel_relaties, markeer_gemeen, op_sleutel, Hash, Model,
    Soort, VersieStatistiek, HEADER_VELDEN,
};
use crate::Fout;

/// Zo lang wordt een waarde in een rapport hoogstens.
pub(crate) const MAX_TEKST: usize = 200;

/// Het verschil tussen versie a en versie b.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Verschil {
    /// Objecten die in b staan en niet in a.
    pub toegevoegd: Vec<ObjectInfo>,
    /// Objecten die in a staan en niet in b.
    pub verwijderd: Vec<ObjectInfo>,
    /// Objecten met een andere inhoud.
    pub gewijzigd: Vec<Wijziging>,
    /// Knopen waar niets naar verwijst (bijv. IfcStyledItem), die erbij kwamen of verdwenen.
    pub losse_knopen: LosseKnopen,
    /// Gewijzigde velden in de HEADER (FILE_NAME.author, FILE_SCHEMA, ...); de tijdstempel telt niet mee.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub header: Vec<AttribuutWijziging>,
    /// Vluchtige velden die veranderden (tijdstempels, CreationDate van een planning...). Ze maken een object niet
    /// gewijzigd en [`Verschil::is_leeg`] telt ze niet.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub vluchtig: Vec<VluchtigeWijziging>,
    pub statistiek: Statistiek,
}

impl Verschil {
    /// Zijn a en b inhoudelijk gelijk? Vluchtige velden tellen niet mee.
    pub fn is_leeg(&self) -> bool {
        self.toegevoegd.is_empty()
            && self.verwijderd.is_empty()
            && self.gewijzigd.is_empty()
            && self.losse_knopen.toegevoegd.is_empty()
            && self.losse_knopen.verwijderd.is_empty()
            && self.header.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectInfo {
    pub globalid: String,
    #[serde(rename = "type")]
    pub type_naam: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub naam: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Wijziging {
    pub globalid: String,
    /// De GlobalId in b, als die anders is (een relatie die bij het opslaan een nieuwe GlobalId kreeg).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub globalid_nieuw: Option<String>,
    #[serde(rename = "type")]
    pub type_naam: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub naam: Option<String>,
    pub attributen: Vec<AttribuutWijziging>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AttribuutWijziging {
    /// De naam uit het schema, of `positie n`; `type` als het type veranderde.
    pub attribuut: String,
    pub oud: String,
    pub nieuw: String,
    /// Bij een lijst van verwijzingen: wat erbij kwam.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub erbij: Vec<String>,
    /// Bij een lijst van verwijzingen: wat eraf ging.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub eraf: Vec<String>,
}

/// Een vluchtig veld dat veranderde.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct VluchtigeWijziging {
    /// Waar het veld staat: `HEADER`, een object (`Basisplanning [GlobalId]`) of een knoop eraan
    /// (`OwnerHistory van W [GlobalId] (+3)`).
    pub waar: String,
    /// De GlobalId als het veld van een object is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub globalid: Option<String>,
    pub attribuut: String,
    pub oud: String,
    pub nieuw: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct LosseKnopen {
    pub toegevoegd: Vec<KnoopInfo>,
    pub verwijderd: Vec<KnoopInfo>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct KnoopInfo {
    #[serde(rename = "type")]
    pub type_naam: String,
    pub tekst: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Statistiek {
    pub a: VersieStatistiek,
    pub b: VersieStatistiek,
    /// Relaties met een nieuwe GlobalId die we aan hun natuurlijke sleutel herkenden.
    pub relaties_gekoppeld: usize,
}

/// Het verschil tussen twee versies van een IFC-bestand.
pub fn verschil(a: &[u8], b: &[u8]) -> Result<Verschil, Fout> {
    let (ma, mb) = std::thread::scope(|s| {
        let a = s.spawn(|| Model::lees(a));
        let b = Model::lees(b);
        (a.join().unwrap_or_else(|_| Err(Fout::intern("lezen mislukt"))), b)
    });
    let (mut ma, mut mb) = (ma?, mb?);
    let relaties_gekoppeld = koppel_relaties(&ma, &mut mb);
    markeer_gemeen(&mut [&mut ma, &mut mb]);
    std::thread::scope(|s| {
        s.spawn(|| ma.bereken());
        mb.bereken();
    });
    for m in [&ma, &mb] {
        if let Some(k) = m.kring {
            return Err(Fout::Kring { regel: m.bestand.regel(m.bestand.instanties[k as usize].bereik.0 as usize) });
        }
    }
    Ok(vergelijk(&ma, &mb, relaties_gekoppeld))
}

pub(crate) fn vergelijk(ma: &Model, mb: &Model, relaties_gekoppeld: usize) -> Verschil {
    let sa = op_sleutel(ma);
    let sb = op_sleutel(mb);
    let mut toegevoegd: Vec<ObjectInfo> = (0..mb.objecten.len())
        .filter(|&o| !sa.contains_key(mb.objecten[o].sleutel.as_str()))
        .map(|o| info(mb, o))
        .collect();
    let mut verwijderd: Vec<ObjectInfo> = (0..ma.objecten.len())
        .filter(|&o| !sb.contains_key(ma.objecten[o].sleutel.as_str()))
        .map(|o| info(ma, o))
        .collect();
    let mut gewijzigd = Vec::new();
    for (oa, object) in ma.objecten.iter().enumerate() {
        let Some(&ob) = sb.get(object.sleutel.as_str()) else {
            continue;
        };
        if mb.objecten[ob].hash != object.hash {
            gewijzigd.push(wijziging(ma, oa, mb, ob));
        }
    }
    toegevoegd.sort();
    verwijderd.sort();
    gewijzigd.sort_by(|x, y| x.globalid.cmp(&y.globalid));
    Verschil {
        toegevoegd,
        verwijderd,
        gewijzigd,
        losse_knopen: losse_knopen(ma, mb),
        header: header_verschil(ma, mb),
        vluchtig: vluchtige_velden(ma, mb, &sb),
        statistiek: Statistiek { a: ma.statistiek.clone(), b: mb.statistiek.clone(), relaties_gekoppeld },
    }
}

pub(crate) fn info(m: &Model, o: usize) -> ObjectInfo {
    let i = m.objecten[o].inst as usize;
    ObjectInfo { globalid: m.objecten[o].globalid.clone(), type_naam: m.type_weergave(i), naam: m.object_naam(i) }
}

fn wijziging(ma: &Model, oa: usize, mb: &Model, ob: usize) -> Wijziging {
    let ia = ma.objecten[oa].inst as usize;
    let ib = mb.objecten[ob].inst as usize;
    let mut attributen = Vec::new();
    if ma.bestand.type_naam(ia) != mb.bestand.type_naam(ib) {
        attributen.push(AttribuutWijziging {
            attribuut: "type".into(),
            oud: ma.type_weergave(ia),
            nieuw: mb.type_weergave(ib),
            erbij: Vec::new(),
            eraf: Vec::new(),
        });
    }
    attribuut_verschillen(ma, ia, mb, ib, 1, "", 0, &mut attributen);
    let globalid_nieuw =
        (ma.objecten[oa].globalid != mb.objecten[ob].globalid).then(|| mb.objecten[ob].globalid.clone());
    Wijziging {
        globalid: ma.objecten[oa].globalid.clone(),
        globalid_nieuw,
        type_naam: mb.type_weergave(ib),
        naam: mb.object_naam(ib).or_else(|| ma.object_naam(ia)),
        attributen,
    }
}

/// Zo diep lopen we in knopen (bijv. IfcTask.TaskTime -> IfcTaskTime) om te laten zien welk veld veranderde.
const MAX_DIEPTE: usize = 3;

/// De gewijzigde attributen van instantie `ia` in a en `ib` in b, vanaf attribuut `vanaf`. Is een attribuut aan
/// beide kanten één verwijzing naar een knoop van hetzelfde type, dan noemen we de gewijzigde velden van die knoop
/// (`TaskTime.ScheduleStart`).
#[allow(clippy::too_many_arguments)]
fn attribuut_verschillen(
    ma: &Model,
    ia: usize,
    mb: &Model,
    ib: usize,
    vanaf: usize,
    pad: &str,
    diepte: usize,
    uit: &mut Vec<AttribuutWijziging>,
) {
    let na = ma.bestand.attributen(ia).len();
    let nb = mb.bestand.attributen(ib).len();
    for j in vanaf..na.max(nb) {
        let ha = (j < na).then(|| ma.attribuut_hash(ia, j));
        let hb = (j < nb).then(|| mb.attribuut_hash(ib, j));
        if ha == hb {
            continue;
        }
        let naam = format!("{pad}{}", if j < na { ma.attribuut_naam(ia, j) } else { mb.attribuut_naam(ib, j) });
        // In een object (diepte 0) telt een ankerknoop op inhoud; dieper is een ankerknoop een anker en lopen we
        // er niet in (dan is het andere anker zelf de wijziging).
        let direct = diepte == 0;
        if j < na && j < nb && diepte < MAX_DIEPTE {
            if let (Some(ka), Some(kb)) =
                (knoop_doel(ma, attribuut_tekst(ma, ia, j), direct), knoop_doel(mb, attribuut_tekst(mb, ib, j), direct))
            {
                if direct {
                    // Met wie de knoop gedeeld is, telt mee (bijv. een gedeelde plaatsing die een kant splitst).
                    let deel_a = ma.deelt_telt(ka).then(|| ma.anker_id[ka]);
                    let deel_b = mb.deelt_telt(kb).then(|| mb.anker_id[kb]);
                    if deel_a != deel_b {
                        let eigen = |m: &Model, i: usize| match m.soort[i] {
                            Soort::Object(o) => Some(o),
                            Soort::Knoop => None,
                        };
                        uit.push(AttribuutWijziging {
                            attribuut: format!("{naam} (gedeeld)"),
                            oud: ma.gedeeld_met(ka, eigen(ma, ia)),
                            nieuw: mb.gedeeld_met(kb, eigen(mb, ib)),
                            erbij: Vec::new(),
                            eraf: Vec::new(),
                        });
                    }
                    if ma.knoop_hash[ka] == mb.knoop_hash[kb] {
                        continue;
                    }
                }
                let zelfde_vorm = ma.bestand.type_naam(ka) == mb.bestand.type_naam(kb)
                    && !ma.bestand.instanties[ka].complex
                    && !mb.bestand.instanties[kb].complex
                    && ma.bestand.attributen(ka).len() == mb.bestand.attributen(kb).len();
                if zelfde_vorm {
                    attribuut_verschillen(ma, ka, mb, kb, 0, &format!("{naam}."), diepte + 1, uit);
                    continue;
                }
            }
        }
        uit.push(attribuut_wijziging(ma, ia, mb, ib, j, naam, direct));
    }
}

/// De knoop als de tekst precies één verwijzing naar een knoop is (niet `direct`: alleen een knoop zonder anker).
fn knoop_doel(m: &Model, tekst: &[u8], direct: bool) -> Option<usize> {
    let n = enkele_verwijzing(tekst)?;
    let k = *m.bestand.index.get(&n)? as usize;
    (m.soort[k] == Soort::Knoop && (direct || m.anker_id[k].is_none())).then_some(k)
}

fn attribuut_wijziging(
    ma: &Model,
    ia: usize,
    mb: &Model,
    ib: usize,
    j: usize,
    attribuut: String,
    direct: bool,
) -> AttribuutWijziging {
    let ta = (j < ma.bestand.attributen(ia).len()).then(|| attribuut_tekst(ma, ia, j));
    let tb = (j < mb.bestand.attributen(ib).len()).then(|| attribuut_tekst(mb, ib, j));
    let (mut erbij, mut eraf) = (Vec::new(), Vec::new());
    if let (Some(ta), Some(tb)) = (ta, tb) {
        if let (Some(la), Some(lb)) = (ma.verwijzingslijst(ta, direct), mb.verwijzingslijst(tb, direct)) {
            let mut tellers: HashMap<&[u8], i64> = HashMap::new();
            for (id, _) in &la {
                *tellers.entry(id).or_default() -= 1;
            }
            for (id, _) in &lb {
                *tellers.entry(id).or_default() += 1;
            }
            let mut rest = tellers.clone();
            for (id, (b, e)) in &lb {
                if let Some(t) = rest.get_mut(id.as_slice()) {
                    if *t > 0 {
                        *t -= 1;
                        erbij.push(mb.toon(&tb[*b..*e], direct, MAX_TEKST));
                    }
                }
            }
            let mut rest = tellers;
            for (id, (b, e)) in &la {
                if let Some(t) = rest.get_mut(id.as_slice()) {
                    if *t < 0 {
                        *t += 1;
                        eraf.push(ma.toon(&ta[*b..*e], direct, MAX_TEKST));
                    }
                }
            }
        }
    }
    AttribuutWijziging {
        attribuut,
        oud: ta.map_or_else(|| "—".into(), |t| ma.toon(t, direct, MAX_TEKST)),
        nieuw: tb.map_or_else(|| "—".into(), |t| mb.toon(t, direct, MAX_TEKST)),
        erbij,
        eraf,
    }
}

/// Gewijzigde velden in FILE_DESCRIPTION en FILE_NAME, zonder de tijdstempel.
fn header_verschil(ma: &Model, mb: &Model) -> Vec<AttribuutWijziging> {
    let mut uit = Vec::new();
    for (regel, velden) in HEADER_VELDEN {
        for (j, veld) in velden.iter().enumerate() {
            if header_vluchtig(regel, j) {
                continue;
            }
            let a = ma.header_parameter(regel, j);
            let b = mb.header_parameter(regel, j);
            if a.map(|x| x.0) == b.map(|x| x.0) {
                continue;
            }
            let toon = |m: &Model, w: Option<(Hash, &[u8])>| {
                w.map_or_else(|| "—".into(), |(_, t)| m.toon(t, false, MAX_TEKST))
            };
            uit.push(AttribuutWijziging {
                attribuut: format!("{regel}.{veld}"),
                oud: toon(ma, a),
                nieuw: toon(mb, b),
                erbij: Vec::new(),
                eraf: Vec::new(),
            });
        }
    }
    uit
}

/// Vluchtige velden die veranderden: van objecten in beide versies, van de knopen waar zo'n object rechtstreeks naar
/// verwijst (bijv. zijn IfcOwnerHistory; elke knoop één keer), en de tijdstempel in de HEADER.
fn vluchtige_velden(ma: &Model, mb: &Model, sb: &HashMap<&str, usize>) -> Vec<VluchtigeWijziging> {
    let mut uit = Vec::new();
    let mut paren: HashSet<(usize, usize)> = HashSet::new();
    let toon = |m: &Model, i: usize, j: usize, direct: bool| m.toon(attribuut_tekst(m, i, j), direct, MAX_TEKST);
    let anders = |ia: usize, ib: usize, j: usize, direct: bool| {
        ma.waarde_hash(attribuut_tekst(ma, ia, j), direct) != mb.waarde_hash(attribuut_tekst(mb, ib, j), direct)
    };
    for (oa, object) in ma.objecten.iter().enumerate() {
        let Some(&ob) = sb.get(object.sleutel.as_str()) else { continue };
        let (ia, ib) = (object.inst as usize, mb.objecten[ob].inst as usize);
        if ma.bestand.type_naam(ia) != mb.bestand.type_naam(ib) {
            continue;
        }
        let n = ma.bestand.attributen(ia).len().min(mb.bestand.attributen(ib).len());
        for j in 1..n {
            if ma.is_vluchtig(ia, j) && mb.is_vluchtig(ib, j) && anders(ia, ib, j, true) {
                uit.push(VluchtigeWijziging {
                    waar: ma.object_weergave(oa),
                    globalid: Some(object.globalid.clone()),
                    attribuut: ma.attribuut_naam(ia, j),
                    oud: toon(ma, ia, j, true),
                    nieuw: toon(mb, ib, j, true),
                });
                continue;
            }
            let (Some(ka), Some(kb)) =
                (knoop_doel(ma, attribuut_tekst(ma, ia, j), true), knoop_doel(mb, attribuut_tekst(mb, ib, j), true))
            else {
                continue;
            };
            let vluchtig = ma.entiteit(ka).is_some_and(|e| e.attributen.iter().any(|a| a.vluchtig));
            if !vluchtig || ma.bestand.type_naam(ka) != mb.bestand.type_naam(kb) || !paren.insert((ka, kb)) {
                continue;
            }
            for jj in 0..ma.bestand.attributen(ka).len() {
                if ma.is_vluchtig(ka, jj) && mb.is_vluchtig(kb, jj) && anders(ka, kb, jj, false) {
                    let anderen = ma.verankerd_door[ka].saturating_sub(1);
                    let gedeeld = if anderen > 0 { format!(" (+{anderen})") } else { String::new() };
                    uit.push(VluchtigeWijziging {
                        waar: format!("{} van {}{gedeeld}", ma.attribuut_naam(ia, j), ma.object_weergave(oa)),
                        globalid: None,
                        attribuut: format!("{}.{}", ma.attribuut_naam(ia, j), ma.attribuut_naam(ka, jj)),
                        oud: toon(ma, ka, jj, false),
                        nieuw: toon(mb, kb, jj, false),
                    });
                }
            }
        }
    }
    for (regel, velden) in HEADER_VELDEN {
        for (j, veld) in velden.iter().enumerate() {
            let (Some(a), Some(b)) = (ma.header_parameter(regel, j), mb.header_parameter(regel, j)) else { continue };
            if header_vluchtig(regel, j) && a.0 != b.0 {
                uit.push(VluchtigeWijziging {
                    waar: "HEADER".into(),
                    globalid: None,
                    attribuut: format!("{regel}.{veld}"),
                    oud: ma.toon(a.1, false, MAX_TEKST),
                    nieuw: mb.toon(b.1, false, MAX_TEKST),
                });
            }
        }
    }
    uit.sort();
    uit
}

/// Losse knopen per sleutel als een gesorteerde lijst van inhoudshashes.
pub(crate) fn losse_groepen(m: &Model) -> HashMap<Hash, Vec<(Hash, u32)>> {
    let mut groepen: HashMap<Hash, Vec<(Hash, u32)>> = HashMap::new();
    for (w, &i) in m.wortels.iter().enumerate() {
        groepen.entry(m.wortel_sleutel[w]).or_default().push((m.knoop_hash[i as usize], i));
    }
    for groep in groepen.values_mut() {
        groep.sort();
    }
    groepen
}

fn losse_knopen(ma: &Model, mb: &Model) -> LosseKnopen {
    let ga = losse_groepen(ma);
    let gb = losse_groepen(mb);
    let mut uit = LosseKnopen::default();
    let leeg = Vec::new();
    for (sleutel, a) in &ga {
        let b = gb.get(sleutel).unwrap_or(&leeg);
        let (weg, bij) = multiset_verschil(a, b);
        uit.verwijderd.extend(weg.into_iter().map(|i| knoop_info(ma, i as usize)));
        uit.toegevoegd.extend(bij.into_iter().map(|i| knoop_info(mb, i as usize)));
    }
    for (sleutel, b) in &gb {
        if !ga.contains_key(sleutel) {
            uit.toegevoegd.extend(b.iter().map(|&(_, i)| knoop_info(mb, i as usize)));
        }
    }
    uit.toegevoegd.sort();
    uit.verwijderd.sort();
    uit
}

/// Wat alleen in a staat en wat alleen in b staat (beide gesorteerd op hash).
pub(crate) fn multiset_verschil(a: &[(Hash, u32)], b: &[(Hash, u32)]) -> (Vec<u32>, Vec<u32>) {
    let (mut i, mut j) = (0, 0);
    let (mut weg, mut bij) = (Vec::new(), Vec::new());
    while i < a.len() || j < b.len() {
        match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) if x.0 == y.0 => {
                i += 1;
                j += 1;
            }
            (Some(x), Some(y)) if x.0 < y.0 => {
                weg.push(x.1);
                i += 1;
            }
            (Some(_), Some(y)) => {
                bij.push(y.1);
                j += 1;
            }
            (Some(x), None) => {
                weg.push(x.1);
                i += 1;
            }
            (None, Some(y)) => {
                bij.push(y.1);
                j += 1;
            }
            (None, None) => break,
        }
    }
    (weg, bij)
}

pub(crate) fn knoop_info(m: &Model, i: usize) -> KnoopInfo {
    KnoopInfo { type_naam: m.type_weergave(i), tekst: m.toon_instantie(i, MAX_TEKST) }
}
