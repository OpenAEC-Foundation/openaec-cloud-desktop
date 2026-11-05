//! Een eigen canonieke vorm van een IFC-bestand, en wat er na samenvoegen in het resultaat hoort te staan. Volgt de
//! regels uit ontwerp §5.6, maar deelt geen code met de bibliotheek.
//!
//! - Een object is zijn type en zijn attributen. Een verwijzing naar een object is zijn GlobalId; de nummers `#n`
//!   tellen niet.
//! - Een knoop waar een object rechtstreeks naar verwijst, schrijven we uit (één niveau), met de andere objecten die
//!   hem ook rechtstreeks gebruiken ("delen telt", behalve bij IfcOwnerHistory). Dieper staat een knoop als type en
//!   inhoudshash.
//! - Een knoop die een anker is van een object (bijv. de plaatsing van verdieping P), telt dieper in de boom als dat
//!   anker: GlobalId, attribuut en plek. In een SET is de plek het type, en de inhoud als er meer leden van dat type
//!   zijn.
//! - Vluchtige velden tellen niet mee; die staan apart.
//! - Losse knopen (waar niets naar verwijst) tellen als verzameling, met per knoop de objecten waar hij bij hoort.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use super::stap::{is_getal, Stap, W};

/// Een anker: GlobalId, attribuut en plek.
pub type AnkerId = (String, usize, String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum C {
    A(String),
    L(Vec<C>),
    /// Een SET of BAG, gesorteerd.
    S(Vec<C>),
    T(String, Vec<C>),
    /// Een object.
    G(String),
    /// Een knoop dieper in de boom: type en inhoudshash.
    N(String, u64),
    /// Een anker van een object.
    Anker(Vec<AnkerId>),
    /// Een knoop waar een object rechtstreeks naar verwijst: type, attributen, en de andere ankers van die knoop.
    K(String, Vec<C>, Vec<AnkerId>),
    /// In een losse knoop: een knoop die bij deze objecten hoort.
    E(Vec<String>, Box<C>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Obj {
    pub ty: String,
    pub attrs: Vec<C>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Vorm {
    pub objecten: BTreeMap<String, Obj>,
    /// De losse knopen, gesorteerd.
    pub los: Vec<C>,
    pub kop: Vec<(String, Vec<C>)>,
    /// Per object: de vluchtige velden van zijn IfcOwnerHistory en zijn eigen vluchtige velden.
    pub vluchtig: BTreeMap<String, Vec<C>>,
}

impl Vorm {
    /// Gelijk, zonder de vluchtige velden.
    pub fn zelfde_inhoud(&self, ander: &Vorm) -> bool {
        self.objecten == ander.objecten && self.los == ander.los && self.kop == ander.kop
    }
}

const VLUCHTIG: &[(&str, &[&str])] = &[
    ("IFCOWNERHISTORY", &["ChangeAction", "LastModifiedDate", "LastModifyingUser", "LastModifyingApplication", "CreationDate"]),
    ("IFCWORKCONTROL", &["CreationDate"]),
];

pub fn is_vluchtig(s: &Stap, n: u64, j: usize) -> bool {
    let Some(e) = s.ent(n) else { return false };
    let Some((naam, _)) = e.attrs.get(j) else { return false };
    VLUCHTIG.iter().any(|(t, velden)| e.voorouders.iter().any(|v| v == t) && velden.contains(&naam.as_str()))
}

/// Een getal of opsomming in een vaste vorm: `1.`, `1.0` en `1.E0` worden gelijk.
fn norm(a: &str) -> String {
    if is_getal(a) {
        if let Ok(i) = a.parse::<i128>() {
            return format!("n{i}");
        }
        let mut t = a.to_ascii_lowercase();
        if let Some(p) = t.find('.') {
            if !t[p + 1..].starts_with(|c: char| c.is_ascii_digit()) {
                t.insert(p + 1, '0');
            }
        }
        let f: f64 = t.parse().unwrap_or_else(|_| panic!("geen getal: {a}"));
        if f.fract() == 0.0 && f.abs() < 1e15 {
            format!("n{}", f as i128)
        } else {
            format!("n{f:e}")
        }
    } else {
        a.to_string()
    }
}

struct Maker<'a> {
    s: &'a Stap,
    gid: HashMap<u64, String>,
    ankers: HashMap<u64, Vec<AnkerId>>,
    eigenaars: HashMap<u64, Vec<String>>,
    memo: RefCell<HashMap<(u64, bool), (String, u64)>>,
    bezig: RefCell<HashSet<(u64, bool)>>,
}

impl<'a> Maker<'a> {
    fn waarde(&self, w: &W, set: bool, puur: bool) -> C {
        match w {
            W::Atoom(a) => C::A(norm(a)),
            W::Ref(n) => self.verwijzing(*n, puur),
            W::Lijst(l) => {
                let mut v: Vec<C> = l.iter().map(|e| self.waarde(e, false, puur)).collect();
                if set {
                    v.sort();
                    C::S(v)
                } else {
                    C::L(v)
                }
            }
            W::Getypt(t, a) => {
                let set_type = self.s.schema.set_typen.contains(t);
                C::T(t.clone(), a.iter().map(|e| self.waarde(e, set_type, puur)).collect())
            }
        }
    }

    fn verwijzing(&self, n: u64, puur: bool) -> C {
        if let Some(g) = self.gid.get(&n) {
            return C::G(g.clone());
        }
        if !self.s.inst.contains_key(&n) {
            return C::A(format!("?#{n}"));
        }
        if !puur {
            if let Some(a) = self.ankers.get(&n) {
                return C::Anker(a.clone());
            }
        }
        let (ty, h) = self.knoop_hash(n, puur);
        C::N(ty, h)
    }

    fn knoop_attrs(&self, n: u64, puur: bool) -> (String, Vec<C>) {
        let i = &self.s.inst[&n];
        let attrs = i
            .attrs
            .iter()
            .enumerate()
            .map(|(j, a)| {
                if is_vluchtig(self.s, n, j) {
                    C::A("~".into())
                } else {
                    self.waarde(a, self.s.is_set(n, j), puur)
                }
            })
            .collect();
        (i.ty.clone(), attrs)
    }

    fn knoop_hash(&self, n: u64, puur: bool) -> (String, u64) {
        if let Some(h) = self.memo.borrow().get(&(n, puur)) {
            return h.clone();
        }
        if !self.bezig.borrow_mut().insert((n, puur)) {
            return ("KRING".into(), 0);
        }
        let (ty, attrs) = self.knoop_attrs(n, puur);
        let mut h = DefaultHasher::new();
        (&ty, &attrs).hash(&mut h);
        let uit = (ty, h.finish());
        self.bezig.borrow_mut().remove(&(n, puur));
        self.memo.borrow_mut().insert((n, puur), uit.clone());
        uit
    }

    /// Een knoop waar object `eigen.0` rechtstreeks naar verwijst.
    fn direct(&self, k: u64, eigen: &AnkerId) -> C {
        let (ty, attrs) = self.knoop_attrs(k, false);
        let gedeeld = if ty == "IFCOWNERHISTORY" {
            Vec::new()
        } else {
            self.ankers.get(&k).map(|a| a.iter().filter(|x| *x != eigen).cloned().collect()).unwrap_or_default()
        };
        C::K(ty, attrs, gedeeld)
    }

    fn object(&self, n: u64, g: &str) -> Obj {
        let i = &self.s.inst[&n];
        let mut attrs = Vec::new();
        for (j, a) in i.attrs.iter().enumerate() {
            if is_vluchtig(self.s, n, j) {
                attrs.push(C::A("~".into()));
                continue;
            }
            let set = self.s.is_set(n, j);
            let c = match a {
                W::Ref(k) if self.s.is_knoop(*k) => self.direct(*k, &(g.to_string(), j, String::new())),
                W::Lijst(l) => {
                    let posities = self.posities(l, set);
                    let mut v: Vec<C> = l
                        .iter()
                        .zip(posities)
                        .map(|(e, p)| match (e, p) {
                            (W::Ref(k), Some(p)) => self.direct(*k, &(g.to_string(), j, p)),
                            (e, _) => self.waarde(e, false, false),
                        })
                        .collect();
                    if set {
                        v.sort();
                        C::S(v)
                    } else {
                        C::L(v)
                    }
                }
                a => self.waarde(a, set, false),
            };
            attrs.push(c);
        }
        Obj { ty: i.ty.clone(), attrs }
    }

    /// De plek van elke knoop in een lijst van een object: het nummer in een LIST, het type (en zo nodig de inhoud)
    /// in een SET of BAG. `None` voor wat geen knoop is.
    fn posities(&self, l: &[W], set: bool) -> Vec<Option<String>> {
        let typen: Vec<Option<String>> = l
            .iter()
            .map(|e| match e {
                W::Ref(k) if self.s.is_knoop(*k) => Some(self.s.inst[k].ty.clone()),
                _ => None,
            })
            .collect();
        l.iter()
            .enumerate()
            .map(|(e, w)| {
                let ty = typen[e].clone()?;
                if !set {
                    return Some(e.to_string());
                }
                let aantal = typen.iter().filter(|t| t.as_deref() == Some(ty.as_str())).count();
                let W::Ref(k) = w else { unreachable!() };
                Some(if aantal == 1 { ty } else { format!("{ty}:{}", self.knoop_hash(*k, true).1) })
            })
            .collect()
    }

    fn los(&self, n: u64, diepte: usize) -> C {
        let i = &self.s.inst[&n];
        let attrs = i.attrs.iter().enumerate().map(|(j, a)| self.los_waarde(a, self.s.is_set(n, j), diepte)).collect();
        C::K(i.ty.clone(), attrs, Vec::new())
    }

    fn los_waarde(&self, w: &W, set: bool, diepte: usize) -> C {
        match w {
            W::Ref(k) if self.s.is_knoop(*k) && !self.ankers.contains_key(k) && diepte < 64 => {
                match self.eigenaars.get(k) {
                    Some(e) => C::E(e.clone(), Box::new(self.verwijzing(*k, false))),
                    None => self.los(*k, diepte + 1),
                }
            }
            W::Lijst(l) => {
                let mut v: Vec<C> = l.iter().map(|e| self.los_waarde(e, false, diepte)).collect();
                if set {
                    v.sort();
                    C::S(v)
                } else {
                    C::L(v)
                }
            }
            w => self.waarde(w, set, false),
        }
    }
}

/// De vorm van `s`. Alleen de objecten in `universum` tellen als anker of als deler; zo verandert een object dat
/// in een van de versies niet bestaat, de andere objecten niet.
pub fn vorm(s: &Stap, universum: &BTreeSet<String>) -> Vorm {
    let objecten = s.objecten();
    let mut m = Maker {
        s,
        gid: objecten.iter().map(|(g, &n)| (n, g.clone())).collect(),
        ankers: HashMap::new(),
        eigenaars: HashMap::new(),
        memo: RefCell::new(HashMap::new()),
        bezig: RefCell::new(HashSet::new()),
    };
    let mut ankers: HashMap<u64, Vec<AnkerId>> = HashMap::new();
    for (g, &n) in objecten.iter().filter(|(g, _)| universum.contains(*g)) {
        for (j, a) in s.inst[&n].attrs.iter().enumerate() {
            match a {
                W::Ref(k) if s.is_knoop(*k) => ankers.entry(*k).or_default().push((g.clone(), j, String::new())),
                W::Lijst(l) => {
                    for (e, p) in l.iter().zip(m.posities(l, s.is_set(n, j))) {
                        if let (W::Ref(k), Some(p)) = (e, p) {
                            ankers.entry(*k).or_default().push((g.clone(), j, p));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for a in ankers.values_mut() {
        a.sort();
        a.dedup();
    }
    m.ankers = ankers;

    // De eigenaars: welke objecten een knoop bereiken, zonder door een anker van een ander object te gaan.
    let mut eigenaars: HashMap<u64, BTreeSet<String>> = HashMap::new();
    for (g, &n) in objecten.iter().filter(|(g, _)| universum.contains(*g)) {
        let mut stapel: Vec<u64> = Vec::new();
        s.inst[&n].refs(&mut |k| stapel.push(k));
        let mut gezien = HashSet::new();
        while let Some(k) = stapel.pop() {
            if !s.is_knoop(k) || !gezien.insert(k) {
                continue;
            }
            if m.ankers.get(&k).is_some_and(|a| a.iter().any(|x| &x.0 != g)) {
                continue;
            }
            eigenaars.entry(k).or_default().insert(g.clone());
            s.inst[&k].refs(&mut |c| stapel.push(c));
        }
    }
    m.eigenaars = eigenaars.into_iter().map(|(k, v)| (k, v.into_iter().collect())).collect();

    let mut verwezen = HashSet::new();
    for i in s.inst.values() {
        i.refs(&mut |k| {
            verwezen.insert(k);
        });
    }
    let mut los: Vec<C> =
        s.inst.keys().filter(|&&n| s.is_knoop(n) && !verwezen.contains(&n)).map(|&n| m.los(n, 0)).collect();
    los.sort();

    let kop = s
        .kop
        .iter()
        .map(|(naam, a)| {
            let attrs = a
                .iter()
                .enumerate()
                .map(|(j, w)| if naam == "FILE_NAME" && j == 1 { C::A("~".into()) } else { m.waarde(w, false, false) })
                .collect();
            (naam.clone(), attrs)
        })
        .collect();

    let mut vluchtig = BTreeMap::new();
    let mut uit = BTreeMap::new();
    for (g, &n) in &objecten {
        uit.insert(g.clone(), m.object(n, g));
        let i = &s.inst[&n];
        let mut v = Vec::new();
        if let Some(W::Ref(oh)) = i.attrs.get(1) {
            if s.inst.get(oh).is_some_and(|x| x.ty == "IFCOWNERHISTORY") {
                for (j, a) in s.inst[oh].attrs.iter().enumerate() {
                    if is_vluchtig(s, *oh, j) {
                        v.push(m.waarde(a, false, false));
                    }
                }
            }
        }
        for (j, a) in i.attrs.iter().enumerate() {
            if is_vluchtig(s, n, j) {
                v.push(m.waarde(a, false, false));
            }
        }
        vluchtig.insert(g.clone(), v);
    }
    Vorm { objecten: uit, los, kop, vluchtig }
}

// --- Wat er na samenvoegen hoort te staan. ---

/// Drieweg voor één waarde: de wijziging van één kant, of dezelfde wijziging van beide. `None` als beide kanten
/// het anders wijzigden.
fn drieweg<T: PartialEq + Clone>(b: &T, x: &T, y: &T) -> Option<T> {
    if x == b {
        Some(y.clone())
    } else if y == b || x == y {
        Some(x.clone())
    } else {
        None
    }
}

/// Drieweg voor een SET of BAG: wat erbij kwam en wat eraf ging, van beide kanten (per element geteld).
fn drieweg_set(b: &[C], x: &[C], y: &[C]) -> Vec<C> {
    let tel = |l: &[C]| {
        let mut m: BTreeMap<C, i64> = BTreeMap::new();
        for c in l {
            *m.entry(c.clone()).or_default() += 1;
        }
        m
    };
    let (tb, tx, ty) = (tel(b), tel(x), tel(y));
    let alle: BTreeSet<&C> = tb.keys().chain(tx.keys()).chain(ty.keys()).collect();
    let mut uit = Vec::new();
    for c in alle {
        let (cb, cx, cy) = (tb.get(c).copied().unwrap_or(0), tx.get(c).copied().unwrap_or(0), ty.get(c).copied().unwrap_or(0));
        let n = drieweg(&cb, &cx, &cy).unwrap_or((cx + cy - cb).max(0));
        uit.extend(std::iter::repeat_n(c.clone(), n as usize));
    }
    uit
}

/// De naam van een eigenschap (het eerste attribuut van een rechtstreekse knoop).
fn naam_van(c: &C) -> Option<&C> {
    match c {
        C::K(_, a, _) => a.first(),
        _ => None,
    }
}

/// Een eigenschappenset per eigenschap (op naam); `Err` als beide kanten dezelfde eigenschap anders wijzigden.
fn drieweg_eigenschappen(b: &[C], x: &[C], y: &[C]) -> Result<Vec<C>, String> {
    let per_naam = |l: &[C]| -> Option<BTreeMap<C, C>> {
        let mut m = BTreeMap::new();
        for c in l {
            if m.insert(naam_van(c)?.clone(), c.clone()).is_some() {
                return None;
            }
        }
        Some(m)
    };
    let (Some(mb), Some(mx), Some(my)) = (per_naam(b), per_naam(x), per_naam(y)) else {
        return Ok(drieweg_set(b, x, y));
    };
    let namen: BTreeSet<&C> = mb.keys().chain(mx.keys()).chain(my.keys()).collect();
    let mut uit = Vec::new();
    for n in namen {
        match drieweg(&mb.get(n), &mx.get(n), &my.get(n)) {
            Some(Some(c)) => uit.push(c.clone()),
            Some(None) => {}
            None => return Err(format!("eigenschap {n:?} anders gewijzigd")),
        }
    }
    uit.sort();
    Ok(uit)
}

/// Het verwachte object, `Ok(None)` als het weg hoort, of `Err` met de reden als er een conflict hoort.
fn verwacht_object(b: Option<&Obj>, x: Option<&Obj>, y: Option<&Obj>) -> Result<Option<Obj>, String> {
    match (b, x, y) {
        (_, None, None) => Ok(None),
        (None, Some(x), None) | (None, None, Some(x)) => Ok(Some(x.clone())),
        (None, Some(x), Some(y)) => {
            if x == y {
                Ok(Some(x.clone()))
            } else {
                Err("beide kanten voegden dit object anders toe".into())
            }
        }
        (Some(b), None, Some(y)) | (Some(b), Some(y), None) => {
            if b == y {
                Ok(None)
            } else {
                Err("de ene kant verwijderde, de andere wijzigde".into())
            }
        }
        (Some(b), Some(x), Some(y)) => {
            if b.ty != x.ty || b.ty != y.ty || b.attrs.len() != x.attrs.len() || b.attrs.len() != y.attrs.len() {
                return drieweg(b, x, y).map(Some).ok_or_else(|| "beide kanten wijzigden het type".into());
            }
            let mut attrs = Vec::new();
            for j in 0..b.attrs.len() {
                let (vb, vx, vy) = (&b.attrs[j], &x.attrs[j], &y.attrs[j]);
                if let Some(v) = drieweg(vb, vx, vy) {
                    attrs.push(v);
                    continue;
                }
                let v = match (vb, vx, vy) {
                    (C::S(sb), C::S(sx), C::S(sy)) => {
                        if matches!(b.ty.as_str(), "IFCPROPERTYSET" | "IFCELEMENTQUANTITY") {
                            C::S(drieweg_eigenschappen(sb, sx, sy).map_err(|e| format!("attribuut {j}: {e}"))?)
                        } else {
                            C::S(drieweg_set(sb, sx, sy))
                        }
                    }
                    _ => return Err(format!("beide kanten wijzigden attribuut {j} anders")),
                };
                attrs.push(v);
            }
            Ok(Some(Obj { ty: b.ty.clone(), attrs }))
        }
    }
}

/// Wat er mis is met resultaat `r` van een gelukte samenvoeging van `x` (ons) en `y` (hun) op basis `b`. Leeg als
/// elke wijziging van beide kanten in het resultaat staat.
pub fn controleer(b: &Vorm, x: &Vorm, y: &Vorm, r: &Vorm) -> Vec<String> {
    let mut fouten = Vec::new();
    let gids: BTreeSet<&String> =
        b.objecten.keys().chain(x.objecten.keys()).chain(y.objecten.keys()).chain(r.objecten.keys()).collect();
    for g in gids {
        let (ob, ox, oy, or) = (b.objecten.get(g), x.objecten.get(g), y.objecten.get(g), r.objecten.get(g));
        match verwacht_object(ob, ox, oy) {
            Err(reden) => fouten.push(format!("{g}: geen conflict, terwijl {reden}; resultaat {or:?}")),
            Ok(verwacht) => {
                if verwacht.as_ref() != or {
                    fouten.push(beschrijf(g, ob, ox, oy, verwacht.as_ref(), or));
                }
            }
        }
    }
    if let Some(verwacht) = drieweg(&b.los, &x.los, &y.los).or_else(|| Some(drieweg_set(&b.los, &x.los, &y.los))) {
        let mut verwacht = verwacht;
        verwacht.sort();
        if verwacht != r.los {
            fouten.push(format!("losse knopen: verwacht {verwacht:?}, resultaat {:?}", r.los));
        }
    }
    for (k, (naam, rb)) in b.kop.iter().enumerate() {
        let (Some(rx), Some(ry), Some(rr)) = (x.kop.get(k), y.kop.get(k), r.kop.get(k)) else {
            fouten.push(format!("HEADER {naam} ontbreekt"));
            continue;
        };
        match drieweg(rb, &rx.1, &ry.1) {
            Some(v) if v == rr.1 => {}
            Some(v) => fouten.push(format!("HEADER {naam}: verwacht {v:?}, resultaat {:?}", rr.1)),
            None => fouten.push(format!("HEADER {naam}: geen conflict, terwijl beide kanten hem anders wijzigden")),
        }
    }
    // Vluchtige velden: alleen hun wijzigde, dan die van hun; anders die van ons.
    for (g, vr) in &r.vluchtig {
        let (Some(vb), Some(vx), Some(vy)) = (b.vluchtig.get(g), x.vluchtig.get(g), y.vluchtig.get(g)) else {
            continue;
        };
        if vb.len() != vx.len() || vb.len() != vy.len() || vb.len() != vr.len() {
            continue;
        }
        for k in 0..vb.len() {
            let verwacht = if vx[k] == vb[k] { &vy[k] } else { &vx[k] };
            if &vr[k] != verwacht {
                fouten.push(format!("{g}: vluchtig veld {k}: verwacht {verwacht:?}, resultaat {:?}", vr[k]));
            }
        }
    }
    fouten
}

fn beschrijf(g: &str, b: Option<&Obj>, x: Option<&Obj>, y: Option<&Obj>, v: Option<&Obj>, r: Option<&Obj>) -> String {
    let mut s = format!("{g}: ");
    match (v, r) {
        (Some(v), Some(r)) if v.ty == r.ty && v.attrs.len() == r.attrs.len() => {
            s.push_str(&format!("type {}\n", v.ty));
            for j in 0..v.attrs.len() {
                if v.attrs[j] != r.attrs[j] {
                    s.push_str(&format!(
                        "  attribuut {j}:\n    basis     {:?}\n    ons       {:?}\n    hun       {:?}\n    verwacht  {:?}\n    resultaat {:?}\n",
                        b.map(|o| &o.attrs[j]),
                        x.map(|o| &o.attrs[j]),
                        y.map(|o| &o.attrs[j]),
                        v.attrs[j],
                        r.attrs[j]
                    ));
                }
            }
        }
        _ => s.push_str(&format!("verwacht {v:?}\n  resultaat {r:?}")),
    }
    s
}
