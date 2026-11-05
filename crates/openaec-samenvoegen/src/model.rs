//! Het objectmodel van een IFC-bestand.
//!
//! Een *object* is een instantie van IfcRoot of een subtype ervan; zijn sleutel is de GlobalId. Al het andere is een
//! *knoop* zonder eigen identiteit (geometrie, plaatsing, eigenschapswaarden, IfcOwnerHistory, eenheden...).
//!
//! Elke knoop krijgt een inhoudshash (BLAKE3, Merkle): het type en de attributen, waarbij een verwijzing naar een
//! knoop de hash van die knoop wordt en een verwijzing naar een object de sleutel van dat object. Verwijzingen naar
//! IfcOwnerHistory tellen niet mee (die verandert bij elk opslaan). Getallen vergelijken we als getal (`1.`, `1.0`
//! en `1.E0` zijn gelijk) en teksten na het decoderen. De nummers `#n` doen nergens mee.
//!
//! Een object krijgt een hash per attribuut (zonder de GlobalId) en een hash over het type en die attribuuthashes.
//!
//! Knopen waar niets naar verwijst, heten *losse knopen* (bijv. IfcStyledItem of IfcPresentationLayerAssignment):
//! ze horen bij geen object, maar dragen wel inhoud. Ze krijgen een eigen sleutel: het type, de naam en wat ze
//! met één verwijzing aanwijzen.
//!
//! Een model bouwen gaat in twee stappen: [`Model::lees`] deelt de instanties in, [`Model::bereken`] rekent de
//! hashes uit. Daartussen kan [`koppel_relaties`] de sleutels van relaties aanpassen.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::ontleden::{self, elementen, Bereik, Bestand, Token, Tokens};
use crate::schema::{self, Entiteit};
use crate::tekst::{decodeer, kort};
use crate::Fout;

pub(crate) type Hash = [u8; 32];

/// Een element van een lijst van verwijzingen: de identiteit (`G` + sleutel van het object, of `K` + hash van de
/// knoop) en het bereik in de tekst van de lijst.
pub(crate) type Element = (Vec<u8>, (usize, usize));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Soort {
    /// Een object; de index in [`Model::objecten`].
    Object(u32),
    Knoop,
    /// Een IfcOwnerHistory: een knoop, maar verwijzingen ernaar tellen niet mee.
    Eigenaar,
}

pub(crate) struct Object {
    pub inst: u32,
    pub globalid: String,
    /// Sleutel waaronder het object samengevoegd wordt: de GlobalId, of de GlobalId in de basis van een gekoppelde
    /// relatie ([`koppel_relaties`]).
    pub sleutel: String,
    pub hash: Hash,
}

/// Cijfers over één versie.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct VersieStatistiek {
    /// De naam uit FILE_SCHEMA.
    pub schema: String,
    /// De schematabel die we gebruiken (IFC2X3, IFC4 of IFC4X3_ADD2); leeg als we het schema niet kennen.
    pub schema_tabel: String,
    pub instanties: usize,
    pub objecten: usize,
    pub knopen: usize,
    /// Knopen waar niets naar verwijst (bijv. IfcStyledItem).
    pub losse_knopen: usize,
    /// Types die niet in het schema staan, met het aantal instanties.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub onbekende_typen: BTreeMap<String, usize>,
    /// Instanties van een bekend type met een ander aantal attributen dan het schema zegt.
    pub afwijkend_aantal_attributen: usize,
    /// Objecten herkend aan een GlobalId als eerste attribuut, omdat het type onbekend is.
    pub objecten_via_heuristiek: usize,
    /// Instanties van een IfcRoot-type zonder GlobalId (behandeld als knoop).
    pub root_zonder_globalid: usize,
    /// Knopen die (via andere knopen) naar zichzelf verwijzen.
    pub cycli: usize,
}

pub(crate) struct Model<'a> {
    pub bestand: Bestand<'a>,
    type_entiteit: Vec<Option<&'static Entiteit>>,
    pub soort: Vec<Soort>,
    pub objecten: Vec<Object>,
    pub op_globalid: HashMap<String, u32>,
    /// Inhoudshash per instantie (alleen voor knopen; na [`Model::bereken`]).
    pub knoop_hash: Vec<Hash>,
    /// Losse knopen (instantie-indexen), met hun sleutel (na [`Model::bereken`]).
    pub wortels: Vec<u32>,
    pub wortel_sleutel: Vec<Hash>,
    pub statistiek: VersieStatistiek,
}

const NUL: Hash = [0; 32];

impl<'a> Model<'a> {
    /// Leest het bestand en deelt de instanties in objecten en knopen in.
    pub fn lees(data: &'a [u8]) -> Result<Self, Fout> {
        let bestand = ontleden::lees(data)?;
        let schema = schema::kies(&bestand.schema);
        let type_entiteit: Vec<Option<&'static Entiteit>> = bestand
            .typen
            .iter()
            .map(|naam| {
                if naam.contains('+') {
                    None
                } else {
                    schema.and_then(|s| s.entiteit(naam))
                }
            })
            .collect();
        let mut statistiek = VersieStatistiek {
            schema: bestand.schema.clone(),
            schema_tabel: schema.map(|s| s.naam.to_string()).unwrap_or_default(),
            instanties: bestand.instanties.len(),
            ..Default::default()
        };
        let mut soort = Vec::with_capacity(bestand.instanties.len());
        let mut objecten = Vec::new();
        let mut op_globalid = HashMap::new();
        for (i, inst) in bestand.instanties.iter().enumerate() {
            let naam = &bestand.typen[inst.type_id as usize];
            let attrs = bestand.attributen(i);
            let entiteit = type_entiteit[inst.type_id as usize];
            let bekend = match entiteit {
                Some(e) if e.attributen.len() == attrs.len() => Some(e),
                Some(_) => {
                    statistiek.afwijkend_aantal_attributen += 1;
                    None
                }
                None => {
                    *statistiek.onbekende_typen.entry(naam.clone()).or_default() += 1;
                    None
                }
            };
            let eerste = attrs.first().map(|&r| eerste_token(bestand.tekst(r)));
            let globalid = match (bekend, eerste) {
                (Some(e), Some(Some(Token::Tekst(t)))) if e.root => Some(decodeer(t)),
                (Some(e), _) if e.root => {
                    statistiek.root_zonder_globalid += 1;
                    None
                }
                (None, Some(Some(Token::Tekst(t)))) if !inst.complex && schema::is_globalid(t) => {
                    statistiek.objecten_via_heuristiek += 1;
                    Some(decodeer(t))
                }
                _ => None,
            };
            let s = match globalid {
                Some(globalid) => {
                    let o = objecten.len() as u32;
                    if op_globalid.insert(globalid.clone(), o).is_some() {
                        return Err(Fout::DubbeleGlobalId {
                            globalid,
                            regel: bestand.regel(inst.bereik.0 as usize),
                        });
                    }
                    objecten.push(Object {
                        inst: i as u32,
                        sleutel: globalid.clone(),
                        globalid,
                        hash: NUL,
                    });
                    Soort::Object(o)
                }
                None if naam == "IFCOWNERHISTORY" => Soort::Eigenaar,
                None => Soort::Knoop,
            };
            soort.push(s);
        }
        // Losse knopen: knopen waar geen enkele instantie naar verwijst.
        let mut heeft_ouder = vec![false; bestand.instanties.len()];
        for &doel in &bestand.verwijzingen {
            heeft_ouder[doel as usize] = true;
        }
        let wortels: Vec<u32> = (0..bestand.instanties.len())
            .filter(|&i| !heeft_ouder[i] && !matches!(soort[i], Soort::Object(_)))
            .map(|i| i as u32)
            .collect();
        statistiek.objecten = objecten.len();
        statistiek.knopen = bestand.instanties.len() - objecten.len();
        statistiek.losse_knopen = wortels.len();
        Ok(Model {
            knoop_hash: Vec::new(),
            bestand,
            type_entiteit,
            soort,
            objecten,
            op_globalid,
            wortels,
            wortel_sleutel: Vec::new(),
            statistiek,
        })
    }

    /// De schema-entiteit van een instantie, als het type bekend is en het aantal attributen klopt.
    pub fn entiteit(&self, i: usize) -> Option<&'static Entiteit> {
        let inst = &self.bestand.instanties[i];
        self.type_entiteit[inst.type_id as usize].filter(|e| e.attributen.len() == self.bestand.attributen(i).len())
    }

    /// Rekent de hashes van knopen, objecten en losse knopen uit.
    pub fn bereken(&mut self) {
        let n = self.bestand.instanties.len();
        let mut hashes = vec![NUL; n];
        // 0 = nog niet bezocht, 1 = bezig (op de stapel), 2 = klaar.
        let mut status = vec![0u8; n];
        let mut stapel: Vec<(u32, u32)> = Vec::new();
        let mut buffer = Vec::new();
        let mut cycli = 0;
        for start in 0..n {
            if status[start] != 0 || matches!(self.soort[start], Soort::Object(_)) {
                continue;
            }
            status[start] = 1;
            stapel.push((start as u32, 0));
            while let Some(&mut (knoop, ref mut volgende)) = stapel.last_mut() {
                let refs = self.bestand.verwijzingen(knoop as usize);
                if (*volgende as usize) < refs.len() {
                    let kind = refs[*volgende as usize] as usize;
                    *volgende += 1;
                    if matches!(self.soort[kind], Soort::Object(_)) {
                        continue;
                    }
                    match status[kind] {
                        0 => {
                            status[kind] = 1;
                            stapel.push((kind as u32, 0));
                        }
                        1 => cycli += 1,
                        _ => {}
                    }
                    continue;
                }
                stapel.pop();
                let k = knoop as usize;
                buffer.clear();
                let canon = Canon {
                    model: self,
                    hashes: &hashes,
                    status: &status,
                };
                canon.instantie(k, &mut buffer);
                hashes[k] = *blake3::hash(&buffer).as_bytes();
                status[k] = 2;
            }
        }
        self.knoop_hash = hashes;
        self.statistiek.cycli = cycli;

        let mut objecthashes = Vec::with_capacity(self.objecten.len());
        for o in &self.objecten {
            objecthashes.push(self.object_hash(o.inst as usize, &mut buffer));
        }
        for (o, h) in self.objecten.iter_mut().zip(objecthashes) {
            o.hash = h;
        }

        let mut sleutels = Vec::with_capacity(self.wortels.len());
        for &w in &self.wortels {
            sleutels.push(self.wortel_sleutel_van(w as usize, &mut buffer));
        }
        self.wortel_sleutel = sleutels;
    }

    fn canon(&self) -> Canon<'_, 'a> {
        Canon {
            model: self,
            hashes: &self.knoop_hash,
            status: &[],
        }
    }

    /// De canonieke vorm van een stuk tekst van dit bestand (na [`Model::bereken`]).
    pub fn canoniek(&self, tekst: &[u8], uit: &mut Vec<u8>) {
        self.canon().waarde(tekst, uit);
    }

    /// De hash van een waarde (een attribuut of een element van een lijst).
    pub fn waarde_hash(&self, tekst: &[u8]) -> Hash {
        let mut buffer = Vec::new();
        self.canoniek(tekst, &mut buffer);
        *blake3::hash(&buffer).as_bytes()
    }

    pub fn attribuut_hash(&self, i: usize, j: usize) -> Hash {
        self.waarde_hash(self.bestand.tekst(self.bestand.attributen(i)[j]))
    }

    /// Hash van een object: het type en de hashes van alle attributen behalve de GlobalId.
    fn object_hash(&self, i: usize, buffer: &mut Vec<u8>) -> Hash {
        let hashes: Vec<Hash> = self
            .bestand
            .attributen(i)
            .iter()
            .skip(1)
            .map(|&attr| {
                buffer.clear();
                self.canoniek(self.bestand.tekst(attr), buffer);
                *blake3::hash(buffer).as_bytes()
            })
            .collect();
        Self::object_hash_uit(self.bestand.type_naam(i), &hashes)
    }

    /// Hash van een object uit zijn type en de hashes van zijn attributen (zonder de GlobalId).
    pub fn object_hash_uit(type_naam: &str, attribuut_hashes: &[Hash]) -> Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"T");
        hasher.update(&(type_naam.len() as u32).to_le_bytes());
        hasher.update(type_naam.as_bytes());
        for h in attribuut_hashes {
            hasher.update(h);
        }
        *hasher.finalize().as_bytes()
    }

    /// Sleutel van een losse knoop: het type, het attribuut Name en de attributen met één verwijzing.
    fn wortel_sleutel_van(&self, i: usize, buffer: &mut Vec<u8>) -> Hash {
        buffer.clear();
        let naam = self.bestand.type_naam(i);
        buffer.extend_from_slice(&(naam.len() as u32).to_le_bytes());
        buffer.extend_from_slice(naam.as_bytes());
        let entiteit = self.entiteit(i);
        for (j, &attr) in self.bestand.attributen(i).iter().enumerate() {
            let tekst = self.bestand.tekst(attr);
            let is_naam = entiteit.is_some_and(|e| e.attributen[j].naam == "Name");
            if is_naam || enkele_verwijzing(tekst).is_some() {
                buffer.extend_from_slice(&(j as u32).to_le_bytes());
                let mut deel = Vec::new();
                self.canoniek(tekst, &mut deel);
                buffer.extend_from_slice(&(deel.len() as u32).to_le_bytes());
                buffer.extend_from_slice(&deel);
            }
        }
        *blake3::hash(buffer).as_bytes()
    }

    /// Is deze instantie een relatie (IfcRelationship, of bij een onbekend type: een naam die met IFCREL begint)?
    pub fn is_relatie(&self, i: usize) -> bool {
        match self.entiteit(i) {
            Some(e) => e.relatie,
            None => self.bestand.type_naam(i).starts_with("IFCREL"),
        }
    }

    /// De natuurlijke sleutel van een relatie: het type en de GlobalIds in de attributen met één verwijzing naar een
    /// object. None als het geen relatie is of als er geen zo'n attribuut is.
    pub fn natuurlijke_sleutel(&self, o: usize) -> Option<Hash> {
        let i = self.objecten[o].inst as usize;
        if !self.is_relatie(i) {
            return None;
        }
        let mut hasher = blake3::Hasher::new();
        let naam = self.bestand.type_naam(i);
        hasher.update(&(naam.len() as u32).to_le_bytes());
        hasher.update(naam.as_bytes());
        let mut gevonden = false;
        for (j, &attr) in self.bestand.attributen(i).iter().enumerate().skip(1) {
            let Some(doel) = enkele_verwijzing(self.bestand.tekst(attr)) else {
                continue;
            };
            let Some(&k) = self.bestand.index.get(&doel) else {
                continue;
            };
            if let Soort::Object(d) = self.soort[k as usize] {
                let globalid = &self.objecten[d as usize].globalid;
                hasher.update(&(j as u32).to_le_bytes());
                hasher.update(&(globalid.len() as u32).to_le_bytes());
                hasher.update(globalid.as_bytes());
                gevonden = true;
            }
        }
        gevonden.then(|| *hasher.finalize().as_bytes())
    }

    /// Het type zoals een mens het leest: de naam uit het schema (`IfcTask`), anders de naam uit het bestand.
    pub fn type_weergave(&self, i: usize) -> String {
        match self.type_entiteit[self.bestand.instanties[i].type_id as usize] {
            Some(e) => e.naam.clone(),
            None => self.bestand.type_naam(i).to_string(),
        }
    }

    /// De naam van een attribuut: uit het schema, anders de positie.
    pub fn attribuut_naam(&self, i: usize, j: usize) -> String {
        match self.entiteit(i) {
            Some(e) => e.attributen[j].naam.clone(),
            None => format!("positie {}", j + 1),
        }
    }

    /// Index van het attribuut Name van een object (bij een onbekend type: de plek van IfcRoot.Name).
    fn naam_index(&self, i: usize) -> Option<usize> {
        match self.entiteit(i) {
            Some(e) => e.attributen.iter().position(|a| a.naam == "Name"),
            None => Some(2),
        }
    }

    /// Het attribuut Name van een object, als het een tekst is.
    pub fn object_naam(&self, i: usize) -> Option<String> {
        let j = self.naam_index(i)?;
        let attr = *self.bestand.attributen(i).get(j)?;
        match eerste_token(self.bestand.tekst(attr)) {
            Some(Token::Tekst(t)) => Some(decodeer(t)),
            _ => None,
        }
    }

    /// Een korte leesbare tekst van een waarde: teksten gedecodeerd, objecten als naam en GlobalId, knopen een
    /// paar niveaus diep uitgeschreven.
    pub fn toon(&self, tekst: &[u8], max: usize) -> String {
        let mut uit = String::new();
        self.toon_in(tekst, max, 2, &mut uit);
        kort(&uit, max)
    }

    fn toon_in(&self, tekst: &[u8], max: usize, diepte: usize, uit: &mut String) {
        let mut vorige_was_waarde = false;
        for (_, token) in Tokens::nieuw(tekst) {
            if uit.len() > max * 4 {
                return;
            }
            let is_sluit = token == Token::Sluit;
            if vorige_was_waarde && !is_sluit {
                uit.push_str(", ");
            }
            match token {
                Token::Open => uit.push('('),
                Token::Sluit => uit.push(')'),
                Token::Leeg => uit.push('$'),
                Token::Afgeleid => uit.push('*'),
                Token::Getal(g) | Token::Trefwoord(g) => uit.push_str(&String::from_utf8_lossy(g)),
                Token::Tekst(t) => {
                    uit.push('\'');
                    uit.push_str(&decodeer(t));
                    uit.push('\'');
                }
                Token::Enum(e) => {
                    uit.push('.');
                    uit.push_str(&String::from_utf8_lossy(e));
                    uit.push('.');
                }
                Token::Binair(b) => {
                    uit.push('"');
                    uit.push_str(&String::from_utf8_lossy(b));
                    uit.push('"');
                }
                Token::Verwijzing(n) => match self.bestand.index.get(&n) {
                    Some(&k) => self.toon_verwijzing(k as usize, max, diepte, uit),
                    None => uit.push_str(&format!("#{n}")),
                },
            }
            vorige_was_waarde = !matches!(token, Token::Open | Token::Trefwoord(_));
        }
    }

    /// Een korte leesbare tekst van een hele instantie.
    pub fn toon_instantie(&self, i: usize, max: usize) -> String {
        let mut uit = String::new();
        self.toon_verwijzing(i, max, 2, &mut uit);
        kort(&uit, max)
    }

    fn toon_verwijzing(&self, k: usize, max: usize, diepte: usize, uit: &mut String) {
        match self.soort[k] {
            Soort::Object(o) => uit.push_str(&self.object_weergave(o as usize)),
            Soort::Eigenaar | Soort::Knoop => {
                let inst = &self.bestand.instanties[k];
                if diepte == 0 {
                    uit.push_str(&self.type_weergave(k));
                    uit.push_str("(…)");
                } else if inst.complex {
                    uit.push('(');
                    self.toon_in(self.bestand.tekst(inst.type_bereik), max, diepte - 1, uit);
                    uit.push(')');
                } else {
                    uit.push_str(&self.type_weergave(k));
                    uit.push('(');
                    for (j, &attr) in self.bestand.attributen(k).iter().enumerate() {
                        if j > 0 {
                            uit.push_str(", ");
                        }
                        self.toon_in(self.bestand.tekst(attr), max, diepte - 1, uit);
                    }
                    uit.push(')');
                }
            }
        }
    }

    /// Een object als `Naam [GlobalId]`, of alleen `[GlobalId]`.
    pub fn object_weergave(&self, o: usize) -> String {
        let object = &self.objecten[o];
        match self.object_naam(object.inst as usize) {
            Some(naam) => format!("{} [{}]", kort(&naam, 60), object.globalid),
            None => format!("[{}]", object.globalid),
        }
    }

    /// De elementen van een lijst als (identiteit, leesbare tekst), als ze allemaal verwijzingen zijn: een object als
    /// zijn sleutel, een knoop als zijn hash. None als de waarde geen lijst van verwijzingen is.
    pub fn verwijzingslijst(&self, tekst: &[u8]) -> Option<Vec<Element>> {
        let delen = elementen(tekst)?;
        let mut uit = Vec::with_capacity(delen.len());
        for (b, e) in delen {
            let doel = enkele_verwijzing(&tekst[b..e])?;
            let &k = self.bestand.index.get(&doel)?;
            let id = match self.soort[k as usize] {
                Soort::Object(o) => {
                    let mut id = b"G".to_vec();
                    id.extend_from_slice(self.objecten[o as usize].sleutel.as_bytes());
                    id
                }
                _ => {
                    let mut id = b"K".to_vec();
                    id.extend_from_slice(&self.knoop_hash[k as usize]);
                    id
                }
            };
            uit.push((id, (b, e)));
        }
        Some(uit)
    }
}

/// Het eerste token van een stuk tekst.
pub(crate) fn eerste_token(tekst: &[u8]) -> Option<Token<'_>> {
    Tokens::nieuw(tekst).next().map(|(_, t)| t)
}

/// Het nummer als de tekst precies één verwijzing `#n` is.
pub(crate) fn enkele_verwijzing(tekst: &[u8]) -> Option<u64> {
    let mut tokens = Tokens::nieuw(tekst);
    match (tokens.next(), tokens.next()) {
        (Some((_, Token::Verwijzing(n))), None) => Some(n),
        _ => None,
    }
}

/// Schrijft de canonieke vorm van waarden. Tijdens [`Model::bereken`] bevat `status` per instantie of de hash al
/// klaar is; een verwijzing naar een knoop die nog op de stapel staat (een cyclus), wordt het type van die knoop.
struct Canon<'m, 'a> {
    model: &'m Model<'a>,
    hashes: &'m [Hash],
    status: &'m [u8],
}

impl Canon<'_, '_> {
    fn instantie(&self, i: usize, uit: &mut Vec<u8>) {
        let b = &self.model.bestand;
        let naam = b.type_naam(i);
        uit.push(b'T');
        uit.extend_from_slice(&(naam.len() as u32).to_le_bytes());
        uit.extend_from_slice(naam.as_bytes());
        uit.push(b'(');
        for &attr in b.attributen(i) {
            self.waarde(b.tekst(attr), uit);
        }
        uit.push(b')');
    }

    fn waarde(&self, tekst: &[u8], uit: &mut Vec<u8>) {
        for (_, token) in Tokens::nieuw(tekst) {
            match token {
                Token::Open => uit.push(b'('),
                Token::Sluit => uit.push(b')'),
                Token::Leeg => uit.push(b'$'),
                Token::Afgeleid => uit.push(b'*'),
                Token::Verwijzing(n) => self.verwijzing(n, uit),
                Token::Getal(g) => getal(g, uit),
                Token::Tekst(t) => {
                    uit.push(b'S');
                    if t.iter().all(|&b| b.is_ascii() && b != b'\'' && b != b'\\') {
                        lengte_en(t, uit);
                    } else {
                        lengte_en(decodeer(t).as_bytes(), uit);
                    }
                }
                Token::Enum(e) => {
                    uit.push(b'E');
                    lengte_en(&e.to_ascii_uppercase(), uit);
                }
                Token::Binair(x) => {
                    uit.push(b'B');
                    lengte_en(&x.to_ascii_uppercase(), uit);
                }
                Token::Trefwoord(w) => {
                    uit.push(b'W');
                    lengte_en(&w.to_ascii_uppercase(), uit);
                }
            }
        }
    }

    fn verwijzing(&self, n: u64, uit: &mut Vec<u8>) {
        let m = self.model;
        let Some(&k) = m.bestand.index.get(&n) else {
            uit.push(b'?');
            return;
        };
        let k = k as usize;
        match m.soort[k] {
            Soort::Object(o) => {
                uit.push(b'G');
                lengte_en(m.objecten[o as usize].sleutel.as_bytes(), uit);
            }
            Soort::Eigenaar => uit.push(b'O'),
            Soort::Knoop => {
                if self.status.get(k) == Some(&1) {
                    uit.push(b'C');
                    lengte_en(m.bestand.type_naam(k).as_bytes(), uit);
                } else {
                    uit.push(b'K');
                    uit.extend_from_slice(&self.hashes[k]);
                }
            }
        }
    }
}

fn lengte_en(bytes: &[u8], uit: &mut Vec<u8>) {
    uit.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    uit.extend_from_slice(bytes);
}

/// Een getal: REAL (met punt of exponent) als f64, anders als geheel getal.
fn getal(g: &[u8], uit: &mut Vec<u8>) {
    let tekst = std::str::from_utf8(g).unwrap_or("");
    let reeel = g.iter().any(|&b| b == b'.' || b == b'e' || b == b'E');
    if reeel {
        if let Ok(mut f) = tekst.parse::<f64>() {
            if f == 0.0 {
                f = 0.0;
            }
            uit.push(b'R');
            uit.extend_from_slice(&f.to_bits().to_le_bytes());
            return;
        }
    } else if let Ok(i) = tekst.parse::<i64>() {
        uit.push(b'I');
        uit.extend_from_slice(&i.to_le_bytes());
        return;
    }
    uit.push(b'N');
    lengte_en(g, uit);
}

/// Koppelt relaties zonder vaste GlobalId: een relatie in `x` die niet in de basis staat, krijgt de sleutel van een
/// relatie in de basis die niet in `x` staat, als ze hetzelfde type en dezelfde natuurlijke sleutel hebben en er
/// aan beide kanten precies één kandidaat is. Geeft het aantal koppelingen.
pub(crate) fn koppel_relaties(basis: &Model, x: &mut Model) -> usize {
    let mut kandidaten: HashMap<Hash, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (o, object) in basis.objecten.iter().enumerate() {
        if !x.op_globalid.contains_key(&object.globalid) {
            if let Some(s) = basis.natuurlijke_sleutel(o) {
                kandidaten.entry(s).or_default().0.push(o);
            }
        }
    }
    if kandidaten.is_empty() {
        return 0;
    }
    for (o, object) in x.objecten.iter().enumerate() {
        if !basis.op_globalid.contains_key(&object.globalid) {
            if let Some(s) = x.natuurlijke_sleutel(o) {
                if let Some(k) = kandidaten.get_mut(&s) {
                    k.1.push(o);
                }
            }
        }
    }
    let mut aantal = 0;
    for (b, xs) in kandidaten.into_values() {
        if let ([b], [o]) = (b.as_slice(), xs.as_slice()) {
            x.objecten[*o].sleutel = basis.objecten[*b].globalid.clone();
            aantal += 1;
        }
    }
    aantal
}

/// De sleutels van `x` in een tabel (sleutel -> objectindex). Een gekoppelde relatie staat onder de GlobalId van de
/// basis.
pub(crate) fn op_sleutel<'m>(x: &'m Model) -> HashMap<&'m str, usize> {
    x.objecten
        .iter()
        .enumerate()
        .map(|(o, object)| (object.sleutel.as_str(), o))
        .collect()
}

/// Bereik van een attribuut als tekst.
pub(crate) fn attribuut_tekst<'m>(m: &'m Model, i: usize, j: usize) -> &'m [u8] {
    let r: Bereik = m.bestand.attributen(i)[j];
    m.bestand.tekst(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ifc(schema: &str, data: &str) -> String {
        format!("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('','',(''),(''),'','','');\nFILE_SCHEMA(('{schema}'));\nENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n")
    }

    fn model(tekst: &str) -> Model<'_> {
        let mut m = Model::lees(tekst.as_bytes()).unwrap();
        m.bereken();
        m
    }

    #[test]
    fn getallen_en_teksten_canoniek() {
        let a = ifc("IFC4", "#1=IFCCARTESIANPOINT((1.,0.0,-0.));\n#2=IFCPROPERTYSINGLEVALUE('a''b',$,IFCLABEL('caf\\X2\\00E9\\X0\\'),$);");
        let b = ifc(
            "IFC4",
            "#7=IFCCARTESIANPOINT((1.0E0,0.,0.));\n#9=IFCPROPERTYSINGLEVALUE('a''b',$,IFCLABEL('café'),$);",
        );
        let (ma, mb) = (model(&a), model(&b));
        assert_eq!(ma.knoop_hash[0], mb.knoop_hash[0]);
        assert_eq!(ma.knoop_hash[1], mb.knoop_hash[1]);
        let c = ifc("IFC4", "#1=IFCCARTESIANPOINT((1.,0.,1.));");
        assert_ne!(model(&c).knoop_hash[0], ma.knoop_hash[0]);
    }

    #[test]
    fn objecten_en_eigenaar() {
        let tekst = ifc(
            "IFC4",
            "#1=IFCOWNERHISTORY($,$,$,.NOCHANGE.,$,$,$,1);\n#2=IFCWALL('3ScNjsTf6weF542amHNyMB',#1,'Muur',$,$,$,$,$,$);\n#3=IFCWALL('3ScNjsTf6weF542amHNyMC',$,'Muur',$,$,$,$,$,$);\n#4=IFCWALL('3ScNjsTf6weF542amHNyMD',#5,'Muur',$,$,$,$,$,$);\n#5=IFCOWNERHISTORY($,$,$,.ADDED.,$,$,$,2);",
        );
        let m = model(&tekst);
        assert_eq!(m.objecten.len(), 3);
        assert_eq!(m.soort[0], Soort::Eigenaar);
        // Een andere IfcOwnerHistory telt niet mee; `$` wel.
        assert_eq!(m.objecten[0].hash, m.objecten[2].hash);
        assert_ne!(m.objecten[0].hash, m.objecten[1].hash);
        assert_eq!(m.object_naam(1).as_deref(), Some("Muur"));
        assert_eq!(m.wortels, Vec::<u32>::new());
    }

    #[test]
    fn heuristiek_en_cyclus() {
        let tekst = ifc(
            "IFC4",
            "#1=IFCONBEKEND('3ScNjsTf6weF542amHNyMB',$,'X');\n#2=IFCANDERS(#3);\n#3=IFCANDERS(#2);\n#4=IFCSTYLEDITEM(#2,(),'rood');",
        );
        let m = model(&tekst);
        assert_eq!(m.objecten.len(), 1);
        assert_eq!(m.statistiek.objecten_via_heuristiek, 1);
        assert_eq!(m.statistiek.onbekende_typen.get("IFCANDERS"), Some(&2));
        assert_eq!(m.statistiek.cycli, 1);
        assert_eq!(m.wortels, vec![3]);
    }

    #[test]
    fn dubbele_globalid() {
        let tekst = ifc(
            "IFC4",
            "#1=IFCWALL('3ScNjsTf6weF542amHNyMB',$,$,$,$,$,$,$,$);\n#2=IFCWALL('3ScNjsTf6weF542amHNyMB',$,$,$,$,$,$,$,$);",
        );
        assert!(matches!(
            Model::lees(tekst.as_bytes()),
            Err(Fout::DubbeleGlobalId { regel: 9, .. })
        ));
    }

    #[test]
    fn weergave() {
        let tekst = ifc(
            "IFC4X3_ADD2",
            "#1=IFCTASK('3ScNjsTf6weF542amHNyMB',$,'Fundering',$,$,$,$,$,$,.F.,$,#2,$);\n#2=IFCTASKTIME($,$,$,$,'P5D','2027-01-04T07:00:00',$,$,$,$,$,$,$,$,$,$,$,$,$,$);\n#3=IFCRELNESTS('3ScNjsTf6weF542amHNyMC',$,$,$,#1,(#1));",
        );
        let m = model(&tekst);
        assert_eq!(m.type_weergave(0), "IfcTask");
        assert_eq!(m.attribuut_naam(0, 11), "TaskTime");
        let tijd = m.toon(attribuut_tekst(&m, 0, 11), 200);
        assert!(
            tijd.starts_with("IfcTaskTime($, $, $, $, 'P5D', '2027-01-04T07:00:00'"),
            "{tijd}"
        );
        assert_eq!(
            m.toon(attribuut_tekst(&m, 2, 5), 200),
            "(Fundering [3ScNjsTf6weF542amHNyMB])"
        );
    }
}
