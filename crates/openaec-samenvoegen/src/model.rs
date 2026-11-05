//! Het objectmodel van een IFC-bestand.
//!
//! Een *object* is een instantie van IfcRoot of een subtype ervan; zijn sleutel is de GlobalId. Al het andere is een
//! *knoop* (geometrie, plaatsing, eigenschapswaarden, IfcOwnerHistory, eenheden...).
//!
//! **Ankers.** Een knoop waar een object rechtstreeks naar verwijst (bijv. IfcProduct.ObjectPlacement), is een
//! *ankerknoop* van dat object, met als anker `(sleutel van het object, attribuut, positie)`. In een LIST of een
//! enkel attribuut is de positie het hoeveelste `#n`. Een SET of BAG heeft geen volgorde: daar koppelen we de leden
//! tussen twee versies alleen als dat eenduidig kan ([`stem_ankers_af`], ontwerp §5.6), en een lid zonder koppeling
//! krijgt een eigen positie (zie [`Anker::positie`]). De identiteit van een ankerknoop is
//! de gesorteerde verzameling van zijn ankers. Een *gedeelde knoop* (waar meer objecten rechtstreeks naar verwijzen,
//! bijv. IfcOwnerHistory of een gedeelde plaatsing) heeft dus meer ankers; er is geen gekozen vertegenwoordiger.
//!
//! In de identiteit tellen alleen de ankers van *gemeenschappelijke* objecten mee: objecten die in alle vergeleken
//! versies staan ([`markeer_gemeen`]). Zo verandert een gedeelde IfcOwnerHistory niet als een versie een object
//! toevoegt. Heeft een knoop geen anker van een gemeenschappelijk object, dan tellen al zijn ankers.
//!
//! **Hashes.** Elke knoop krijgt een inhoudshash (BLAKE3): het type en de attributen. Daarin wordt een verwijzing
//! naar een object de sleutel van dat object, een verwijzing naar een ankerknoop zijn identiteit (zijn ankers), en
//! een verwijzing naar een andere knoop de inhoudshash van die knoop. Een object krijgt per attribuut (zonder de
//! GlobalId) een hash waarin zijn eigen ankerknopen op inhoud meetellen, en een hash over het type en die
//! attribuuthashes. Zo is een wand die aan de plaatsing van P hangt, anders dan een wand aan de plaatsing van Q, ook
//! als die twee plaatsingen dezelfde inhoud hebben. Bij een ankerknoop telt ook mee met wie hij gedeeld is (behalve
//! bij IfcOwnerHistory): splitst een versie een gedeelde plaatsing, dan veranderen de objecten die hem deelden.
//!
//! Getallen vergelijken we precies op hun waarde (`1.`, `1.0` en `1.E0` zijn gelijk, zonder afronden), teksten na het
//! decoderen. Een SET of BAG vergelijken we zonder volgorde. De nummers `#n` doen nergens mee. Vluchtige velden
//! ([`crate::schema::VLUCHTIG`]) tellen niet mee.
//!
//! **Losse knopen.** Knopen waar niets naar verwijst (bijv. IfcStyledItem, IfcIndexedColourMap,
//! IfcPresentationLayerAssignment), horen bij geen object maar dragen wel inhoud. Ze krijgen een eigen sleutel: het
//! type, het attribuut Name en wat ze met één verwijzing aanwijzen.
//!
//! Een model bouwen gaat in twee stappen: [`Model::lees`] deelt de instanties in, [`Model::bereken`] rekent ankers
//! en hashes uit. Daartussen kan [`koppel_relaties`] de sleutels van relaties aanpassen.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use crate::ontleden::{self, elementen, Bestand, Token, Tokens};
use crate::schema::{self, Aggregaat, Entiteit};
use crate::tekst::{decodeer, kort};
use crate::Fout;

pub(crate) type Hash = [u8; 32];

/// Een element van een lijst van verwijzingen: de identiteit (zie [`Model::verwijzing_id`]) en het bereik in de
/// tekst van de lijst.
pub(crate) type Element = (Vec<u8>, (usize, usize));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Soort {
    /// Een object; de index in [`Model::objecten`].
    Object(u32),
    Knoop,
}

pub(crate) struct Object {
    pub inst: u32,
    pub globalid: String,
    /// Sleutel waaronder het object samengevoegd wordt: de GlobalId, of de GlobalId in de basis van een gekoppelde
    /// relatie ([`koppel_relaties`]).
    pub sleutel: String,
    pub hash: Hash,
    /// Staat de sleutel in alle vergeleken versies? Alleen dan telt het object mee in de identiteit van zijn
    /// ankerknopen ([`markeer_gemeen`]).
    pub gemeen: bool,
}

/// Een anker: het object, het attribuut en de positie van de verwijzing in dat attribuut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Anker {
    pub object: u32,
    pub attribuut: u32,
    /// In een LIST of een enkel attribuut: het hoeveelste `#n`. In een SET of BAG: [`ONGEORDEND`] plus een hash van
    /// het lid (zijn plek in de basis als het gekoppeld is, anders zijn inhoud en plek in deze versie) en het
    /// hoeveelste `#n` erin ([`set_positie`]).
    pub positie: u64,
}

/// Bit in [`Anker::positie`] voor een verwijzing in een SET of BAG.
pub(crate) const ONGEORDEND: u64 = 1 << 63;

/// In [`Model::eigenaar`]: de knoop hoort bij geen object.
pub(crate) const GEEN_EIGENAAR: u32 = u32::MAX;
/// In [`Model::eigenaar`]: de knoop hoort bij meer dan één object.
pub(crate) const MEER_EIGENAARS: u32 = u32::MAX - 1;

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
    /// Knopen waar meer dan één object rechtstreeks naar verwijst (bijv. IfcOwnerHistory).
    pub gedeelde_knopen: usize,
    /// Types die niet in het schema staan, met het aantal instanties.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub onbekende_typen: BTreeMap<String, usize>,
    /// Instanties van een bekend type met een ander aantal attributen dan het schema zegt.
    pub afwijkend_aantal_attributen: usize,
    /// Objecten herkend aan een GlobalId als eerste attribuut, omdat het type onbekend is.
    pub objecten_via_heuristiek: usize,
    /// Instanties van een IfcRoot-type zonder GlobalId (behandeld als knoop).
    pub root_zonder_globalid: usize,
    /// Kringen tussen knopen (ook via ankerknopen): het aantal verwijzingen terug.
    pub cycli: usize,
}

/// Een lid van een SET of BAG ([`Model::set_elementen`]): per `#n` de knoop (None voor een object) en de plek van
/// `#n` in de tekst van het attribuut, en de inhoudshash zonder ankers.
type Lid = (Vec<(Option<u32>, usize)>, Hash);

/// Per instantie de instanties die naar haar verwijzen ([`Model::verwijzers`]).
pub(crate) struct Verwijzers {
    begin: Vec<u32>,
    van: Vec<u32>,
}

impl Verwijzers {
    pub fn van(&self, k: usize) -> &[u32] {
        &self.van[self.begin[k] as usize..self.begin[k + 1] as usize]
    }
}

pub(crate) struct Model<'a> {
    pub bestand: Bestand<'a>,
    pub schema: Option<&'static schema::Schema>,
    type_entiteit: Vec<Option<&'static Entiteit>>,
    pub soort: Vec<Soort>,
    pub objecten: Vec<Object>,
    pub op_globalid: HashMap<String, u32>,
    /// Alle ankers, gesorteerd per knoop en dan op (sleutel, attribuut, positie); [`Model::ankers`] geeft die van
    /// één knoop (na [`Model::bereken`]).
    anker_lijst: Vec<Anker>,
    anker_bereik: Vec<(u32, u32)>,
    /// De identiteit van een ankerknoop: de hash van zijn gesorteerde ankers; None = geen ankerknoop.
    pub anker_id: Vec<Option<Hash>>,
    /// Hoeveel verschillende objecten rechtstreeks naar de knoop verwijzen (na [`Model::bereken`]).
    pub verankerd_door: Vec<u32>,
    /// Telt de knoop in zijn identiteit meer dan één object (zie [`Model::deelt_telt`])?
    gedeeld: Vec<bool>,
    /// Inhoudshash per instantie (alleen voor knopen; na [`Model::bereken`]).
    pub knoop_hash: Vec<Hash>,
    /// Losse knopen (instantie-indexen), met hun sleutel (na [`Model::bereken`]).
    pub wortels: Vec<u32>,
    pub wortel_sleutel: Vec<Hash>,
    /// Inhoudshashes zonder ankers, voor de elementen van een SET of BAG van een object en wat eronder hangt
    /// ([`Model::bereken_puur`]).
    puur: Option<HashMap<u32, Hash>>,
    /// De koppeling van de leden van elke SET of BAG met die van de basis: (object, attribuut, lid) -> lid in de
    /// basis ([`stem_ankers_af`]). None: deze versie is zelf de basis.
    koppeling: Option<HashMap<(u32, u32, u32), u32>>,
    /// Per instantie het object (index in `objecten`) waar een knoop bij hoort, of [`GEEN_EIGENAAR`] of
    /// [`MEER_EIGENAARS`] (na [`Model::bereken`]). Vanaf een object tellen zijn knopen rechtstreeks, en daaronder
    /// de knopen zonder anker; een ankerknoop van een ander object hoort bij dat object.
    pub eigenaar: Vec<u32>,
    /// Een knoop die via andere knopen (ook ankerknopen) naar zichzelf verwijst, als die er is (na
    /// [`Model::bereken`]).
    pub kring: Option<u32>,
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
            .map(|naam| if naam.contains('+') { None } else { schema.and_then(|s| s.entiteit(naam)) })
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
                        return Err(Fout::DubbeleGlobalId { globalid, regel: bestand.regel(inst.bereik.0 as usize) });
                    }
                    objecten.push(Object {
                        inst: i as u32,
                        sleutel: globalid.clone(),
                        globalid,
                        hash: NUL,
                        gemeen: true,
                    });
                    Soort::Object(o)
                }
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
            .filter(|&i| !heeft_ouder[i] && soort[i] == Soort::Knoop)
            .map(|i| i as u32)
            .collect();
        statistiek.objecten = objecten.len();
        statistiek.knopen = bestand.instanties.len() - objecten.len();
        statistiek.losse_knopen = wortels.len();
        Ok(Model {
            anker_lijst: Vec::new(),
            anker_bereik: Vec::new(),
            anker_id: Vec::new(),
            verankerd_door: Vec::new(),
            gedeeld: Vec::new(),
            knoop_hash: Vec::new(),
            bestand,
            schema,
            type_entiteit,
            soort,
            objecten,
            op_globalid,
            wortels,
            wortel_sleutel: Vec::new(),
            puur: None,
            koppeling: None,
            eigenaar: Vec::new(),
            kring: None,
            statistiek,
        })
    }

    /// De schema-entiteit van een instantie, als het type bekend is en het aantal attributen klopt.
    pub fn entiteit(&self, i: usize) -> Option<&'static Entiteit> {
        let inst = &self.bestand.instanties[i];
        self.type_entiteit[inst.type_id as usize].filter(|e| e.attributen.len() == self.bestand.attributen(i).len())
    }

    /// Is attribuut `j` van instantie `i` vluchtig (telt niet mee)?
    pub fn is_vluchtig(&self, i: usize, j: usize) -> bool {
        self.entiteit(i).is_some_and(|e| e.attributen[j].vluchtig)
    }

    /// Is dit trefwoord een TYPE dat een SET of BAG is (bijv. IFCPROPERTYSETDEFINITIONSET)?
    fn is_ongeordend_type(&self, trefwoord: &[u8]) -> bool {
        self.schema.is_some_and(|s| {
            std::str::from_utf8(trefwoord).is_ok_and(|w| s.is_ongeordend_type(&w.to_ascii_uppercase()))
        })
    }

    /// Is attribuut `j` van instantie `i` een SET of BAG (de volgorde telt niet)?
    pub fn is_ongeordend(&self, i: usize, j: usize) -> bool {
        self.entiteit(i).is_some_and(|e| matches!(e.attributen[j].aggregaat, Some(Aggregaat::Set | Aggregaat::Bag)))
    }

    /// Telt bij een verwijzing van een object naar deze ankerknoop mee met wie hij gedeeld is? Alleen als hij gedeeld
    /// is (met meer dan één object in zijn identiteit), en niet bij IfcOwnerHistory: sommige programma's geven een
    /// gewijzigd object een eigen IfcOwnerHistory, en dat is geen wijziging van de andere objecten.
    pub fn deelt_telt(&self, k: usize) -> bool {
        self.gedeeld[k] && self.bestand.type_naam(k) != "IFCOWNERHISTORY"
    }

    /// Rekent de inhoudshashes zonder ankers uit, voor het koppelen van de leden van een SET of BAG. Vóór
    /// [`stem_ankers_af`];
    /// [`Model::bereken`] doet het zelf als het nog niet gebeurd is.
    pub fn bereken_puur(&mut self) {
        self.puur = Some(self.pure_hashes());
    }

    /// Rekent ankers en de hashes van knopen, objecten en losse knopen uit.
    pub fn bereken(&mut self) {
        if self.puur.is_none() {
            self.bereken_puur();
        }
        self.bereken_ankers();
        let n = self.bestand.instanties.len();
        let mut hashes = vec![NUL; n];
        // 0 = nog niet bezocht, 1 = bezig (op de stapel), 2 = klaar.
        let mut status = vec![0u8; n];
        let mut stapel: Vec<(u32, u32)> = Vec::new();
        let mut buffer = Vec::new();
        for start in 0..n {
            if status[start] != 0 || self.soort[start] != Soort::Knoop {
                continue;
            }
            status[start] = 1;
            stapel.push((start as u32, 0));
            while let Some(&mut (knoop, ref mut volgende)) = stapel.last_mut() {
                let refs = self.bestand.verwijzingen(knoop as usize);
                if (*volgende as usize) < refs.len() {
                    let kind = refs[*volgende as usize] as usize;
                    *volgende += 1;
                    // Een object of een ankerknoop telt als sleutel of anker mee: geen inhoud nodig.
                    if self.soort[kind] != Soort::Knoop || self.anker_id[kind].is_some() {
                        continue;
                    }
                    // Status 1 is een kring; die meldt `kringen`.
                    if status[kind] == 0 {
                        status[kind] = 1;
                        stapel.push((kind as u32, 0));
                    }
                    continue;
                }
                stapel.pop();
                let k = knoop as usize;
                buffer.clear();
                let canon = Canon { model: self, hashes: &hashes, puur: None };
                canon.instantie(k, &mut buffer);
                hashes[k] = *blake3::hash(&buffer).as_bytes();
                status[k] = 2;
            }
        }
        self.knoop_hash = hashes;
        let (cycli, kring) = self.kringen();
        self.statistiek.cycli = cycli;
        self.kring = kring;

        let objecthashes: Vec<Hash> = self.objecten.iter().map(|o| self.object_hash(o.inst as usize)).collect();
        for (o, h) in self.objecten.iter_mut().zip(objecthashes) {
            o.hash = h;
        }
        self.eigenaar = self.eigenaars();
        let mut terug = None;
        self.wortel_sleutel = self.wortels.iter().map(|&w| self.wortel_sleutel_van(w as usize, &mut terug)).collect();
    }

    /// Kringen tussen knopen, ook via ankerknopen (bijv. twee plaatsingen die relatief aan elkaar staan): het aantal
    /// verwijzingen terug en de eerste knoop waar we er een vonden. Een hash stopt bij een anker; deze controle niet.
    fn kringen(&self) -> (usize, Option<u32>) {
        let n = self.bestand.instanties.len();
        // 0 = nog niet bezocht, 1 = op de stapel, 2 = klaar.
        let mut status = vec![0u8; n];
        let mut stapel: Vec<(u32, u32)> = Vec::new();
        let (mut aantal, mut eerste) = (0, None);
        for start in 0..n {
            if status[start] != 0 || self.soort[start] != Soort::Knoop {
                continue;
            }
            status[start] = 1;
            stapel.push((start as u32, 0));
            while let Some(&mut (k, ref mut volgende)) = stapel.last_mut() {
                let refs = self.bestand.verwijzingen(k as usize);
                if (*volgende as usize) < refs.len() {
                    let c = refs[*volgende as usize] as usize;
                    *volgende += 1;
                    if self.soort[c] != Soort::Knoop {
                        continue;
                    }
                    match status[c] {
                        0 => {
                            status[c] = 1;
                            stapel.push((c as u32, 0));
                        }
                        1 => {
                            aantal += 1;
                            eerste.get_or_insert(c as u32);
                        }
                        _ => {}
                    }
                    continue;
                }
                status[k as usize] = 2;
                stapel.pop();
            }
        }
        (aantal, eerste)
    }

    /// Legt per knoop zijn ankers vast, zijn identiteit, en hoeveel objecten rechtstreeks naar hem verwijzen.
    fn bereken_ankers(&mut self) {
        let n = self.bestand.instanties.len();
        // Rang van elk object in de volgorde van de sleutels, zodat we op getallen kunnen sorteren.
        let mut volgorde: Vec<u32> = (0..self.objecten.len() as u32).collect();
        volgorde.sort_by(|&a, &b| self.objecten[a as usize].sleutel.cmp(&self.objecten[b as usize].sleutel));
        let mut rang = vec![0u32; self.objecten.len()];
        for (r, &o) in volgorde.iter().enumerate() {
            rang[o as usize] = r as u32;
        }
        let mut alle: Vec<(u32, u32, Anker)> = Vec::new();
        for (o, object) in self.objecten.iter().enumerate() {
            let i = object.inst as usize;
            for j in 1..self.bestand.attributen(i).len() {
                self.posities(i, j, |k, positie, _| {
                    alle.push((k, rang[o], Anker { object: o as u32, attribuut: j as u32, positie }));
                });
            }
        }
        alle.sort_by_key(|&(k, r, a)| (k, r, a.attribuut, a.positie));
        let mut bereik = vec![(0u32, 0u32); n];
        let mut id = vec![None; n];
        let mut aantal = vec![0u32; n];
        let mut gedeeld = vec![false; n];
        let mut i = 0;
        while i < alle.len() {
            let k = alle[i].0 as usize;
            let begin = i;
            let mut gemeen = blake3::Hasher::new();
            let mut allemaal = blake3::Hasher::new();
            allemaal.update(b"X");
            let mut heeft_gemeen = false;
            let mut gemeen_aantal = 0;
            let mut vorige = u32::MAX;
            while i < alle.len() && alle[i].0 as usize == k {
                let a = alle[i].2;
                let object = &self.objecten[a.object as usize];
                if a.object != vorige {
                    aantal[k] += 1;
                    gemeen_aantal += u32::from(object.gemeen);
                    vorige = a.object;
                }
                let sleutel = object.sleutel.as_bytes();
                for h in [&mut allemaal].into_iter().chain(object.gemeen.then_some(&mut gemeen)) {
                    h.update(&(sleutel.len() as u32).to_le_bytes());
                    h.update(sleutel);
                    h.update(&a.attribuut.to_le_bytes());
                    h.update(&a.positie.to_le_bytes());
                }
                heeft_gemeen |= object.gemeen;
                i += 1;
            }
            bereik[k] = (begin as u32, (i - begin) as u32);
            let hasher = if heeft_gemeen { gemeen } else { allemaal };
            id[k] = Some(*hasher.finalize().as_bytes());
            gedeeld[k] = if heeft_gemeen { gemeen_aantal > 1 } else { aantal[k] > 1 };
        }
        self.statistiek.gedeelde_knopen = aantal.iter().filter(|&&a| a > 1).count();
        self.anker_lijst = alle.into_iter().map(|(_, _, a)| a).collect();
        self.anker_bereik = bereik;
        self.anker_id = id;
        self.verankerd_door = aantal;
        self.gedeeld = gedeeld;
    }

    /// Roept `f(knoop, positie, begin)` aan voor elke verwijzing naar een knoop in attribuut `j` van
    /// object-instantie `i` (zie [`Anker::positie`]); `begin` is de plek van `#n` in de tekst van het attribuut.
    /// Na [`Model::bereken`], of tijdens [`Model::bereken_ankers`].
    pub fn posities(&self, i: usize, j: usize, mut f: impl FnMut(u32, u64, usize)) {
        let tekst = attribuut_tekst(self, i, j);
        let knoop = |n: u64| self.bestand.index.get(&n).copied().filter(|&k| self.soort[k as usize] == Soort::Knoop);
        let delen = if self.is_ongeordend(i, j) { elementen(tekst) } else { None };
        let Some(delen) = delen else {
            let mut p = 0u64;
            for (begin, token) in Tokens::nieuw(tekst) {
                if let Token::Verwijzing(n) = token {
                    if let Some(k) = knoop(n) {
                        f(k, p, begin);
                    }
                    p += 1;
                }
            }
            return;
        };
        let o = match self.soort[i] {
            Soort::Object(o) => o,
            Soort::Knoop => u32::MAX,
        };
        for (e, (doelen, inhoud)) in self.set_elementen(tekst, delen).into_iter().enumerate() {
            let e = e as u32;
            let lid = match &self.koppeling {
                None => Some(e),
                Some(koppeling) => koppeling.get(&(o, j as u32, e)).copied(),
            };
            for (r, (doel, begin)) in doelen.into_iter().enumerate() {
                if let Some(k) = doel {
                    f(k, set_positie(lid, &inhoud, e, r as u32), begin);
                }
            }
        }
    }

    /// De leden `delen` van een SET of BAG met minstens één verwijzing naar een knoop: per lid de knopen (per `#n`:
    /// de knoop of None voor een object, en de plek van `#n` in `tekst`) en de inhoudshash zonder ankers.
    fn set_elementen(&self, tekst: &[u8], delen: Vec<(usize, usize)>) -> Vec<Lid> {
        debug_assert!(self.puur.is_some(), "eerst bereken_puur");
        let knoop = |n: u64| self.bestand.index.get(&n).copied().filter(|&k| self.soort[k as usize] == Soort::Knoop);
        let mut buffer = Vec::new();
        let mut uit = Vec::new();
        for (b, e) in delen {
            let stuk = &tekst[b..e];
            let doelen: Vec<(Option<u32>, usize)> = Tokens::nieuw(stuk)
                .filter_map(|(plek, t)| match t {
                    Token::Verwijzing(n) => Some((knoop(n), b + plek)),
                    _ => None,
                })
                .collect();
            if doelen.iter().all(|d| d.0.is_none()) {
                continue;
            }
            buffer.clear();
            Canon { model: self, hashes: &[], puur: self.puur.as_ref() }.waarde(stuk, false, &mut buffer);
            uit.push((doelen, *blake3::hash(&buffer).as_bytes()));
        }
        uit
    }

    /// De leden van SET- of BAG-attribuut `j` van object-instantie `i` ([`Model::set_elementen`]); None als het
    /// attribuut geen SET of BAG is.
    fn set_leden(&self, i: usize, j: usize) -> Option<Vec<Lid>> {
        if !self.is_ongeordend(i, j) {
            return None;
        }
        let tekst = attribuut_tekst(self, i, j);
        Some(self.set_elementen(tekst, elementen(tekst)?))
    }

    /// De attributen van knoop `k` als hashes zonder ankers (na [`Model::bereken_puur`]).
    fn pure_attributen(&self, k: usize) -> Vec<Hash> {
        let canon = Canon { model: self, hashes: &[], puur: self.puur.as_ref() };
        (0..self.bestand.attributen(k).len())
            .map(|j| {
                let mut buffer = Vec::new();
                canon.attribuut(k, j, false, &mut buffer);
                *blake3::hash(&buffer).as_bytes()
            })
            .collect()
    }

    /// De objecten die (rechtstreeks of via knopen) naar knoop `k` verwijzen en in alle versies staan, als
    /// gesorteerde sleutels.
    fn verwijzende_objecten(&self, k: usize, terug: &Verwijzers) -> Vec<&str> {
        let mut objecten = Vec::new();
        let mut gezien: HashSet<u32> = HashSet::from([k as u32]);
        let mut stapel = vec![k as u32];
        while let Some(x) = stapel.pop() {
            for &r in terug.van(x as usize) {
                match self.soort[r as usize] {
                    Soort::Object(o) if self.objecten[o as usize].gemeen => {
                        objecten.push(self.objecten[o as usize].sleutel.as_str())
                    }
                    Soort::Object(_) => {}
                    Soort::Knoop if gezien.insert(r) => stapel.push(r),
                    Soort::Knoop => {}
                }
            }
        }
        objecten.sort_unstable();
        objecten.dedup();
        objecten
    }

    /// De knoop op positie `positie` in attribuut `j` van object-instantie `i` (zie [`Anker::positie`]), met de plek
    /// van zijn `#n` in de tekst van het attribuut.
    pub fn op_positie(&self, i: usize, j: usize, positie: u64) -> Option<(u32, usize)> {
        let mut gevonden = None;
        self.posities(i, j, |k, p, begin| {
            if p == positie && gevonden.is_none() {
                gevonden = Some((k, begin));
            }
        });
        gevonden
    }

    /// Inhoudshashes zonder ankers (elke knoop telt op inhoud) van de knopen in een SET of BAG van een object, en van
    /// wat eronder hangt. Een kring telt als een lege hash.
    fn pure_hashes(&self) -> HashMap<u32, Hash> {
        let mut puur: HashMap<u32, Hash> = HashMap::new();
        let mut bezig: HashSet<u32> = HashSet::new();
        let mut stapel: Vec<(u32, u32)> = Vec::new();
        let mut buffer = Vec::new();
        for object in &self.objecten {
            let i = object.inst as usize;
            for j in 1..self.bestand.attributen(i).len() {
                if !self.is_ongeordend(i, j) {
                    continue;
                }
                for (_, token) in Tokens::nieuw(attribuut_tekst(self, i, j)) {
                    let Token::Verwijzing(n) = token else { continue };
                    let Some(&start) = self.bestand.index.get(&n) else { continue };
                    if self.soort[start as usize] != Soort::Knoop || puur.contains_key(&start) {
                        continue;
                    }
                    bezig.insert(start);
                    stapel.push((start, 0));
                    while let Some(&mut (k, ref mut volgende)) = stapel.last_mut() {
                        let refs = self.bestand.verwijzingen(k as usize);
                        if (*volgende as usize) < refs.len() {
                            let c = refs[*volgende as usize];
                            *volgende += 1;
                            if self.soort[c as usize] == Soort::Knoop && !puur.contains_key(&c) && bezig.insert(c) {
                                stapel.push((c, 0));
                            }
                            continue;
                        }
                        stapel.pop();
                        buffer.clear();
                        Canon { model: self, hashes: &[], puur: Some(&puur) }.instantie(k as usize, &mut buffer);
                        puur.insert(k, *blake3::hash(&buffer).as_bytes());
                        bezig.remove(&k);
                    }
                }
            }
        }
        puur
    }

    /// De ankers van een knoop, gesorteerd op (sleutel, attribuut, positie).
    pub fn ankers(&self, k: usize) -> &[Anker] {
        let (begin, aantal) = self.anker_bereik[k];
        &self.anker_lijst[begin as usize..(begin + aantal) as usize]
    }

    fn canon(&self) -> Canon<'_, 'a> {
        Canon { model: self, hashes: &self.knoop_hash, puur: None }
    }

    /// De canonieke vorm van een waarde van dit bestand (na [`Model::bereken`]). `direct`: de waarde is een
    /// attribuut van een object, dus zijn ankerknopen tellen op inhoud mee; anders tellen ankerknopen als anker.
    pub fn canoniek(&self, tekst: &[u8], direct: bool, uit: &mut Vec<u8>) {
        self.canon().waarde(tekst, direct, uit);
    }

    /// De hash van een waarde (een attribuut of een element van een lijst); zie [`Model::canoniek`].
    pub fn waarde_hash(&self, tekst: &[u8], direct: bool) -> Hash {
        let mut buffer = Vec::new();
        self.canoniek(tekst, direct, &mut buffer);
        *blake3::hash(&buffer).as_bytes()
    }

    /// De hash van attribuut `j` van instantie `i` (een object of een knoop). Een vluchtig attribuut heeft altijd
    /// dezelfde hash; een SET of BAG telt zonder volgorde.
    pub fn attribuut_hash(&self, i: usize, j: usize) -> Hash {
        if self.is_vluchtig(i, j) {
            return *blake3::hash(b"V").as_bytes();
        }
        let direct = matches!(self.soort[i], Soort::Object(_));
        let mut buffer = Vec::new();
        self.canon().attribuut(i, j, direct, &mut buffer);
        *blake3::hash(&buffer).as_bytes()
    }

    /// Hash van een object: het type en de hashes van alle attributen behalve de GlobalId.
    fn object_hash(&self, i: usize) -> Hash {
        let hashes: Vec<Hash> = (1..self.bestand.attributen(i).len()).map(|j| self.attribuut_hash(i, j)).collect();
        Self::object_hash_uit(self.bestand.type_naam(i), &hashes)
    }

    /// De knoop waar attribuut `j` van object-instantie `i` als enige verwijzing naar wijst, als die knoop alleen bij
    /// dat object hoort: één anker (dit attribuut), niet gedeeld. Bijv. de IfcTaskTime van een IfcTask.
    pub fn eigen_knoop(&self, i: usize, j: usize) -> Option<usize> {
        let n = enkele_verwijzing(attribuut_tekst(self, i, j))?;
        let c = *self.bestand.index.get(&n)? as usize;
        (self.soort[c] == Soort::Knoop && self.anker_id[c].is_some() && self.ankers(c).len() == 1).then_some(c)
    }

    /// De hash van een attribuut dat rechtstreeks naar een eigen knoop wijst ([`Model::eigen_knoop`]), waarvan elk
    /// attribuut uit een andere versie kan komen: per attribuut het model en de knoop. Dezelfde vorm als
    /// [`Model::attribuut_hash`] bij zo'n verwijzing (`K` en de inhoud van de knoop).
    pub fn samengestelde_knoop_hash(type_naam: &str, delen: &[(&Model, usize)]) -> Hash {
        let mut buffer = vec![b'T'];
        lengte_en(type_naam.as_bytes(), &mut buffer);
        buffer.push(b'(');
        for (l, &(m, c)) in delen.iter().enumerate() {
            if m.is_vluchtig(c, l) {
                buffer.push(b'V');
            } else {
                m.canon().attribuut(c, l, false, &mut buffer);
            }
        }
        buffer.push(b')');
        let mut attribuut = vec![b'K'];
        attribuut.extend_from_slice(blake3::hash(&buffer).as_bytes());
        *blake3::hash(&attribuut).as_bytes()
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

    /// Zie [`Model::eigenaar`].
    fn eigenaars(&self) -> Vec<u32> {
        let mut e = vec![GEEN_EIGENAAR; self.bestand.instanties.len()];
        let mut stapel: Vec<u32> = Vec::new();
        for (o, object) in self.objecten.iter().enumerate() {
            let o = o as u32;
            let refs = self.bestand.verwijzingen(object.inst as usize);
            stapel.extend(refs.iter().copied().filter(|&c| self.soort[c as usize] == Soort::Knoop));
            while let Some(x) = stapel.pop() {
                let x = x as usize;
                match e[x] {
                    GEEN_EIGENAAR => e[x] = o,
                    y if y == o || y == MEER_EIGENAARS => continue,
                    _ => e[x] = MEER_EIGENAARS,
                }
                for &c in self.bestand.verwijzingen(x) {
                    if self.soort[c as usize] == Soort::Knoop && self.anker_id[c as usize].is_none() {
                        stapel.push(c);
                    }
                }
            }
        }
        e
    }

    /// Wie naar elke instantie verwijst, voor [`Model::alle_eigenaars`].
    pub fn verwijzers(&self) -> Verwijzers {
        let n = self.bestand.instanties.len();
        let mut begin = vec![0u32; n + 1];
        for i in 0..n {
            for &c in self.bestand.verwijzingen(i) {
                begin[c as usize + 1] += 1;
            }
        }
        for i in 0..n {
            begin[i + 1] += begin[i];
        }
        let mut plek = begin.clone();
        let mut van = vec![0u32; begin[n] as usize];
        for i in 0..n {
            for &c in self.bestand.verwijzingen(i) {
                van[plek[c as usize] as usize] = i as u32;
                plek[c as usize] += 1;
            }
        }
        Verwijzers { begin, van }
    }

    /// Alle objecten waar knoop `k` bij hoort (zie [`Model::eigenaar`]), gesorteerd: de objecten die rechtstreeks
    /// naar hem verwijzen en, als hij geen ankerknoop is, de eigenaars van de knopen die naar hem verwijzen.
    pub fn alle_eigenaars(&self, k: usize, terug: &Verwijzers) -> Vec<u32> {
        let mut objecten = Vec::new();
        let mut gezien: HashSet<u32> = HashSet::from([k as u32]);
        let mut stapel = vec![k as u32];
        while let Some(x) = stapel.pop() {
            let anker = self.anker_id[x as usize].is_some();
            for &r in terug.van(x as usize) {
                match self.soort[r as usize] {
                    Soort::Object(o) => objecten.push(o),
                    Soort::Knoop if !anker && gezien.insert(r) => stapel.push(r),
                    Soort::Knoop => {}
                }
            }
        }
        objecten.sort_unstable();
        objecten.dedup();
        objecten
    }

    /// Een identiteit van een groep eigenaars die tussen de versies te vergelijken is: een hash over de sleutels van
    /// de objecten die in alle drie de versies staan, of over alle sleutels als er zo geen object bij is (net als
    /// bij de identiteit van een anker).
    pub fn eigenaars_id(&self, objecten: &[u32]) -> Hash {
        let gemeen = objecten.iter().any(|&o| self.objecten[o as usize].gemeen);
        let mut sleutels: Vec<&str> = objecten
            .iter()
            .map(|&o| &self.objecten[o as usize])
            .filter(|x| x.gemeen || !gemeen)
            .map(|x| x.sleutel.as_str())
            .collect();
        sleutels.sort_unstable();
        sleutels.dedup();
        let mut h = blake3::Hasher::new();
        h.update(if gemeen { b"G" } else { b"X" });
        for s in sleutels {
            h.update(&(s.len() as u32).to_le_bytes());
            h.update(s.as_bytes());
        }
        *h.finalize().as_bytes()
    }

    /// Sleutel van een losse knoop: het type, het attribuut Name en de attributen met één verwijzing naar een object
    /// of naar een knoop die bij een object hoort (bijv. de geometrie waar een IfcStyledItem of IfcIndexedColourMap
    /// op ligt), met de sleutel van dat object. Zo valt de stijl van W in een andere groep dan die van V, ook als hun
    /// geometrie gelijk is. Hoort de knoop bij meer objecten, dan telt die verzameling ([`Model::eigenaars_id`]): de
    /// stijl op de vorm van W en V valt in een andere groep dan die op de vorm van X en Y. `terug` maken we pas als
    /// het nodig is. Een verwijzing naar een waarde die alleen de losse knoop gebruikt (bijv. de kleuren in
    /// `Colours`), hoort niet in de sleutel: anders vallen twee versies van dezelfde kleurkaart in verschillende
    /// groepen.
    fn wortel_sleutel_van(&self, i: usize, terug: &mut Option<Verwijzers>) -> Hash {
        let mut buffer = Vec::new();
        let naam = self.bestand.type_naam(i);
        lengte_en(naam.as_bytes(), &mut buffer);
        let entiteit = self.entiteit(i);
        for (j, &attr) in self.bestand.attributen(i).iter().enumerate() {
            let tekst = self.bestand.tekst(attr);
            let is_naam = entiteit.is_some_and(|e| e.attributen[j].naam == "Name");
            let doel = enkele_verwijzing(tekst).and_then(|n| self.bestand.index.get(&n)).map(|&k| k as usize);
            let eigenaar = doel.map(|k| match self.soort[k] {
                Soort::Object(o) => o,
                Soort::Knoop => self.eigenaar[k],
            });
            if is_naam || eigenaar.is_some_and(|e| e != GEEN_EIGENAAR) {
                buffer.extend_from_slice(&(j as u32).to_le_bytes());
                let mut deel = Vec::new();
                self.canoniek(tekst, false, &mut deel);
                match (eigenaar, doel) {
                    (Some(MEER_EIGENAARS), Some(k)) => {
                        let t = terug.get_or_insert_with(|| self.verwijzers());
                        deel.push(b'M');
                        deel.extend_from_slice(&self.eigenaars_id(&self.alle_eigenaars(k, t)));
                    }
                    (Some(o), _) if o != GEEN_EIGENAAR => {
                        deel.push(b'O');
                        lengte_en(self.objecten[o as usize].sleutel.as_bytes(), &mut deel);
                    }
                    _ => {}
                }
                lengte_en(&deel, &mut buffer);
            }
        }
        *blake3::hash(&buffer).as_bytes()
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

    /// Staat elke verwijzing naar een object (behalve de GlobalId) alleen in een attribuut? Een relatie met een SET of
    /// LIST van objecten (bijv. IfcRelContainedInSpatialStructure) is niet door haar objecten bepaald.
    pub fn alleen_enkele_verwijzingen(&self, o: usize) -> bool {
        let i = self.objecten[o].inst as usize;
        self.bestand.attributen(i).iter().skip(1).all(|&attr| {
            let tekst = self.bestand.tekst(attr);
            enkele_verwijzing(tekst).is_some()
                || Tokens::nieuw(tekst).all(|(_, t)| match t {
                    Token::Verwijzing(n) => {
                        !self.bestand.index.get(&n).is_some_and(|&k| matches!(self.soort[k as usize], Soort::Object(_)))
                    }
                    _ => true,
                })
        })
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
    /// paar niveaus diep uitgeschreven. `direct`: zie [`Model::canoniek`]; dieper staat een ankerknoop als
    /// `ObjectPlacement van Naam [GlobalId]`.
    pub fn toon(&self, tekst: &[u8], direct: bool, max: usize) -> String {
        let mut uit = String::new();
        self.toon_in(tekst, max, 2, direct, &mut uit);
        kort(&uit, max)
    }

    fn toon_in(&self, tekst: &[u8], max: usize, diepte: usize, direct: bool, uit: &mut String) {
        let mut vorige_was_waarde = false;
        for (_, token) in Tokens::nieuw(tekst) {
            if uit.len() > max * 4 {
                return;
            }
            if vorige_was_waarde && token != Token::Sluit {
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
                    Some(&k) => self.toon_verwijzing(k as usize, max, diepte, direct, uit),
                    None => uit.push_str(&format!("#{n}")),
                },
            }
            vorige_was_waarde = !matches!(token, Token::Open | Token::Trefwoord(_));
        }
    }

    fn toon_verwijzing(&self, k: usize, max: usize, diepte: usize, direct: bool, uit: &mut String) {
        if let Soort::Object(o) = self.soort[k] {
            uit.push_str(&self.object_weergave(o as usize));
            return;
        }
        if !direct && self.anker_id[k].is_some() {
            uit.push_str(&self.anker_weergave(k));
            return;
        }
        self.toon_knoop(k, max, diepte, uit);
    }

    fn toon_knoop(&self, k: usize, max: usize, diepte: usize, uit: &mut String) {
        let inst = &self.bestand.instanties[k];
        if diepte == 0 {
            uit.push_str(&self.type_weergave(k));
            uit.push_str("(…)");
        } else if inst.complex {
            uit.push('(');
            self.toon_in(self.bestand.tekst(inst.type_bereik), max, diepte - 1, false, uit);
            uit.push(')');
        } else {
            uit.push_str(&self.type_weergave(k));
            uit.push('(');
            for (j, &attr) in self.bestand.attributen(k).iter().enumerate() {
                if j > 0 {
                    uit.push_str(", ");
                }
                self.toon_in(self.bestand.tekst(attr), max, diepte - 1, false, uit);
            }
            uit.push(')');
        }
    }

    /// Een ankerknoop als `ObjectPlacement van Naam [GlobalId]`, met `(+n)` als hij gedeeld is.
    pub fn anker_weergave(&self, k: usize) -> String {
        let ankers = self.ankers(k);
        let Some(a) = ankers.first() else { return self.type_weergave(k) };
        let i = self.objecten[a.object as usize].inst as usize;
        let mut uit = self.attribuut_naam(i, a.attribuut as usize);
        if a.positie > 0 && a.positie & ONGEORDEND == 0 {
            uit += &format!("[{}]", a.positie + 1);
        }
        uit += " van ";
        uit += &self.object_weergave(a.object as usize);
        if ankers.len() > 1 {
            uit += &format!(" (+{})", ankers.len() - 1);
        }
        uit
    }

    /// Een korte leesbare tekst van een hele instantie.
    pub fn toon_instantie(&self, i: usize, max: usize) -> String {
        let mut uit = String::new();
        self.toon_knoop(i, max, 2, &mut uit);
        kort(&uit, max)
    }

    /// Een object als `Naam [GlobalId]`, of alleen `[GlobalId]`.
    pub fn object_weergave(&self, o: usize) -> String {
        let object = &self.objecten[o];
        match self.object_naam(object.inst as usize) {
            Some(naam) => format!("{} [{}]", kort(&naam, 60), object.globalid),
            None => format!("[{}]", object.globalid),
        }
    }

    /// De identiteit van het doel van een verwijzing: `G` + sleutel voor een object, `A` + identiteit voor een
    /// ankerknoop (alleen als niet `direct`), anders `K` + inhoudshash (bij een ankerknoop met `D` + met wie hij
    /// gedeeld is, zie [`Model::deelt_telt`]).
    pub fn verwijzing_id(&self, k: usize, direct: bool) -> Vec<u8> {
        let mut id = Vec::with_capacity(72);
        match self.soort[k] {
            Soort::Object(o) => {
                id.push(b'G');
                id.extend_from_slice(self.objecten[o as usize].sleutel.as_bytes());
            }
            Soort::Knoop => self.canon().knoop(k, direct, &mut id),
        }
        id
    }

    /// Met welke andere objecten een ankerknoop gedeeld is, zoals een mens het leest.
    pub fn gedeeld_met(&self, k: usize, eigen: Option<u32>) -> String {
        let mut anderen: Vec<u32> = self.ankers(k).iter().map(|a| a.object).filter(|&o| Some(o) != eigen).collect();
        anderen.dedup();
        if anderen.is_empty() {
            return "niet gedeeld".into();
        }
        let mut uit = String::from("gedeeld met ");
        for (n, o) in anderen.iter().take(3).enumerate() {
            if n > 0 {
                uit += ", ";
            }
            uit += &self.object_weergave(*o as usize);
        }
        if anderen.len() > 3 {
            uit += &format!(" (+{})", anderen.len() - 3);
        }
        uit
    }

    /// De elementen van een lijst als (identiteit, bereik), als ze allemaal verwijzingen zijn. None als de waarde
    /// geen lijst van verwijzingen is.
    pub fn verwijzingslijst(&self, tekst: &[u8], direct: bool) -> Option<Vec<Element>> {
        let delen = elementen(tekst)?;
        let mut uit = Vec::with_capacity(delen.len());
        for (b, e) in delen {
            let doel = enkele_verwijzing(&tekst[b..e])?;
            let &k = self.bestand.index.get(&doel)?;
            uit.push((self.verwijzing_id(k as usize, direct), (b, e)));
        }
        Some(uit)
    }

    /// De canonieke hash van parameter `j` van de HEADER-regel `naam` (bijv. FILE_NAME), met de ruwe tekst.
    pub fn header_parameter(&self, naam: &str, j: usize) -> Option<(Hash, &'a [u8])> {
        let regel = self.bestand.header.iter().find(|r| r.naam == naam)?;
        let &bereik = regel.parameters.get(j)?;
        let tekst: &'a [u8] = &self.bestand.data[bereik.0 as usize..bereik.1 as usize];
        Some((self.waarde_hash(tekst, false), tekst))
    }
}

/// De parameters van de HEADER die we vergelijken, met hun naam. FILE_NAME.time_stamp verandert bij elk opslaan en
/// is vluchtig ([`header_vluchtig`]).
pub(crate) const HEADER_VELDEN: &[(&str, &[&str])] = &[
    ("FILE_DESCRIPTION", &["description", "implementation_level"]),
    (
        "FILE_NAME",
        &[
            "name",
            "time_stamp",
            "author",
            "organization",
            "preprocessor_version",
            "originating_system",
            "authorization",
        ],
    ),
    ("FILE_SCHEMA", &["schema_identifiers"]),
];

/// Is deze HEADER-parameter vluchtig?
pub(crate) fn header_vluchtig(naam: &str, j: usize) -> bool {
    naam == "FILE_NAME" && j == 1
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

/// Schrijft de canonieke vorm van waarden. Tijdens [`Model::bereken`] zijn de hashes van knopen in een kring nog
/// niet klaar; dan geeft [`crate::verschil`] een fout ([`Model::kring`]).
struct Canon<'m, 'a> {
    model: &'m Model<'a>,
    hashes: &'m [Hash],
    /// Zonder ankers: elke knoop telt op inhoud, uit deze tabel ([`Model::pure_hashes`]).
    puur: Option<&'m HashMap<u32, Hash>>,
}

impl Canon<'_, '_> {
    /// Een hele knoop: type en attributen; ankerknopen als anker, vluchtige attributen als `V`.
    fn instantie(&self, i: usize, uit: &mut Vec<u8>) {
        let b = &self.model.bestand;
        uit.push(b'T');
        lengte_en(b.type_naam(i).as_bytes(), uit);
        uit.push(b'(');
        for j in 0..b.attributen(i).len() {
            if self.model.is_vluchtig(i, j) {
                uit.push(b'V');
            } else {
                self.attribuut(i, j, false, uit);
            }
        }
        uit.push(b')');
    }

    /// Attribuut `j` van instantie `i`; een SET of BAG met de elementen gesorteerd.
    fn attribuut(&self, i: usize, j: usize, direct: bool, uit: &mut Vec<u8>) {
        let m = self.model;
        let tekst = attribuut_tekst(m, i, j);
        if m.is_ongeordend(i, j) {
            if let Some(delen) = elementen(tekst) {
                self.verzameling(tekst, delen, direct, uit);
                return;
            }
        }
        self.waarde(tekst, direct, uit);
    }

    /// De elementen `delen` van een SET of BAG in `tekst`, elk canoniek en gesorteerd: `{...}`.
    fn verzameling(&self, tekst: &[u8], delen: Vec<(usize, usize)>, direct: bool, uit: &mut Vec<u8>) {
        let stukken: Vec<Vec<u8>> = delen
            .into_iter()
            .map(|(b, e)| {
                let mut stuk = Vec::new();
                self.waarde(&tekst[b..e], direct, &mut stuk);
                stuk
            })
            .collect();
        ongeordend(stukken, uit);
    }

    fn waarde(&self, tekst: &[u8], direct: bool, uit: &mut Vec<u8>) {
        // `basis`: waar `tokens` begint in `tekst` (na een getypte SET gaan we verder achter het sluitende haakje).
        let mut basis = 0;
        let mut tokens = Tokens::nieuw(tekst);
        while let Some((_, token)) = tokens.next() {
            match token {
                Token::Open => uit.push(b'('),
                Token::Sluit => uit.push(b')'),
                Token::Leeg => uit.push(b'$'),
                Token::Afgeleid => uit.push(b'*'),
                Token::Verwijzing(n) => self.verwijzing(n, direct, uit),
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
                    // Een getypte SET of BAG, bijv. IFCPROPERTYSETDEFINITIONSET((#1,#2)): de volgorde telt niet.
                    if self.model.is_ongeordend_type(w) {
                        let na = basis + tokens.positie();
                        if let Some(g) = getypt_aggregaat(&tekst[na..]) {
                            uit.push(b'(');
                            self.verzameling(&tekst[na + g.binnen..], g.delen, direct, uit);
                            uit.push(b')');
                            basis = na + g.lengte;
                            tokens = Tokens::nieuw(&tekst[basis..]);
                        }
                    }
                }
            }
        }
    }

    fn verwijzing(&self, n: u64, direct: bool, uit: &mut Vec<u8>) {
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
            Soort::Knoop => self.knoop(k, direct, uit),
        }
    }

    /// Een verwijzing naar knoop `k`: zonder ankers op inhoud (`P`), als anker (`A`), of op inhoud (`K`), bij een
    /// verwijzing van een object naar een ankerknoop met erachter met wie hij gedeeld is (`D`).
    fn knoop(&self, k: usize, direct: bool, uit: &mut Vec<u8>) {
        let m = self.model;
        if let Some(puur) = self.puur {
            uit.push(b'P');
            uit.extend_from_slice(puur.get(&(k as u32)).unwrap_or(&NUL));
            return;
        }
        match m.anker_id[k] {
            Some(a) if !direct => {
                uit.push(b'A');
                uit.extend_from_slice(&a);
            }
            anker => {
                uit.push(b'K');
                uit.extend_from_slice(self.hashes.get(k).unwrap_or(&NUL));
                if let Some(a) = anker.filter(|_| m.deelt_telt(k)) {
                    uit.push(b'D');
                    uit.extend_from_slice(&a);
                }
            }
        }
    }
}

/// De canonieke vorm van een SET of BAG uit de canonieke vormen van de elementen: gesorteerd.
fn ongeordend(mut stukken: Vec<Vec<u8>>, uit: &mut Vec<u8>) {
    stukken.sort_unstable();
    uit.push(b'{');
    for stuk in &stukken {
        lengte_en(stuk, uit);
    }
    uit.push(b'}');
}

/// De hash van een SET of BAG uit de canonieke vormen van de elementen (zoals [`Model::attribuut_hash`]).
pub(crate) fn ongeordend_hash(stukken: Vec<Vec<u8>>) -> Hash {
    let mut uit = Vec::new();
    ongeordend(stukken, &mut uit);
    *blake3::hash(&uit).as_bytes()
}

/// Het aggregaat in een getypte waarde: `( aggregaat )` achter het trefwoord.
struct GetyptAggregaat {
    /// Tot en met het sluitende haakje van de getypte waarde.
    lengte: usize,
    /// Waar het aggregaat begint.
    binnen: usize,
    /// De elementen, vanaf `binnen`.
    delen: Vec<(usize, usize)>,
}

/// Het aggregaat achter het trefwoord van een getypte waarde (`rest` begint net na het trefwoord), of None als er
/// geen aggregaat staat.
fn getypt_aggregaat(rest: &[u8]) -> Option<GetyptAggregaat> {
    let mut tokens = Tokens::nieuw(rest);
    let Some((_, Token::Open)) = tokens.next() else { return None };
    let binnen = tokens.positie();
    let delen = elementen(&rest[binnen..])?;
    let mut diepte = 1usize;
    while let Some((_, token)) = tokens.next() {
        match token {
            Token::Open => diepte += 1,
            Token::Sluit => {
                diepte -= 1;
                if diepte == 0 {
                    return Some(GetyptAggregaat { lengte: tokens.positie(), binnen, delen });
                }
            }
            _ => {}
        }
    }
    None
}

fn lengte_en(bytes: &[u8], uit: &mut Vec<u8>) {
    uit.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    uit.extend_from_slice(bytes);
}

/// Een getal, precies en zonder afronden: het teken, de cijfers zonder nullen vooraan (en bij een REAL zonder
/// nullen achteraan) en bij een REAL de exponent. Zo zijn `1.`, `1.0` en `1.E0` gelijk, en `9007199254740992.` en
/// `9007199254740993.` niet (als f64 wel). Een REAL heeft een punt of een exponent. De parser controleerde het getal
/// al (`ontleden`), dus het is geldig.
fn getal(g: &[u8], uit: &mut Vec<u8>) {
    let (negatief, rest) = match g.first() {
        Some(b'-') => (true, &g[1..]),
        Some(b'+') => (false, &g[1..]),
        _ => (false, g),
    };
    let (mantisse, exponent) = match rest.iter().position(|&b| b == b'e' || b == b'E') {
        Some(p) => (&rest[..p], std::str::from_utf8(&rest[p + 1..]).ok().and_then(|t| t.parse::<i64>().ok())),
        None => (rest, Some(0)),
    };
    let (geheel, breuk) = match mantisse.iter().position(|&b| b == b'.') {
        Some(p) => (&mantisse[..p], &mantisse[p + 1..]),
        None => (mantisse, &mantisse[mantisse.len()..]),
    };
    let reeel = rest.len() != geheel.len();
    let geldig = geheel.len() + breuk.len() > 0 && geheel.iter().chain(breuk).all(u8::is_ascii_digit);
    let (Some(exponent), true) = (exponent, geldig) else {
        unreachable!("een getal dat de parser goedkeurde, is toch ongeldig: {}", String::from_utf8_lossy(g));
    };
    let cijfer = |p: usize| if p < geheel.len() { geheel[p] } else { breuk[p - geheel.len()] };
    let n = geheel.len() + breuk.len();
    let begin = (0..n).find(|&p| cijfer(p) != b'0').unwrap_or(n);
    let mut eind = n;
    if reeel {
        while eind > begin && cijfer(eind - 1) == b'0' {
            eind -= 1;
        }
    }
    let nul = begin == eind;
    uit.push(if reeel { b'R' } else { b'I' });
    uit.push(if negatief && !nul { b'-' } else { b'+' });
    uit.extend_from_slice(&((eind - begin) as u32).to_le_bytes());
    uit.extend((begin..eind).map(cijfer));
    if reeel {
        let e = if nul { 0 } else { exponent - breuk.len() as i64 + (n - eind) as i64 };
        uit.extend_from_slice(&e.to_le_bytes());
    }
}

/// De positie van verwijzing `r` in lid `e` van een SET of BAG: [`ONGEORDEND`] plus een hash van `lid` (het lid in de
/// basis waarmee het gekoppeld is) of, zonder koppeling, van de inhoud en `e`. Zo valt een lid zonder koppeling
/// nooit samen met een lid van de basis.
fn set_positie(lid: Option<u32>, inhoud: &Hash, e: u32, r: u32) -> u64 {
    let mut h = blake3::Hasher::new();
    match lid {
        Some(b) => h.update(b"B").update(&b.to_le_bytes()),
        None => h.update(b"N").update(inhoud).update(&e.to_le_bytes()),
    };
    h.update(&r.to_le_bytes());
    let p = u64::from_le_bytes(h.finalize().as_bytes()[..8].try_into().unwrap_or_default());
    ONGEORDEND | (p >> 1)
}

/// Koppelt de leden van elke SET of BAG van `x` aan die van de basis (ontwerp §5.6, "Ankers in een SET"), alleen
/// als dat eenduidig kan:
/// 1. op gelijke inhoud, als die inhoud aan beide kanten precies één keer voorkomt;
/// 2. daarna per type, voor de leden die over zijn (met één verwijzing naar een knoop): het paar met dezelfde
///    objecten die ernaar verwijzen en de meeste gelijke attributen, alleen als elk van beide de enige beste keuze
///    van de ander is.
///
/// Een gekoppeld lid krijgt de positie van zijn lid in de basis. Een lid zonder koppeling is aan de ene kant nieuw en
/// aan de andere weg; elk object dat dieper naar zo'n lid verwijst, telt als gewijzigd. Een andere volgorde verbergt
/// zo nooit een wijziging. Na [`Model::bereken_puur`] van beide en [`markeer_gemeen`], vóór [`Model::bereken`] van
/// `x`.
pub(crate) fn stem_ankers_af(basis: &Model, x: &mut Model) {
    let sb = op_sleutel(basis);
    let mut koppeling = HashMap::new();
    let mut terug: [Option<Verwijzers>; 2] = [None, None];
    for (o, object) in x.objecten.iter().enumerate() {
        let Some(&ob) = sb.get(object.sleutel.as_str()) else { continue };
        let (i, ib) = (object.inst as usize, basis.objecten[ob].inst as usize);
        let n = x.bestand.attributen(i).len();
        if x.bestand.type_naam(i) != basis.bestand.type_naam(ib) || n != basis.bestand.attributen(ib).len() {
            continue;
        }
        for j in 1..n {
            let (Some(lb), Some(lx)) = (basis.set_leden(ib, j), x.set_leden(i, j)) else { continue };
            for (e, b) in koppel_leden([basis, x], [&lb, &lx], &mut terug) {
                koppeling.insert((o as u32, j as u32, e), b);
            }
        }
    }
    x.koppeling = Some(koppeling);
}

/// De koppeling van de leden van één SET: (lid in x, lid in de basis). Zie [`stem_ankers_af`].
fn koppel_leden(m: [&Model; 2], leden: [&[Lid]; 2], terug: &mut [Option<Verwijzers>; 2]) -> Vec<(u32, u32)> {
    let mut uit = Vec::new();
    // 1. Gelijke inhoud die aan beide kanten precies één keer voorkomt.
    let mut op_inhoud: HashMap<Hash, [Vec<u32>; 2]> = HashMap::new();
    for (kant, lijst) in leden.iter().enumerate() {
        for (e, (_, inhoud)) in lijst.iter().enumerate() {
            op_inhoud.entry(*inhoud).or_default()[kant].push(e as u32);
        }
    }
    // 2. De rest per type, alleen leden met één verwijzing naar een knoop.
    let mut per_type: BTreeMap<&str, [Vec<(u32, usize)>; 2]> = BTreeMap::new();
    for [b, x] in op_inhoud.into_values() {
        if let ([b], [x]) = (&b[..], &x[..]) {
            uit.push((*x, *b));
            continue;
        }
        for (kant, lijst) in [b, x].into_iter().enumerate() {
            for e in lijst {
                if let [(Some(k), _)] = leden[kant][e as usize].0[..] {
                    per_type.entry(m[kant].bestand.type_naam(k as usize)).or_default()[kant].push((e, k as usize));
                }
            }
        }
    }
    for [b, x] in per_type.into_values().filter(|[b, x]| !b.is_empty() && !x.is_empty()) {
        let [ib, ix] = [(0, &b), (1, &x)].map(|(kant, lijst)| {
            let t = terug[kant].get_or_insert_with(|| m[kant].verwijzers());
            lijst
                .iter()
                .map(|&(_, k)| (m[kant].verwijzende_objecten(k, t), m[kant].pure_attributen(k)))
                .collect::<Vec<_>>()
        });
        // Per paar: None als de verwijzende objecten verschillen, anders het aantal gelijke attributen.
        let score: Vec<Vec<Option<usize>>> = (ib.iter())
            .map(|(ob, ab)| {
                (ix.iter())
                    .map(|(ox, ax)| {
                        (ob == ox && ab.len() == ax.len()).then(|| ab.iter().zip(ax).filter(|(u, v)| u == v).count())
                    })
                    .collect()
            })
            .collect();
        for (p, rij) in score.iter().enumerate() {
            let Some(q) = enige_beste(rij.iter().copied()) else { continue };
            if enige_beste(score.iter().map(|r| r[q])) == Some(p) {
                uit.push((x[q].0, b[p].0));
            }
        }
    }
    uit
}

/// De plek van de enige hoogste score, als die er is.
fn enige_beste(scores: impl Iterator<Item = Option<usize>>) -> Option<usize> {
    let mut beste: Option<(usize, usize, bool)> = None;
    for (q, s) in scores.enumerate() {
        beste = match (s, beste) {
            (None, _) => beste,
            (Some(s), Some((b, p, _))) if s == b => Some((b, p, false)),
            (Some(s), Some((b, _, _))) if s < b => beste,
            (Some(s), _) => Some((s, q, true)),
        };
    }
    beste.and_then(|(_, p, enig)| enig.then_some(p))
}

/// Markeert per object of zijn sleutel in alle `modellen` staat ([`Object::gemeen`]). Vóór [`Model::bereken`].
pub(crate) fn markeer_gemeen(modellen: &mut [&mut Model]) {
    let mut tellers: HashMap<String, usize> = HashMap::new();
    for m in modellen.iter() {
        for o in &m.objecten {
            *tellers.entry(o.sleutel.clone()).or_default() += 1;
        }
    }
    let n = modellen.len();
    for m in modellen.iter_mut() {
        for o in m.objecten.iter_mut() {
            o.gemeen = tellers.get(&o.sleutel) == Some(&n);
        }
    }
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

/// Koppelt een relatie die ons en hun allebei nieuw toevoegden, met een andere GlobalId: een relatie die alleen
/// enkele verwijzingen naar objecten heeft (bijv. IfcRelSequence van taak A naar taak B) is door die objecten
/// bepaald. Heeft hun precies één nieuwe relatie met hetzelfde type en dezelfde objecten als precies één nieuwe
/// relatie van ons, dan krijgt die van hun de sleutel van ons. Bij gelijke inhoud blijft er dan één over (die van
/// ons), bij een andere inhoud (bijv. een andere SequenceType) is het een conflict. Geeft het aantal koppelingen en
/// de gekoppelde relaties waarbij OPS onder de GlobalId iets anders bewaart ([`ops_relatiegegevens`]): per relatie
/// de sleutel en wat ons en hun bewaren. Dat moet een conflict worden, want de relatie houdt de GlobalId van ons.
pub(crate) fn koppel_toevoegingen(basis: &Model, ons: &Model, hun: &mut Model) -> (usize, Vec<(String, [String; 2])>) {
    let nieuw = |x: &Model, ander: &Model| -> HashMap<Hash, Vec<usize>> {
        let in_basis: HashSet<&str> = basis.objecten.iter().map(|o| o.sleutel.as_str()).collect();
        let in_ander: HashSet<&str> = ander.objecten.iter().map(|o| o.sleutel.as_str()).collect();
        let mut uit: HashMap<Hash, Vec<usize>> = HashMap::new();
        for (o, object) in x.objecten.iter().enumerate() {
            if in_basis.contains(object.sleutel.as_str()) || in_ander.contains(object.sleutel.as_str()) {
                continue;
            }
            if let Some(s) = x.natuurlijke_sleutel(o).filter(|_| x.alleen_enkele_verwijzingen(o)) {
                uit.entry(s).or_default().push(o);
            }
        }
        uit
    };
    let bij_ons = nieuw(ons, hun);
    let mut paren: Vec<(usize, usize)> = nieuw(hun, ons)
        .into_iter()
        .filter_map(|(s, h)| match (bij_ons.get(&s).map(Vec::as_slice), h.as_slice()) {
            (Some(&[o]), &[h]) => Some((o, h)),
            _ => None,
        })
        .collect();
    paren.sort_unstable();
    let mut anders = Vec::new();
    if !paren.is_empty() {
        let (eo, eh) = (ops_relatiegegevens(ons), ops_relatiegegevens(hun));
        for &(o, h) in &paren {
            let (go, gh) = (onder_globalid(&eo, &ons.objecten[o].globalid), onder_globalid(&eh, &hun.objecten[h].globalid));
            if go != gh {
                anders.push((ons.objecten[o].sleutel.clone(), [go, gh]));
            }
        }
    }
    for &(o, h) in &paren {
        hun.objecten[h].sleutel = ons.objecten[o].sleutel.clone();
    }
    (paren.len(), anders)
}

/// De eigenschappen waarin OPS gegevens van een relatie bewaart, met de GlobalId van de relatie als sleutel. Ze staan
/// in de eigenschappenset `OPS_Sequences` op het werkschema, want een IfcRelSequence kan zelf geen eigenschappenset
/// dragen: het record van een lag in dagen met een breuk (bijv. een halve dag, in IFC `PT4H0M0S`), dat van een lag in
/// minuten met een breuk (150,4 minuten, in IFC `PT2H30M0S`) en de lijst met de P6-grens.
const OPS_RELATIEGEGEVENS: [&str; 3] =
    ["DayMeasureLags", "ExactMinuteLags", "P6StartAtPredecessorFinishBoundarySequenceGuids"];

/// De waarden van de eigenschappen uit [`OPS_RELATIEGEGEVENS`] in `m`: per eigenschap de naam en de tekst (JSON).
fn ops_relatiegegevens(m: &Model) -> Vec<(String, String)> {
    let tekst = |i: usize, j: usize| {
        let a = m.bestand.attributen(i);
        let t = Tokens::nieuw(m.bestand.tekst(*a.get(j)?)).find_map(|(_, t)| match t {
            Token::Tekst(t) => Some(decodeer(t)),
            _ => None,
        });
        t
    };
    (0..m.bestand.instanties.len())
        .filter(|&i| m.bestand.type_naam(i) == "IFCPROPERTYSINGLEVALUE")
        .filter_map(|i| Some((tekst(i, 0).filter(|n| OPS_RELATIEGEGEVENS.contains(&n.as_str()))?, tekst(i, 2)?)))
        .collect()
}

/// Wat er in `eigenschappen` ([`ops_relatiegegevens`]) onder `globalid` staat, als tekst voor het rapport: per
/// eigenschap de naam en de waarde (bij de P6-grens alleen de naam). Een waarde die geen JSON-object of -lijst is,
/// telt helemaal mee als de GlobalId erin staat. Leeg: niets.
fn onder_globalid(eigenschappen: &[(String, String)], globalid: &str) -> String {
    let mut uit = Vec::new();
    for (naam, waarde) in eigenschappen {
        match serde_json::from_str::<serde_json::Value>(waarde) {
            Ok(serde_json::Value::Object(o)) => uit.extend(o.get(globalid).map(|v| format!("{naam} {v}"))),
            Ok(serde_json::Value::Array(a)) => {
                uit.extend(a.iter().any(|v| v.as_str() == Some(globalid)).then(|| naam.clone()))
            }
            _ => uit.extend(waarde.contains(globalid).then(|| format!("{naam} {waarde}"))),
        }
    }
    uit.sort();
    uit.join("; ")
}

/// De sleutels van `x` in een tabel (sleutel -> objectindex). Een gekoppelde relatie staat onder de GlobalId van de
/// basis.
pub(crate) fn op_sleutel<'m>(x: &'m Model) -> HashMap<&'m str, usize> {
    x.objecten.iter().enumerate().map(|(o, object)| (object.sleutel.as_str(), o)).collect()
}

/// De tekst van attribuut `j` van instantie `i`.
pub(crate) fn attribuut_tekst<'m>(m: &'m Model, i: usize, j: usize) -> &'m [u8] {
    m.bestand.tekst(m.bestand.attributen(i)[j])
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
    fn eigenaar_vluchtige_velden() {
        let tekst = ifc(
            "IFC4",
            "#1=IFCOWNERHISTORY(#6,$,$,.NOCHANGE.,$,$,$,1);\n#2=IFCWALL('3ScNjsTf6weF542amHNyMB',#1,'Muur',$,$,$,$,$,$);\n#3=IFCWALL('3ScNjsTf6weF542amHNyMC',$,'Muur',$,$,$,$,$,$);\n#4=IFCWALL('3ScNjsTf6weF542amHNyMD',#5,'Muur',$,$,$,$,$,$);\n#5=IFCOWNERHISTORY(#6,$,$,.ADDED.,2,$,$,2);\n#6=IFCPERSONANDORGANIZATION(#7,#8,$);\n#7=IFCPERSON($,'Anne',$,$,$,$,$,$);\n#8=IFCORGANIZATION($,'Bouw BV',$,$,$);\n#9=IFCOWNERHISTORY(#10,$,$,.NOCHANGE.,$,$,$,1);\n#10=IFCPERSONANDORGANIZATION(#11,#8,$);\n#11=IFCPERSON($,'Bram',$,$,$,$,$,$);\n#12=IFCWALL('3ScNjsTf6weF542amHNyME',#9,'Muur',$,$,$,$,$,$);",
        );
        let m = model(&tekst);
        assert_eq!(m.objecten.len(), 4);
        // Alleen tijdstempel en ChangeAction verschillen: gelijk. Een andere auteur of `$`: anders.
        assert_eq!(m.objecten[0].hash, m.objecten[2].hash);
        assert_ne!(m.objecten[0].hash, m.objecten[1].hash);
        assert_ne!(m.objecten[0].hash, m.objecten[3].hash);
        assert_eq!(m.object_naam(1).as_deref(), Some("Muur"));
        assert_eq!(m.verankerd_door[0], 1);
        assert!(m.wortels.is_empty());
    }

    #[test]
    fn ankers_onderscheiden_gelijke_plaatsingen() {
        // P en Q hebben elk een eigen plaatsing met dezelfde inhoud. W1 hangt aan P, W2 aan Q.
        let tekst = ifc(
            "IFC4",
            "#1=IFCCARTESIANPOINT((0.,0.,0.));\n#2=IFCAXIS2PLACEMENT3D(#1,$,$);\n#10=IFCLOCALPLACEMENT($,#2);\n#11=IFCLOCALPLACEMENT($,#2);\n#20=IFCBUILDINGSTOREY('0P00000000000000000000',$,'P',$,$,#10,$,$,.ELEMENT.,$);\n#21=IFCBUILDINGSTOREY('0Q00000000000000000000',$,'Q',$,$,#11,$,$,.ELEMENT.,$);\n#30=IFCLOCALPLACEMENT(#10,#2);\n#31=IFCLOCALPLACEMENT(#11,#2);\n#40=IFCWALL('0W10000000000000000000',$,'W',$,$,#30,$,$,$);\n#41=IFCWALL('0W20000000000000000000',$,'W',$,$,#31,$,$,$);",
        );
        let m = model(&tekst);
        let (w1, w2) = (m.op_globalid["0W10000000000000000000"], m.op_globalid["0W20000000000000000000"]);
        assert_ne!(m.objecten[w1 as usize].hash, m.objecten[w2 as usize].hash);
        // De plaatsingen van P en Q zelf hebben dezelfde inhoud.
        let (p, q) = (m.op_globalid["0P00000000000000000000"], m.op_globalid["0Q00000000000000000000"]);
        let (ip, iq) = (m.objecten[p as usize].inst as usize, m.objecten[q as usize].inst as usize);
        assert_eq!(m.attribuut_hash(ip, 5), m.attribuut_hash(iq, 5));
        let w1_inst = m.objecten[w1 as usize].inst as usize;
        let tekst = m.toon(attribuut_tekst(&m, w1_inst, 5), true, 200);
        assert!(tekst.contains("ObjectPlacement van P [0P00000000000000000000]"), "{tekst}");
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
        assert!(matches!(Model::lees(tekst.as_bytes()), Err(Fout::DubbeleGlobalId { regel: 9, .. })));
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
        let tijd = m.toon(attribuut_tekst(&m, 0, 11), true, 200);
        assert!(tijd.starts_with("IfcTaskTime($, $, $, $, 'P5D', '2027-01-04T07:00:00'"), "{tijd}");
        assert_eq!(m.toon(attribuut_tekst(&m, 2, 5), true, 200), "(Fundering [3ScNjsTf6weF542amHNyMB])");
    }

    /// §5.6: een lid wordt alleen gekoppeld als elk van beide de enige beste keuze van de ander is. Eigenschap a
    /// verandert en b valt weg: de enige overgebleven eigenschap is de beste keuze van a én van b, maar alleen a is
    /// haar beste keuze (de naam is gelijk).
    #[test]
    fn koppeling_alleen_bij_een_wederzijdse_beste_keuze() {
        let set = "#3=IFCPROPERTYSET('0Pset000000000000000000',$,'P',$,";
        let a = ifc(
            "IFC4",
            &format!("#1=IFCPROPERTYSINGLEVALUE('a',$,IFCINTEGER(1),$);\n#2=IFCPROPERTYSINGLEVALUE('b',$,IFCINTEGER(2),$);\n{set}(#1,#2));"),
        );
        let b = ifc("IFC4", &format!("#1=IFCPROPERTYSINGLEVALUE('a',$,IFCINTEGER(3),$);\n{set}(#1));"));
        let (mut ma, mut mb) = (Model::lees(a.as_bytes()).unwrap(), Model::lees(b.as_bytes()).unwrap());
        markeer_gemeen(&mut [&mut ma, &mut mb]);
        ma.bereken_puur();
        mb.bereken_puur();
        let leden = [&ma, &mb].map(|m| m.set_leden(m.objecten[0].inst as usize, 4).unwrap());
        assert_eq!(koppel_leden([&ma, &mb], [&leden[0], &leden[1]], &mut [None, None]), [(0, 0)]);
        stem_ankers_af(&ma, &mut mb);
        assert_eq!(mb.koppeling, Some(HashMap::from([((0, 4, 0), 0)])));
    }
}
