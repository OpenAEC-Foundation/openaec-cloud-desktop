//! Bewerkingen zoals modelleurs en planners ze doen, op een gelezen bestand ([`Stap`]). Elke bewerking kiest haar
//! doel met een getal uit een vaste volgorde (op GlobalId of nummer), zodat ons en hun met hetzelfde getal hetzelfde
//! doel raken. Heeft een bestand geen geschikt doel, dan doet de bewerking niets.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use super::gemeen::Toeval;
use super::stap::{Inst, Stap, W};

#[derive(Clone, Debug)]
pub enum Bewerking {
    /// Een object een andere naam geven.
    Hernoem(u32, u8),
    /// De omschrijving van een object wijzigen.
    Omschrijving(u32, u8),
    /// Een getal of tekst in een knoop van één object wijzigen (bijv. een coördinaat of een taakdatum).
    Knoopwaarde(u32, u32, u8),
    /// De waarde van een eigenschap in een eigenschappenset wijzigen.
    Eigenschap(u32, u8),
    /// Een eigenschap aan een eigenschappenset toevoegen.
    NieuweEigenschap(u32, u8),
    /// Een element of taak kopiëren (een nieuw object, met een eigen plaatsing en geometrie).
    Kopie(u32),
    /// Een object wissen, met de relaties en knopen die alleen bij hem hoorden.
    Wis(u32),
    /// Een element naar een andere verdieping of ruimte, met zijn plaatsing.
    Verdieping(u32, u32),
    /// Een gedeelde plaatsing splitsen: het object krijgt een eigen kopie.
    Splits(u32),
    /// Een nieuwe eigenschappenset met een relatie aan een object hangen.
    Relatie(u32, u8),
    /// De volgorde in een SET of BAG omgooien.
    SetVolgorde(u32, u64),
    /// Een vluchtig veld wijzigen (een tijdstempel in IfcOwnerHistory, of CreationDate van een planning).
    Tijd(u32, u8),
}

/// Een bewerking op de basis: object A gebruikt voortaan de plaatsing van object B (een gedeelde plaatsing).
pub fn deel_plaatsing(s: &mut Stap, a: u32, b: u32) {
    let k = met_plaatsing(s);
    if k.len() < 2 {
        return;
    }
    let (na, nb) = (k[a as usize % k.len()], k[b as usize % k.len()]);
    let j = s.plek(na, "ObjectPlacement").unwrap();
    let pb = s.inst[&nb].attrs[s.plek(nb, "ObjectPlacement").unwrap()].clone();
    if na == nb || s.inst[&na].attrs[j] == pb {
        return;
    }
    let eerst = verwezen(s);
    let mut t = s.clone();
    t.inst.get_mut(&na).unwrap().attrs[j] = pb;
    if ruim_op(&mut t, &eerst, &HashSet::new()) {
        *s = t;
    }
}

/// Past `b` toe. `kant` en `teller` maken nieuwe GlobalIds uniek per kant.
pub fn pas_toe(s: &mut Stap, b: &Bewerking, kant: char, teller: &mut u32) {
    *teller += 1;
    let sleutel = format!("{kant}{teller}{b:?}");
    match *b {
        Bewerking::Hernoem(k, w) => wijzig_tekst(s, k, "Name", w),
        Bewerking::Omschrijving(k, w) => wijzig_tekst(s, k, "Description", w),
        Bewerking::Knoopwaarde(k, l, w) => knoopwaarde(s, k, l, w),
        Bewerking::Eigenschap(k, w) => eigenschap(s, k, w),
        Bewerking::NieuweEigenschap(k, w) => nieuwe_eigenschap(s, k, w),
        Bewerking::Kopie(k) => kopie(s, k, &sleutel),
        Bewerking::Wis(k) => wis(s, k),
        Bewerking::Verdieping(k, d) => verdieping(s, k, d, &sleutel),
        Bewerking::Splits(k) => splits(s, k),
        Bewerking::Relatie(k, w) => relatie(s, k, w, &sleutel),
        Bewerking::SetVolgorde(k, zaad) => set_volgorde(s, k, zaad),
        Bewerking::Tijd(k, w) => tijd(s, k, w),
    }
}

/// Een GlobalId van 22 tekens uit een sleutel.
fn nieuwe_gid(sleutel: &str) -> String {
    const TEKENS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    let mut uit = String::from("1");
    let mut n = 0u64;
    while uit.len() < 22 {
        let mut h = DefaultHasher::new();
        (sleutel, n).hash(&mut h);
        let mut v = h.finish();
        for _ in 0..10 {
            if uit.len() < 22 {
                uit.push(TEKENS[(v & 63) as usize] as char);
                v >>= 6;
            }
        }
        n += 1;
    }
    uit
}

fn kies<T: Copy>(lijst: &[T], k: u32) -> Option<T> {
    (!lijst.is_empty()).then(|| lijst[k as usize % lijst.len()])
}

fn tekst_met(oud: &W, w: u8) -> W {
    match oud {
        W::Atoom(a) if a.starts_with('\'') => W::Atoom(format!("{}-{w}'", &a[..a.len() - 1])),
        _ => W::Atoom(format!("'nieuw-{w}'")),
    }
}

/// Een getal plus `w + 1`, in de vorm van een STEP-getal.
fn getal_met(a: &str, w: u8) -> String {
    if let Ok(i) = a.parse::<i64>() {
        return (i + i64::from(w) + 1).to_string();
    }
    let mut t = a.to_ascii_lowercase();
    if let Some(p) = t.find('.') {
        if !t[p + 1..].starts_with(|c: char| c.is_ascii_digit()) {
            t.insert(p + 1, '0');
        }
    }
    let v: f64 = t.parse::<f64>().unwrap() + f64::from(w) + 1.0;
    let s = format!("{v:E}");
    let (m, e) = s.split_once('E').unwrap();
    let m = if m.contains('.') { m.to_string() } else { format!("{m}.") };
    format!("{m}E{e}")
}

fn wijzig_tekst(s: &mut Stap, k: u32, attribuut: &str, w: u8) {
    let objecten: Vec<u64> = s.objecten().values().copied().filter(|&n| s.plek(n, attribuut).is_some()).collect();
    let Some(n) = kies(&objecten, k) else { return };
    let j = s.plek(n, attribuut).unwrap();
    let i = s.inst.get_mut(&n).unwrap();
    i.attrs[j] = tekst_met(&i.attrs[j], w);
}

/// Knopen die alleen bij object `n` horen: elke knoop op de weg heeft precies één verwijzer. We gaan niet door een
/// IfcOwnerHistory of een anker van een ander object.
fn eigen_knopen(s: &Stap, n: u64, verwijzers: &HashMap<u64, Vec<(u64, usize)>>) -> Vec<u64> {
    let mut uit = Vec::new();
    let mut stapel = Vec::new();
    s.inst[&n].refs(&mut |k| stapel.push(k));
    let mut gezien = HashSet::new();
    while let Some(k) = stapel.pop() {
        if !s.is_knoop(k) || !gezien.insert(k) || s.inst[&k].ty == "IFCOWNERHISTORY" {
            continue;
        }
        if verwijzers.get(&k).map_or(0, |v| v.len()) != 1 {
            continue;
        }
        uit.push(k);
        s.inst[&k].refs(&mut |c| stapel.push(c));
    }
    uit.sort_unstable();
    uit
}

fn knoopwaarde(s: &mut Stap, k: u32, l: u32, w: u8) {
    let verwijzers = s.verwijzers();
    // (knoop, attribuut, element in een lijst)
    let mut per_object = Vec::new();
    for &n in s.objecten().values() {
        let mut plekken = Vec::new();
        for knoop in eigen_knopen(s, n, &verwijzers) {
            for (j, a) in s.inst[&knoop].attrs.iter().enumerate() {
                match a {
                    a if a.is_getal() || a.is_tekst() => plekken.push((knoop, j, None)),
                    W::Lijst(lijst) => {
                        for (e, x) in lijst.iter().enumerate() {
                            if x.is_getal() || x.is_tekst() {
                                plekken.push((knoop, j, Some(e)));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if !plekken.is_empty() {
            per_object.push(plekken);
        }
    }
    let Some(i) = kies(&(0..per_object.len()).collect::<Vec<_>>(), k) else { return };
    let Some((knoop, j, e)) = kies(&per_object[i], l) else { return };
    let a = &mut s.inst.get_mut(&knoop).unwrap().attrs[j];
    let doel = match (a, e) {
        (W::Lijst(lijst), Some(e)) => &mut lijst[e],
        (a, _) => a,
    };
    let nieuw = match &*doel {
        W::Atoom(x) if x.starts_with('\'') => tekst_met(doel, w),
        W::Atoom(x) => W::Atoom(getal_met(x, w)),
        _ => unreachable!(),
    };
    *doel = nieuw;
}

/// De IfcPropertySingleValue-knopen met een getypte waarde: (eigenschappenset, eigenschap), op GlobalId en naam.
fn eigenschappen(s: &Stap) -> Vec<(u64, u64)> {
    let mut uit = Vec::new();
    for &n in s.objecten().values() {
        if s.inst[&n].ty != "IFCPROPERTYSET" {
            continue;
        }
        let j = s.plek(n, "HasProperties").unwrap();
        let W::Lijst(l) = &s.inst[&n].attrs[j] else { continue };
        let mut leden: Vec<(String, u64)> = l
            .iter()
            .filter_map(|e| match e {
                W::Ref(p) if s.inst.get(p).is_some_and(|i| i.ty == "IFCPROPERTYSINGLEVALUE") => {
                    let i = &s.inst[p];
                    matches!(i.attrs.get(2), Some(W::Getypt(_, v)) if v.len() == 1 && matches!(v[0], W::Atoom(_)))
                        .then(|| (format!("{:?}", i.attrs[0]), *p))
                }
                _ => None,
            })
            .collect();
        leden.sort();
        uit.extend(leden.into_iter().map(|(_, p)| (n, p)));
    }
    uit
}

fn eigenschap(s: &mut Stap, k: u32, w: u8) {
    let Some((set, p)) = kies(&eigenschappen(s), k) else { return };
    let mut i = s.inst[&p].clone();
    let W::Getypt(_, v) = &mut i.attrs[2] else { unreachable!() };
    v[0] = match &v[0] {
        W::Atoom(a) if a == ".T." => W::Atoom(".F.".into()),
        W::Atoom(a) if a == ".F." => W::Atoom(".T.".into()),
        W::Atoom(a) if a.starts_with('\'') => tekst_met(&v[0], w),
        W::Atoom(a) if super::stap::is_getal(a) => W::Atoom(getal_met(a, w)),
        _ => return,
    };
    // Een eigenschap die meer sets delen, kopiëren we voor deze set (zoals een programma dat doet).
    if s.verwijzers().get(&p).map_or(0, |v| v.len()) > 1 {
        let nieuw = s.nieuw_nummer();
        s.inst.insert(nieuw, i);
        let j = s.plek(set, "HasProperties").unwrap();
        s.inst.get_mut(&set).unwrap().attrs[j].refs_mut(&mut |r| {
            if *r == p {
                *r = nieuw;
            }
        });
    } else {
        s.inst.insert(p, i);
    }
}

fn nieuwe_eigenschap(s: &mut Stap, k: u32, w: u8) {
    let sets: Vec<u64> = s.objecten().values().copied().filter(|n| s.inst[n].ty == "IFCPROPERTYSET").collect();
    let Some(set) = kies(&sets, k) else { return };
    let naam = format!("'Proef{}'", w % 2);
    let j = s.plek(set, "HasProperties").unwrap();
    let W::Lijst(l) = &s.inst[&set].attrs[j] else { return };
    if l.iter().any(|e| matches!(e, W::Ref(p) if s.inst.get(p).is_some_and(|i| i.attrs.first() == Some(&W::Atoom(naam.clone()))))) {
        return;
    }
    let nieuw = s.nieuw_nummer();
    s.inst.insert(nieuw, Inst {
        ty: "IFCPROPERTYSINGLEVALUE".into(),
        attrs: vec![
            W::Atoom(naam),
            W::Atoom("$".into()),
            W::Getypt("IFCLABEL".into(), vec![W::Atoom(format!("'w{w}'"))]),
            W::Atoom("$".into()),
        ],
    });
    if let W::Lijst(l) = &mut s.inst.get_mut(&set).unwrap().attrs[j] {
        l.push(W::Ref(nieuw));
    }
}

/// Kopieert de knopen onder `k` diep, behalve objecten, IfcOwnerHistory en ankers van andere objecten dan `eigen`.
fn kopieer(s: &mut Stap, k: u64, eigen: &str, direct: &HashMap<u64, BTreeSet<String>>, memo: &mut HashMap<u64, u64>) -> u64 {
    if !s.is_knoop(k) || s.inst[&k].ty == "IFCOWNERHISTORY" {
        return k;
    }
    if direct.get(&k).is_some_and(|d| d.iter().any(|g| g != eigen)) {
        return k;
    }
    if let Some(&n) = memo.get(&k) {
        return n;
    }
    let mut i = s.inst[&k].clone();
    let nieuw = s.nieuw_nummer();
    s.inst.insert(nieuw, i.clone());
    memo.insert(k, nieuw);
    let mut kinderen = Vec::new();
    i.refs(&mut |c| kinderen.push(c));
    let kopie: HashMap<u64, u64> = kinderen.into_iter().map(|c| (c, kopieer(s, c, eigen, direct, memo))).collect();
    i.attrs.iter_mut().for_each(|a| a.refs_mut(&mut |r| *r = kopie[r]));
    s.inst.insert(nieuw, i);
    nieuw
}

/// Elementen in een IfcRelContainedInSpatialStructure, en taken: (object, relaties waar hij in een lijst staat).
fn kopie(s: &mut Stap, k: u32, sleutel: &str) {
    let verwijzers = s.verwijzers();
    let mut kandidaten = Vec::new();
    for (g, &n) in &s.objecten() {
        let ty = &s.inst[&n].ty;
        let relaties: Vec<(u64, usize)> = verwijzers
            .get(&n)
            .into_iter()
            .flatten()
            .copied()
            .filter(|(r, j)| {
                let rt = &s.inst[r].ty;
                matches!(s.inst[r].attrs[*j], W::Lijst(_))
                    && (rt == "IFCRELCONTAINEDINSPATIALSTRUCTURE" || rt == "IFCRELNESTS" || rt == "IFCRELASSIGNSTOCONTROL")
            })
            .collect();
        let bevat = relaties.iter().any(|(r, _)| s.inst[r].ty == "IFCRELCONTAINEDINSPATIALSTRUCTURE");
        if (bevat && s.plek(n, "ObjectPlacement").is_some()) || (ty == "IFCTASK" && !relaties.is_empty()) {
            kandidaten.push((g.clone(), n, relaties));
        }
    }
    let Some(i) = kies(&(0..kandidaten.len()).collect::<Vec<_>>(), k) else { return };
    let (g, n, relaties) = kandidaten.swap_remove(i);
    let direct = s.directe_objecten();
    let mut memo = HashMap::new();
    let mut nieuw = s.inst[&n].clone();
    nieuw.attrs[0] = W::Atoom(format!("'{}'", nieuwe_gid(sleutel)));
    if let Some(j) = s.plek(n, "Name") {
        nieuw.attrs[j] = W::Atoom("'Kopie'".into());
    }
    for j in 2..nieuw.attrs.len() {
        let mut a = nieuw.attrs[j].clone();
        let mut kinderen = Vec::new();
        a.refs(&mut |c| kinderen.push(c));
        let kopie: HashMap<u64, u64> = kinderen.into_iter().map(|c| (c, kopieer(s, c, &g, &direct, &mut memo))).collect();
        a.refs_mut(&mut |r| *r = kopie[r]);
        nieuw.attrs[j] = a;
    }
    let nummer = s.nieuw_nummer();
    s.inst.insert(nummer, nieuw);
    for (r, j) in relaties {
        if let W::Lijst(l) = &mut s.inst.get_mut(&r).unwrap().attrs[j] {
            l.push(W::Ref(nummer));
        }
    }
}

/// Wist `start`, met wat alleen door hem bestond. Lukt het niet netjes (iets anders dan een relatie verwijst ernaar,
/// of het is de ouder in een boomrelatie), dan blijft `s` zoals het was.
fn wis(s: &mut Stap, k: u32) {
    let kandidaten: Vec<u64> = s
        .objecten()
        .values()
        .copied()
        .filter(|n| {
            let ty = &s.inst[n].ty;
            !ty.starts_with("IFCREL") && ty != "IFCPROJECT"
        })
        .collect();
    let Some(start) = kies(&kandidaten, k) else { return };
    let eerst = verwezen(s);
    let mut t = s.clone();
    let mut weg = HashSet::new();
    let mut werk = vec![start];
    while let Some(o) = werk.pop() {
        if !weg.insert(o) {
            continue;
        }
        let verwijzers = t.verwijzers();
        for &(r, j) in verwijzers.get(&o).into_iter().flatten() {
            if weg.contains(&r) {
                continue;
            }
            let rt = t.inst[&r].ty.clone();
            if !rt.starts_with("IFCREL") {
                return;
            }
            let a = &mut t.inst.get_mut(&r).unwrap().attrs[j];
            match a {
                W::Lijst(l) if l.len() >= 2 => l.retain(|e| *e != W::Ref(o)),
                W::Ref(_) if matches!(
                    rt.as_str(),
                    "IFCRELAGGREGATES" | "IFCRELNESTS" | "IFCRELCONTAINEDINSPATIALSTRUCTURE"
                ) =>
                {
                    return
                }
                _ => werk.push(r),
            }
        }
        // Een relatie die weggaat, neemt een eigenschappenset mee waar verder niets naar verwijst.
        if t.inst[&o].ty.starts_with("IFCREL") {
            let mut sets = Vec::new();
            t.inst[&o].refs(&mut |p| {
                if t.inst.get(&p).is_some_and(|i| i.ty.contains("PROPERTYSET") || i.ty == "IFCELEMENTQUANTITY") {
                    sets.push(p);
                }
            });
            for p in sets {
                if verwijzers.get(&p).into_iter().flatten().all(|(r, _)| weg.contains(r) || *r == o) {
                    werk.push(p);
                }
            }
        }
    }
    for o in &weg {
        t.inst.remove(o);
    }
    if ruim_op(&mut t, &eerst, &weg) {
        *s = t;
    }
}

/// De nummers waar iets naar verwijst.
fn verwezen(s: &Stap) -> HashSet<u64> {
    let mut uit = HashSet::new();
    for i in s.inst.values() {
        i.refs(&mut |k| {
            uit.insert(k);
        });
    }
    uit
}

/// Haalt knopen weg waar niets meer naar verwijst, terwijl er vóór de bewerking (`eerst`) wel iets naar verwees, en
/// losse knopen die naar iets verwijzen dat weg is. `false` als er daarna nog een verwijzing naar iets dat er niet is
/// overblijft.
fn ruim_op(s: &mut Stap, eerst: &HashSet<u64>, weg: &HashSet<u64>) -> bool {
    let mut verwijderd: HashSet<u64> = weg.clone();
    loop {
        let nu_verwezen = verwezen(s);
        let mut nu: Vec<u64> = Vec::new();
        for (&n, i) in &s.inst {
            if !s.is_knoop(n) || nu_verwezen.contains(&n) {
                continue;
            }
            let mut naar_weg = false;
            i.refs(&mut |k| naar_weg |= verwijderd.contains(&k) || !s.inst.contains_key(&k));
            if eerst.contains(&n) || naar_weg {
                nu.push(n);
            }
        }
        if nu.is_empty() {
            break;
        }
        for n in nu {
            s.inst.remove(&n);
            verwijderd.insert(n);
        }
    }
    s.losse_verwijzingen().is_empty()
}

fn verdieping(s: &mut Stap, k: u32, d: u32, sleutel: &str) {
    let objecten = s.objecten();
    let gid_van: HashMap<u64, String> = objecten.iter().map(|(g, &n)| (n, g.clone())).collect();
    let mut elementen = Vec::new();
    let mut structuren = BTreeSet::new();
    for &r in objecten.values() {
        let i = &s.inst[&r];
        if i.ty != "IFCRELCONTAINEDINSPATIALSTRUCTURE" {
            continue;
        }
        let (W::Lijst(l), W::Ref(st)) = (&i.attrs[4], &i.attrs[5]) else { continue };
        structuren.insert((gid_van[st].clone(), *st));
        for e in l {
            if let W::Ref(e) = e {
                elementen.push((gid_van[e].clone(), *e, r, *st));
            }
        }
    }
    for (g, &n) in &objecten {
        if s.inst[&n].ty == "IFCBUILDINGSTOREY" {
            structuren.insert((g.clone(), n));
        }
    }
    elementen.sort();
    let structuren: Vec<u64> = structuren.into_iter().map(|(_, n)| n).collect();
    let Some(i) = kies(&(0..elementen.len()).collect::<Vec<_>>(), k) else { return };
    let (_, e, r1, s1) = elementen[i].clone();
    let Some(s2) = kies(&structuren, d) else { return };
    if s2 == s1 {
        return;
    }
    // Uit de oude relatie (of de relatie weg als hij leeg wordt).
    let oud = s.inst[&r1].clone();
    let W::Lijst(l) = &oud.attrs[4] else { return };
    if l.len() >= 2 {
        if let W::Lijst(l) = &mut s.inst.get_mut(&r1).unwrap().attrs[4] {
            l.retain(|x| *x != W::Ref(e));
        }
    } else {
        s.inst.remove(&r1);
    }
    // In een relatie van de nieuwe structuur, of een nieuwe relatie.
    let r2 = s
        .objecten()
        .values()
        .copied()
        .find(|r| s.inst[r].ty == "IFCRELCONTAINEDINSPATIALSTRUCTURE" && s.inst[r].attrs[5] == W::Ref(s2));
    match r2 {
        Some(r2) => {
            if let W::Lijst(l) = &mut s.inst.get_mut(&r2).unwrap().attrs[4] {
                l.push(W::Ref(e));
            }
        }
        None => {
            let mut i = oud.clone();
            i.attrs[0] = W::Atoom(format!("'{}'", nieuwe_gid(sleutel)));
            i.attrs[4] = W::Lijst(vec![W::Ref(e)]);
            i.attrs[5] = W::Ref(s2);
            let n = s.nieuw_nummer();
            s.inst.insert(n, i);
        }
    }
    // De plaatsing: stond hij relatief aan de oude structuur, dan nu aan de nieuwe.
    let (Some(je), Some(j1), Some(j2)) = (s.plek(e, "ObjectPlacement"), s.plek(s1, "ObjectPlacement"), s.plek(s2, "ObjectPlacement")) else {
        return;
    };
    let (W::Ref(pe), p1, p2) = (s.inst[&e].attrs[je].clone(), s.inst[&s1].attrs[j1].clone(), s.inst[&s2].attrs[j2].clone()) else {
        return;
    };
    if !matches!(p2, W::Ref(_)) || s.inst.get(&pe).map(|i| i.ty.as_str()) != Some("IFCLOCALPLACEMENT") || s.inst[&pe].attrs[0] != p1 {
        return;
    }
    let mut plaatsing = s.inst[&pe].clone();
    plaatsing.attrs[0] = p2;
    if s.verwijzers().get(&pe).map_or(0, |v| v.len()) > 1 {
        let n = s.nieuw_nummer();
        s.inst.insert(n, plaatsing);
        s.inst.get_mut(&e).unwrap().attrs[je] = W::Ref(n);
    } else {
        s.inst.insert(pe, plaatsing);
    }
}

/// Objecten met een plaatsing, op GlobalId.
fn met_plaatsing(s: &Stap) -> Vec<u64> {
    s.objecten()
        .values()
        .copied()
        .filter(|&n| s.plek(n, "ObjectPlacement").is_some_and(|j| matches!(s.inst[&n].attrs[j], W::Ref(_))))
        .collect()
}

fn splits(s: &mut Stap, k: u32) {
    let met = met_plaatsing(s);
    let plaatsing = |n: u64| s.inst[&n].attrs[s.plek(n, "ObjectPlacement").unwrap()].clone();
    let gedeeld: Vec<u64> =
        met.iter().copied().filter(|&n| met.iter().filter(|&&m| plaatsing(m) == plaatsing(n)).count() > 1).collect();
    let Some(n) = kies(&gedeeld, k) else { return };
    let j = s.plek(n, "ObjectPlacement").unwrap();
    let W::Ref(p) = s.inst[&n].attrs[j] else { return };
    let g = s.gid(n).unwrap();
    // De plaatsing zelf altijd kopiëren; daaronder alleen wat bij dit object hoort.
    let direct = s.directe_objecten();
    let mut i = s.inst[&p].clone();
    let mut memo = HashMap::new();
    let mut kinderen = Vec::new();
    i.refs(&mut |c| kinderen.push(c));
    let kopie: HashMap<u64, u64> = kinderen.into_iter().map(|c| (c, kopieer(s, c, &g, &direct, &mut memo))).collect();
    i.attrs.iter_mut().for_each(|a| a.refs_mut(&mut |r| *r = kopie[r]));
    let nieuw = s.nieuw_nummer();
    s.inst.insert(nieuw, i);
    s.inst.get_mut(&n).unwrap().attrs[j] = W::Ref(nieuw);
}

fn relatie(s: &mut Stap, k: u32, w: u8, sleutel: &str) {
    let doelen: Vec<u64> = s
        .objecten()
        .values()
        .copied()
        .filter(|n| {
            let ty = &s.inst[n].ty;
            !ty.starts_with("IFCREL") && !ty.contains("PROPERTYSET") && s.schema.is(ty, "IFCOBJECT")
        })
        .collect();
    let Some(doel) = kies(&doelen, k) else { return };
    let eigenaar = s.inst[&doel].attrs[1].clone();
    let (p, set, rel) = (s.nieuw_nummer(), s.nieuw_nummer() + 1, s.nieuw_nummer() + 2);
    let a = |x: &str| W::Atoom(x.to_string());
    s.inst.insert(p, Inst {
        ty: "IFCPROPERTYSINGLEVALUE".into(),
        attrs: vec![a("'Waarde'"), a("$"), W::Getypt("IFCLABEL".into(), vec![a(&format!("'w{w}'"))]), a("$")],
    });
    s.inst.insert(set, Inst {
        ty: "IFCPROPERTYSET".into(),
        attrs: vec![
            a(&format!("'{}'", nieuwe_gid(&format!("{sleutel}set")))),
            eigenaar.clone(),
            a(&format!("'Pset_Proef{}'", w % 2)),
            a("$"),
            W::Lijst(vec![W::Ref(p)]),
        ],
    });
    s.inst.insert(rel, Inst {
        ty: "IFCRELDEFINESBYPROPERTIES".into(),
        attrs: vec![
            a(&format!("'{}'", nieuwe_gid(&format!("{sleutel}rel")))),
            eigenaar,
            a("$"),
            a("$"),
            W::Lijst(vec![W::Ref(doel)]),
            W::Ref(set),
        ],
    });
}

fn set_volgorde(s: &mut Stap, k: u32, zaad: u64) {
    let mut plekken = Vec::new();
    for (&n, i) in &s.inst {
        for (j, a) in i.attrs.iter().enumerate() {
            if matches!(a, W::Lijst(l) if l.len() >= 2) && s.is_set(n, j) {
                plekken.push((n, j));
            }
        }
    }
    let Some((n, j)) = kies(&plekken, k) else { return };
    if let W::Lijst(l) = &mut s.inst.get_mut(&n).unwrap().attrs[j] {
        let oud = l.clone();
        let mut toeval = Toeval::nieuw(zaad);
        toeval.schud(l);
        if *l == oud {
            l.rotate_left(1);
        }
    }
}

fn tijd(s: &mut Stap, k: u32, w: u8) {
    let mut plekken = Vec::new();
    for (&n, i) in &s.inst {
        if i.ty == "IFCOWNERHISTORY" {
            for veld in ["LastModifiedDate", "CreationDate"] {
                plekken.push((n, s.plek(n, veld).unwrap(), true));
            }
        } else if s.schema.is(&i.ty, "IFCWORKCONTROL") {
            let j = s.plek(n, "CreationDate").unwrap();
            if i.attrs[j].is_tekst() {
                plekken.push((n, j, false));
            }
        }
    }
    let Some((n, j, getal)) = kies(&plekken, k) else { return };
    s.inst.get_mut(&n).unwrap().attrs[j] = if getal {
        W::Atoom((1_800_000_000 + u64::from(w) * 1000 + u64::from(k % 7)).to_string())
    } else {
        W::Atoom(format!("'2027-0{}-01T00:00:00'", w + 1))
    };
}
