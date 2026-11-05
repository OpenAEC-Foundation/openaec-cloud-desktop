//! De stromende weg: [`verschil_stromend`] voor bestanden van vele GB, met begrensd geheugen.
//!
//! [`Model`] houdt alles van een versie in het geheugen, ongeveer tien keer de bestandsgrootte. Hier blijft het
//! bestand waar het is (de aanroeper geeft bijv. een mmap). Per instantie houden we alleen de plek in het bestand en
//! een paar bits bij (5 bytes), per object het nummer, de sleutel en de hash. Attributen en verwijzingen lezen we
//! opnieuw uit de tekst als we ze nodig hebben. De regels zijn dezelfde code als bij [`Model`]: de canonieke vorm, de
//! ankers, de weergave en het verschil per object staan in [`Bron`] en in [`crate::verschil`].
//!
//! Wat globaal is, rekenen we vooraf uit, in een paar doorlopen:
//! 1. Lezen: per instantie de plek en de bits (object, wie ernaar verwijst), per object de GlobalId. Een tweede
//!    doorloop leest alleen de instanties die naar verderop in de tekst verwijzen.
//! 2. Kringen tussen knopen: dan geeft `verschil` een fout, hier ook.
//! 3. Relaties met een nieuwe GlobalId koppelen, en welke objecten in beide versies staan.
//! 4. De identiteit van de ankerknopen waar het op aankomt: waar een knoop naar verwijst (`A`), of die meer objecten
//!    delen (`D`). Van de andere ankerknopen telt alleen dát ze een anker zijn.
//! 5. De hash van elk object. De knopen eronder rekenen we uit als we ze tegenkomen; alleen knopen met meer
//!    verwijzers onthouden we, tot een grens.
//! 6. Vergelijken op sleutel; de details alleen voor wat veranderde.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::model::{
    anker_identiteit, eerste_token, enkele_verwijzing, herken, koppel_leden, posities, set_leden, wortel_sleutel,
    Anker, Bron, Canon, Hash, Knopen, Model, Soort, VersieStatistiek, GEEN_EIGENAAR, MEER_EIGENAARS,
};
use crate::ontleden::{doorloop, instantie_op, regel, Delen, Kop, Token, Tokens};
use crate::schema::{self, Entiteit};
use crate::tekst::decodeer;
use crate::verschil::{
    header_verschil, info, losse_knopen, vluchtige_header, vluchtige_velden, wijziging, Groepen, Statistiek, Verschil,
};
use crate::Fout;

// Bits per instantie.
/// Een object.
const OBJECT: u8 = 1;
/// Er verwijst een instantie naar.
const VERWEZEN: u8 = 2;
/// Er verwijzen meer instanties naar.
const MEER_VERWEZEN: u8 = 4;
/// Er verwijst een object rechtstreeks naar: bij een knoop een ankerknoop.
const ANKER: u8 = 8;
/// Er verwijzen meer verschillende objecten rechtstreeks naar.
const MEER_ANKERS: u8 = 16;
/// Er verwijst een knoop naar.
const DOOR_KNOOP: u8 = 32;
/// Het type heeft vluchtige velden (bijv. IfcOwnerHistory).
const VLUCHTIG_TYPE: u8 = 64;
/// Een bit voor één doorloop, daarna weer uit: bij het lezen "verwijst naar verderop in de tekst", bij het zoeken
/// naar kringen "bezocht".
const TIJDELIJK: u8 = 128;

const NUL: Hash = [0; 32];
/// De identiteit van een ankerknoop die we niet uitrekenden: geen verwijzing van een knoop en niet gedeeld (zie
/// [`Versie::nodig`]). Geen regel gebruikt dan de waarde, alleen dát het een ankerknoop is.
const NIET_NODIG: Hash = [0xA5; 32];
/// Zoveel knoophashes onthouden we hoogstens (ongeveer 50 MB); daarboven beginnen we opnieuw.
const MAX_ONTHOUDEN: usize = 1 << 20;

/// Het verschil tussen twee versies, zoals [`crate::verschil`], maar met begrensd geheugen: ongeveer 5 bytes per
/// instantie en 64 per object, plus wat voor de gewijzigde objecten nodig is. Bedoeld voor bestanden van vele GB (ook
/// boven 4 GiB), bijv. met een mmap van het bestand als invoer. De uitkomst is dezelfde als die van
/// [`crate::verschil`].
pub fn verschil_stromend(a: &[u8], b: &[u8]) -> Result<Verschil, Fout> {
    let (va, vb) = std::thread::scope(|s| {
        let a = s.spawn(|| Versie::lees(a));
        let b = Versie::lees(b);
        (a.join().unwrap_or_else(|_| Err(Fout::intern("lezen mislukt"))), b)
    });
    let (mut va, mut vb) = (va?, vb?);
    tegelijk(&mut va, &mut vb, &|v| {
        let (aantal, eerste) = v.kringen();
        v.statistiek.cycli = aantal;
        v.kring = eerste;
    });
    for v in [&va, &vb] {
        if let Some(n) = v.kring {
            return Err(Fout::Kring { regel: regel(v.data, v.begin(n) as usize) });
        }
    }
    let gekoppeld = koppel_relaties(&va, &mut vb);
    markeer_gemeen(&mut va, &mut vb);
    va.bereken_ankers(None);
    vb.bereken_ankers(Some(&va));
    tegelijk(&mut va, &mut vb, &|v| v.bereken_hashes());
    Ok(vergelijk(&va, &vb, gekoppeld))
}

/// Roept `f` aan voor beide versies tegelijk, elk in een eigen thread. Een `Versie` is niet te delen tussen threads
/// (de cache), wel door te geven.
fn tegelijk(va: &mut Versie, vb: &mut Versie, f: &(dyn Fn(&mut Versie) + Sync)) {
    std::thread::scope(|s| {
        s.spawn(|| f(va));
        f(vb);
    });
}

/// Een object in een [`Versie`].
struct Object {
    n: u64,
    /// De sleutel ([`crate::model::Object::sleutel`]) als 128 bits uit BLAKE3.
    sleutel: u128,
    hash: Hash,
    gemeen: bool,
}

/// De identiteit van een ankerknoop die ertoe doet ([`Versie::nodig`]).
struct AnkerInfo {
    id: Hash,
    gedeeld: bool,
    aantal: u32,
    ankers: Vec<Anker>,
}

/// Per instantienummer de plek in het bestand en de bits.
enum Plekken {
    Rang(Rang),
    /// Als de nummers niet oplopen in de tekst, of erg verspreid liggen: per instantie de plek en de bits.
    Kaart(HashMap<u64, (u64, Cell<u8>)>),
}

/// Lopen de nummers op in de tekst (zoals bijna altijd), dan staan de plek en de bits per instantie in de volgorde
/// van de tekst. Het hoeveelste nummer `#n` is (de rang), volgt uit één bit per nummer en een telling per 512 nummers.
#[derive(Default)]
struct Rang {
    /// Bit `n` staat aan als instantie `#n` bestaat.
    bits: Vec<u64>,
    /// Per 512 nummers: hoeveel instanties er vóór staan.
    voor: Vec<u32>,
    /// Per [`BLOK`] instanties de plek van de eerste; `plek` is de afstand daarvandaan.
    basis: Vec<u64>,
    plek: Vec<u32>,
    vlag: Vec<Cell<u8>>,
    hoogste: Option<u64>,
}

const BLOK: usize = 4096;

impl Rang {
    fn rang(&self, n: u64) -> Option<usize> {
        let w = usize::try_from(n / 64).ok()?;
        let woord = *self.bits.get(w)?;
        let bit = 1u64 << (n % 64);
        if woord & bit == 0 {
            return None;
        }
        let blok = w / 8;
        let eerder: u32 = self.bits[blok * 8..w].iter().map(|x| x.count_ones()).sum();
        Some(self.voor[blok] as usize + eerder as usize + (woord & (bit - 1)).count_ones() as usize)
    }

    fn begin(&self, r: usize) -> u64 {
        self.basis[r / BLOK] + u64::from(self.plek[r])
    }

    /// Voegt `#n` toe op plek `begin`. Geeft false als dat niet kan: `n` is niet groter dan de vorige, de nummers
    /// liggen te verspreid (meer dan 8 bytes aan bits per instantie), of de plek is te ver van het begin van het blok.
    fn voeg_toe(&mut self, n: u64, begin: u64) -> bool {
        let aantal = self.plek.len();
        let w = n / 64;
        if self.hoogste.is_some_and(|h| n <= h) || w > aantal as u64 + (1 << 17) || u32::try_from(aantal).is_err() {
            return false;
        }
        let basis = if aantal.is_multiple_of(BLOK) { begin } else { self.basis[aantal / BLOK] };
        let Ok(afstand) = u32::try_from(begin - basis) else { return false };
        let w = w as usize;
        while self.bits.len() <= w {
            if self.bits.len().is_multiple_of(8) {
                // Alle nummers tot nu toe liggen vóór dit blok.
                self.voor.push(aantal as u32);
            }
            self.bits.push(0);
        }
        self.bits[w] |= 1 << (n % 64);
        if aantal.is_multiple_of(BLOK) {
            self.basis.push(begin);
        }
        self.plek.push(afstand);
        self.vlag.push(Cell::new(0));
        self.hoogste = Some(n);
        true
    }

    /// De nummers in de volgorde van de tekst, met hun rang.
    fn nummers(&self) -> impl Iterator<Item = (u64, usize)> + '_ {
        self.bits
            .iter()
            .enumerate()
            .flat_map(|(w, &woord)| {
                std::iter::successors((woord != 0).then_some(woord), |&x| Some(x & (x - 1)).filter(|&y| y != 0))
                    .map(move |x| w as u64 * 64 + u64::from(x.trailing_zeros()))
            })
            .enumerate()
            .map(|(r, n)| (n, r))
    }
}

/// De bits van een instantie na een nieuwe verwijzing ernaar (`nieuw`: [`VERWEZEN`] met [`ANKER`] of [`DOOR_KNOOP`]).
fn verwezen(v: u8, nieuw: u8) -> u8 {
    let mut x = v | nieuw;
    if v & VERWEZEN != 0 {
        x |= MEER_VERWEZEN;
    }
    if v & nieuw & ANKER != 0 {
        x |= MEER_ANKERS;
    }
    x
}

impl Plekken {
    fn begin(&self, n: u64) -> Option<u64> {
        match self {
            Plekken::Rang(r) => r.rang(n).map(|i| r.begin(i)),
            Plekken::Kaart(k) => k.get(&n).map(|x| x.0),
        }
    }

    fn vlag(&self, n: u64) -> u8 {
        match self {
            Plekken::Rang(r) => r.rang(n).map_or(0, |i| r.vlag[i].get()),
            Plekken::Kaart(k) => k.get(&n).map_or(0, |x| x.1.get()),
        }
    }

    /// Past de bits van `#n` aan met `f`. Geeft false als `#n` niet bestaat.
    fn pas_aan(&self, n: u64, f: impl FnOnce(u8) -> u8) -> bool {
        let v = match self {
            Plekken::Rang(r) => r.rang(n).map(|i| &r.vlag[i]),
            Plekken::Kaart(k) => k.get(&n).map(|x| &x.1),
        };
        v.map(|v| v.set(f(v.get()))).is_some()
    }

    /// Zet bit `b` uit bij alle instanties.
    fn wis(&self, b: u8) {
        match self {
            Plekken::Rang(r) => r.vlag.iter().for_each(|v| v.set(v.get() & !b)),
            Plekken::Kaart(k) => k.values().for_each(|x| x.1.set(x.1.get() & !b)),
        }
    }

    /// Voegt `#n` toe op plek `begin`. Geeft false als het nummer al bestond.
    fn voeg_toe(&mut self, n: u64, begin: u64) -> bool {
        if let Plekken::Rang(r) = self {
            if r.voeg_toe(n, begin) {
                return true;
            }
            let kaart = r.nummers().map(|(n, i)| (n, (r.begin(i), r.vlag[i].clone()))).collect();
            *self = Plekken::Kaart(kaart);
        }
        let Plekken::Kaart(k) = self else { return false };
        match k.entry(n) {
            Entry::Occupied(_) => false,
            Entry::Vacant(plek) => {
                plek.insert((begin, Cell::new(0)));
                true
            }
        }
    }

    /// Alle instanties in de volgorde van de tekst, met hun bits (zoals ze zijn op het moment dat de instantie aan de
    /// beurt is).
    fn op_volgorde(&self) -> Box<dyn Iterator<Item = (u64, u8)> + '_> {
        match self {
            Plekken::Rang(r) => Box::new(r.nummers().map(|(n, i)| (n, r.vlag[i].get()))),
            Plekken::Kaart(k) => {
                let mut lijst: Vec<(u64, u64)> = k.iter().map(|(&n, x)| (x.0, n)).collect();
                lijst.sort_unstable();
                Box::new(lijst.into_iter().map(|(_, n)| (n, k[&n].1.get())))
            }
        }
    }
}

/// Een instantie zoals we haar opnieuw lazen.
struct Geparst {
    n: u64,
    type_id: u32,
    complex: bool,
    type_bereik: (usize, usize),
    attributen: Vec<(usize, usize)>,
    refs: Vec<u64>,
}

/// De laatst gelezen instanties; een paar is genoeg, want de canonieke vorm vraagt één instantie vaak na elkaar.
#[derive(Default)]
struct Cache {
    plekken: Vec<Arc<Geparst>>,
    volgende: usize,
}

const CACHE: usize = 8;

/// Eén versie van het bestand, als index op de tekst ([`Bron`]).
pub(crate) struct Versie<'d> {
    data: &'d [u8],
    kop: Kop,
    schema: Option<&'static schema::Schema>,
    plekken: Plekken,
    /// In de volgorde van de tekst.
    objecten: Vec<Object>,
    /// Objectindexen, gesorteerd op nummer.
    op_nummer: Vec<u32>,
    typen: HashMap<Vec<u8>, u32>,
    namen: Vec<String>,
    type_entiteit: Vec<Option<&'static Entiteit>>,
    /// Knopen die naar een knoop verderop in de tekst verwijzen, of naar zichzelf: elke kring gaat door zo'n knoop.
    vooruit: Vec<u64>,
    /// De sleutel van een gekoppelde relatie: de GlobalId in de basis.
    gekoppeld: HashMap<u32, String>,
    ankers: HashMap<u64, AnkerInfo>,
    /// De eerste knoop van een kring ([`Versie::kringen`]).
    kring: Option<u64>,
    pub statistiek: VersieStatistiek,
    cache: RefCell<Cache>,
    knoop_memo: RefCell<HashMap<u64, Hash>>,
    puur_memo: RefCell<HashMap<u64, Hash>>,
    /// Per knoop met vluchtige velden: hoeveel objecten er rechtstreeks naar verwijzen (pas als het nodig is).
    verankerd: RefCell<Option<HashMap<u64, u32>>>,
    /// Uitkomsten van [`Versie::verwijzende_objecten`] die het hele bestand doorzochten.
    verwijzend: RefCell<HashMap<u64, Vec<String>>>,
}

fn sleutel_van(globalid: &str) -> u128 {
    let h = blake3::hash(globalid.as_bytes());
    u128::from_le_bytes(h.as_bytes()[..16].try_into().unwrap_or_default())
}

impl<'d> Versie<'d> {
    /// Leest het bestand één keer door. Geeft dezelfde fouten als [`Model::lees`], in dezelfde volgorde.
    fn lees(data: &'d [u8]) -> Result<Self, Fout> {
        let mut plekken = Plekken::Rang(Rang::default());
        let mut typen: HashMap<Vec<u8>, u32> = HashMap::new();
        let mut namen: Vec<String> = Vec::new();
        let mut type_entiteit: Vec<Option<&'static Entiteit>> = Vec::new();
        let mut objecten: Vec<Object> = Vec::new();
        let mut statistiek = VersieStatistiek::default();
        let mut gekozen: Option<Option<&'static schema::Schema>> = None;
        let mut aantal = 0usize;
        let mut dubbel: Option<(usize, u64)> = None;
        let mut vooruit: Vec<u64> = Vec::new();
        let mut doelen: Vec<u64> = Vec::new();
        let kop = doorloop(data, &mut |kop, g| {
            let schema = *gekozen.get_or_insert_with(|| schema::kies(&kop.schema));
            let type_id = match typen.get(g.naam) {
                Some(&t) => t,
                None => {
                    let t = namen.len() as u32;
                    let naam = String::from_utf8_lossy(g.naam).into_owned();
                    type_entiteit.push(if naam.contains('+') { None } else { schema.and_then(|s| s.entiteit(&naam)) });
                    namen.push(naam);
                    typen.insert(g.naam.to_vec(), t);
                    t
                }
            };
            let attrs: Vec<&[u8]> = g.attributen.iter().map(|&(a, b)| &data[a..b]).collect();
            let entiteit = type_entiteit[type_id as usize];
            let globalid = herken(entiteit, &namen[type_id as usize], &attrs, g.complex, &mut statistiek);
            aantal += 1;
            let begin = g.bereik.0 as u64;
            if !plekken.voeg_toe(g.id, begin) && dubbel.is_none() {
                dubbel = Some((g.bereik.0, g.id));
            }
            let object = globalid.is_some();
            let mut eigen = 0;
            if let Some(globalid) = &globalid {
                eigen |= OBJECT;
                objecten.push(Object { n: g.id, sleutel: sleutel_van(globalid), hash: NUL, gemeen: true });
            }
            if entiteit.is_some_and(|e| e.attributen.iter().any(|a| a.vluchtig)) {
                eigen |= VLUCHTIG_TYPE;
            }
            if eigen != 0 {
                plekken.pas_aan(g.id, |v| v | eigen);
            }
            doelen.clear();
            doelen.extend_from_slice(g.refs);
            doelen.sort_unstable();
            doelen.dedup();
            let nieuw = VERWEZEN | if object { ANKER } else { DOOR_KNOOP };
            let mut later = false;
            for &d in &doelen {
                if d == g.id && !object {
                    vooruit.push(g.id);
                }
                // Een nummer dat we nog niet zagen: in de tweede doorloop.
                later |= !plekken.pas_aan(d, |v| verwezen(v, nieuw));
            }
            if later {
                plekken.pas_aan(g.id, |v| v | TIJDELIJK);
            }
            Ok(())
        })?;
        // Dezelfde volgorde van fouten als `ontleden::lees` en `Model::lees`.
        if let Some((begin, n)) = dubbel {
            return Err(Fout::Ongeldig {
                regel: regel(data, begin),
                melding: format!("instantie #{n} komt twee keer voor"),
            });
        }
        // De tweede doorloop: alleen de instanties die naar verderop in de tekst verwijzen. Een doel vóór de
        // instantie telde de eerste doorloop al.
        let mut wees = None;
        let mut delen = Delen::default();
        for (n, v) in plekken.op_volgorde().filter(|x| x.1 & TIJDELIJK != 0) {
            let begin = plekken.begin(n).unwrap_or(0);
            instantie_op(data, begin as usize, &mut delen);
            doelen.clear();
            doelen.extend_from_slice(&delen.refs);
            doelen.sort_unstable();
            doelen.dedup();
            let object = v & OBJECT != 0;
            let nieuw = VERWEZEN | if object { ANKER } else { DOOR_KNOOP };
            for &d in &doelen {
                match plekken.begin(d) {
                    None => {
                        wees.get_or_insert(begin);
                    }
                    Some(b) if b > begin => {
                        plekken.pas_aan(d, |x| verwezen(x, nieuw));
                        if !object && plekken.vlag(d) & OBJECT == 0 {
                            vooruit.push(n);
                        }
                    }
                    Some(_) => {}
                }
            }
        }
        plekken.wis(TIJDELIJK);
        if let Some(begin) = wees {
            // De eerste verwijzing van die instantie naar een nummer dat niet bestaat, zoals `ontleden::lees`.
            let n = instantie_op(data, begin as usize, &mut delen).map_or(0, |x| x.0);
            let d = delen.refs.iter().copied().find(|&d| plekken.begin(d).is_none()).unwrap_or(0);
            return Err(Fout::Ongeldig {
                regel: regel(data, begin as usize),
                melding: format!("#{n} verwijst naar #{d}, maar die bestaat niet"),
            });
        }
        vooruit.sort_unstable();
        vooruit.dedup();
        let schema = schema::kies(&kop.schema);
        statistiek.schema = kop.schema.clone();
        statistiek.schema_tabel = schema.map(|s| s.naam.to_string()).unwrap_or_default();
        statistiek.instanties = aantal;
        statistiek.objecten = objecten.len();
        statistiek.knopen = aantal - objecten.len();
        for (_, v) in plekken.op_volgorde().filter(|x| x.1 & OBJECT == 0) {
            statistiek.losse_knopen += usize::from(v & VERWEZEN == 0);
            statistiek.gedeelde_knopen += usize::from(v & MEER_ANKERS != 0);
        }
        let mut op_nummer: Vec<u32> = (0..objecten.len() as u32).collect();
        op_nummer.sort_unstable_by_key(|&o| objecten[o as usize].n);
        let versie = Versie {
            data,
            kop,
            schema,
            plekken,
            objecten,
            op_nummer,
            typen,
            namen,
            type_entiteit,
            vooruit,
            gekoppeld: HashMap::new(),
            ankers: HashMap::new(),
            kring: None,
            statistiek,
            cache: RefCell::default(),
            knoop_memo: RefCell::default(),
            puur_memo: RefCell::default(),
            verankerd: RefCell::default(),
            verwijzend: RefCell::default(),
        };
        versie.dubbele_globalid()?;
        Ok(versie)
    }

    /// Twee objecten met dezelfde GlobalId: de fout van [`Model::lees`], bij het eerste object (in de volgorde van de
    /// tekst) waarvan de GlobalId al eerder voorkwam.
    fn dubbele_globalid(&self) -> Result<(), Fout> {
        let mut lijst: Vec<(u128, u32)> =
            self.objecten.iter().enumerate().map(|(o, x)| (x.sleutel, o as u32)).collect();
        lijst.sort_unstable();
        let tweede = lijst.windows(2).filter(|w| w[0].0 == w[1].0).map(|w| w[1].1).min();
        drop(lijst);
        let Some(o) = tweede else { return Ok(()) };
        let begin = self.begin(self.objecten[o as usize].n) as usize;
        Err(Fout::DubbeleGlobalId { globalid: self.globalid(o).into_owned(), regel: regel(self.data, begin) })
    }

    fn begin(&self, n: u64) -> u64 {
        self.plekken.begin(n).unwrap_or(0)
    }

    fn vlag(&self, n: u64) -> u8 {
        self.plekken.vlag(n)
    }

    /// De instantie met nummer `n`, opnieuw gelezen (of uit de cache).
    fn geparst(&self, n: u64) -> Arc<Geparst> {
        let mut cache = self.cache.borrow_mut();
        if let Some(g) = cache.plekken.iter().find(|g| g.n == n) {
            return Arc::clone(g);
        }
        let mut delen = Delen::default();
        let begin = self.begin(n) as usize;
        let (complex, type_bereik) = match instantie_op(self.data, begin, &mut delen) {
            Some((_, complex, type_bereik, _)) => (complex, type_bereik),
            None => (false, (begin, begin)),
        };
        let g = Arc::new(Geparst {
            n,
            type_id: self.typen.get(&delen.naam).copied().unwrap_or(0),
            complex,
            type_bereik,
            attributen: delen.attributen,
            refs: delen.refs,
        });
        if cache.plekken.len() < CACHE {
            cache.plekken.push(Arc::clone(&g));
        } else {
            let plek = cache.volgende;
            cache.plekken[plek] = Arc::clone(&g);
            cache.volgende = (plek + 1) % CACHE;
        }
        g
    }

    fn object_op_nummer(&self, n: u64) -> Option<u32> {
        let p = self.op_nummer.binary_search_by_key(&n, |&o| self.objecten[o as usize].n).ok()?;
        Some(self.op_nummer[p])
    }

    /// Kringen tussen knopen, ook via ankerknopen, zoals [`Model`] ze vindt: het aantal verwijzingen terug en de eerste
    /// knoop waar we er een vonden. Elke kring gaat door een knoop uit [`Versie::vooruit`]; zoeken vanaf die knopen is
    /// dus genoeg om te weten óf er een kring is. Is er een, dan zoeken we opnieuw zoals [`Model`] (vanaf elke knoop
    /// in de volgorde van de tekst), voor dezelfde eerste knoop in de foutmelding.
    fn kringen(&self) -> (usize, Option<u64>) {
        if self.vooruit.is_empty() || self.zoek_kringen(self.vooruit.iter().copied()).0 == 0 {
            return (0, None);
        }
        self.zoek_kringen(self.plekken.op_volgorde().filter(|x| x.1 & OBJECT == 0).map(|x| x.0))
    }

    /// Diepte-eerst vanaf elke knoop in `starts` die nog niet bezocht is, langs verwijzingen naar knopen. Een knoop
    /// die klaar is, krijgt de bit [`TIJDELIJK`]; daarna gaat die bit weer uit.
    fn zoek_kringen(&self, starts: impl Iterator<Item = u64>) -> (usize, Option<u64>) {
        let klaar = |n: u64| self.vlag(n) & TIJDELIJK != 0;
        let mut op_stapel: HashSet<u64> = HashSet::new();
        let (mut aantal, mut eerste) = (0, None);
        for start in starts {
            if klaar(start) {
                continue;
            }
            op_stapel.insert(start);
            let mut stapel: Vec<(Arc<Geparst>, usize)> = vec![(self.geparst(start), 0)];
            while let Some((g, volgende)) = stapel.last_mut() {
                if *volgende < g.refs.len() {
                    let c = g.refs[*volgende];
                    *volgende += 1;
                    if self.vlag(c) & OBJECT != 0 || klaar(c) {
                        continue;
                    }
                    if !op_stapel.insert(c) {
                        aantal += 1;
                        eerste.get_or_insert(c);
                        continue;
                    }
                    let kind = self.geparst(c);
                    stapel.push((kind, 0));
                    continue;
                }
                op_stapel.remove(&g.n);
                self.plekken.pas_aan(g.n, |v| v | TIJDELIJK);
                stapel.pop();
            }
        }
        self.plekken.wis(TIJDELIJK);
        (aantal, eerste)
    }

    /// Een ankerknoop waarvan de identiteit ertoe doet: een knoop verwijst ernaar (`A`), of hij is gedeeld en het delen
    /// kan tellen (`D`, niet bij IfcOwnerHistory). Van de andere ankerknopen gebruikt geen regel de identiteit.
    fn nodig(&self, k: u64) -> bool {
        let v = self.vlag(k);
        v & (OBJECT | ANKER) == ANKER
            && (v & DOOR_KNOOP != 0 || (v & MEER_ANKERS != 0 && self.type_naam(k as usize) != "IFCOWNERHISTORY"))
    }

    /// De identiteit van de ankerknopen die ertoe doen ([`Versie::nodig`]), zoals [`Model`] ze uitrekent. In een SET
    /// stemmen we af met de basis, zoals [`crate::model::stem_ankers_af`].
    fn bereken_ankers(&mut self, basis: Option<&Versie>) {
        let basis_op_sleutel: Option<HashMap<u128, u32>> =
            basis.map(|b| b.objecten.iter().enumerate().map(|(o, x)| (x.sleutel, o as u32)).collect());
        let mut per_knoop: HashMap<u64, Vec<(u32, u32, u64)>> = HashMap::new();
        for o in 0..self.objecten.len() {
            let i = self.objecten[o].n as usize;
            let aantal = self.aantal_attributen(i);
            for j in 1..aantal {
                let tekst = self.attribuut(i, j);
                let raakt = Tokens::nieuw(tekst).any(|(_, t)| matches!(t, Token::Verwijzing(n) if self.nodig(n)));
                if !raakt {
                    continue;
                }
                let px = |k: usize| self.puur(k as u64);
                // De koppeling van de leden van een SET met die van de basis, zoals `stem_ankers_af`.
                let mut koppeling: HashMap<u32, u32> = HashMap::new();
                if let (Some(b), Some(op)) = (basis, &basis_op_sleutel) {
                    if let Some(&ob) = op.get(&self.objecten[o].sleutel) {
                        let ib = b.objecten[ob as usize].n as usize;
                        let pb = |k: usize| b.puur(k as u64);
                        if b.type_naam(ib) == self.type_naam(i) && b.aantal_attributen(ib) == aantal {
                            if let (Some(lb), Some(lx)) = (set_leden(b, ib, j, &pb), set_leden(&*self, i, j, &px)) {
                                let m = [b, &*self];
                                let mut verwijzend = |kant: usize, k: usize| {
                                    m[kant].verwijzende_objecten(k as u64, [ib, i][kant] as u64)
                                };
                                koppeling.extend(koppel_leden(m, [&lb, &lx], [&pb, &px], &mut verwijzend));
                            }
                        }
                    }
                }
                let lid = |e: u32| if basis.is_some() { koppeling.get(&e).copied() } else { Some(e) };
                posities(&*self, i, j, tekst, &px, &lid, |k, p, _| {
                    if self.nodig(k as u64) {
                        per_knoop.entry(k as u64).or_default().push((o as u32, j as u32, p));
                    }
                });
            }
        }
        let mut ankers = HashMap::with_capacity(per_knoop.len());
        for (k, lijst) in per_knoop {
            let mut lijst: Vec<(String, Anker)> = lijst
                .into_iter()
                .map(|(o, j, p)| (self.sleutel(o).into_owned(), Anker { object: o, attribuut: j, positie: p }))
                .collect();
            lijst.sort_unstable_by(|x, y| (&x.0, x.1.attribuut, x.1.positie).cmp(&(&y.0, y.1.attribuut, y.1.positie)));
            let (id, gedeeld, aantal) = anker_identiteit(
                lijst.iter().map(|(s, a)| (*a, s.as_bytes(), self.objecten[a.object as usize].gemeen)),
            );
            let ankers_lijst = lijst.into_iter().map(|(_, a)| a).collect();
            ankers.insert(k, AnkerInfo { id, gedeeld, aantal, ankers: ankers_lijst });
        }
        self.ankers = ankers;
        self.puur_memo.borrow_mut().clear();
    }

    /// De hash van elk object, zoals [`Model`]: het type en de hashes van de attributen zonder de GlobalId.
    fn bereken_hashes(&mut self) {
        for o in 0..self.objecten.len() {
            let i = self.objecten[o].n as usize;
            let hashes: Vec<Hash> = (1..self.aantal_attributen(i)).map(|j| self.attribuut_hash(i, j)).collect();
            self.objecten[o].hash = Model::object_hash_uit(self.type_naam(i), &hashes);
        }
        self.knoop_memo.borrow_mut().clear();
    }

    /// De inhoudshash van een knoop (zie [`Model::bereken`]); `puur`: zonder ankers (zie [`Model::bereken_puur`]).
    /// Knopen eronder rekenen we eerst uit, zonder recursie. Een knoop met meer verwijzers onthouden we.
    fn bereken_knoop(&self, k: u64, puur: bool) -> Hash {
        let memo = if puur { &self.puur_memo } else { &self.knoop_memo };
        if let Some(&h) = memo.borrow().get(&k) {
            return h;
        }
        // Zonder ankers lopen we in elke knoop; anders niet in ankerknopen (die tellen als anker).
        let kind_telt = |c: u64| {
            let v = self.vlag(c);
            v & OBJECT == 0 && (puur || v & ANKER == 0)
        };
        let mut bezig: HashSet<u64> = HashSet::from([k]);
        let mut klaar: HashMap<u64, Hash> = HashMap::new();
        let mut stapel: Vec<(Arc<Geparst>, usize)> = vec![(self.geparst(k), 0)];
        let mut buffer = Vec::new();
        while let Some((g, volgende)) = stapel.last_mut() {
            if *volgende < g.refs.len() {
                let c = g.refs[*volgende];
                *volgende += 1;
                if !kind_telt(c) || klaar.contains_key(&c) || memo.borrow().contains_key(&c) || !bezig.insert(c) {
                    continue;
                }
                let kind = self.geparst(c);
                stapel.push((kind, 0));
                continue;
            }
            let n = g.n;
            stapel.pop();
            buffer.clear();
            {
                let opzoeken = |c: usize| {
                    let c = c as u64;
                    klaar.get(&c).copied().or_else(|| memo.borrow().get(&c).copied()).unwrap_or(NUL)
                };
                let knopen = if puur { Knopen::Puur(&opzoeken) } else { Knopen::Gewoon(&opzoeken) };
                Canon { bron: self, knopen }.instantie(n as usize, &mut buffer);
            }
            bezig.remove(&n);
            klaar.insert(n, *blake3::hash(&buffer).as_bytes());
        }
        let h = klaar.get(&k).copied().unwrap_or(NUL);
        let mut memo = memo.borrow_mut();
        if memo.len() > MAX_ONTHOUDEN {
            memo.clear();
        }
        memo.extend(klaar.into_iter().filter(|&(n, _)| self.vlag(n) & MEER_VERWEZEN != 0));
        h
    }

    fn puur(&self, k: u64) -> Hash {
        self.bereken_knoop(k, true)
    }

    /// De objecten die (rechtstreeks of via knopen) naar knoop `k` verwijzen en in beide versies staan, als
    /// gesorteerde sleutels ([`Model::verwijzende_objecten`]). `eigenaar` verwijst zelf naar `k` (`k` staat in een SET
    /// ervan). Heeft `k` één verwijzer, dan is dat `eigenaar`. Anders zoeken we de verwijzers in het hele bestand, laag
    /// voor laag; dat onthouden we per knoop.
    fn verwijzende_objecten(&self, k: u64, eigenaar: u64) -> Vec<String> {
        let sleutel = |n: u64| {
            let o = self.object_op_nummer(n).filter(|&o| self.objecten[o as usize].gemeen)?;
            Some(self.sleutel(o).into_owned())
        };
        if self.vlag(k) & MEER_VERWEZEN == 0 {
            return sleutel(eigenaar).into_iter().collect();
        }
        if let Some(uit) = self.verwijzend.borrow().get(&k) {
            return uit.clone();
        }
        let mut uit = Vec::new();
        let mut gezien: HashSet<u64> = HashSet::from([k]);
        let mut laag: HashSet<u64> = HashSet::from([k]);
        let mut delen = Delen::default();
        while !laag.is_empty() {
            let mut volgende = HashSet::new();
            for (n, v) in self.plekken.op_volgorde() {
                delen.refs.clear();
                instantie_op(self.data, self.begin(n) as usize, &mut delen);
                if !delen.refs.iter().any(|r| laag.contains(r)) {
                    continue;
                }
                if v & OBJECT != 0 {
                    uit.extend(sleutel(n));
                } else if gezien.insert(n) {
                    volgende.insert(n);
                }
            }
            laag = volgende;
        }
        uit.sort_unstable();
        uit.dedup();
        self.verwijzend.borrow_mut().insert(k, uit.clone());
        uit
    }

    /// Per knoop in `doelen` de eigenaar ([`Model::eigenaar`]): het enige object dat er komt (via een rechtstreekse
    /// verwijzing en daarna knopen zonder anker), of [`MEER_EIGENAARS`]. Net als bij [`Model`] lopen we verder in een
    /// knoop met meer verwijzers alleen als zijn eigenaar verandert. Een andere knoop heeft één verwijzer; die komen we
    /// dus hoogstens zo vaak tegen als die verwijzer (we nemen elke verwijzing per instantie één keer).
    fn eigenaars(&self, doelen: &HashSet<u64>) -> HashMap<u64, u32> {
        let kinderen = |n: u64, masker: u8| {
            let mut uit: Vec<u64> =
                self.geparst(n).refs.iter().copied().filter(|&c| self.vlag(c) & masker == 0).collect();
            uit.sort_unstable();
            uit.dedup();
            uit
        };
        let mut uit: HashMap<u64, u32> = HashMap::new();
        let mut gedeeld: HashMap<u64, u32> = HashMap::new();
        let mut stapel: Vec<u64> = Vec::new();
        for (o, object) in self.objecten.iter().enumerate() {
            let o = o as u32;
            stapel.extend(kinderen(object.n, OBJECT));
            while let Some(x) = stapel.pop() {
                if doelen.contains(&x) {
                    let e = uit.entry(x).or_insert(GEEN_EIGENAAR);
                    *e = match *e {
                        GEEN_EIGENAAR => o,
                        y if y == o || y == MEER_EIGENAARS => y,
                        _ => MEER_EIGENAARS,
                    };
                }
                if self.vlag(x) & MEER_VERWEZEN != 0 {
                    let e = gedeeld.entry(x).or_insert(GEEN_EIGENAAR);
                    match *e {
                        GEEN_EIGENAAR => *e = o,
                        y if y == o || y == MEER_EIGENAARS => continue,
                        _ => *e = MEER_EIGENAARS,
                    }
                }
                stapel.extend(kinderen(x, OBJECT | ANKER));
            }
        }
        uit
    }

    /// De losse knopen per groep, zoals [`crate::verschil::losse_groepen`]: bij een gelijke hash in de volgorde van
    /// de tekst.
    fn losse_groepen(&self) -> Groepen {
        let wortels: Vec<u64> =
            self.plekken.op_volgorde().filter(|x| x.1 & (OBJECT | VERWEZEN) == 0).map(|x| x.0).collect();
        let mut doelen = HashSet::new();
        for &w in &wortels {
            for tekst in self.attributen(w as usize) {
                if let Some(k) = enkele_verwijzing(tekst).filter(|&n| self.vlag(n) & OBJECT == 0) {
                    doelen.insert(k);
                }
            }
        }
        let eigenaar = if doelen.is_empty() { HashMap::new() } else { self.eigenaars(&doelen) };
        let mut groepen: HashMap<Hash, Vec<(Hash, u64, usize)>> = HashMap::new();
        for w in wortels {
            let sleutel =
                wortel_sleutel(self, w as usize, &|k| eigenaar.get(&(k as u64)).copied().unwrap_or(GEEN_EIGENAAR));
            groepen.entry(sleutel).or_default().push((self.knoop_hash(w as usize), self.begin(w), w as usize));
        }
        groepen
            .into_iter()
            .map(|(s, mut v)| {
                v.sort_unstable();
                (s, v.into_iter().map(|(h, _, w)| (h, w)).collect())
            })
            .collect()
    }
}

impl Bron for Versie<'_> {
    fn zoek(&self, n: u64) -> Option<usize> {
        self.plekken.begin(n).map(|_| n as usize)
    }

    fn soort(&self, k: usize) -> Soort {
        if self.vlag(k as u64) & OBJECT != 0 {
            if let Some(o) = self.object_op_nummer(k as u64) {
                return Soort::Object(o);
            }
        }
        Soort::Knoop
    }

    fn type_naam(&self, k: usize) -> &str {
        &self.namen[self.geparst(k as u64).type_id as usize]
    }

    fn is_complex(&self, k: usize) -> bool {
        self.geparst(k as u64).complex
    }

    fn type_tekst(&self, k: usize) -> &[u8] {
        let (a, b) = self.geparst(k as u64).type_bereik;
        &self.data[a..b]
    }

    fn attributen(&self, k: usize) -> Vec<&[u8]> {
        self.geparst(k as u64).attributen.iter().map(|&(a, b)| &self.data[a..b]).collect()
    }

    fn attribuut(&self, k: usize, j: usize) -> &[u8] {
        let (a, b) = self.geparst(k as u64).attributen[j];
        &self.data[a..b]
    }

    fn aantal_attributen(&self, k: usize) -> usize {
        self.geparst(k as u64).attributen.len()
    }

    fn schema(&self) -> Option<&'static schema::Schema> {
        self.schema
    }

    fn entiteit(&self, k: usize) -> Option<&'static Entiteit> {
        let g = self.geparst(k as u64);
        self.type_entiteit[g.type_id as usize].filter(|e| e.attributen.len() == g.attributen.len())
    }

    fn object_inst(&self, o: u32) -> usize {
        self.objecten[o as usize].n as usize
    }

    /// Zoals [`herken`] hem leest.
    fn globalid(&self, o: u32) -> Cow<'_, str> {
        match eerste_token(self.attribuut(self.object_inst(o), 0)) {
            Some(Token::Tekst(t)) => Cow::Owned(decodeer(t)),
            _ => Cow::Borrowed(""),
        }
    }

    fn sleutel(&self, o: u32) -> Cow<'_, str> {
        match self.gekoppeld.get(&o) {
            Some(s) => Cow::Borrowed(s),
            None => self.globalid(o),
        }
    }

    fn anker_id(&self, k: usize) -> Option<Hash> {
        if self.vlag(k as u64) & (OBJECT | ANKER) != ANKER {
            return None;
        }
        Some(self.ankers.get(&(k as u64)).map_or(NIET_NODIG, |a| a.id))
    }

    fn ankers(&self, k: usize) -> Cow<'_, [Anker]> {
        match self.ankers.get(&(k as u64)) {
            Some(a) => Cow::Borrowed(&a.ankers),
            None => Cow::Owned(Vec::new()),
        }
    }

    fn verankerd_door(&self, k: usize) -> u32 {
        if let Some(a) = self.ankers.get(&(k as u64)) {
            return a.aantal;
        }
        let mut verankerd = self.verankerd.borrow_mut();
        let tellingen = verankerd.get_or_insert_with(|| {
            // Eén keer voor alle knopen met vluchtige velden: hoeveel objecten verwijzen er rechtstreeks naar.
            let mut uit: HashMap<u64, u32> = HashMap::new();
            for object in &self.objecten {
                let mut doelen = self.geparst(object.n).refs.clone();
                doelen.sort_unstable();
                doelen.dedup();
                for d in doelen {
                    if self.vlag(d) & (VLUCHTIG_TYPE | OBJECT) == VLUCHTIG_TYPE {
                        *uit.entry(d).or_default() += 1;
                    }
                }
            }
            uit
        });
        tellingen.get(&(k as u64)).copied().unwrap_or(0)
    }

    fn deelt_telt(&self, k: usize) -> bool {
        self.ankers.get(&(k as u64)).is_some_and(|a| a.gedeeld) && self.type_naam(k) != "IFCOWNERHISTORY"
    }

    fn knoop_hash(&self, k: usize) -> Hash {
        self.bereken_knoop(k as u64, false)
    }

    fn header_tekst(&self, naam: &str, j: usize) -> Option<&[u8]> {
        let regel = self.kop.header.iter().find(|r| r.naam == naam)?;
        let &(a, b) = regel.parameters.get(j)?;
        Some(&self.data[a as usize..b as usize])
    }
}

/// Per object: de index van het object met dezelfde sleutel in de andere versie, of u32::MAX.
fn partners(a: &Versie, b: &Versie) -> Vec<u32> {
    let in_b: HashMap<u128, u32> = b.objecten.iter().enumerate().map(|(o, x)| (x.sleutel, o as u32)).collect();
    a.objecten.iter().map(|x| in_b.get(&x.sleutel).copied().unwrap_or(u32::MAX)).collect()
}

/// [`crate::model::koppel_relaties`]: een relatie in `x` die niet in de basis staat, krijgt de sleutel van een
/// relatie in de basis die niet in `x` staat, als ze dezelfde natuurlijke sleutel hebben en er aan beide kanten
/// precies één kandidaat is. Geeft het aantal koppelingen.
fn koppel_relaties(basis: &Versie, x: &mut Versie) -> usize {
    let in_x: HashSet<u128> = x.objecten.iter().map(|o| o.sleutel).collect();
    let mut kandidaten: HashMap<Hash, (Vec<u32>, Vec<u32>)> = HashMap::new();
    for (o, object) in basis.objecten.iter().enumerate() {
        if !in_x.contains(&object.sleutel) {
            if let Some(s) = basis.natuurlijke_sleutel(o as u32) {
                kandidaten.entry(s).or_default().0.push(o as u32);
            }
        }
    }
    drop(in_x);
    if kandidaten.is_empty() {
        return 0;
    }
    let in_basis: HashSet<u128> = basis.objecten.iter().map(|o| o.sleutel).collect();
    for (o, object) in x.objecten.iter().enumerate() {
        if !in_basis.contains(&object.sleutel) {
            if let Some(k) = x.natuurlijke_sleutel(o as u32).and_then(|s| kandidaten.get_mut(&s)) {
                k.1.push(o as u32);
            }
        }
    }
    let mut aantal = 0;
    for (b, xs) in kandidaten.into_values() {
        if let ([b], [o]) = (b.as_slice(), xs.as_slice()) {
            let globalid = basis.globalid(*b).into_owned();
            x.objecten[*o as usize].sleutel = sleutel_van(&globalid);
            x.gekoppeld.insert(*o, globalid);
            aantal += 1;
        }
    }
    aantal
}

/// [`crate::model::markeer_gemeen`] voor twee versies.
fn markeer_gemeen(a: &mut Versie, b: &mut Versie) {
    let in_a: HashSet<u128> = a.objecten.iter().map(|o| o.sleutel).collect();
    let in_b: HashSet<u128> = b.objecten.iter().map(|o| o.sleutel).collect();
    for o in &mut a.objecten {
        o.gemeen = in_b.contains(&o.sleutel);
    }
    for o in &mut b.objecten {
        o.gemeen = in_a.contains(&o.sleutel);
    }
}

/// [`crate::verschil::vergelijk`] voor twee versies als index.
fn vergelijk(va: &Versie, vb: &Versie, relaties_gekoppeld: usize) -> Verschil {
    let partner = partners(va, vb);
    let terug = partners(vb, va);
    let mut toegevoegd: Vec<_> =
        (0..vb.objecten.len()).filter(|&o| terug[o] == u32::MAX).map(|o| info(vb, o)).collect();
    let mut verwijderd: Vec<_> =
        (0..va.objecten.len()).filter(|&o| partner[o] == u32::MAX).map(|o| info(va, o)).collect();
    let mut gewijzigd = Vec::new();
    let mut vluchtig = Vec::new();
    let mut paren = HashSet::new();
    for (oa, &ob) in partner.iter().enumerate() {
        if ob == u32::MAX {
            continue;
        }
        let ob = ob as usize;
        if vb.objecten[ob].hash != va.objecten[oa].hash {
            gewijzigd.push(wijziging(va, oa, vb, ob));
        }
        if heeft_vluchtig(va, oa) {
            vluchtige_velden(va, oa, vb, ob, &mut paren, &mut vluchtig);
        }
    }
    vluchtige_header(va, vb, &mut vluchtig);
    toegevoegd.sort();
    verwijderd.sort();
    gewijzigd.sort_by(|x, y| x.globalid.cmp(&y.globalid));
    vluchtig.sort();
    Verschil {
        toegevoegd,
        verwijderd,
        gewijzigd,
        losse_knopen: losse_knopen(va, &va.losse_groepen(), vb, &vb.losse_groepen()),
        header: header_verschil(va, vb),
        vluchtig,
        statistiek: Statistiek { a: va.statistiek.clone(), b: vb.statistiek.clone(), relaties_gekoppeld },
    }
}

/// Kan [`vluchtige_velden`] bij dit object iets vinden: heeft het type vluchtige velden, of verwijst het rechtstreeks
/// naar een knoop met vluchtige velden?
fn heeft_vluchtig(v: &Versie, o: usize) -> bool {
    let n = v.objecten[o].n;
    v.vlag(n) & VLUCHTIG_TYPE != 0
        || v.geparst(n).refs.iter().any(|&d| v.vlag(d) & (VLUCHTIG_TYPE | OBJECT) == VLUCHTIG_TYPE)
}
