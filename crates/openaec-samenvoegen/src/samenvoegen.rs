//! Three-way samenvoegen per object: [`voeg_samen`].
//!
//! De regels (ontwerp §5.6):
//! - Alleen ons of alleen hun wijzigde, voegde toe of verwijderde een object: die kant.
//! - Beide wijzigden: per attribuut. Gelijk aan de basis aan één kant: de andere kant. Beide gelijk: die waarde.
//!   Anders: een SET (of BAG) van verwijzingen voegen we samen (wat erbij kwam en wat eraf ging, van beide kanten),
//!   eigenschappen en hoeveelheden per naam; al het andere is een conflict.
//! - De ene kant verwijderde, de andere wijzigde: conflict. Beide voegden hetzelfde object anders toe: conflict.
//! - Na het samenvoegen controleren we: verwijzingen naar verwijderde objecten, INVERSE-bovengrenzen, de grenzen van
//!   aggregaten, koppelingen met definities, de boom (geen kring, niets raakt los van het project) en of het
//!   geschreven resultaat inhoudelijk is wat we bedoelden (zelfcontrole). Elk probleem is een conflict.
//! - Een onbekend schema of een geval dat we niet kennen: conflict, geen samenvoeging.
//!
//! Met `bij_conflict` = `ons` of `hun` wint die kant per conflict; het rapport noemt de conflicten toch. Een conflict
//! dat pas na het schrijven blijkt, lossen we op door de betrokken objecten in hun geheel van de winnende kant te
//! nemen, en dan opnieuw te schrijven en te controleren (hoogstens [`MAX_RONDES`] keer).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::koppelregel::koppelregel;
use crate::model::{
    attribuut_tekst, enkele_verwijzing, header_vluchtig, koppel_relaties, op_sleutel, Hash, Model, Soort,
    VersieStatistiek, HEADER_VELDEN,
};
use crate::ontleden::{elementen, Token, Tokens};
use crate::schema::Aggregaat;
use crate::schrijven;
use crate::tekst::decodeer;
use crate::verschil::{losse_groepen, MAX_TEKST};
use crate::Fout;

pub(crate) const B: usize = 0;
pub(crate) const O: usize = 1;
pub(crate) const H: usize = 2;
const KANTEN: [Kant; 3] = [Kant::Basis, Kant::Ons, Kant::Hun];

/// Zo vaak schrijven en controleren we hoogstens als `bij_conflict` ons of hun is.
pub const MAX_RONDES: usize = 8;

/// Wat er gebeurt bij een conflict.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BijConflict {
    /// Geen resultaat, wel het rapport (standaard).
    #[default]
    Weiger,
    /// Ons wint per conflict.
    Ons,
    /// Hun wint per conflict.
    Hun,
}

#[derive(Debug, Clone, Default)]
pub struct Opties {
    pub bij_conflict: BijConflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kant {
    Basis,
    Ons,
    Hun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSoort {
    /// Beide kanten wijzigden hetzelfde attribuut anders.
    BeideGewijzigd,
    /// De ene kant verwijderde, de andere wijzigde.
    VerwijderdEnGewijzigd,
    /// Beide kanten voegden een object met dezelfde GlobalId toe, met een andere inhoud.
    BeideToegevoegd,
    /// Na het samenvoegen wijst iets naar een object dat er niet meer is.
    VerwijzingNaarVerwijderd,
    /// Na het samenvoegen heeft een object meer relaties dan een INVERSE-bovengrens toestaat.
    TeVeelRelaties,
    /// Een aggregaat valt buiten de grenzen uit het schema (bijv. een lege `RelatedElements`).
    BuitenGrenzen,
    /// Een koppeling met een definitie verdween door de ene kant, terwijl de andere kant de definitie wijzigde.
    KoppelingVerdwenen,
    /// Een kring in de boom, of een object dat los raakte van het project.
    Boom,
    /// Een gedeelde ankerknoop is in het resultaat niet eenduidig.
    GedeeldAnker,
    /// Beide kanten wijzigden dezelfde losse knopen (bijv. een kleur) anders.
    LosseKnopen,
    /// Beide kanten wijzigden hetzelfde veld in de HEADER anders.
    Header,
    /// Een schema of geval dat de bibliotheek niet kent.
    NietOndersteund,
    /// Het geschreven resultaat is inhoudelijk niet wat we bedoelden.
    Zelfcontrole,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    pub soort: ConflictSoort,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub globalid: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_naam: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub naam: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribuut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ons: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hun: Option<String>,
    /// Wat er aan de hand is, in gewone taal.
    pub melding: String,
    /// Bij `bij_conflict` ons of hun: de kant die won.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gekozen: Option<Kant>,
}

impl Conflict {
    pub(crate) fn nieuw(soort: ConflictSoort, melding: impl Into<String>) -> Self {
        Conflict {
            soort,
            globalid: None,
            type_naam: None,
            naam: None,
            attribuut: None,
            basis: None,
            ons: None,
            hun: None,
            melding: melding.into(),
            gekozen: None,
        }
    }

    /// Vult globalid, type en naam uit het eerste model dat het object heeft.
    pub(crate) fn over(mut self, m: &[Model; 3], s: &Sleutel) -> Self {
        for k in [O, H, B] {
            if let Some(o) = s.obj[k] {
                let i = m[k].objecten[o].inst as usize;
                self.globalid = Some(m[k].objecten[o].globalid.clone());
                self.type_naam = Some(m[k].type_weergave(i));
                self.naam = m[k].object_naam(i);
                break;
            }
        }
        self
    }
}

/// Cijfers over een samenvoeging.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SamenvoegStatistiek {
    pub basis: VersieStatistiek,
    pub ons: VersieStatistiek,
    pub hun: VersieStatistiek,
    /// Relaties met een nieuwe GlobalId, herkend aan hun natuurlijke sleutel (ons en hun samen).
    pub relaties_gekoppeld: usize,
    /// Objecten die we ongewijzigd van ons houden.
    pub objecten_van_ons: usize,
    /// Objecten die we in hun geheel van hun nemen (gewijzigd of toegevoegd door hun).
    pub objecten_van_hun: usize,
    /// Objecten die beide kanten wijzigden en die we per attribuut samenvoegden.
    pub objecten_samengevoegd: usize,
    /// Objecten die er na het samenvoegen niet meer zijn (verwijderd door een kant of door beide).
    pub objecten_verwijderd: usize,
    /// Nieuwe instanties achteraan (van hun overgenomen knopen en objecten).
    pub instanties_nieuw: usize,
    /// Regels van ons die we herschreven (objecten en hun ankerknopen).
    pub instanties_herschreven: usize,
    /// Regels van ons die we weghaalden.
    pub instanties_verwijderd: usize,
    /// Hoe vaak we schreven en controleerden.
    pub rondes: usize,
}

/// De uitkomst van [`voeg_samen`].
#[derive(Debug, Clone, Serialize)]
pub struct Samenvoeging {
    /// Het samengevoegde bestand; None als er een conflict is dat niet opgelost mocht of kon worden.
    #[serde(skip)]
    pub inhoud: Option<Vec<u8>>,
    /// Is er een resultaat?
    pub gelukt: bool,
    pub conflicten: Vec<Conflict>,
    pub statistiek: SamenvoegStatistiek,
}

/// Conflicten met de sleutels (in het plan) van de objecten die erbij horen. Met `bij_conflict` = ons of hun nemen
/// we die objecten in hun geheel van de winnende kant.
pub(crate) type Gevonden = Vec<(Conflict, Vec<usize>)>;

/// Wat er met een object gebeurt.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Besluit {
    /// Niet in het resultaat.
    Weg,
    /// Het hele object van deze kant (bij ons: de regel blijft staan).
    Heel(usize),
    /// Per attribuut samengevoegd; het type en de GlobalId komen van ons.
    Samen(Vec<Deel>),
}

/// Waar een attribuut vandaan komt.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Deel {
    /// Het attribuut van deze kant.
    Van(usize),
    /// Een samengevoegde lijst: per element de kant en het bereik in de tekst van het attribuut van die kant.
    Lijst(Vec<(usize, (usize, usize))>),
}

pub(crate) struct Sleutel {
    pub sleutel: String,
    /// Index in `objecten` per kant (basis, ons, hun).
    pub obj: [Option<usize>; 3],
    pub besluit: Besluit,
}

/// Alle besluiten.
pub(crate) struct Plan {
    pub sleutels: Vec<Sleutel>,
    pub op_sleutel: HashMap<String, usize>,
    /// Per sleutel van een groep losse knopen: de kant waarvan we de groep nemen.
    pub groepen: Vec<(Hash, usize)>,
    /// Per HEADER-parameter (regel, index) die van een andere kant dan ons komt: die kant.
    pub header: HashMap<(&'static str, usize), usize>,
}

impl Plan {
    pub fn in_resultaat(&self, sleutel: &str) -> bool {
        self.op_sleutel.get(sleutel).is_some_and(|&s| self.sleutels[s].besluit != Besluit::Weg)
    }
}

fn winnaar(opties: &Opties) -> Option<usize> {
    match opties.bij_conflict {
        BijConflict::Weiger => None,
        BijConflict::Ons => Some(O),
        BijConflict::Hun => Some(H),
    }
}

/// Voegt de wijzigingen van ons en hun ten opzichte van de basis samen.
pub fn voeg_samen(basis: &[u8], ons: &[u8], hun: &[u8], opties: &Opties) -> Result<Samenvoeging, Fout> {
    let (mb, mo, mh) = std::thread::scope(|s| {
        let b = s.spawn(|| Model::lees(basis));
        let h = s.spawn(|| Model::lees(hun));
        let o = Model::lees(ons);
        let fout = || Err(Fout::intern("lezen mislukt"));
        (b.join().unwrap_or_else(|_| fout()), o, h.join().unwrap_or_else(|_| fout()))
    });
    let (mut mb, mut mo, mut mh) = (mb?, mo?, mh?);
    let gekoppeld = koppel_relaties(&mb, &mut mo) + koppel_relaties(&mb, &mut mh);
    std::thread::scope(|s| {
        s.spawn(|| mb.bereken());
        s.spawn(|| mh.bereken());
        mo.bereken();
    });
    let m = [mb, mo, mh];
    let mut statistiek = SamenvoegStatistiek {
        basis: m[B].statistiek.clone(),
        ons: m[O].statistiek.clone(),
        hun: m[H].statistiek.clone(),
        relaties_gekoppeld: gekoppeld,
        ..Default::default()
    };
    let mislukt = |conflicten, statistiek| Ok(Samenvoeging { inhoud: None, gelukt: false, conflicten, statistiek });

    let conflicten = niet_ondersteund(&m);
    if !conflicten.is_empty() {
        return mislukt(conflicten, statistiek);
    }
    let mut conflicten = Vec::new();
    let mut plan = besluit(&m, opties, &mut conflicten);
    // Wijzigde een kant niets, dan is het resultaat de andere kant, byte voor byte. Eerst op bytes, zodat
    // (b, b, hun) = hun en (b, ons, b) = ons ook gelden als alle drie inhoudelijk gelijk zijn.
    let ongewijzigd = if hun == basis {
        Some(ons)
    } else if ons == basis {
        Some(hun)
    } else if gelijk(&m[B], &m[H]) || gelijk(&m[O], &m[H]) {
        Some(ons)
    } else if gelijk(&m[B], &m[O]) {
        Some(hun)
    } else {
        None
    };
    if let Some(inhoud) = ongewijzigd {
        tel_besluiten(&plan, &mut statistiek);
        return Ok(Samenvoeging { inhoud: Some(inhoud.to_vec()), gelukt: true, conflicten, statistiek });
    }
    let win = winnaar(opties);
    let koppel = koppelregel(&m, &plan);
    match win {
        None => conflicten.extend(koppel.into_iter().map(|(c, _)| c)),
        Some(w) => {
            los_op(&mut plan, koppel, w, &mut conflicten);
        }
    }
    if win.is_none() && !conflicten.is_empty() {
        tel_besluiten(&plan, &mut statistiek);
        return mislukt(conflicten, statistiek);
    }
    for ronde in 1..=MAX_RONDES {
        statistiek.rondes = ronde;
        let mut gevonden: Gevonden = controleer_verwijzingen(&m, &plan);
        let mut geschreven = None;
        if gevonden.is_empty() {
            match schrijven::schrijf(&m, &plan) {
                Ok(g) => geschreven = Some(g),
                Err(c) => gevonden = c,
            }
        }
        if let Some(g) = &geschreven {
            match Model::lees(&g.inhoud) {
                Ok(mut r) => {
                    for object in &mut r.objecten {
                        match g.alias.get(&object.globalid) {
                            Some(sleutel) => object.sleutel = sleutel.clone(),
                            None => gevonden.push((
                                Conflict::nieuw(
                                    ConflictSoort::Zelfcontrole,
                                    format!("het resultaat bevat een onbekend object {}", object.globalid),
                                ),
                                Vec::new(),
                            )),
                        }
                    }
                    r.bereken();
                    controleer_resultaat(&m, &plan, &r, &mut gevonden);
                }
                Err(fout) => gevonden.push((
                    Conflict::nieuw(
                        ConflictSoort::Zelfcontrole,
                        format!("het resultaat is geen geldig bestand: {fout}"),
                    ),
                    Vec::new(),
                )),
            }
        }
        if gevonden.is_empty() {
            let g = geschreven.expect("geschreven als er geen conflict is");
            statistiek.instanties_nieuw = g.nieuw;
            statistiek.instanties_herschreven = g.herschreven;
            statistiek.instanties_verwijderd = g.verwijderd;
            tel_besluiten(&plan, &mut statistiek);
            return Ok(Samenvoeging { inhoud: Some(g.inhoud), gelukt: true, conflicten, statistiek });
        }
        let Some(w) = win else {
            conflicten.extend(gevonden.into_iter().map(|(c, _)| c));
            tel_besluiten(&plan, &mut statistiek);
            return mislukt(conflicten, statistiek);
        };
        let (veranderd, onoplosbaar) = los_op(&mut plan, gevonden, w, &mut conflicten);
        if onoplosbaar || !veranderd {
            tel_besluiten(&plan, &mut statistiek);
            return mislukt(conflicten, statistiek);
        }
    }
    tel_besluiten(&plan, &mut statistiek);
    conflicten.push(Conflict::nieuw(
        ConflictSoort::Zelfcontrole,
        format!("na {MAX_RONDES} rondes waren er nog steeds conflicten"),
    ));
    mislukt(conflicten, statistiek)
}

/// Lost conflicten op met de winnende kant: de betrokken objecten in hun geheel van die kant (of weg, als die kant
/// ze niet heeft). Geeft (er veranderde een besluit, er was een conflict zonder betrokken objecten).
fn los_op(plan: &mut Plan, gevonden: Gevonden, w: usize, conflicten: &mut Vec<Conflict>) -> (bool, bool) {
    let mut veranderd = false;
    let mut onoplosbaar = false;
    for (mut c, betrokken) in gevonden {
        if betrokken.is_empty() {
            onoplosbaar = true;
        }
        for s in betrokken {
            let nieuw = match plan.sleutels[s].obj[w] {
                Some(_) => Besluit::Heel(w),
                None => Besluit::Weg,
            };
            if plan.sleutels[s].besluit != nieuw {
                plan.sleutels[s].besluit = nieuw;
                veranderd = true;
            }
        }
        c.gekozen = Some(KANTEN[w]);
        if !conflicten.contains(&c) {
            conflicten.push(c);
        }
    }
    (veranderd, onoplosbaar)
}

/// Zijn twee versies inhoudelijk gelijk (dezelfde objecten met dezelfde inhoud, dezelfde losse knopen en HEADER)?
fn gelijk(a: &Model, b: &Model) -> bool {
    if a.objecten.len() != b.objecten.len() || a.wortels.len() != b.wortels.len() {
        return false;
    }
    let sb = op_sleutel(b);
    let objecten = a.objecten.iter().all(|o| sb.get(o.sleutel.as_str()).is_some_and(|&x| b.objecten[x].hash == o.hash));
    if !objecten {
        return false;
    }
    let groepen = |m: &Model| -> HashMap<Hash, Vec<Hash>> {
        losse_groepen(m)
            .into_iter()
            .map(|(s, v)| {
                let mut h: Vec<Hash> = v.into_iter().map(|x| x.0).collect();
                h.sort();
                (s, h)
            })
            .collect()
    };
    if groepen(a) != groepen(b) {
        return false;
    }
    HEADER_VELDEN.iter().all(|&(regel, velden)| {
        (0..velden.len()).all(|j| {
            header_vluchtig(regel, j)
                || a.header_parameter(regel, j).map(|x| x.0) == b.header_parameter(regel, j).map(|x| x.0)
        })
    })
}

fn tel_besluiten(plan: &Plan, s: &mut SamenvoegStatistiek) {
    s.objecten_van_ons = 0;
    s.objecten_van_hun = 0;
    s.objecten_samengevoegd = 0;
    s.objecten_verwijderd = 0;
    for sl in &plan.sleutels {
        match sl.besluit {
            Besluit::Weg => s.objecten_verwijderd += usize::from(sl.obj[O].is_some() || sl.obj[B].is_some()),
            Besluit::Heel(O) => s.objecten_van_ons += 1,
            Besluit::Heel(_) => s.objecten_van_hun += 1,
            Besluit::Samen(_) => s.objecten_samengevoegd += 1,
        }
    }
}

/// Gevallen die we niet kennen: een onbekend schema, verschillende schema's, onbekende types, cycli...
fn niet_ondersteund(m: &[Model; 3]) -> Vec<Conflict> {
    let mut uit = Vec::new();
    for (k, model) in m.iter().enumerate() {
        let s = &model.statistiek;
        let kant = ["basis", "ons", "hun"][k];
        let mut meld = |melding: String| uit.push(Conflict::nieuw(ConflictSoort::NietOndersteund, melding));
        if s.schema_tabel.is_empty() {
            meld(format!("{kant}: het schema '{}' kennen we niet (alleen IFC2X3, IFC4 en IFC4X3)", s.schema));
        }
        if !s.onbekende_typen.is_empty() {
            let typen: Vec<&str> = s.onbekende_typen.keys().map(String::as_str).take(5).collect();
            meld(format!("{kant}: types die niet in het schema staan: {}", typen.join(", ")));
        }
        if s.afwijkend_aantal_attributen > 0 {
            meld(format!(
                "{kant}: {} instanties hebben een ander aantal attributen dan het schema zegt",
                s.afwijkend_aantal_attributen
            ));
        }
        if s.root_zonder_globalid > 0 {
            meld(format!("{kant}: {} objecten zonder GlobalId", s.root_zonder_globalid));
        }
        if s.cycli > 0 {
            meld(format!("{kant}: knopen die via andere knopen naar zichzelf verwijzen"));
        }
    }
    if uit.is_empty()
        && (m[B].statistiek.schema_tabel != m[O].statistiek.schema_tabel
            || m[B].statistiek.schema_tabel != m[H].statistiek.schema_tabel)
    {
        uit.push(Conflict::nieuw(
            ConflictSoort::NietOndersteund,
            format!(
                "de versies hebben een ander schema ({}, {}, {})",
                m[B].statistiek.schema, m[O].statistiek.schema, m[H].statistiek.schema
            ),
        ));
    }
    uit
}

/// De besluiten per object, per groep losse knopen en per HEADER-veld.
fn besluit(m: &[Model; 3], opties: &Opties, conflicten: &mut Vec<Conflict>) -> Plan {
    let mut sleutels: Vec<Sleutel> = Vec::new();
    let mut op_sleutel: HashMap<String, usize> = HashMap::new();
    for k in [O, H, B] {
        for (o, object) in m[k].objecten.iter().enumerate() {
            let s = *op_sleutel.entry(object.sleutel.clone()).or_insert_with(|| {
                sleutels.push(Sleutel { sleutel: object.sleutel.clone(), obj: [None; 3], besluit: Besluit::Weg });
                sleutels.len() - 1
            });
            sleutels[s].obj[k] = Some(o);
        }
    }
    let win = winnaar(opties);
    for s in &mut sleutels {
        s.besluit = besluit_object(m, s, win, conflicten);
    }
    let groepen = besluit_groepen(m, win, conflicten);
    let header = besluit_header(m, win, conflicten);
    Plan { sleutels, op_sleutel, groepen, header }
}

fn besluit_object(m: &[Model; 3], s: &Sleutel, win: Option<usize>, conflicten: &mut Vec<Conflict>) -> Besluit {
    let hash = |k: usize| s.obj[k].map(|o| m[k].objecten[o].hash);
    let (hb, ho, hh) = (hash(B), hash(O), hash(H));
    let gekozen = win.map(|w| KANTEN[w]);
    match (hb, ho, hh) {
        (Some(b), Some(o), Some(h)) => {
            if o == h || h == b {
                Besluit::Heel(O)
            } else if o == b {
                Besluit::Heel(H)
            } else {
                beide_gewijzigd(m, s, win, conflicten)
            }
        }
        (Some(b), Some(o), None) => {
            if o == b {
                Besluit::Weg
            } else {
                let mut c = Conflict::nieuw(
                    ConflictSoort::VerwijderdEnGewijzigd,
                    "ons wijzigde dit object, hun verwijderde het",
                )
                .over(m, s);
                c.gekozen = gekozen;
                conflicten.push(c);
                if win == Some(H) {
                    Besluit::Weg
                } else {
                    Besluit::Heel(O)
                }
            }
        }
        (Some(b), None, Some(h)) => {
            if h == b {
                Besluit::Weg
            } else {
                let mut c = Conflict::nieuw(
                    ConflictSoort::VerwijderdEnGewijzigd,
                    "hun wijzigde dit object, ons verwijderde het",
                )
                .over(m, s);
                c.gekozen = gekozen;
                conflicten.push(c);
                if win == Some(H) {
                    Besluit::Heel(H)
                } else {
                    Besluit::Weg
                }
            }
        }
        (Some(_), None, None) | (None, None, None) => Besluit::Weg,
        (None, Some(o), Some(h)) => {
            if o != h {
                let mut c = Conflict::nieuw(
                    ConflictSoort::BeideToegevoegd,
                    "ons en hun voegden allebei dit object toe, met een andere inhoud",
                )
                .over(m, s);
                c.gekozen = gekozen;
                conflicten.push(c);
                if win == Some(H) {
                    return Besluit::Heel(H);
                }
            }
            Besluit::Heel(O)
        }
        (None, Some(_), None) => Besluit::Heel(O),
        (None, None, Some(_)) => Besluit::Heel(H),
    }
}

/// Beide kanten wijzigden het object: per attribuut.
fn beide_gewijzigd(m: &[Model; 3], s: &Sleutel, win: Option<usize>, conflicten: &mut Vec<Conflict>) -> Besluit {
    let inst = |k: usize| m[k].objecten[s.obj[k].unwrap()].inst as usize;
    let i = [inst(B), inst(O), inst(H)];
    let gekozen = win.map(|w| KANTEN[w]);
    let n = m[O].bestand.attributen(i[O]).len();
    let zelfde_vorm = (0..3).all(|k| {
        m[k].bestand.type_naam(i[k]) == m[O].bestand.type_naam(i[O]) && m[k].bestand.attributen(i[k]).len() == n
    });
    if !zelfde_vorm || m[O].entiteit(i[O]).is_none() {
        let mut c =
            Conflict::nieuw(ConflictSoort::BeideGewijzigd, "beide kanten wijzigden dit object, en het type veranderde")
                .over(m, s);
        c.attribuut = Some("type".into());
        c.basis = Some(m[B].type_weergave(i[B]));
        c.ons = Some(m[O].type_weergave(i[O]));
        c.hun = Some(m[H].type_weergave(i[H]));
        c.gekozen = gekozen;
        conflicten.push(c);
        return Besluit::Heel(win.unwrap_or(O));
    }
    let entiteit = m[O].entiteit(i[O]).expect("gecontroleerd");
    let mut delen = vec![Deel::Van(O)];
    for j in 1..n {
        let h = [m[B].attribuut_hash(i[B], j), m[O].attribuut_hash(i[O], j), m[H].attribuut_hash(i[H], j)];
        if h[O] == h[H] || h[H] == h[B] {
            delen.push(Deel::Van(O));
            continue;
        }
        if h[O] == h[B] {
            delen.push(Deel::Van(H));
            continue;
        }
        let attribuut = &entiteit.attributen[j];
        let toon = |k: usize| m[k].toon(attribuut_tekst(&m[k], i[k], j), true, MAX_TEKST);
        if matches!(attribuut.aggregaat, Some(Aggregaat::Set) | Some(Aggregaat::Bag)) {
            let tas = attribuut.aggregaat == Some(Aggregaat::Bag);
            if let Some((lijst, botsingen)) = verzameling(m, i, j, tas, win) {
                for (groep, teksten) in botsingen {
                    let mut c = Conflict::nieuw(
                        ConflictSoort::BeideGewijzigd,
                        format!("beide kanten wijzigden '{groep}' in {} anders", attribuut.naam),
                    )
                    .over(m, s);
                    c.attribuut = Some(format!("{}[{groep}]", attribuut.naam));
                    [c.basis, c.ons, c.hun] = teksten;
                    c.gekozen = gekozen;
                    conflicten.push(c);
                }
                delen.push(Deel::Lijst(lijst));
                continue;
            }
        }
        let mut c =
            Conflict::nieuw(ConflictSoort::BeideGewijzigd, format!("beide kanten wijzigden {} anders", attribuut.naam))
                .over(m, s);
        c.attribuut = Some(attribuut.naam.clone());
        c.basis = Some(toon(B));
        c.ons = Some(toon(O));
        c.hun = Some(toon(H));
        c.gekozen = gekozen;
        conflicten.push(c);
        delen.push(Deel::Van(win.unwrap_or(O)));
    }
    Besluit::Samen(delen)
}

/// Een element van een verzameling: de groep (de identiteit, of bij een eigenschap de naam), de waarde en het
/// bereik in de tekst.
struct Element {
    groep: Vec<u8>,
    waarde: Vec<u8>,
    bereik: (usize, usize),
    /// De naam van een eigenschap of hoeveelheid, voor het rapport.
    naam: Option<String>,
}

type Botsing = (String, [Option<String>; 3]);

/// Een samengevoegde lijst: per element de kant en het bereik in de tekst van het attribuut van die kant.
type LijstDelen = Vec<(usize, (usize, usize))>;

/// Three-way samenvoegen van een SET of BAG van verwijzingen. None als niet alle drie de waarden een lijst van
/// verwijzingen zijn. Anders de lijst (kant, bereik) en de groepen die botsten (eigenschappen met twee nieuwe
/// waarden); bij een botsing staat in de lijst de groep van de winnende kant (of van ons, als niemand wint).
fn verzameling(
    m: &[Model; 3],
    i: [usize; 3],
    j: usize,
    tas: bool,
    win: Option<usize>,
) -> Option<(LijstDelen, Vec<Botsing>)> {
    let mut lijsten: [Vec<Element>; 3] = Default::default();
    for k in 0..3 {
        let tekst = attribuut_tekst(&m[k], i[k], j);
        for (id, bereik) in m[k].verwijzingslijst(tekst, true)? {
            let doel = enkele_verwijzing(&tekst[bereik.0..bereik.1])?;
            let d = *m[k].bestand.index.get(&doel)? as usize;
            let naam = benoemd(&m[k], d);
            let groep = match &naam {
                Some(n) => [b"N".as_slice(), n.as_bytes()].concat(),
                None => id.clone(),
            };
            lijsten[k].push(Element { groep, waarde: id, bereik, naam });
        }
    }
    // Per groep de gesorteerde waarden per kant.
    let mut groepen: HashMap<&[u8], [Vec<&[u8]>; 3]> = HashMap::new();
    let mut volgorde: Vec<&[u8]> = Vec::new();
    for (k, lijst) in lijsten.iter().enumerate() {
        for e in lijst {
            let ingang = groepen.entry(&e.groep).or_insert_with(|| {
                volgorde.push(&e.groep);
                Default::default()
            });
            ingang[k].push(&e.waarde);
        }
    }
    // Besluit per groep: (kant, aantal); aantal None = alle elementen van die kant.
    let mut keuze: HashMap<&[u8], (usize, Option<usize>)> = HashMap::new();
    let mut botsingen = Vec::new();
    for groep in &volgorde {
        let w = groepen.get_mut(groep).expect("groep bestaat");
        for v in w.iter_mut() {
            v.sort();
        }
        let besluit = if w[O] == w[H] || w[H] == w[B] {
            (O, None)
        } else if w[O] == w[B] {
            (H, None)
        } else if !groep.starts_with(b"N") {
            // Dezelfde identiteit: tel. SET: erin als ons en hun hem hebben, of als een kant hem toevoegde.
            let (b, o, h) = (w[B].len(), w[O].len(), w[H].len());
            let mut aantal = (o + h).saturating_sub(b);
            if !tas {
                aantal = aantal.min(1);
            }
            (O, Some(aantal))
        } else {
            let tekst = |k: usize| -> Option<String> {
                let e: Vec<String> = lijsten[k]
                    .iter()
                    .filter(|e| e.groep.as_slice() == *groep)
                    .map(|e| m[k].toon(&attribuut_tekst(&m[k], i[k], j)[e.bereik.0..e.bereik.1], true, MAX_TEKST))
                    .collect();
                (!e.is_empty()).then(|| e.join(", "))
            };
            let naam = lijsten.iter().flatten().find(|e| e.groep.as_slice() == *groep).and_then(|e| e.naam.clone());
            botsingen.push((naam.unwrap_or_default(), [tekst(B), tekst(O), tekst(H)]));
            (win.unwrap_or(O), None)
        };
        keuze.insert(groep, besluit);
    }
    // De lijst: eerst wat van ons komt (in de volgorde van ons), dan wat van hun komt.
    let mut uit = Vec::new();
    let mut gebruikt: HashMap<&[u8], usize> = HashMap::new();
    for k in [O, H] {
        for e in &lijsten[k] {
            let (kant, aantal) = keuze[e.groep.as_slice()];
            let teller = gebruikt.entry(&e.groep).or_default();
            let neem = match aantal {
                None => kant == k,
                Some(n) => *teller < n,
            };
            if neem {
                *teller += 1;
                uit.push((k, e.bereik));
            }
        }
    }
    Some((uit, botsingen))
}

/// De naam van een eigenschap of hoeveelheid (IfcProperty, IfcPhysicalQuantity: het eerste attribuut).
fn benoemd(m: &Model, k: usize) -> Option<String> {
    if m.soort[k] != Soort::Knoop || !m.entiteit(k)?.benoemd {
        return None;
    }
    let &eerste = m.bestand.attributen(k).first()?;
    match Tokens::nieuw(m.bestand.tekst(eerste)).next() {
        Some((_, Token::Tekst(t))) => Some(decodeer(t)),
        _ => None,
    }
}

/// Groepen losse knopen: per sleutel de groep van ons, van hun of (bij een conflict) van de winnaar.
fn besluit_groepen(m: &[Model; 3], win: Option<usize>, conflicten: &mut Vec<Conflict>) -> Vec<(Hash, usize)> {
    let g = [losse_groepen(&m[B]), losse_groepen(&m[O]), losse_groepen(&m[H])];
    let mut sleutels: Vec<Hash> = Vec::new();
    let mut gezien = HashSet::new();
    for k in [O, H, B] {
        let mut ks: Vec<&Hash> = g[k].keys().collect();
        ks.sort();
        for s in ks {
            if gezien.insert(*s) {
                sleutels.push(*s);
            }
        }
    }
    let leeg = Vec::new();
    let mut uit = Vec::new();
    for s in sleutels {
        let hashes = |k: usize| -> Vec<Hash> { g[k].get(&s).unwrap_or(&leeg).iter().map(|(h, _)| *h).collect() };
        let (b, o, h) = (hashes(B), hashes(O), hashes(H));
        let kant = if o == h || h == b {
            O
        } else if o == b {
            H
        } else {
            let toon = |k: usize| -> Option<String> {
                let e: Vec<String> = g[k]
                    .get(&s)
                    .unwrap_or(&leeg)
                    .iter()
                    .map(|&(_, i)| m[k].toon_instantie(i as usize, MAX_TEKST))
                    .collect();
                (!e.is_empty()).then(|| e.join(", "))
            };
            let voorbeeld =
                [O, H, B].into_iter().find_map(|k| g[k].get(&s).and_then(|v| v.first()).map(|&(_, i)| (k, i)));
            let mut c =
                Conflict::nieuw(ConflictSoort::LosseKnopen, "beide kanten wijzigden dezelfde losse knopen anders");
            if let Some((k, i)) = voorbeeld {
                c.type_naam = Some(m[k].type_weergave(i as usize));
            }
            [c.basis, c.ons, c.hun] = [toon(B), toon(O), toon(H)];
            c.gekozen = win.map(|w| KANTEN[w]);
            conflicten.push(c);
            win.unwrap_or(O)
        };
        uit.push((s, kant));
    }
    uit
}

/// De HEADER: per veld three-way; de tijdstempel blijft van ons.
fn besluit_header(
    m: &[Model; 3],
    win: Option<usize>,
    conflicten: &mut Vec<Conflict>,
) -> HashMap<(&'static str, usize), usize> {
    let mut uit = HashMap::new();
    for &(regel, velden) in HEADER_VELDEN {
        for (j, veld) in velden.iter().enumerate() {
            if header_vluchtig(regel, j) {
                continue;
            }
            let w: [Option<(Hash, &[u8])>; 3] = [0, 1, 2].map(|k| m[k].header_parameter(regel, j));
            let h = w.map(|x| x.map(|y| y.0));
            if h[O].is_none() {
                continue;
            }
            if h[O] == h[H] || h[H] == h[B] {
                continue;
            }
            if h[O] == h[B] {
                if h[H].is_some() {
                    uit.insert((regel, j), H);
                }
                continue;
            }
            let toon = |k: usize| w[k].map(|(_, t)| m[k].toon(t, false, MAX_TEKST));
            let mut c = Conflict::nieuw(ConflictSoort::Header, format!("beide kanten wijzigden {regel}.{veld} anders"));
            c.attribuut = Some(format!("{regel}.{veld}"));
            [c.basis, c.ons, c.hun] = [toon(B), toon(O), toon(H)];
            c.gekozen = win.map(|w| KANTEN[w]);
            conflicten.push(c);
            if win == Some(H) && h[H].is_some() {
                uit.insert((regel, j), H);
            }
        }
    }
    uit
}

/// De bronnen van een object in het resultaat: per attribuut de kant en de tekst, of de elementen van een lijst.
pub(crate) fn bronnen<'m>(m: &'m [Model<'_>; 3], s: &Sleutel) -> Vec<(usize, usize, &'m [u8])> {
    let mut uit = Vec::new();
    match &s.besluit {
        Besluit::Weg => {}
        Besluit::Heel(k) => {
            let i = m[*k].objecten[s.obj[*k].unwrap()].inst as usize;
            for j in 0..m[*k].bestand.attributen(i).len() {
                uit.push((*k, j, attribuut_tekst(&m[*k], i, j)));
            }
        }
        Besluit::Samen(delen) => {
            for (j, deel) in delen.iter().enumerate() {
                match deel {
                    Deel::Van(k) => {
                        let i = m[*k].objecten[s.obj[*k].unwrap()].inst as usize;
                        uit.push((*k, j, attribuut_tekst(&m[*k], i, j)));
                    }
                    Deel::Lijst(elementen) => {
                        for &(k, (b, e)) in elementen {
                            let i = m[k].objecten[s.obj[k].unwrap()].inst as usize;
                            uit.push((k, j, &attribuut_tekst(&m[k], i, j)[b..e]));
                        }
                    }
                }
            }
        }
    }
    uit
}

/// Verwijzingen naar objecten die niet in het resultaat staan, rechtstreeks of via knopen (zonder anker) van het
/// object of van een losse knoop.
fn controleer_verwijzingen(m: &[Model; 3], plan: &Plan) -> Gevonden {
    let mut uit = Vec::new();
    // Per kant per knoop: de eerste ontbrekende sleutel in de knopen eronder (zonder anker), of None.
    let mut memo: [HashMap<u32, Option<String>>; 3] = Default::default();
    let ontbreekt = |k: usize, tekst: &[u8], memo: &mut [HashMap<u32, Option<String>>; 3], direct: bool| {
        zoek_ontbrekend(&m[k], tekst, plan, &mut memo[k], direct)
    };
    for (si, s) in plan.sleutels.iter().enumerate() {
        if s.besluit == Besluit::Weg {
            continue;
        }
        for (k, _j, tekst) in bronnen(m, s) {
            if let Some(weg) = ontbreekt(k, tekst, &mut memo, true) {
                let mut c = Conflict::nieuw(
                    ConflictSoort::VerwijzingNaarVerwijderd,
                    format!("dit object verwijst naar {weg}, dat na het samenvoegen niet meer bestaat"),
                )
                .over(m, s);
                c.attribuut = None;
                let betrokken = vec![si, plan.op_sleutel[&weg]];
                uit.push((c, betrokken));
                break;
            }
        }
    }
    let g: [HashMap<Hash, Vec<(Hash, u32)>>; 3] = [losse_groepen(&m[B]), losse_groepen(&m[O]), losse_groepen(&m[H])];
    for &(sleutel, k) in &plan.groepen {
        for &(_, i) in g[k].get(&sleutel).map(Vec::as_slice).unwrap_or(&[]) {
            let tekst = m[k].bestand.inhoud(i as usize);
            if let Some(weg) = ontbreekt(k, tekst, &mut memo, false) {
                let mut c = Conflict::nieuw(
                    ConflictSoort::VerwijzingNaarVerwijderd,
                    format!("een losse knoop verwijst naar {weg}, dat na het samenvoegen niet meer bestaat"),
                );
                c.type_naam = Some(m[k].type_weergave(i as usize));
                uit.push((c, vec![plan.op_sleutel[&weg]]));
            }
        }
    }
    uit
}

/// De eerste ontbrekende sleutel in een tekst en de knopen eronder. Een knoop met een anker hoort bij zijn eigen
/// object(en); daar lopen we niet in, behalve vanuit een attribuut van het object zelf (`direct`).
fn zoek_ontbrekend(
    m: &Model,
    tekst: &[u8],
    plan: &Plan,
    memo: &mut HashMap<u32, Option<String>>,
    direct: bool,
) -> Option<String> {
    let mut stapel: Vec<u32> = Vec::new();
    let eerst = |n: u64, stapel: &mut Vec<u32>| -> Option<String> {
        let k = *m.bestand.index.get(&n)? as usize;
        match m.soort[k] {
            Soort::Object(o) => {
                let sleutel = &m.objecten[o as usize].sleutel;
                (!plan.in_resultaat(sleutel)).then(|| sleutel.clone())
            }
            Soort::Knoop => {
                if !direct && m.anker_id[k].is_some() {
                    return None;
                }
                if let Some(v) = memo.get(&(k as u32)) {
                    return v.clone();
                }
                stapel.push(k as u32);
                None
            }
        }
    };
    for (_, token) in Tokens::nieuw(tekst) {
        if let Token::Verwijzing(n) = token {
            if let Some(weg) = eerst(n, &mut stapel) {
                return Some(weg);
            }
        }
    }
    // Knopen eronder, zonder anker; het resultaat per knoop onthouden we.
    let mut bezocht: Vec<u32> = Vec::new();
    let mut gevonden = None;
    while let Some(k) = stapel.pop() {
        if memo.contains_key(&k) {
            if let Some(Some(w)) = memo.get(&k) {
                gevonden = Some(w.clone());
                break;
            }
            continue;
        }
        memo.insert(k, None);
        bezocht.push(k);
        for &d in m.bestand.verwijzingen(k as usize) {
            let d = d as usize;
            match m.soort[d] {
                Soort::Object(o) => {
                    let sleutel = &m.objecten[o as usize].sleutel;
                    if !plan.in_resultaat(sleutel) {
                        gevonden = Some(sleutel.clone());
                    }
                }
                Soort::Knoop if m.anker_id[d].is_none() && !memo.contains_key(&(d as u32)) => stapel.push(d as u32),
                Soort::Knoop => {
                    if let Some(Some(w)) = memo.get(&(d as u32)) {
                        gevonden = Some(w.clone());
                    }
                }
            }
            if gevonden.is_some() {
                break;
            }
        }
        if gevonden.is_some() {
            break;
        }
    }
    if let Some(w) = &gevonden {
        // Zonder precies pad: markeer alle bezochte knopen als "hier ontbreekt iets onder" is te ruim; vergeet ze,
        // zodat een volgende vraag ze opnieuw bekijkt.
        for k in bezocht {
            memo.remove(&k);
        }
        return Some(w.clone());
    }
    gevonden
}

/// De controles op het geschreven resultaat `r`.
fn controleer_resultaat(m: &[Model; 3], plan: &Plan, r: &Model, uit: &mut Gevonden) {
    zelfcontrole(m, plan, r, uit);
    if !uit.is_empty() {
        return;
    }
    inverse_grenzen(m, plan, r, uit);
    aggregaat_grenzen(m, plan, r, uit);
    boom(m, plan, r, uit);
}

/// De verwachte hash van een object in het resultaat.
pub(crate) fn verwachte_hash(m: &[Model; 3], s: &Sleutel) -> Option<Hash> {
    match &s.besluit {
        Besluit::Weg => None,
        Besluit::Heel(k) => Some(m[*k].objecten[s.obj[*k].unwrap()].hash),
        Besluit::Samen(delen) => {
            let i_ons = m[O].objecten[s.obj[O].unwrap()].inst as usize;
            let mut hashes = Vec::with_capacity(delen.len());
            for (j, deel) in delen.iter().enumerate().skip(1) {
                hashes.push(match deel {
                    Deel::Van(k) => m[*k].attribuut_hash(m[*k].objecten[s.obj[*k].unwrap()].inst as usize, j),
                    Deel::Lijst(elementen) => {
                        let mut buffer = vec![b'('];
                        for &(k, (b, e)) in elementen {
                            let i = m[k].objecten[s.obj[k].unwrap()].inst as usize;
                            m[k].canoniek(&attribuut_tekst(&m[k], i, j)[b..e], true, &mut buffer);
                        }
                        buffer.push(b')');
                        *blake3::hash(&buffer).as_bytes()
                    }
                });
            }
            Some(Model::object_hash_uit(m[O].bestand.type_naam(i_ons), &hashes))
        }
    }
}

/// Is het geschreven resultaat inhoudelijk wat we bedoelden?
fn zelfcontrole(m: &[Model; 3], plan: &Plan, r: &Model, uit: &mut Gevonden) {
    let in_r = op_sleutel(r);
    for (si, s) in plan.sleutels.iter().enumerate() {
        let verwacht = verwachte_hash(m, s);
        let gevonden = in_r.get(s.sleutel.as_str()).map(|&o| r.objecten[o].hash);
        if verwacht != gevonden {
            let melding = match (verwacht, gevonden) {
                (Some(_), None) => "het object ontbreekt in het geschreven resultaat".to_string(),
                (None, Some(_)) => "het object staat in het resultaat, maar hoort er niet in".to_string(),
                _ => "de inhoud van het object klopt niet na het schrijven (bijv. een gedeelde of verplaatste \
                      ankerknoop die niet eenduidig over te nemen is)"
                    .to_string(),
            };
            uit.push((Conflict::nieuw(ConflictSoort::Zelfcontrole, melding).over(m, s), vec![si]));
        }
    }
    for object in &r.objecten {
        if !plan.op_sleutel.contains_key(&object.sleutel) {
            uit.push((
                Conflict::nieuw(
                    ConflictSoort::Zelfcontrole,
                    format!("onbekend object {} in het resultaat", object.globalid),
                ),
                Vec::new(),
            ));
        }
    }
    // Losse knopen.
    let g = [losse_groepen(&m[B]), losse_groepen(&m[O]), losse_groepen(&m[H])];
    let gr = losse_groepen(r);
    let mut verwacht: HashMap<Hash, Vec<Hash>> = HashMap::new();
    for &(sleutel, k) in &plan.groepen {
        let v: Vec<Hash> = g[k].get(&sleutel).map(|v| v.iter().map(|x| x.0).collect()).unwrap_or_default();
        if !v.is_empty() {
            verwacht.insert(sleutel, v);
        }
    }
    let gevonden: HashMap<Hash, Vec<Hash>> =
        gr.into_iter().map(|(s, v)| (s, v.into_iter().map(|x| x.0).collect())).collect();
    if verwacht != gevonden {
        uit.push((
            Conflict::nieuw(
                ConflictSoort::Zelfcontrole,
                "de losse knopen in het resultaat kloppen niet na het schrijven",
            ),
            Vec::new(),
        ));
    }
    // HEADER.
    for &(regel, velden) in HEADER_VELDEN {
        for j in 0..velden.len() {
            if header_vluchtig(regel, j) || m[O].header_parameter(regel, j).is_none() {
                continue;
            }
            let k = plan.header.get(&(regel, j)).copied().unwrap_or(O);
            let verwacht = m[k].header_parameter(regel, j).map(|x| x.0);
            if r.header_parameter(regel, j).map(|x| x.0) != verwacht {
                uit.push((
                    Conflict::nieuw(ConflictSoort::Zelfcontrole, format!("{regel} klopt niet na het schrijven")),
                    Vec::new(),
                ));
            }
        }
    }
}

/// Per object en INVERSE met een bovengrens: hoeveel instanties verwijzen er via het FOR-attribuut naar.
fn inverse_tellingen(m: &Model) -> HashMap<(String, &'static str), (usize, u32)> {
    let mut tel: HashMap<(String, &'static str), (usize, u32)> = HashMap::new();
    // Per type: de attributen die een FOR-attribuut van een begrensde inverse zijn.
    let mut per_type: HashMap<u32, Vec<(usize, &'static str)>> = HashMap::new();
    for object in &m.objecten {
        let i = object.inst as usize;
        let Some(e) = m.entiteit(i) else { continue };
        let type_id = m.bestand.instanties[i].type_id;
        let velden = per_type.entry(type_id).or_insert_with(|| {
            let mut v = Vec::new();
            for (j, a) in e.attributen.iter().enumerate() {
                v.push((j, a.naam.as_str()));
            }
            v
        });
        for &(j, veld) in velden.iter() {
            for (_, token) in Tokens::nieuw(attribuut_tekst(m, i, j)) {
                let Token::Verwijzing(n) = token else { continue };
                let Some(&d) = m.bestand.index.get(&n) else { continue };
                let Soort::Object(od) = m.soort[d as usize] else { continue };
                let Some(doel) = m.entiteit(d as usize) else { continue };
                for inv in &doel.inverses {
                    if let Some(max) = inv.max {
                        if inv.voor == veld && e.is_een(&inv.bron) {
                            let ingang = tel
                                .entry((m.objecten[od as usize].sleutel.clone(), inv.naam.as_str()))
                                .or_insert((0, max));
                            ingang.0 += 1;
                        }
                    }
                }
            }
        }
    }
    tel
}

/// Meer relaties dan een INVERSE-bovengrens toestaat, terwijl dat in ons en in hun elk klopte.
fn inverse_grenzen(m: &[Model; 3], plan: &Plan, r: &Model, uit: &mut Gevonden) {
    let tr = inverse_tellingen(r);
    let to = inverse_tellingen(&m[O]);
    let th = inverse_tellingen(&m[H]);
    let mut lijst: Vec<_> = tr.iter().filter(|(_, &(n, max))| n > max as usize).collect();
    lijst.sort_by(|a, b| a.0.cmp(b.0));
    for ((sleutel, inverse), &(n, max)) in lijst {
        let k = (sleutel.clone(), *inverse);
        let binnen = |t: &HashMap<(String, &str), (usize, u32)>| t.get(&k).map_or(0, |x| x.0) <= max as usize;
        if !(binnen(&to) && binnen(&th)) {
            continue;
        }
        let Some(&si) = plan.op_sleutel.get(sleutel) else { continue };
        let mut c = Conflict::nieuw(
            ConflictSoort::TeVeelRelaties,
            format!("na het samenvoegen heeft dit object {n} relaties voor {inverse}, terwijl het schema hoogstens {max} toestaat"),
        )
        .over(m, &plan.sleutels[si]);
        c.attribuut = Some(inverse.to_string());
        // Betrokken: het object en de relaties die ernaar verwijzen.
        let mut betrokken = vec![si];
        betrokken.extend(relaties_naar(r, plan, sleutel));
        uit.push((c, betrokken));
    }
}

/// De sleutels (in het plan) van de relaties in `r` die naar dit object verwijzen.
fn relaties_naar(r: &Model, plan: &Plan, sleutel: &str) -> Vec<usize> {
    let mut uit = Vec::new();
    for object in &r.objecten {
        let i = object.inst as usize;
        if !r.is_relatie(i) {
            continue;
        }
        let noemt = r.bestand.verwijzingen(i).iter().any(|&d| match r.soort[d as usize] {
            Soort::Object(o) => r.objecten[o as usize].sleutel == sleutel,
            Soort::Knoop => false,
        });
        if noemt {
            if let Some(&s) = plan.op_sleutel.get(&object.sleutel) {
                uit.push(s);
            }
        }
    }
    uit
}

/// Het aantal elementen als een aggregaat buiten zijn grenzen valt.
fn buiten_grenzen(m: &Model, i: usize, j: usize) -> Option<(usize, u32, Option<u32>)> {
    let a = &m.entiteit(i)?.attributen[j];
    if !matches!(a.aggregaat, Some(Aggregaat::Set) | Some(Aggregaat::List) | Some(Aggregaat::Bag)) {
        return None;
    }
    let n = elementen(attribuut_tekst(m, i, j))?.len();
    let te_weinig = n < a.minimum as usize;
    let te_veel = a.maximum.is_some_and(|max| n > max as usize);
    (te_weinig || te_veel).then_some((n, a.minimum, a.maximum))
}

/// Aggregaten buiten de grenzen uit het schema, terwijl ons en hun daar binnen bleven.
fn aggregaat_grenzen(m: &[Model; 3], plan: &Plan, r: &Model, uit: &mut Gevonden) {
    for object in &r.objecten {
        let Some(&si) = plan.op_sleutel.get(&object.sleutel) else { continue };
        let s = &plan.sleutels[si];
        if s.besluit == Besluit::Heel(O) || s.besluit == Besluit::Heel(H) {
            continue;
        }
        let i = object.inst as usize;
        for j in 0..r.bestand.attributen(i).len() {
            let Some((n, min, max)) = buiten_grenzen(r, i, j) else { continue };
            let kant_binnen = |k: usize| {
                s.obj[k].is_none_or(|o| {
                    let ik = m[k].objecten[o].inst as usize;
                    j >= m[k].bestand.attributen(ik).len() || buiten_grenzen(&m[k], ik, j).is_none()
                })
            };
            if !(kant_binnen(O) && kant_binnen(H)) {
                continue;
            }
            let naam = r.attribuut_naam(i, j);
            let grenzen = format!("[{min}:{}]", max.map_or("?".to_string(), |x| x.to_string()));
            let mut c = Conflict::nieuw(
                ConflictSoort::BuitenGrenzen,
                format!("na het samenvoegen heeft {naam} {n} elementen; het schema eist {grenzen}"),
            )
            .over(m, s);
            c.attribuut = Some(naam);
            uit.push((c, vec![si]));
        }
    }
}

/// De boomrelaties: (ouder, kind) per relatie, met de sleutel van de relatie.
fn boomranden(m: &Model) -> Vec<(String, String, String)> {
    let mut uit = Vec::new();
    for object in &m.objecten {
        let i = object.inst as usize;
        let Some(e) = m.entiteit(i) else { continue };
        if !(e.is_een("IFCRELAGGREGATES") || e.is_een("IFCRELNESTS") || e.is_een("IFCRELCONTAINEDINSPATIALSTRUCTURE")) {
            continue;
        }
        let mut ouders = Vec::new();
        let mut kinderen = Vec::new();
        for (j, a) in e.attributen.iter().enumerate() {
            let rol = match a.naam.as_str() {
                "RelatingObject" | "RelatingStructure" => &mut ouders,
                "RelatedObjects" | "RelatedElements" => &mut kinderen,
                _ => continue,
            };
            for (_, token) in Tokens::nieuw(attribuut_tekst(m, i, j)) {
                if let Token::Verwijzing(n) = token {
                    if let Some(&d) = m.bestand.index.get(&n) {
                        if let Soort::Object(o) = m.soort[d as usize] {
                            rol.push(m.objecten[o as usize].sleutel.clone());
                        }
                    }
                }
            }
        }
        for ouder in &ouders {
            for kind in &kinderen {
                uit.push((ouder.clone(), kind.clone(), object.sleutel.clone()));
            }
        }
    }
    uit
}

/// Wat vanaf een IfcProject bereikbaar is via de boomrelaties.
fn bereikbaar_vanaf_project(m: &Model, randen: &[(String, String, String)]) -> HashSet<String> {
    let mut kinderen: HashMap<&str, Vec<&str>> = HashMap::new();
    for (ouder, kind, _) in randen {
        kinderen.entry(ouder).or_default().push(kind);
    }
    let mut uit: HashSet<String> = HashSet::new();
    let mut stapel: Vec<&str> = m
        .objecten
        .iter()
        .filter(|o| m.entiteit(o.inst as usize).is_some_and(|e| e.is_een("IFCPROJECT")))
        .map(|o| o.sleutel.as_str())
        .collect();
    while let Some(x) = stapel.pop() {
        if !uit.insert(x.to_string()) {
            continue;
        }
        if let Some(k) = kinderen.get(x) {
            stapel.extend(k.iter().copied());
        }
    }
    uit
}

/// De boomregel: geen kring in IfcRelAggregates, IfcRelNests en IfcRelContainedInSpatialStructure, en wat in ons én
/// in hun vanaf het project bereikbaar was, is dat in het resultaat ook.
/// De kringen in de boomrelaties: per kring het eerste lid en de sleutels van de leden en de relaties.
fn kringen(randen: &[(String, String, String)]) -> Vec<(String, Vec<String>)> {
    let mut kinderen: HashMap<&str, Vec<&str>> = HashMap::new();
    for (ouder, kind, _) in randen {
        kinderen.entry(ouder).or_default().push(kind);
    }
    // Diepte-eerst zoeken zonder recursie; 1 = op de stapel, 2 = klaar.
    let mut status: HashMap<&str, u8> = HashMap::new();
    let mut knopen: Vec<&str> = kinderen.keys().copied().collect();
    knopen.sort();
    let mut uit: Vec<(String, Vec<String>)> = Vec::new();
    for start in knopen {
        if status.contains_key(start) {
            continue;
        }
        let mut stapel: Vec<(&str, usize)> = vec![(start, 0)];
        status.insert(start, 1);
        while let Some(&mut (x, ref mut volgende)) = stapel.last_mut() {
            let lijst = kinderen.get(x).map(Vec::as_slice).unwrap_or(&[]);
            if *volgende < lijst.len() {
                let kind = lijst[*volgende];
                *volgende += 1;
                match status.get(kind) {
                    None => {
                        status.insert(kind, 1);
                        stapel.push((kind, 0));
                    }
                    Some(1) => {
                        let begin = stapel.iter().position(|(y, _)| *y == kind).unwrap_or(0);
                        let leden: Vec<String> = stapel[begin..].iter().map(|(y, _)| y.to_string()).collect();
                        let relaties: Vec<String> = randen
                            .iter()
                            .filter(|(o, k, _)| leden.contains(o) && leden.contains(k))
                            .map(|(_, _, rel)| rel.clone())
                            .collect();
                        uit.push((kind.to_string(), [leden, relaties].concat()));
                    }
                    _ => {}
                }
                continue;
            }
            status.insert(x, 2);
            stapel.pop();
        }
    }
    uit
}

/// De boomregel: geen kring in IfcRelAggregates, IfcRelNests en IfcRelContainedInSpatialStructure, en wat in ons én
/// in hun vanaf het project bereikbaar was, is dat in het resultaat ook. Een kring die ons of hun zelf al had, komt
/// niet door het samenvoegen; die melden we niet.
fn boom(m: &[Model; 3], plan: &Plan, r: &Model, uit: &mut Gevonden) {
    let rr = boomranden(r);
    let ro = boomranden(&m[O]);
    let rh = boomranden(&m[H]);
    let in_kring = if kringen(&ro).is_empty() && kringen(&rh).is_empty() { kringen(&rr) } else { Vec::new() };
    for (eerste, betrokken) in in_kring {
        let Some(&si) = plan.op_sleutel.get(&eerste) else { continue };
        let c = Conflict::nieuw(
            ConflictSoort::Boom,
            "na het samenvoegen zit er een kring in de boom (IfcRelAggregates, IfcRelNests of \
             IfcRelContainedInSpatialStructure)",
        )
        .over(m, &plan.sleutels[si]);
        let betrokken = betrokken.iter().filter_map(|s| plan.op_sleutel.get(s).copied()).collect();
        uit.push((c, betrokken));
    }
    // Los geraakt van het project.
    let bo = bereikbaar_vanaf_project(&m[O], &ro);
    let bh = bereikbaar_vanaf_project(&m[H], &rh);
    let br = bereikbaar_vanaf_project(r, &rr);
    let mut los: Vec<&String> =
        bo.iter().filter(|s| bh.contains(*s) && !br.contains(*s) && plan.in_resultaat(s)).collect();
    los.sort();
    for s in los {
        let si = plan.op_sleutel[s];
        let c = Conflict::nieuw(
            ConflictSoort::Boom,
            "dit object hing in ons en in hun aan het project, maar na het samenvoegen niet meer",
        )
        .over(m, &plan.sleutels[si]);
        let mut betrokken = vec![si];
        betrokken.extend(relaties_naar(r, plan, s));
        for k in [O, H] {
            let i = plan.sleutels[si].obj[k];
            if i.is_some() {
                betrokken.extend(relaties_naar(&m[k], plan, s));
            }
        }
        uit.push((c, betrokken));
    }
}
