//! Het samengevoegde bestand schrijven.
//!
//! We beginnen bij de tekst van ons. Een object dat we van ons houden, blijft staan zoals het is. Een object dat
//! (deels) van hun komt, schrijven we opnieuw op zijn regel in ons; is het nieuw, dan komt het achteraan, met een
//! nummer boven het hoogste nummer van ons. Voor de knopen van hun:
//! - Een ankerknoop van hun met dezelfde ankers als een knoop van ons: is de inhoud gelijk, dan die van ons. Is de
//!   inhoud anders en komen al die ankers in het resultaat van hun, dan herschrijven we de regel van ons. Anders een
//!   kopie achteraan.
//! - Een ankerknoop van hun zonder tegenhanger: een ankerknoop van ons met dezelfde inhoud waar geen knoop naar
//!   verwijst (bijv. een gedeelde IfcOwnerHistory als hun een object toevoegde), anders een kopie.
//! - Een verwijzing dieper naar een ankerknoop wordt de knoop die het object van dat anker in het resultaat heeft.
//!   Wijzen de ankers niet naar precies één knoop, dan is dat een conflict.
//! - Een knoop zonder anker: een knoop van ons met dezelfde inhoud (bij voorkeur van hetzelfde object), anders een
//!   kopie. Een losse knoop van hun (bijv. een IfcStyledItem) moet daarbij eenduidig zijn; anders een conflict.
//!
//! Knopen van ons waar na het schrijven niets meer naar verwijst, halen we weg. De HEADER blijft van ons, behalve de
//! velden die van hun komen. Of het resultaat klopt, controleert [`crate::voeg_samen`] daarna zelf.

use std::collections::HashMap;
use std::io::Write as _;

use crate::model::{attribuut_tekst, Hash, Model, Soort};
use crate::ontleden::{Token, Tokens};
use crate::samenvoegen::{Besluit, Conflict, ConflictSoort, Deel, Gevonden, Plan, B, H, O};
use crate::verschil::losse_groepen;

pub(crate) struct Geschreven {
    pub inhoud: Vec<u8>,
    /// GlobalId in het resultaat -> sleutel in het plan.
    pub alias: HashMap<String, String>,
    pub nieuw: usize,
    pub herschreven: usize,
    pub verwijderd: usize,
}

const GEEN: u32 = u32::MAX;
const MEER: u32 = u32::MAX - 1;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Modus {
    /// Een attribuut van een object; `lijst`: een element van een samengevoegde lijst.
    Direct { lijst: bool },
    /// In een knoop.
    Dieper,
    /// In een losse knoop zelf: een verwijzing naar een knoop zonder anker moet eenduidig zijn.
    Wortel,
}

struct Schrijver<'p, 'm, 'a> {
    m: &'m [Model<'a>; 3],
    plan: &'p Plan,
    /// Per sleutel: het nummer in het resultaat (None = niet in het resultaat).
    object_id: Vec<Option<u64>>,
    volgende: u64,
    /// Hun-knoop -> nummer in het resultaat, en of de keuze dubbelzinnig was.
    vertaald: HashMap<u32, (u64, bool)>,
    /// Ons-knoop -> hun-knoop waarvan we de inhoud op de regel van ons zetten, met het object waar het om gaat.
    herschrijf: HashMap<u32, (u32, usize)>,
    /// Nieuwe regels: nummer en tekst (na `#n=`, zonder `;`).
    nieuw: Vec<(u64, Vec<u8>)>,
    /// Herschreven regels van ons: instantie -> tekst (na `#n=`, zonder `;`).
    vervangen: HashMap<u32, Vec<u8>>,
    /// Regels van ons die weg moeten (objecten en losse knopen).
    weg: Vec<bool>,
    /// Hun-knopen om te kopiëren: instantie, nummer, en het object waar het om gaat.
    wachtrij: Vec<(u32, u64, Option<usize>)>,
    anker_memo: HashMap<(usize, u32, u32), Option<u64>>,
    /// Ankers van objecten van ons die naar een andere knoop gaan wijzen (de deling van hun).
    omleiding: HashMap<(usize, u32, u32), u64>,
    /// Kopie (nummer) -> hun-knoop.
    kopie_van: HashMap<u64, u32>,
    ons_op_anker: Option<HashMap<Hash, u32>>,
    ons_herbruikbaar: Option<HashMap<Hash, Vec<u32>>>,
    ons_op_waarde: Option<HashMap<Hash, Vec<u32>>>,
    knoopouder: [Option<Vec<bool>>; 3],
    eigenaar: [Option<Vec<u32>>; 3],
    /// Het object dat we nu schrijven (voor een conflict).
    huidig: Option<usize>,
    conflicten: Gevonden,
}

/// Schrijft het resultaat volgens het plan, of geeft de conflicten die het schrijven onmogelijk maakten.
pub(crate) fn schrijf(m: &[Model; 3], plan: &Plan) -> Result<Geschreven, Gevonden> {
    let mut s = Schrijver {
        m,
        plan,
        object_id: vec![None; plan.sleutels.len()],
        volgende: m[O].bestand.hoogste + 1,
        vertaald: HashMap::new(),
        herschrijf: HashMap::new(),
        nieuw: Vec::new(),
        vervangen: HashMap::new(),
        weg: vec![false; m[O].bestand.instanties.len()],
        wachtrij: Vec::new(),
        anker_memo: HashMap::new(),
        omleiding: HashMap::new(),
        kopie_van: HashMap::new(),
        ons_op_anker: None,
        ons_herbruikbaar: None,
        ons_op_waarde: None,
        knoopouder: [None, None, None],
        eigenaar: [None, None, None],
        huidig: None,
        conflicten: Vec::new(),
    };
    s.nummers();
    s.herschrijvingen();
    s.objecten();
    s.losse_knopen();
    s.wachtrij_af();
    s.omleidingen();
    if !s.conflicten.is_empty() {
        return Err(s.conflicten);
    }
    Ok(s.samenstellen())
}

/// De p-de verwijzing in een tekst: het nummer en het begin.
fn verwijzing_op(tekst: &[u8], p: u32) -> Option<(u64, usize)> {
    Tokens::nieuw(tekst)
        .filter_map(|(begin, t)| match t {
            Token::Verwijzing(n) => Some((n, begin)),
            _ => None,
        })
        .nth(p as usize)
}

/// De inhoud van een instantie zonder de `;` aan het eind.
fn inhoud_zonder_puntkomma<'t>(m: &'t Model, i: usize) -> &'t [u8] {
    let t = m.bestand.inhoud(i);
    let t = t.strip_suffix(b";").unwrap_or(t);
    let wit = t.iter().rev().take_while(|&&b| b <= b' ').count();
    &t[..t.len() - wit]
}

impl Schrijver<'_, '_, '_> {
    fn nieuw_nummer(&mut self) -> u64 {
        let n = self.volgende;
        self.volgende += 1;
        n
    }

    fn ons_nummer(&self, i: usize) -> u64 {
        self.m[O].bestand.instanties[i].id
    }

    fn inst(&self, k: usize, s: usize) -> usize {
        self.m[k].objecten[self.plan.sleutels[s].obj[k].expect("object bestaat aan deze kant")].inst as usize
    }

    fn sleutel_van(&self, k: usize, o: u32) -> Option<usize> {
        self.plan.op_sleutel.get(&self.m[k].objecten[o as usize].sleutel).copied()
    }

    fn conflict(&mut self, soort: ConflictSoort, melding: String, mut betrokken: Vec<usize>) {
        let mut c = Conflict::nieuw(soort, melding);
        if let Some(s) = self.huidig {
            c = c.over(self.m, &self.plan.sleutels[s]);
            betrokken.insert(0, s);
        }
        betrokken.dedup();
        if !self.conflicten.iter().any(|(x, _)| *x == c) {
            self.conflicten.push((c, betrokken));
        }
    }

    // --- Tabellen die we alleen maken als ze nodig zijn. ---

    fn ons_op_anker(&mut self, a: &Hash) -> Option<u32> {
        let m = self.m;
        self.ons_op_anker
            .get_or_insert_with(|| {
                m[O].anker_id.iter().enumerate().filter_map(|(k, a)| a.map(|a| (a, k as u32))).collect()
            })
            .get(a)
            .copied()
    }

    fn knoopouder(&mut self, k: usize) -> &[bool] {
        let m = self.m;
        self.knoopouder[k].get_or_insert_with(|| {
            let mut v = vec![false; m[k].bestand.instanties.len()];
            for x in 0..v.len() {
                if m[k].soort[x] == Soort::Knoop {
                    for &c in m[k].bestand.verwijzingen(x) {
                        v[c as usize] = true;
                    }
                }
            }
            v
        })
    }

    /// Een ankerknoop van ons zonder knoop-ouder, met deze inhoud, die we niet herschrijven.
    fn herbruikbaar(&mut self, h: &Hash) -> Option<u32> {
        if self.ons_herbruikbaar.is_none() {
            let m = self.m;
            let ouder = self.knoopouder(O).to_vec();
            let mut kaart: HashMap<Hash, Vec<u32>> = HashMap::new();
            for (k, &heeft_ouder) in ouder.iter().enumerate() {
                if m[O].soort[k] == Soort::Knoop && m[O].anker_id[k].is_some() && !heeft_ouder {
                    kaart.entry(m[O].knoop_hash[k]).or_default().push(k as u32);
                }
            }
            self.ons_herbruikbaar = Some(kaart);
        }
        let kandidaten = self.ons_herbruikbaar.as_ref()?.get(h)?;
        kandidaten.iter().copied().find(|k| !self.herschrijf.contains_key(k))
    }

    /// Knopen van ons zonder anker (en geen losse knoop) met deze inhoud.
    fn ons_op_waarde(&mut self, h: &Hash) -> Vec<u32> {
        let m = self.m;
        self.ons_op_waarde
            .get_or_insert_with(|| {
                let mut wortel = vec![false; m[O].bestand.instanties.len()];
                for &w in &m[O].wortels {
                    wortel[w as usize] = true;
                }
                let mut kaart: HashMap<Hash, Vec<u32>> = HashMap::new();
                for (k, &is_wortel) in wortel.iter().enumerate() {
                    if m[O].soort[k] == Soort::Knoop && m[O].anker_id[k].is_none() && !is_wortel {
                        kaart.entry(m[O].knoop_hash[k]).or_default().push(k as u32);
                    }
                }
                kaart
            })
            .get(h)
            .cloned()
            .unwrap_or_default()
    }

    /// Per knoop het object (sleutel in het plan) waar hij bij hoort: GEEN, MEER (meer objecten) of de sleutel.
    fn eigenaar(&mut self, k: usize) -> &[u32] {
        let m = self.m;
        let plan = self.plan;
        self.eigenaar[k].get_or_insert_with(|| {
            let model = &m[k];
            let mut e = vec![GEEN; model.bestand.instanties.len()];
            let mut stapel: Vec<u32> = Vec::new();
            for object in &model.objecten {
                let Some(&s) = plan.op_sleutel.get(&object.sleutel) else { continue };
                let s = s as u32;
                stapel.extend(
                    model
                        .bestand
                        .verwijzingen(object.inst as usize)
                        .iter()
                        .filter(|&&c| model.soort[c as usize] == Soort::Knoop),
                );
                while let Some(x) = stapel.pop() {
                    let x = x as usize;
                    match e[x] {
                        GEEN => e[x] = s,
                        MEER => continue,
                        y if y == s => continue,
                        _ => e[x] = MEER,
                    }
                    for &c in model.bestand.verwijzingen(x) {
                        if model.soort[c as usize] == Soort::Knoop && model.anker_id[c as usize].is_none() {
                            stapel.push(c);
                        }
                    }
                }
            }
            e
        })
    }

    // --- De stappen. ---

    /// Het nummer van elk object in het resultaat.
    fn nummers(&mut self) {
        for s in 0..self.plan.sleutels.len() {
            let sl = &self.plan.sleutels[s];
            self.object_id[s] = match (&sl.besluit, sl.obj[O]) {
                (Besluit::Weg, _) => None,
                (_, Some(o)) => Some(self.ons_nummer(self.m[O].objecten[o].inst as usize)),
                (_, None) => Some(self.nieuw_nummer()),
            };
        }
    }

    /// Komt attribuut j van object s in het resultaat in zijn geheel van hun?
    fn attribuut_van_hun(&self, s: usize, j: usize) -> bool {
        match &self.plan.sleutels[s].besluit {
            Besluit::Heel(k) => *k == H,
            Besluit::Samen(delen) => delen.get(j) == Some(&Deel::Van(H)),
            Besluit::Weg => false,
        }
    }

    /// Ankerknopen van ons die we herschrijven met de inhoud van hun: dezelfde ankers, andere inhoud, en al die
    /// ankers komen in het resultaat van hun.
    fn herschrijvingen(&mut self) {
        let m = self.m;
        for s in 0..self.plan.sleutels.len() {
            if self.plan.sleutels[s].obj[H].is_none() {
                continue;
            }
            let i = self.inst(H, s);
            for j in 1..m[H].bestand.attributen(i).len() {
                if !self.attribuut_van_hun(s, j) {
                    continue;
                }
                for (_, token) in Tokens::nieuw(attribuut_tekst(&m[H], i, j)) {
                    let Token::Verwijzing(n) = token else { continue };
                    let Some(&c) = m[H].bestand.index.get(&n) else { continue };
                    let Some(a) = m[H].anker_id[c as usize] else { continue };
                    let Some(ok) = self.ons_op_anker(&a) else { continue };
                    if m[O].knoop_hash[ok as usize] == m[H].knoop_hash[c as usize] || self.herschrijf.contains_key(&ok)
                    {
                        continue;
                    }
                    let alle = m[O].ankers(ok as usize).iter().all(|a| {
                        self.sleutel_van(O, a.object).is_some_and(|x| self.attribuut_van_hun(x, a.attribuut as usize))
                    });
                    if alle {
                        self.herschrijf.insert(ok, (c, s));
                        self.vertaald.insert(c, (self.ons_nummer(ok as usize), false));
                    }
                }
            }
        }
    }

    /// De objecten die (deels) van hun komen of weg moeten.
    fn objecten(&mut self) {
        for s in 0..self.plan.sleutels.len() {
            let sl = &self.plan.sleutels[s];
            match sl.besluit {
                Besluit::Weg => {
                    if let Some(o) = sl.obj[O] {
                        self.weg[self.m[O].objecten[o].inst as usize] = true;
                    }
                    continue;
                }
                Besluit::Heel(k) if k == O => continue,
                _ => {}
            }
            self.huidig = Some(s);
            let tekst = self.object_tekst(s);
            match sl.obj[O] {
                Some(o) => {
                    self.vervangen.insert(self.m[O].objecten[o].inst, tekst);
                }
                None => {
                    let n = self.object_id[s].expect("in het resultaat");
                    self.nieuw.push((n, tekst));
                }
            }
        }
        self.huidig = None;
    }

    fn object_tekst(&mut self, s: usize) -> Vec<u8> {
        let m = self.m;
        let sl = &self.plan.sleutels[s];
        let (k, i) = match &sl.besluit {
            Besluit::Heel(k) => (*k, self.inst(*k, s)),
            _ => (O, self.inst(O, s)),
        };
        let mut uit = m[k].bestand.tekst(m[k].bestand.instanties[i].type_bereik).to_vec();
        uit.push(b'(');
        let n = m[k].bestand.attributen(i).len();
        for j in 0..n {
            if j > 0 {
                uit.push(b',');
            }
            if j == 0 && sl.obj[O].is_some() {
                uit.extend_from_slice(attribuut_tekst(&m[O], self.inst(O, s), 0));
                continue;
            }
            match &sl.besluit {
                Besluit::Samen(delen) => match &delen[j] {
                    Deel::Van(x) if *x == O => uit.extend_from_slice(attribuut_tekst(&m[O], self.inst(O, s), j)),
                    Deel::Van(_) => {
                        let t =
                            self.vertaal(attribuut_tekst(&m[H], self.inst(H, s), j), Modus::Direct { lijst: false });
                        uit.extend_from_slice(&t);
                    }
                    Deel::Lijst(elementen) => {
                        if elementen.is_empty() {
                            let optioneel = m[O].entiteit(i).is_some_and(|e| e.attributen[j].optioneel);
                            uit.extend_from_slice(if optioneel { b"$" } else { b"()" });
                            continue;
                        }
                        uit.push(b'(');
                        for (x, &(kant, (b, e))) in elementen.iter().enumerate() {
                            if x > 0 {
                                uit.push(b',');
                            }
                            let tekst = &attribuut_tekst(&m[kant], self.inst(kant, s), j)[b..e];
                            if kant == O {
                                uit.extend_from_slice(tekst);
                            } else {
                                let t = self.vertaal(tekst, Modus::Direct { lijst: true });
                                uit.extend_from_slice(&t);
                            }
                        }
                        uit.push(b')');
                    }
                },
                _ => {
                    let t = self.vertaal(attribuut_tekst(&m[k], i, j), Modus::Direct { lijst: false });
                    uit.extend_from_slice(&t);
                }
            }
        }
        uit.push(b')');
        uit
    }

    /// Groepen losse knopen die van hun komen: die van ons eruit, die van hun erbij.
    fn losse_knopen(&mut self) {
        let m = self.m;
        let go = losse_groepen(&m[O]);
        let gh = losse_groepen(&m[H]);
        for &(sleutel, kant) in &self.plan.groepen {
            if kant != H {
                continue;
            }
            for &(_, i) in go.get(&sleutel).map(Vec::as_slice).unwrap_or(&[]) {
                self.weg[i as usize] = true;
            }
            for &(_, i) in gh.get(&sleutel).map(Vec::as_slice).unwrap_or(&[]) {
                let n = self.nieuw_nummer();
                let t = self.vertaal(inhoud_zonder_puntkomma(&m[H], i as usize), Modus::Wortel);
                self.nieuw.push((n, t));
            }
        }
    }

    /// Schrijft de herschreven ankerknopen en de kopieën van hun.
    fn wachtrij_af(&mut self) {
        let m = self.m;
        let mut lijst: Vec<(u32, (u32, usize))> = self.herschrijf.iter().map(|(&k, &v)| (k, v)).collect();
        lijst.sort_unstable();
        for (ok, (c, s)) in lijst {
            self.huidig = Some(s);
            let t = self.vertaal(inhoud_zonder_puntkomma(&m[H], c as usize), Modus::Dieper);
            self.vervangen.insert(ok, t);
        }
        while let Some((c, n, s)) = self.wachtrij.pop() {
            self.huidig = s;
            let t = self.vertaal(inhoud_zonder_puntkomma(&m[H], c as usize), Modus::Dieper);
            self.nieuw.push((n, t));
        }
        self.huidig = None;
    }

    // --- Vertalen van verwijzingen van hun naar nummers in het resultaat. ---

    /// Een tekst van hun met de verwijzingen vertaald naar nummers in het resultaat.
    fn vertaal(&mut self, tekst: &[u8], modus: Modus) -> Vec<u8> {
        let mut uit = Vec::with_capacity(tekst.len() + 16);
        let mut vorige = 0;
        let mut tokens = Tokens::nieuw(tekst);
        while let Some((begin, token)) = tokens.next() {
            let Token::Verwijzing(n) = token else { continue };
            let eind = tokens.positie();
            uit.extend_from_slice(&tekst[vorige..begin]);
            let nummer = self.doel(n, modus).unwrap_or(0);
            let _ = write!(uit, "#{nummer}");
            vorige = eind;
        }
        uit.extend_from_slice(&tekst[vorige..]);
        uit
    }

    fn doel(&mut self, n: u64, modus: Modus) -> Option<u64> {
        let m = self.m;
        let Some(&d) = m[H].bestand.index.get(&n) else {
            self.conflict(ConflictSoort::Zelfcontrole, format!("hun verwijst naar #{n}, dat niet bestaat"), Vec::new());
            return None;
        };
        let d = d as usize;
        match m[H].soort[d] {
            Soort::Object(o) => {
                let s = self.sleutel_van(H, o);
                match s.and_then(|s| self.object_id[s]) {
                    Some(n) => Some(n),
                    None => {
                        let weergave = m[H].object_weergave(o as usize);
                        self.conflict(
                            ConflictSoort::VerwijzingNaarVerwijderd,
                            format!("verwijst naar {weergave}, dat na het samenvoegen niet meer bestaat"),
                            s.into_iter().collect(),
                        );
                        None
                    }
                }
            }
            Soort::Knoop => match modus {
                Modus::Direct { lijst } => Some(self.direct(d as u32, lijst)),
                _ if m[H].anker_id[d].is_some() => self.via_anker(d as u32),
                _ => self.waarde(d as u32, modus == Modus::Wortel),
            },
        }
    }

    fn kopie(&mut self, c: u32) -> u64 {
        let n = self.nieuw_nummer();
        self.vertaald.insert(c, (n, false));
        self.kopie_van.insert(n, c);
        self.wachtrij.push((c, n, self.huidig));
        n
    }

    /// Een ankerknoop van hun waar een object van hun rechtstreeks naar verwijst.
    fn direct(&mut self, c: u32, lijst: bool) -> u64 {
        let m = self.m;
        if let Some(&(n, _)) = self.vertaald.get(&c) {
            return n;
        }
        let h = m[H].knoop_hash[c as usize];
        if !lijst {
            if let Some(a) = m[H].anker_id[c as usize] {
                if let Some(ok) = self.ons_op_anker(&a) {
                    if m[O].knoop_hash[ok as usize] == h && !self.herschrijf.contains_key(&ok) {
                        let n = self.ons_nummer(ok as usize);
                        self.vertaald.insert(c, (n, false));
                        return n;
                    }
                }
            }
            if !self.knoopouder(H)[c as usize] {
                if let Some(ok) = self.herbruikbaar(&h) {
                    let n = self.ons_nummer(ok as usize);
                    self.vertaald.insert(c, (n, false));
                    return n;
                }
            }
        }
        self.kopie(c)
    }

    /// Een verwijzing dieper naar een ankerknoop van hun: de knoop die de objecten van die ankers in het resultaat
    /// hebben. Dat moet precies één knoop zijn.
    ///
    /// Deelt of splitst hun een knoop anders dan de basis, en liet ons die deling ongemoeid, dan nemen we de deling
    /// van hun over: we laten de objecten van ons naar de knoop van hun wijzen. Hun inhoud blijft gelijk, dus de
    /// objecten zelf veranderen niet.
    fn via_anker(&mut self, d: u32) -> Option<u64> {
        let m = self.m;
        let mut doelen: Vec<((usize, u32, u32), Option<u64>)> = Vec::new();
        let mut eigenaars = Vec::new();
        let mut verwijderd = false;
        let mut onbekend = false;
        for a in m[H].ankers(d as usize) {
            let Some(s) = self.sleutel_van(H, a.object) else {
                onbekend = true;
                continue;
            };
            eigenaars.push(s);
            if self.plan.sleutels[s].besluit == Besluit::Weg {
                verwijderd = true;
                continue;
            }
            let t = self.anker_doel(s, a.attribuut, a.positie);
            doelen.push(((s, a.attribuut, a.positie), t));
        }
        if verwijderd {
            let anker = m[H].anker_weergave(d as usize);
            self.conflict(
                ConflictSoort::VerwijzingNaarVerwijderd,
                format!("verwijst naar {anker}, van een object dat na het samenvoegen niet meer bestaat"),
                eigenaars,
            );
            return None;
        }
        let eerste = doelen.first().and_then(|x| x.1);
        let mut eenduidig = !onbekend && eerste.is_some() && doelen.iter().all(|x| x.1.is_some());
        if let (true, Some(doel)) = (eenduidig, eerste) {
            // Hun deelt wat ons apart heeft.
            let inhoud = self.inhoud_van(doel);
            for &(e, t) in &doelen {
                if t != Some(doel) && !inhoud.is_some_and(|h| self.leid_om(e, doel, h)) {
                    eenduidig = false;
                }
            }
            // Hun splitste wat ons deelt: de andere ankers van die knoop van ons krijgen de knoop van hun.
            if let (true, Some(ok)) = (eenduidig, self.ons_knoop(doel)) {
                let van_hun: Vec<(usize, u32, u32)> = doelen.iter().map(|x| x.0).collect();
                for a in m[O].ankers(ok) {
                    let Some(s) = self.sleutel_van(O, a.object) else { continue };
                    let e = (s, a.attribuut, a.positie);
                    if van_hun.contains(&e) {
                        continue;
                    }
                    let Some(c) = self.hun_knoop_op(e) else { continue };
                    if m[H].knoop_hash[c as usize] != m[O].knoop_hash[ok] {
                        continue;
                    }
                    let x = self.direct(c, false);
                    if x != doel {
                        self.leid_om(e, x, m[H].knoop_hash[c as usize]);
                    }
                }
            }
        }
        if !eenduidig {
            let anker = m[H].anker_weergave(d as usize);
            self.conflict(
                ConflictSoort::GedeeldAnker,
                format!(
                    "verwijst naar {anker}; in het resultaat wijzen de objecten van dat anker niet naar precies één \
                     knoop (een kant splitste of deelde die knoop anders)"
                ),
                eigenaars,
            );
            return None;
        }
        eerste
    }

    /// De inhoud die knoop `n` in het resultaat krijgt.
    fn inhoud_van(&self, n: u64) -> Option<Hash> {
        let m = self.m;
        if let Some(&c) = self.kopie_van.get(&n) {
            return Some(m[H].knoop_hash[c as usize]);
        }
        let k = *m[O].bestand.index.get(&n)?;
        if let Some(&(c, _)) = self.herschrijf.get(&k) {
            return Some(m[H].knoop_hash[c as usize]);
        }
        (m[O].soort[k as usize] == Soort::Knoop).then(|| m[O].knoop_hash[k as usize])
    }

    /// De knoop van ons met nummer `n` (geen kopie).
    fn ons_knoop(&self, n: u64) -> Option<usize> {
        if self.kopie_van.contains_key(&n) {
            return None;
        }
        let k = *self.m[O].bestand.index.get(&n)? as usize;
        (self.m[O].soort[k] == Soort::Knoop).then_some(k)
    }

    /// De knoop van hun op anker e.
    fn hun_knoop_op(&self, (s, j, p): (usize, u32, u32)) -> Option<u32> {
        let m = self.m;
        let o = self.plan.sleutels[s].obj[H]?;
        let i = m[H].objecten[o].inst as usize;
        let (n, _) = verwijzing_op(attribuut_tekst(&m[H], i, j as usize), p)?;
        let &c = m[H].bestand.index.get(&n)?;
        (m[H].soort[c as usize] == Soort::Knoop).then_some(c)
    }

    /// Laat anker e (van een object dat we van ons houden) naar knoop `naar` wijzen, als dat het object niet
    /// verandert (dezelfde inhoud) en ons de deling op dat anker niet wijzigde ten opzichte van de basis.
    fn leid_om(&mut self, e: (usize, u32, u32), naar: u64, inhoud: Hash) -> bool {
        let m = self.m;
        let (s, j, p) = e;
        let sl = &self.plan.sleutels[s];
        if sl.besluit != Besluit::Heel(O) {
            return false;
        }
        let (Some(o), Some(ob)) = (sl.obj[O], sl.obj[B]) else { return false };
        let knoop_op = |model: &Model, i: usize| -> Option<usize> {
            let (n, _) = verwijzing_op(attribuut_tekst(model, i, j as usize), p)?;
            model.bestand.index.get(&n).map(|&k| k as usize)
        };
        let Some(ok) = knoop_op(&m[O], m[O].objecten[o].inst as usize) else { return false };
        let Some(kb) = knoop_op(&m[B], m[B].objecten[ob].inst as usize) else {
            return false;
        };
        if m[O].soort[ok] != Soort::Knoop || m[O].knoop_hash[ok] != inhoud {
            return false;
        }
        if m[B].anker_id[kb].is_none() || m[B].anker_id[kb] != m[O].anker_id[ok] {
            return false;
        }
        if self.omleiding.get(&e).is_some_and(|&x| x != naar) {
            return false;
        }
        self.omleiding.insert(e, naar);
        self.anker_memo.insert(e, Some(naar));
        true
    }

    /// Schrijft de objecten van ons opnieuw waarvan een anker naar een andere knoop wijst ([`Self::leid_om`]).
    fn omleidingen(&mut self) {
        let m = self.m;
        let mut per_object: HashMap<usize, Vec<(u32, u32, u64)>> = HashMap::new();
        for (&(s, j, p), &n) in &self.omleiding {
            per_object.entry(s).or_default().push((j, p, n));
        }
        for (s, lijst) in per_object {
            let i = self.inst(O, s);
            let mut uit = m[O].bestand.tekst(m[O].bestand.instanties[i].type_bereik).to_vec();
            uit.push(b'(');
            for j in 0..m[O].bestand.attributen(i).len() {
                if j > 0 {
                    uit.push(b',');
                }
                let tekst = attribuut_tekst(&m[O], i, j);
                let mut vorige = 0;
                let mut p = 0u32;
                let mut tokens = Tokens::nieuw(tekst);
                while let Some((begin, token)) = tokens.next() {
                    let Token::Verwijzing(_) = token else { continue };
                    let eind = tokens.positie();
                    if let Some(&(_, _, n)) = lijst.iter().find(|x| x.0 == j as u32 && x.1 == p) {
                        uit.extend_from_slice(&tekst[vorige..begin]);
                        let _ = write!(uit, "#{n}");
                        vorige = eind;
                    }
                    p += 1;
                }
                uit.extend_from_slice(&tekst[vorige..]);
            }
            uit.push(b')');
            self.vervangen.insert(i as u32, uit);
        }
    }

    /// De knoop op anker (object s, attribuut j, positie p) in het resultaat.
    fn anker_doel(&mut self, s: usize, j: u32, p: u32) -> Option<u64> {
        if let Some(&n) = self.omleiding.get(&(s, j, p)) {
            return Some(n);
        }
        if let Some(&n) = self.anker_memo.get(&(s, j, p)) {
            return n;
        }
        let m = self.m;
        let ons = |x: &Self| -> Option<u64> {
            let i = x.inst(O, s);
            let (n, _) = verwijzing_op(attribuut_tekst(&m[O], i, j as usize), p)?;
            let &k = m[O].bestand.index.get(&n)?;
            Some(m[O].bestand.instanties[k as usize].id)
        };
        let uit = match &self.plan.sleutels[s].besluit {
            Besluit::Weg => None,
            Besluit::Heel(k) if *k == O => ons(self),
            Besluit::Heel(k) if *k == H => self.hun_anker(s, j, p, false),
            Besluit::Heel(_) => None,
            Besluit::Samen(delen) => match delen.get(j as usize) {
                Some(Deel::Van(k)) if *k == O => ons(self),
                Some(Deel::Van(_)) => self.hun_anker(s, j, p, false),
                Some(Deel::Lijst(elementen)) => {
                    let i = self.inst(H, s);
                    let plek = verwijzing_op(attribuut_tekst(&m[H], i, j as usize), p).map(|x| x.1);
                    let gekozen = plek.is_some_and(|x| elementen.iter().any(|&(k, (b, e))| k == H && b <= x && x < e));
                    if gekozen {
                        self.hun_anker(s, j, p, true)
                    } else {
                        None
                    }
                }
                None => None,
            },
        };
        self.anker_memo.insert((s, j, p), uit);
        uit
    }

    fn hun_anker(&mut self, s: usize, j: u32, p: u32, lijst: bool) -> Option<u64> {
        let m = self.m;
        let i = self.inst(H, s);
        let (n, _) = verwijzing_op(attribuut_tekst(&m[H], i, j as usize), p)?;
        let &c = m[H].bestand.index.get(&n)?;
        match m[H].soort[c as usize] {
            Soort::Knoop => Some(self.direct(c, lijst)),
            Soort::Object(o) => self.sleutel_van(H, o).and_then(|x| self.object_id[x]),
        }
    }

    /// Een knoop van hun zonder anker: een knoop van ons met dezelfde inhoud, anders een kopie.
    fn waarde(&mut self, d: u32, wortel: bool) -> Option<u64> {
        let m = self.m;
        if let Some(&(n, dubbel)) = self.vertaald.get(&d) {
            if wortel && dubbel {
                self.dubbelzinnig(d);
                return None;
            }
            return Some(n);
        }
        // Alleen knopen van ons die net als deze knoop wel (of niet) bij een object horen: anders verandert de
        // sleutel van een losse knoop die ernaar wijst.
        let eh = self.eigenaar(H)[d as usize];
        let kandidaten: Vec<(u32, u32)> = {
            let lijst = self.ons_op_waarde(&m[H].knoop_hash[d as usize]);
            let eo = self.eigenaar(O);
            lijst.into_iter().map(|k| (k, eo[k as usize])).filter(|&(_, e)| (e == GEEN) == (eh == GEEN)).collect()
        };
        if kandidaten.is_empty() {
            return Some(self.kopie(d));
        }
        let hun_nummer = m[H].bestand.instanties[d as usize].id;
        let zelfde: Vec<u32> = kandidaten.iter().filter(|&&(_, e)| eh < MEER && e == eh).map(|&(k, _)| k).collect();
        let (keuze, dubbel) = if let Some(&(k, _)) =
            kandidaten.iter().find(|&&(k, _)| m[O].bestand.instanties[k as usize].id == hun_nummer)
        {
            (k, false)
        } else if kandidaten.len() == 1 {
            (kandidaten[0].0, false)
        } else if let [k] = zelfde.as_slice() {
            (*k, false)
        } else {
            (zelfde.first().copied().unwrap_or(kandidaten[0].0), eh != GEEN)
        };
        if wortel && dubbel {
            self.dubbelzinnig(d);
            return None;
        }
        let n = self.ons_nummer(keuze as usize);
        self.vertaald.insert(d, (n, dubbel));
        Some(n)
    }

    fn dubbelzinnig(&mut self, d: u32) {
        let t = self.m[H].toon_instantie(d as usize, 120);
        self.conflict(
            ConflictSoort::LosseKnopen,
            format!(
                "een losse knoop van hun (bijv. een kleur of stijl) wijst naar {t}; ons heeft meer knopen met precies \
                 die inhoud, dus het is niet eenduidig welke bedoeld is"
            ),
            Vec::new(),
        );
    }

    // --- Samenstellen. ---

    fn samenstellen(mut self) -> Geschreven {
        let m = self.m;
        let ons = &m[O].bestand;
        let n_o = ons.instanties.len();
        let eerste_nieuw = ons.hoogste + 1;
        self.nieuw.sort_unstable_by_key(|x| x.0);
        let knoop_van = |n: u64| -> Option<usize> {
            if n >= eerste_nieuw {
                let x = (n - eerste_nieuw) as usize;
                return Some(n_o + x);
            }
            ons.index.get(&n).map(|&k| k as usize)
        };
        let refs_van = |tekst: &[u8]| -> Vec<usize> {
            Tokens::nieuw(tekst)
                .filter_map(|(_, t)| match t {
                    Token::Verwijzing(n) => knoop_van(n),
                    _ => None,
                })
                .collect()
        };
        // Wat in ons bereikbaar was (vanaf objecten en losse knopen).
        let mut bereik_ons = vec![false; n_o];
        let mut stapel: Vec<usize> =
            m[O].objecten.iter().map(|o| o.inst as usize).chain(m[O].wortels.iter().map(|&w| w as usize)).collect();
        while let Some(x) = stapel.pop() {
            if std::mem::replace(&mut bereik_ons[x], true) {
                continue;
            }
            stapel.extend(ons.verwijzingen(x).iter().map(|&c| c as usize).filter(|&c| !bereik_ons[c]));
        }
        // Wat in het resultaat bereikbaar is.
        let totaal = n_o + self.nieuw.len();
        let mut bereikt = vec![false; totaal];
        let mut stapel: Vec<usize> = Vec::new();
        for o in &m[O].objecten {
            if !self.weg[o.inst as usize] {
                stapel.push(o.inst as usize);
            }
        }
        for &w in &m[O].wortels {
            if !self.weg[w as usize] {
                stapel.push(w as usize);
            }
        }
        stapel.extend(n_o..totaal);
        while let Some(x) = stapel.pop() {
            if x >= totaal || std::mem::replace(&mut bereikt[x], true) {
                continue;
            }
            if x >= n_o {
                stapel.extend(refs_van(&self.nieuw[x - n_o].1));
            } else if let Some(t) = self.vervangen.get(&(x as u32)) {
                stapel.extend(refs_van(t));
            } else {
                stapel.extend(ons.verwijzingen(x).iter().map(|&c| c as usize));
            }
        }
        let verwijder: Vec<bool> = (0..n_o).map(|i| self.weg[i] || (bereik_ons[i] && !bereikt[i])).collect();

        // De wijzigingen in de tekst van ons, op volgorde.
        let nl: &[u8] = if ons.crlf() { b"\r\n" } else { b"\n" };
        let data = ons.data;
        let mut wijzigingen: Vec<(usize, usize, Vec<u8>)> = Vec::new();
        let mut verwijderd = 0;
        let mut herschreven = 0;
        for (i, inst) in ons.instanties.iter().enumerate() {
            let (b, e) = (inst.bereik.0 as usize, inst.bereik.1 as usize);
            if verwijder[i] {
                verwijderd += 1;
                wijzigingen.push((regel_begin(data, b), regel_eind(data, b, e), Vec::new()));
            } else if let Some(t) = self.vervangen.get(&(i as u32)) {
                if t.as_slice() == inhoud_zonder_puntkomma(&m[O], i) {
                    continue;
                }
                herschreven += 1;
                let mut tekst = format!("#{}=", inst.id).into_bytes();
                tekst.extend_from_slice(t);
                tekst.push(b';');
                wijzigingen.push((b, e, tekst));
            }
        }
        for (&(regel, j), &kant) in &self.plan.header {
            let Some(r) = ons.header.iter().find(|r| r.naam == regel) else { continue };
            let Some(&(b, e)) = r.parameters.get(j) else { continue };
            let Some((_, tekst)) = m[kant].header_parameter(regel, j) else { continue };
            wijzigingen.push((b as usize, e as usize, tekst.to_vec()));
        }
        let mut erbij = Vec::new();
        if !self.nieuw.is_empty() {
            let voor = &data[..ons.data_einde];
            if !voor.ends_with(b"\n") {
                erbij.extend_from_slice(nl);
            }
            for (n, t) in &self.nieuw {
                let _ = write!(erbij, "#{n}=");
                erbij.extend_from_slice(t);
                erbij.push(b';');
                erbij.extend_from_slice(nl);
            }
            wijzigingen.push((ons.data_einde, ons.data_einde, erbij));
        }
        wijzigingen.sort_by_key(|w| (w.0, w.1));
        let mut inhoud = Vec::with_capacity(data.len() + 1024);
        let mut vorige = 0;
        for (b, e, tekst) in wijzigingen {
            let b = b.max(vorige);
            inhoud.extend_from_slice(&data[vorige..b]);
            inhoud.extend_from_slice(&tekst);
            vorige = e.max(b);
        }
        inhoud.extend_from_slice(&data[vorige..]);

        // GlobalId in het resultaat -> sleutel.
        let mut alias = HashMap::new();
        for sl in &self.plan.sleutels {
            let globalid = match (&sl.besluit, sl.obj[O], sl.obj[H]) {
                (Besluit::Weg, _, _) => continue,
                (_, Some(o), _) => &m[O].objecten[o].globalid,
                (_, None, Some(o)) => &m[H].objecten[o].globalid,
                _ => continue,
            };
            alias.insert(globalid.clone(), sl.sleutel.clone());
        }
        Geschreven { inhoud, alias, nieuw: self.nieuw.len(), herschreven, verwijderd }
    }
}

/// Het begin van de regel als er alleen witruimte voor de instantie staat; anders het begin van de instantie.
fn regel_begin(data: &[u8], b: usize) -> usize {
    let mut x = b;
    while x > 0 && (data[x - 1] == b' ' || data[x - 1] == b'\t') {
        x -= 1;
    }
    if x == 0 || data[x - 1] == b'\n' {
        x
    } else {
        b
    }
}

/// Het einde van de regel (na het regeleinde) als er alleen witruimte na de instantie staat; anders het einde van de
/// instantie.
fn regel_eind(data: &[u8], b: usize, e: usize) -> usize {
    if regel_begin(data, b) == b && b > 0 && data[b - 1] != b'\n' {
        return e;
    }
    let mut x = e;
    while x < data.len() && (data[x] == b' ' || data[x] == b'\t' || data[x] == b'\r') {
        x += 1;
    }
    if x < data.len() && data[x] == b'\n' {
        x + 1
    } else if x == data.len() {
        x
    } else {
        e
    }
}
