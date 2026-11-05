//! Een eigen, kleine STEP-lezer en -schrijver voor de eigenschappentests. Hij deelt geen code met de bibliotheek.
//! Alleen de schematabellen (`src/schema/*.txt`, data uit de EXPRESS-schema's van buildingSMART) lezen we mee: welk
//! type een object is (IfcRoot), welk attribuut een SET of BAG is, en de namen van de attributen.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

/// Een waarde zoals ze in de tekst staat.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum W {
    /// `$`, `*`, een getal, een tekst (met aanhalingstekens), een opsomming of binair, letterlijk.
    Atoom(String),
    Ref(u64),
    Lijst(Vec<W>),
    /// Een getypte waarde, bijv. `IFCLABEL('x')`: het type in hoofdletters en de waarden tussen de haakjes.
    Getypt(String, Vec<W>),
}

impl W {
    fn schrijf(&self, uit: &mut String) {
        match self {
            W::Atoom(a) => uit.push_str(a),
            W::Ref(n) => {
                uit.push('#');
                uit.push_str(&n.to_string());
            }
            W::Lijst(l) => schrijf_lijst(l, uit),
            W::Getypt(t, l) => {
                uit.push_str(t);
                schrijf_lijst(l, uit);
            }
        }
    }

    /// Alle `#n` in deze waarde.
    pub fn refs(&self, f: &mut impl FnMut(u64)) {
        match self {
            W::Ref(n) => f(*n),
            W::Lijst(l) | W::Getypt(_, l) => l.iter().for_each(|w| w.refs(f)),
            W::Atoom(_) => {}
        }
    }

    pub fn refs_mut(&mut self, f: &mut impl FnMut(&mut u64)) {
        match self {
            W::Ref(n) => f(n),
            W::Lijst(l) | W::Getypt(_, l) => l.iter_mut().for_each(|w| w.refs_mut(f)),
            W::Atoom(_) => {}
        }
    }

    pub fn is_tekst(&self) -> bool {
        matches!(self, W::Atoom(a) if a.starts_with('\''))
    }

    pub fn is_getal(&self) -> bool {
        matches!(self, W::Atoom(a) if is_getal(a))
    }
}

pub fn is_getal(a: &str) -> bool {
    let b = a.as_bytes();
    match b.first() {
        Some(c) if c.is_ascii_digit() || *c == b'-' || *c == b'+' => true,
        Some(b'.') => b.get(1).is_some_and(|c| c.is_ascii_digit()),
        _ => false,
    }
}

fn schrijf_lijst(l: &[W], uit: &mut String) {
    uit.push('(');
    for (i, w) in l.iter().enumerate() {
        if i > 0 {
            uit.push(',');
        }
        w.schrijf(uit);
    }
    uit.push(')');
}

/// Een instantie: het type in hoofdletters en de attributen. Bij een complexe instantie (`#n=(A()B());`) is het type
/// leeg en zijn de attributen de delen, elk als getypte waarde.
#[derive(Clone, Debug, PartialEq)]
pub struct Inst {
    pub ty: String,
    pub attrs: Vec<W>,
}

impl Inst {
    pub fn refs(&self, f: &mut impl FnMut(u64)) {
        self.attrs.iter().for_each(|w| w.refs(f));
    }
}

/// Een gelezen IFC-bestand.
#[derive(Clone)]
pub struct Stap {
    /// Alles tot en met `DATA;`.
    voor: String,
    pub inst: BTreeMap<u64, Inst>,
    /// Alles vanaf de `ENDSEC;` van de DATA-sectie.
    na: String,
    /// De HEADER: naam en parameters.
    pub kop: Vec<(String, Vec<W>)>,
    pub schema: &'static Schema,
}

struct Lezer<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Lezer<'a> {
    fn wit(&mut self) {
        loop {
            while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.b[self.i..].starts_with(b"/*") {
                let rest = &self.b[self.i + 2..];
                self.i = match rest.windows(2).position(|w| w == b"*/") {
                    Some(p) => self.i + 2 + p + 2,
                    None => self.b.len(),
                };
            } else {
                return;
            }
        }
    }

    fn kijk(&self) -> u8 {
        self.b.get(self.i).copied().unwrap_or(0)
    }

    fn stuk(&self, van: usize) -> String {
        String::from_utf8(self.b[van..self.i].to_vec()).unwrap()
    }

    fn woord(&mut self) -> String {
        let s = self.i;
        while self.i < self.b.len() && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'_') {
            self.i += 1;
        }
        self.stuk(s).to_ascii_uppercase()
    }

    fn getal(&mut self) -> u64 {
        let s = self.i;
        while self.kijk().is_ascii_digit() {
            self.i += 1;
        }
        self.stuk(s).parse().unwrap_or_else(|_| panic!("geen nummer bij byte {s}"))
    }

    fn waarde(&mut self) -> W {
        self.wit();
        let s = self.i;
        match self.kijk() {
            b'(' => W::Lijst(self.lijst()),
            b'#' => {
                self.i += 1;
                W::Ref(self.getal())
            }
            b'\'' => {
                self.i += 1;
                loop {
                    match self.kijk() {
                        b'\'' if self.b.get(self.i + 1) == Some(&b'\'') => self.i += 2,
                        b'\'' => {
                            self.i += 1;
                            break;
                        }
                        0 => panic!("tekst zonder einde bij byte {s}"),
                        _ => self.i += 1,
                    }
                }
                W::Atoom(self.stuk(s))
            }
            b'"' => {
                self.i += 1;
                while self.kijk() != b'"' {
                    self.i += 1;
                }
                self.i += 1;
                W::Atoom(self.stuk(s).to_ascii_uppercase())
            }
            b'$' | b'*' => {
                self.i += 1;
                W::Atoom(self.stuk(s))
            }
            b'.' if self.b.get(self.i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') => {
                self.i += 1;
                while self.kijk() != b'.' {
                    self.i += 1;
                }
                self.i += 1;
                W::Atoom(self.stuk(s).to_ascii_uppercase())
            }
            c if c.is_ascii_alphabetic() => {
                let w = self.woord();
                W::Getypt(w, self.lijst())
            }
            _ => {
                while matches!(self.kijk(), b'0'..=b'9' | b'+' | b'-' | b'.' | b'E' | b'e') {
                    self.i += 1;
                }
                assert!(self.i > s, "onverwacht teken {:?} bij byte {s}", self.kijk() as char);
                W::Atoom(self.stuk(s))
            }
        }
    }

    fn lijst(&mut self) -> Vec<W> {
        self.wit();
        assert_eq!(self.kijk(), b'(', "verwacht ( bij byte {}", self.i);
        self.i += 1;
        let mut uit = Vec::new();
        self.wit();
        if self.kijk() == b')' {
            self.i += 1;
            return uit;
        }
        loop {
            uit.push(self.waarde());
            self.wit();
            match self.kijk() {
                b',' => self.i += 1,
                b')' => {
                    self.i += 1;
                    return uit;
                }
                c => panic!("verwacht , of ) bij byte {}, niet {:?}", self.i, c as char),
            }
        }
    }

    fn punt_komma(&mut self) {
        self.wit();
        assert_eq!(self.kijk(), b';', "verwacht ; bij byte {}", self.i);
        self.i += 1;
    }
}

impl Stap {
    pub fn lees(tekst: &str) -> Stap {
        let b = tekst.as_bytes();
        let kop_begin = tekst.find("HEADER;").expect("HEADER;") + "HEADER;".len();
        let data = tekst.find("DATA;").expect("DATA;") + "DATA;".len();
        let mut kop = Vec::new();
        let mut l = Lezer { b: &b[..data], i: kop_begin };
        loop {
            l.wit();
            if l.b[l.i..].starts_with(b"ENDSEC") {
                break;
            }
            let naam = l.woord();
            let args = l.lijst();
            l.punt_komma();
            kop.push((naam, args));
        }
        let mut l = Lezer { b, i: data };
        let mut inst = BTreeMap::new();
        let eind = loop {
            l.wit();
            if l.b[l.i..].starts_with(b"ENDSEC") {
                break l.i;
            }
            assert_eq!(l.kijk(), b'#', "verwacht # bij byte {}", l.i);
            l.i += 1;
            let n = l.getal();
            l.wit();
            assert_eq!(l.kijk(), b'=');
            l.i += 1;
            l.wit();
            let i = if l.kijk() == b'(' {
                l.i += 1;
                let mut delen = Vec::new();
                loop {
                    l.wit();
                    if l.kijk() == b')' {
                        l.i += 1;
                        break;
                    }
                    let w = l.woord();
                    delen.push(W::Getypt(w, l.lijst()));
                }
                Inst { ty: String::new(), attrs: delen }
            } else {
                let ty = l.woord();
                Inst { ty, attrs: l.lijst() }
            };
            l.punt_komma();
            assert!(inst.insert(n, i).is_none(), "#{n} komt twee keer voor");
        };
        let schema_naam = kop
            .iter()
            .find(|(n, _)| n == "FILE_SCHEMA")
            .and_then(|(_, a)| match a.first() {
                Some(W::Lijst(l)) => l.first().cloned(),
                _ => None,
            })
            .and_then(|w| match w {
                W::Atoom(a) => Some(a.trim_matches('\'').to_ascii_uppercase()),
                _ => None,
            })
            .expect("FILE_SCHEMA");
        Stap { voor: tekst[..data].to_string(), inst, na: tekst[eind..].to_string(), kop, schema: schema_van(&schema_naam) }
    }

    pub fn schrijf(&self) -> String {
        let mut s = String::with_capacity(self.voor.len() + self.na.len() + self.inst.len() * 64);
        s.push_str(&self.voor);
        s.push('\n');
        for (n, i) in &self.inst {
            s.push('#');
            s.push_str(&n.to_string());
            s.push('=');
            if i.ty.is_empty() {
                s.push('(');
                for d in &i.attrs {
                    d.schrijf(&mut s);
                }
                s.push(')');
            } else {
                s.push_str(&i.ty);
                schrijf_lijst(&i.attrs, &mut s);
            }
            s.push_str(";\n");
        }
        s.push_str(&self.na);
        s
    }

    pub fn ent(&self, n: u64) -> Option<&'static Entiteit> {
        self.inst.get(&n).and_then(|i| self.schema.entiteiten.get(&i.ty))
    }

    /// De GlobalId van `#n` als het een object is.
    pub fn gid(&self, n: u64) -> Option<String> {
        let i = self.inst.get(&n)?;
        if !self.schema.entiteiten.get(&i.ty)?.object {
            return None;
        }
        match i.attrs.first() {
            Some(W::Atoom(a)) if a.starts_with('\'') => Some(a.trim_matches('\'').to_string()),
            _ => None,
        }
    }

    pub fn is_object(&self, n: u64) -> bool {
        self.gid(n).is_some()
    }

    pub fn is_knoop(&self, n: u64) -> bool {
        self.inst.contains_key(&n) && !self.is_object(n)
    }

    /// Alle objecten: GlobalId en nummer, op GlobalId.
    pub fn objecten(&self) -> BTreeMap<String, u64> {
        self.inst.keys().filter_map(|&n| self.gid(n).map(|g| (g, n))).collect()
    }

    /// De plek van attribuut `naam` in instantie `#n`.
    pub fn plek(&self, n: u64, naam: &str) -> Option<usize> {
        self.ent(n)?.attrs.iter().position(|(a, _)| a == naam)
    }

    /// De soort van attribuut `j` van `#n`: `S`, `B`, `L`, `A` of `' '` (geen aggregaat).
    pub fn soort(&self, n: u64, j: usize) -> char {
        self.ent(n).and_then(|e| e.attrs.get(j)).map_or(' ', |a| a.1)
    }

    pub fn is_set(&self, n: u64, j: usize) -> bool {
        matches!(self.soort(n, j), 'S' | 'B')
    }

    pub fn nieuw_nummer(&self) -> u64 {
        self.inst.keys().next_back().map_or(1, |n| n + 1)
    }

    /// Per nummer: welke instanties ernaar verwijzen, en in welk attribuut (zonder dubbele).
    pub fn verwijzers(&self) -> HashMap<u64, Vec<(u64, usize)>> {
        let mut uit: HashMap<u64, Vec<(u64, usize)>> = HashMap::new();
        for (&n, i) in &self.inst {
            for (j, a) in i.attrs.iter().enumerate() {
                a.refs(&mut |k| {
                    let v = uit.entry(k).or_default();
                    if !v.contains(&(n, j)) {
                        v.push((n, j));
                    }
                });
            }
        }
        uit
    }

    /// Per knoop: de GlobalIds van de objecten die er rechtstreeks (in een attribuut of een lijst) naar verwijzen.
    pub fn directe_objecten(&self) -> HashMap<u64, BTreeSet<String>> {
        let mut uit: HashMap<u64, BTreeSet<String>> = HashMap::new();
        for (g, &n) in &self.objecten() {
            for a in &self.inst[&n].attrs {
                let mut direct = |w: &W| {
                    if let W::Ref(k) = w {
                        if self.is_knoop(*k) {
                            uit.entry(*k).or_default().insert(g.clone());
                        }
                    }
                };
                match a {
                    W::Lijst(l) => l.iter().for_each(&mut direct),
                    w => direct(w),
                }
            }
        }
        uit
    }

    /// Nummers waar niets naar verwijst, terwijl er wel iets naar verwijst.
    pub fn losse_verwijzingen(&self) -> Vec<String> {
        let mut uit = Vec::new();
        for (n, i) in &self.inst {
            i.refs(&mut |k| {
                if !self.inst.contains_key(&k) {
                    uit.push(format!("#{n} verwijst naar #{k}, dat er niet is"));
                }
            });
        }
        uit
    }
}

// --- De schematabellen. ---

pub struct Entiteit {
    pub object: bool,
    /// Naam en soort van elk attribuut (`S`, `B`, `L`, `A`, of `' '` als het geen aggregaat is).
    pub attrs: Vec<(String, char)>,
    /// Het type zelf en zijn supertypes, in hoofdletters.
    pub voorouders: Vec<String>,
}

pub struct Schema {
    pub entiteiten: HashMap<String, Entiteit>,
    /// TYPE's die een SET zijn, bijv. IFCPROPERTYSETDEFINITIONSET.
    pub set_typen: HashSet<String>,
}

impl Schema {
    pub fn is(&self, ty: &str, voorouder: &str) -> bool {
        self.entiteiten.get(ty).is_some_and(|e| e.voorouders.iter().any(|v| v == voorouder))
    }
}

fn laad(tekst: &str) -> Schema {
    let mut supers = HashMap::new();
    let mut entiteiten = HashMap::new();
    let mut set_typen = HashSet::new();
    for regel in tekst.lines() {
        if let Some(t) = regel.strip_prefix('~') {
            set_typen.extend(t.split(',').filter(|t| !t.is_empty()).map(|t| t.to_ascii_uppercase()));
            continue;
        }
        if regel.starts_with('!') || regel.is_empty() {
            continue;
        }
        let d: Vec<&str> = regel.split('|').collect();
        let naam = d[0].to_ascii_uppercase();
        let attrs = d[3]
            .split(',')
            .filter(|a| !a.is_empty())
            .map(|a| {
                let (n, s) = a.split_once(':').unwrap_or((a, ""));
                (n.trim_end_matches('?').to_string(), s.chars().next().unwrap_or(' '))
            })
            .collect();
        supers.insert(naam.clone(), d[1].to_ascii_uppercase());
        entiteiten.insert(naam, Entiteit { object: d[2].contains('R'), attrs, voorouders: Vec::new() });
    }
    for (naam, e) in entiteiten.iter_mut() {
        let mut t = naam.clone();
        while t != "-" {
            e.voorouders.push(t.clone());
            t = supers.get(&t).cloned().unwrap_or_else(|| "-".into());
        }
    }
    Schema { entiteiten, set_typen }
}

pub fn schema_van(naam: &str) -> &'static Schema {
    static S: OnceLock<[Schema; 3]> = OnceLock::new();
    let s = S.get_or_init(|| {
        [
            laad(include_str!("../../src/schema/ifc2x3.txt")),
            laad(include_str!("../../src/schema/ifc4.txt")),
            laad(include_str!("../../src/schema/ifc4x3_add2.txt")),
        ]
    });
    if naam.starts_with("IFC2X3") {
        &s[0]
    } else if naam.starts_with("IFC4X3") {
        &s[2]
    } else if naam.starts_with("IFC4") {
        &s[1]
    } else {
        panic!("onbekend schema {naam}")
    }
}
