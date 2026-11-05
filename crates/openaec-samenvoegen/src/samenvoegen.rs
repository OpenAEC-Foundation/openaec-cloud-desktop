//! Three-way samenvoegen per object: [`voeg_samen`].
//!
//! De regels (ontwerp §5.6):
//! - Alleen ons of alleen hun wijzigde, voegde toe of verwijderde een object: die kant.
//! - Beide wijzigden: per attribuut. Gelijk aan de basis aan één kant: de andere kant. Beide gelijk: die waarde.
//!   Anders: een SET (of BAG) van verwijzingen voegen we samen (wat erbij kwam en wat eraf ging, van beide kanten),
//!   eigenschappen en hoeveelheden per naam; al het andere is een conflict.
//! - De ene kant verwijderde, de andere wijzigde: conflict. Beide voegden hetzelfde object anders toe: conflict.
//!   Beide vervingen dezelfde relatie door een nieuwe met een andere GlobalId: conflict.
//! - Vluchtige velden ([`crate::schema::VLUCHTIG`], de tijdstempel in FILE_NAME): wijzigde alleen hun het veld, dan
//!   de waarde van hun, anders die van ons, welke kant de rest van het object ook levert. Het veld van een knoop
//!   (bijv. een IfcOwnerHistory) telt per object dat hem gebruikt ([`VluchtigeKnoop`]). Alleen als objecten die in
//!   het resultaat één knoop delen een andere waarde vragen, is het een conflict.
//! - Na het samenvoegen controleren we: verwijzingen naar verwijderde objecten, INVERSE-bovengrenzen (ook bij
//!   knopen, bijv. twee stijlen op één vorm), de grenzen van aggregaten, koppelingen met definities, de boom (geen
//!   kring, niets raakt los van het project), geen kring in de plaatsingen, en of het geschreven resultaat
//!   inhoudelijk is wat we bedoelden (zelfcontrole). Elk probleem is een conflict.
//! - Een onbekend schema, een ander schema in FILE_SCHEMA of een geval dat we niet kennen: conflict, geen
//!   samenvoeging.
//!
//! Bij een conflict is er geen resultaat, wel het rapport (K4). Er is geen modus "ons wint" of "hun wint": die
//! verliest ook werk van de andere kant dat niet botst.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use crate::koppelregel::koppelregel;
use crate::model::{
    attribuut_tekst, enkele_verwijzing, header_vluchtig, koppel_relaties, koppel_toevoegingen, markeer_gemeen,
    ongeordend_hash, op_sleutel, stem_ankers_af, Hash, Model, Soort, VersieStatistiek, HEADER_VELDEN,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSoort {
    /// Beide kanten wijzigden hetzelfde attribuut anders.
    BeideGewijzigd,
    /// De ene kant verwijderde, de andere wijzigde.
    VerwijderdEnGewijzigd,
    /// Beide kanten voegden een object met dezelfde GlobalId toe, met een andere inhoud.
    BeideToegevoegd,
    /// Beide kanten vervingen dezelfde relatie door een nieuwe, met een andere GlobalId.
    RelatieVervangen,
    /// Na het samenvoegen wijst iets naar een object dat er niet meer is.
    VerwijzingNaarVerwijderd,
    /// Na het samenvoegen heeft een object of knoop meer relaties dan een INVERSE-bovengrens toestaat.
    TeVeelRelaties,
    /// Een aggregaat valt buiten de grenzen uit het schema (bijv. een lege `RelatedElements`).
    BuitenGrenzen,
    /// Een koppeling met een definitie verdween door de ene kant, terwijl de andere kant de definitie wijzigde.
    KoppelingVerdwenen,
    /// Een kring in de boom, of een object dat los raakte van het project.
    Boom,
    /// Een kring in de plaatsingen (`PlacementRelTo`).
    PlaatsingKring,
    /// Een kring in de volgorde van taken (`IfcRelSequence`).
    VolgordeKring,
    /// Een gedeelde ankerknoop is in het resultaat niet eenduidig.
    GedeeldAnker,
    /// Beide kanten wijzigden dezelfde losse knopen (bijv. een kleur) anders.
    LosseKnopen,
    /// Beide kanten wijzigden hetzelfde veld in de HEADER anders.
    Header,
    /// Een schema of geval dat de bibliotheek niet kent, of versies met een ander schema.
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
    /// Objecten die beide kanten wijzigden en die we per attribuut samenvoegden (ook voor een vluchtig veld).
    pub objecten_samengevoegd: usize,
    /// Objecten die er na het samenvoegen niet meer zijn (verwijderd door een kant of door beide).
    pub objecten_verwijderd: usize,
    /// Nieuwe instanties achteraan (van hun overgenomen knopen en objecten).
    pub instanties_nieuw: usize,
    /// Regels van ons die we herschreven (objecten en hun ankerknopen).
    pub instanties_herschreven: usize,
    /// Regels van ons die we weghaalden.
    pub instanties_verwijderd: usize,
}

/// De uitkomst van [`voeg_samen`].
#[derive(Debug, Clone, Serialize)]
pub struct Samenvoeging {
    /// Het samengevoegde bestand; None als er een conflict is.
    #[serde(skip)]
    pub inhoud: Option<Vec<u8>>,
    /// Is er een resultaat?
    pub gelukt: bool,
    pub conflicten: Vec<Conflict>,
    pub statistiek: SamenvoegStatistiek,
}

/// Conflicten die een controle of het schrijven vond.
pub(crate) type Gevonden = Vec<Conflict>;

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
    /// De eigen knoop van het object (bijv. de IfcTaskTime van een taak), per attribuut samengevoegd: per attribuut
    /// van de knoop de kant. De schrijver herschrijft de knoop van ons.
    Knoop(Vec<usize>),
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
    /// De vluchtige velden van knopen (bijv. de tijdstempel in een IfcOwnerHistory) waar ons en hun verschillen, met
    /// de gekozen waarde per anker. De schrijver zet die in de knoop die het object in het resultaat heeft.
    pub vluchtige_knopen: Vec<VluchtigeKnoop>,
    /// Wat elk vluchtig veld in het resultaat moet zijn (voor de zelfcontrole).
    pub vluchtig_verwacht: HashMap<Vluchtig, Hash>,
}

/// Een anker: (sleutel in het plan, attribuut, positie).
pub(crate) type AnkerPlek = (usize, u32, u64);

/// Een vluchtig veld van een knoop met een keuze: het attribuut van de knoop, de gekozen kant en de hash van de
/// gekozen waarde.
pub(crate) type VluchtigVeld = (usize, usize, Hash);

/// De ankers van objecten die bij ons knoop `knopen[0]` hebben en bij hun `knopen[1]` (en in de basis dezelfde
/// knoop), met per vluchtig veld waar die twee verschillen de gekozen kant en waarde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VluchtigeKnoop {
    pub knopen: [u32; 2],
    pub velden: Vec<VluchtigVeld>,
    pub ankers: Vec<AnkerPlek>,
}

impl Plan {
    pub fn in_resultaat(&self, sleutel: &str) -> bool {
        self.op_sleutel.get(sleutel).is_some_and(|&s| self.sleutels[s].besluit != Besluit::Weg)
    }
}

/// Een vluchtig veld van een object (sleutel, attribuut) of van de HEADER. De vluchtige velden van knopen tellen
/// per anker van een object ([`vluchtige_knopen`], [`VluchtigeKnoop`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Vluchtig {
    Object(String, usize),
    Header(&'static str, usize),
}

/// Per knoop met een anker en vluchtige velden (bijv. een IfcOwnerHistory): de velden met de hash van hun waarde.
type VluchtigeKnopen = HashMap<u32, Vec<(usize, Hash)>>;

fn vluchtige_knopen(m: &Model) -> VluchtigeKnopen {
    let mut uit = HashMap::new();
    for (k, anker) in m.anker_id.iter().enumerate() {
        let (Some(_), Some(e)) = (anker, m.entiteit(k)) else { continue };
        let velden: Vec<(usize, Hash)> = (0..e.attributen.len())
            .filter(|&f| e.attributen[f].vluchtig)
            .map(|f| (f, m.waarde_hash(attribuut_tekst(m, k, f), false)))
            .collect();
        if !velden.is_empty() {
            uit.insert(k as u32, velden);
        }
    }
    uit
}

/// De knoop met vluchtige velden op anker (attribuut `j`, positie `p`) van object `o` (index) in `m`.
fn vluchtige_knoop_op(m: &Model, knopen: &VluchtigeKnopen, o: usize, j: u32, p: u64) -> Option<u32> {
    let (k, _) = m.op_positie(m.objecten[o].inst as usize, j as usize, p)?;
    knopen.contains_key(&k).then_some(k)
}

/// Hebben twee versies op elk anker dezelfde vluchtige waarden in hun knopen? Na [`gelijk`]: dezelfde objecten.
fn zelfde_ankers(a: &Model, ka: &VluchtigeKnopen, b: &Model, kb: &VluchtigeKnopen) -> bool {
    let tel = |m: &Model, k: &VluchtigeKnopen| k.keys().map(|&x| m.ankers(x as usize).len()).sum::<usize>();
    if tel(a, ka) != tel(b, kb) {
        return false;
    }
    let sb = op_sleutel(b);
    ka.iter().all(|(&k, velden)| {
        a.ankers(k as usize).iter().all(|an| {
            let Some(&ob) = sb.get(a.objecten[an.object as usize].sleutel.as_str()) else { return false };
            vluchtige_knoop_op(b, kb, ob, an.attribuut, an.positie).and_then(|x| kb.get(&x)) == Some(velden)
        })
    })
}

/// De vluchtige velden van een versie met de hash van hun waarde, en bij een knoop de instantie.
fn vluchtige_velden(m: &Model) -> HashMap<Vluchtig, (Hash, Option<u32>)> {
    let mut uit = HashMap::new();
    for object in &m.objecten {
        let i = object.inst as usize;
        let Some(e) = m.entiteit(i) else { continue };
        for (j, a) in e.attributen.iter().enumerate() {
            if a.vluchtig {
                let h = m.waarde_hash(attribuut_tekst(m, i, j), true);
                uit.insert(Vluchtig::Object(object.sleutel.clone(), j), (h, None));
            }
        }
    }
    for &(regel, velden) in HEADER_VELDEN {
        for j in 0..velden.len() {
            if header_vluchtig(regel, j) {
                if let Some((h, _)) = m.header_parameter(regel, j) {
                    uit.insert(Vluchtig::Header(regel, j), (h, None));
                }
            }
        }
    }
    uit
}

/// De regel voor een vluchtig veld: wijzigde alleen hun het, dan hun; anders ons.
fn vluchtige_kant(b: Option<Hash>, o: Hash, h: Hash) -> usize {
    if b == Some(o) && h != o {
        H
    } else {
        O
    }
}

/// Voegt de wijzigingen van ons en hun ten opzichte van de basis samen. Bij een conflict is er geen resultaat,
/// wel een rapport.
pub fn voeg_samen(basis: &[u8], ons: &[u8], hun: &[u8]) -> Result<Samenvoeging, Fout> {
    let (mb, mo, mh) = std::thread::scope(|s| {
        let b = s.spawn(|| Model::lees(basis));
        let h = s.spawn(|| Model::lees(hun));
        let o = Model::lees(ons);
        let fout = || Err(Fout::intern("lezen mislukt"));
        (b.join().unwrap_or_else(|_| fout()), o, h.join().unwrap_or_else(|_| fout()))
    });
    let (mut mb, mut mo, mut mh) = (mb?, mo?, mh?);
    let gekoppeld = koppel_relaties(&mb, &mut mo) + koppel_relaties(&mb, &mut mh);
    // Dezelfde nieuwe relatie aan beide kanten (bijv. A -> B in een planning), met een andere GlobalId.
    let gekoppeld = gekoppeld + koppel_toevoegingen(&mb, &mo, &mut mh);
    markeer_gemeen(&mut [&mut mb, &mut mo, &mut mh]);
    std::thread::scope(|s| {
        s.spawn(|| mb.bereken_puur());
        s.spawn(|| mh.bereken_puur());
        mo.bereken_puur();
    });
    // Een anker in een SET dat aan één kant een andere inhoud kreeg, houdt de positie van de basis.
    stem_ankers_af(&mb, &mut mo);
    stem_ankers_af(&mb, &mut mh);
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
    let vluchtig = [vluchtige_velden(&m[B]), vluchtige_velden(&m[O]), vluchtige_velden(&m[H])];
    let knopen = [vluchtige_knopen(&m[B]), vluchtige_knopen(&m[O]), vluchtige_knopen(&m[H])];
    // Wijzigde een kant niets (ook geen vluchtig veld), dan is het resultaat de andere kant, byte voor byte. Eerst op
    // bytes, zodat (b, b, hun) = hun en (b, ons, b) = ons ook gelden als alle drie inhoudelijk gelijk zijn.
    let zelfde = |a: usize, b: usize| {
        gelijk(&m[a], &m[b])
            && zelfde_vluchtig(&vluchtig[a], &vluchtig[b])
            && zelfde_ankers(&m[a], &knopen[a], &m[b], &knopen[b])
    };
    let ongewijzigd = if hun == basis {
        Some(ons)
    } else if ons == basis {
        Some(hun)
    } else if zelfde(B, H) || zelfde(O, H) {
        Some(ons)
    } else if zelfde(B, O) {
        Some(hun)
    } else {
        None
    };
    let mut conflicten = Vec::new();
    let plan = besluit(&m, &vluchtig, &knopen, &mut conflicten);
    if let Some(inhoud) = ongewijzigd {
        tel_besluiten(&plan, &mut statistiek);
        return Ok(Samenvoeging { inhoud: Some(inhoud.to_vec()), gelukt: true, conflicten, statistiek });
    }
    conflicten.extend(koppelregel(&m, &plan));
    conflicten.extend(relaties_vervangen(&m, &plan));
    if conflicten.is_empty() {
        conflicten = controleer_verwijzingen(&m, &plan);
    }
    tel_besluiten(&plan, &mut statistiek);
    if !conflicten.is_empty() {
        return mislukt(conflicten, statistiek);
    }
    let g = match schrijven::schrijf(&m, &plan) {
        Ok(g) => g,
        Err(c) => return mislukt(c, statistiek),
    };
    let mut gevonden = Vec::new();
    match Model::lees(&g.inhoud) {
        Ok(mut r) => {
            let gemeen: HashSet<&str> = m[B].objecten.iter().filter(|o| o.gemeen).map(|o| o.sleutel.as_str()).collect();
            for object in &mut r.objecten {
                match g.alias.get(&object.globalid) {
                    Some(sleutel) => object.sleutel = sleutel.clone(),
                    None => gevonden.push(Conflict::nieuw(
                        ConflictSoort::Zelfcontrole,
                        format!("het resultaat bevat een onbekend object {}", object.globalid),
                    )),
                }
                object.gemeen = gemeen.contains(object.sleutel.as_str());
            }
            r.bereken_puur();
            stem_ankers_af(&m[B], &mut r);
            r.bereken();
            controleer_resultaat(&m, &plan, &r, &mut gevonden);
        }
        Err(fout) => gevonden.push(Conflict::nieuw(
            ConflictSoort::Zelfcontrole,
            format!("het resultaat is geen geldig bestand: {fout}"),
        )),
    }
    if !gevonden.is_empty() {
        return mislukt(gevonden, statistiek);
    }
    statistiek.instanties_nieuw = g.nieuw;
    statistiek.instanties_herschreven = g.herschreven;
    statistiek.instanties_verwijderd = g.verwijderd;
    Ok(Samenvoeging { inhoud: Some(g.inhoud), gelukt: true, conflicten, statistiek })
}

/// Hebben twee versies dezelfde waarden in hun vluchtige velden?
fn zelfde_vluchtig(a: &HashMap<Vluchtig, (Hash, Option<u32>)>, b: &HashMap<Vluchtig, (Hash, Option<u32>)>) -> bool {
    a.len() == b.len() && a.iter().all(|(k, (h, _))| b.get(k).is_some_and(|(x, _)| x == h))
}

/// Zijn twee versies inhoudelijk gelijk (dezelfde objecten met dezelfde inhoud, dezelfde losse knopen en HEADER)?
/// Vluchtige velden tellen niet mee.
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

/// Gevallen die we niet kennen: een onbekend schema, een ander schema in FILE_SCHEMA, onbekende types, cycli...
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
    // Een ander FILE_SCHEMA is een wijziging die we niet samenvoegen (ook IFC4X3 tegenover IFC4X3_ADD2).
    let schema = m.each_ref().map(|x| x.header_parameter("FILE_SCHEMA", 0));
    if uit.is_empty()
        && (schema[B].map(|x| x.0) != schema[O].map(|x| x.0) || schema[B].map(|x| x.0) != schema[H].map(|x| x.0))
    {
        let toon = |k: usize| schema[k].map(|(_, t)| m[k].toon(t, false, MAX_TEKST));
        let [b, o, h] = [toon(B), toon(O), toon(H)];
        let tekst = |x: &Option<String>| x.clone().unwrap_or_else(|| "geen".into());
        let mut c = Conflict::nieuw(
            ConflictSoort::NietOndersteund,
            format!(
                "de versies hebben een ander FILE_SCHEMA (basis {}, ons {}, hun {})",
                tekst(&b),
                tekst(&o),
                tekst(&h)
            ),
        );
        c.attribuut = Some("FILE_SCHEMA".into());
        [c.basis, c.ons, c.hun] = [b, o, h];
        uit.push(c);
    }
    uit
}

/// De besluiten per object, per groep losse knopen, per HEADER-veld en per vluchtig veld.
fn besluit(
    m: &[Model; 3],
    vluchtig: &[HashMap<Vluchtig, (Hash, Option<u32>)>; 3],
    knopen: &[VluchtigeKnopen; 3],
    conflicten: &mut Vec<Conflict>,
) -> Plan {
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
    for s in &mut sleutels {
        s.besluit = besluit_object(m, s, conflicten);
    }
    // Vluchtige velden: per veld de kant volgens de regel, welke kant de rest van het object ook levert; wat het
    // resultaat moet worden, onthouden we.
    let mut vluchtig_verwacht = HashMap::new();
    let mut vluchtige_knopen = Vec::new();
    let mut header = besluit_header(m, conflicten);
    for (veld, &(ho, _)) in &vluchtig[O] {
        let Some(&(hh, _)) = vluchtig[H].get(veld) else { continue };
        let hb = vluchtig[B].get(veld).map(|x| x.0);
        let kant = vluchtige_kant(hb, ho, hh);
        vluchtig_verwacht.insert(veld.clone(), if kant == H { hh } else { ho });
        match veld {
            Vluchtig::Object(sleutel, j) => neem_vluchtig_van(m, &mut sleutels[op_sleutel[sleutel]], *j, kant),
            Vluchtig::Header(regel, j) => {
                if kant == H {
                    header.insert((regel, *j), H);
                }
            }
        }
    }
    // De waarden die hun per type heeft: heeft hun er geen andere dan een knoop van ons, dan verschilt er op geen
    // van diens ankers iets.
    let mut bij_hun: HashMap<&str, HashSet<&[(usize, Hash)]>> = HashMap::new();
    for (&kh, velden) in &knopen[H] {
        bij_hun.entry(m[H].bestand.type_naam(kh as usize)).or_default().insert(velden.as_slice());
    }
    let mut volgorde: Vec<u32> = knopen[O]
        .iter()
        .filter(|(&ko, velden)| {
            bij_hun
                .get(m[O].bestand.type_naam(ko as usize))
                .is_some_and(|x| x.len() > 1 || !x.contains(velden.as_slice()))
        })
        .map(|(&ko, _)| ko)
        .collect();
    volgorde.sort_unstable();
    // De vluchtige velden van knopen, per anker van een object dat ons en hun allebei hebben, en alleen waar ons en
    // hun verschillen; ankers met dezelfde knopen bij ons, hun en de basis horen samen. Na de objectvelden:
    // [`neem_deling_van`] kijkt naar het besluit per attribuut.
    let mut groepen: BTreeMap<(u32, u32, Option<u32>), Vec<AnkerPlek>> = BTreeMap::new();
    for &ko in &volgorde {
        for a in m[O].ankers(ko as usize) {
            let s = op_sleutel[m[O].objecten[a.object as usize].sleutel.as_str()];
            let op = |k: usize| {
                let x = sleutels[s].obj[k]?;
                vluchtige_knoop_op(&m[k], &knopen[k], x, a.attribuut, a.positie)
                    .filter(|&n| m[k].bestand.type_naam(n as usize) == m[O].bestand.type_naam(ko as usize))
            };
            let Some(kh) = op(H) else { continue };
            groepen.entry((ko, kh, op(B))).or_default().push((s, a.attribuut, a.positie));
        }
    }
    let waarde = |k: usize, knoop: u32, f: usize| knopen[k][&knoop].iter().find(|x| x.0 == f).map(|x| x.1);
    for ((ko, kh, kb), ankers) in groepen {
        let velden: Vec<VluchtigVeld> = knopen[O][&ko]
            .iter()
            .filter_map(|&(f, ho)| {
                let hh = waarde(H, kh, f)?;
                let kant = vluchtige_kant(kb.and_then(|x| waarde(B, x, f)), ho, hh);
                (ho != hh).then_some((f, kant, if kant == H { hh } else { ho }))
            })
            .collect();
        let Some(&(_, kant, _)) = velden.first() else { continue };
        for &(s, j, _) in &ankers {
            neem_deling_van(m, &mut sleutels[s], j as usize, kant, [ko, kh]);
        }
        vluchtige_knopen.push(VluchtigeKnoop { knopen: [ko, kh], velden, ankers });
    }
    let groepen = besluit_groepen(m, conflicten);
    Plan { sleutels, op_sleutel, groepen, header, vluchtige_knopen, vluchtig_verwacht }
}

/// Vluchtig attribuut `j` komt van `kant`, welke kant de rest van het object ook levert: het besluit zo aanpassen
/// dat alleen dat attribuut van `kant` komt. Hebben ons en hun een ander type, dan blijft het besluit; de
/// zelfcontrole meldt dan dat het veld niet klopt.
fn neem_vluchtig_van(m: &[Model; 3], s: &mut Sleutel, j: usize, kant: usize) {
    let levert = match &s.besluit {
        Besluit::Heel(k) => Some(*k),
        Besluit::Samen(delen) => match delen.get(j) {
            Some(Deel::Van(k)) => Some(*k),
            _ => None,
        },
        Besluit::Weg => return,
    };
    if levert == Some(kant) {
        return;
    }
    let (Some(o), Some(h)) = (s.obj[O], s.obj[H]) else { return };
    let (io, ih) = (m[O].objecten[o].inst as usize, m[H].objecten[h].inst as usize);
    let n = m[O].bestand.attributen(io).len();
    if m[O].bestand.type_naam(io) != m[H].bestand.type_naam(ih) || m[H].bestand.attributen(ih).len() != n {
        return;
    }
    let mut delen = match &s.besluit {
        Besluit::Heel(k) => vec![Deel::Van(*k); n],
        Besluit::Samen(delen) => delen.clone(),
        Besluit::Weg => return,
    };
    delen[0] = Deel::Van(O);
    delen[j] = Deel::Van(kant);
    s.besluit = Besluit::Samen(delen);
}

/// Een vluchtig veld van de knoop op attribuut `j` komt van `kant` en is daar anders dan bij de andere kant, en
/// `kant` deelt de knoop anders, met minder objecten (bijv. een eigen IfcOwnerHistory voor dit object). Is attribuut `j` verder aan beide kanten gelijk,
/// dan nemen we het van `kant`: zo krijgt het object die deling, en botst de gekozen waarde niet met die van de
/// andere objecten van de ruimer gedeelde knoop. `knopen`: de knoop op het anker bij ons en bij hun.
fn neem_deling_van(m: &[Model; 3], s: &mut Sleutel, j: usize, kant: usize, knopen: [u32; 2]) {
    let ander = if kant == H { O } else { H };
    let door = |k: usize| m[k].verankerd_door[knopen[k - 1] as usize];
    let id = |k: usize| m[k].anker_id[knopen[k - 1] as usize];
    if id(kant) == id(ander) || door(kant) >= door(ander) {
        return;
    }
    let (Some(o), Some(h)) = (s.obj[O], s.obj[H]) else { return };
    let (io, ih) = (m[O].objecten[o].inst as usize, m[H].objecten[h].inst as usize);
    let n = m[O].bestand.attributen(io).len();
    if m[O].bestand.type_naam(io) != m[H].bestand.type_naam(ih) || m[H].bestand.attributen(ih).len() != n {
        return;
    }
    if m[O].attribuut_hash(io, j) != m[H].attribuut_hash(ih, j) {
        return;
    }
    match &mut s.besluit {
        Besluit::Heel(k) if *k == ander => {
            let mut delen = vec![Deel::Van(ander); n];
            delen[0] = Deel::Van(O);
            delen[j] = Deel::Van(kant);
            s.besluit = Besluit::Samen(delen);
        }
        Besluit::Samen(delen) if delen[j] == Deel::Van(ander) => delen[j] = Deel::Van(kant),
        _ => {}
    }
}

fn besluit_object(m: &[Model; 3], s: &Sleutel, conflicten: &mut Vec<Conflict>) -> Besluit {
    let hash = |k: usize| s.obj[k].map(|o| m[k].objecten[o].hash);
    let (hb, ho, hh) = (hash(B), hash(O), hash(H));
    match (hb, ho, hh) {
        (Some(b), Some(o), Some(h)) => {
            if o == h || h == b {
                Besluit::Heel(O)
            } else if o == b {
                Besluit::Heel(H)
            } else {
                beide_gewijzigd(m, s, conflicten)
            }
        }
        (Some(b), Some(o), None) => {
            if o != b {
                conflicten.push(
                    Conflict::nieuw(
                        ConflictSoort::VerwijderdEnGewijzigd,
                        "ons wijzigde dit object, hun verwijderde het",
                    )
                    .over(m, s),
                );
            }
            Besluit::Weg
        }
        (Some(b), None, Some(h)) => {
            if h != b {
                conflicten.push(
                    Conflict::nieuw(
                        ConflictSoort::VerwijderdEnGewijzigd,
                        "hun wijzigde dit object, ons verwijderde het",
                    )
                    .over(m, s),
                );
            }
            Besluit::Weg
        }
        (Some(_), None, None) | (None, None, None) => Besluit::Weg,
        (None, Some(o), Some(h)) => {
            if o != h {
                conflicten.push(
                    Conflict::nieuw(
                        ConflictSoort::BeideToegevoegd,
                        "ons en hun voegden allebei dit object toe, met een andere inhoud",
                    )
                    .over(m, s),
                );
            }
            Besluit::Heel(O)
        }
        (None, Some(_), None) => Besluit::Heel(O),
        (None, None, Some(_)) => Besluit::Heel(H),
    }
}

/// De objecten (sleutels) waar een relatie naar verwijst, behalve via OwnerHistory.
fn relatie_objecten(m: &Model, o: usize) -> HashSet<String> {
    let i = m.objecten[o].inst as usize;
    let mut uit = HashSet::new();
    for (j, _) in m.bestand.attributen(i).iter().enumerate().skip(2) {
        for (_, token) in Tokens::nieuw(attribuut_tekst(m, i, j)) {
            let Token::Verwijzing(n) = token else { continue };
            if let Some(&d) = m.bestand.index.get(&n) {
                if let Soort::Object(x) = m.soort[d as usize] {
                    uit.insert(m.objecten[x as usize].sleutel.clone());
                }
            }
        }
    }
    uit
}

/// Beide kanten vervingen dezelfde relatie door een nieuwe met een andere GlobalId (bijv. elk een eigen
/// IfcRelAssociatesMaterial voor W en V): een relatie uit de basis die ons en hun allebei weghaalden, terwijl ons en
/// hun elk een nieuwe relatie van hetzelfde type toevoegden die naar een van dezelfde objecten wijst. Dan weten we
/// niet welke vervanging geldt, en zouden we er twee schrijven.
fn relaties_vervangen(m: &[Model; 3], plan: &Plan) -> Vec<Conflict> {
    let mut nieuw: [Vec<(usize, HashSet<String>)>; 3] = Default::default();
    for (si, s) in plan.sleutels.iter().enumerate() {
        for k in [O, H] {
            if let (None, Some(o)) = (s.obj[B], s.obj[k]) {
                if s.obj[O].is_some() != s.obj[H].is_some() && m[k].is_relatie(m[k].objecten[o].inst as usize) {
                    nieuw[k].push((si, relatie_objecten(&m[k], o)));
                }
            }
        }
    }
    let mut uit = Vec::new();
    if nieuw[O].is_empty() || nieuw[H].is_empty() {
        return uit;
    }
    for s in &plan.sleutels {
        let (Some(ob), None, None) = (s.obj[B], s.obj[O], s.obj[H]) else { continue };
        let ib = m[B].objecten[ob].inst as usize;
        if !m[B].is_relatie(ib) {
            continue;
        }
        let type_naam = m[B].bestand.type_naam(ib);
        let objecten = relatie_objecten(&m[B], ob);
        let vervangers = |k: usize| -> Vec<String> {
            nieuw[k]
                .iter()
                .filter(|(si, doelen)| {
                    let o = plan.sleutels[*si].obj[k].expect("nieuw aan deze kant");
                    m[k].bestand.type_naam(m[k].objecten[o].inst as usize) == type_naam
                        && !doelen.is_disjoint(&objecten)
                })
                .map(|(si, _)| plan.sleutels[*si].sleutel.clone())
                .collect()
        };
        let (vo, vh) = (vervangers(O), vervangers(H));
        if vo.is_empty() || vh.is_empty() {
            continue;
        }
        let mut c = Conflict::nieuw(
            ConflictSoort::RelatieVervangen,
            "ons en hun vervingen deze relatie allebei door een nieuwe, met een andere GlobalId; welke geldt, is niet \
             zeker",
        )
        .over(m, s);
        c.basis = Some(m[B].object_weergave(ob));
        c.ons = Some(vo.join(", "));
        c.hun = Some(vh.join(", "));
        uit.push(c);
    }
    uit
}

/// Beide kanten wijzigden het object: per attribuut.
fn beide_gewijzigd(m: &[Model; 3], s: &Sleutel, conflicten: &mut Vec<Conflict>) -> Besluit {
    let inst = |k: usize| m[k].objecten[s.obj[k].unwrap()].inst as usize;
    let i = [inst(B), inst(O), inst(H)];
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
        conflicten.push(c);
        return Besluit::Heel(O);
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
            if let Some((lijst, botsingen)) = verzameling(m, i, j, tas) {
                for (groep, teksten) in botsingen {
                    let mut c = Conflict::nieuw(
                        ConflictSoort::BeideGewijzigd,
                        format!("beide kanten wijzigden '{groep}' in {} anders", attribuut.naam),
                    )
                    .over(m, s);
                    c.attribuut = Some(format!("{}[{groep}]", attribuut.naam));
                    [c.basis, c.ons, c.hun] = teksten;
                    conflicten.push(c);
                }
                delen.push(Deel::Lijst(lijst));
                continue;
            }
        }
        if let Some(kanten) = eigen_knoop_samenvoegen(m, s, i, j, &attribuut.naam, conflicten) {
            delen.push(Deel::Knoop(kanten));
            continue;
        }
        let mut c =
            Conflict::nieuw(ConflictSoort::BeideGewijzigd, format!("beide kanten wijzigden {} anders", attribuut.naam))
                .over(m, s);
        c.attribuut = Some(attribuut.naam.clone());
        c.basis = Some(toon(B));
        c.ons = Some(toon(O));
        c.hun = Some(toon(H));
        conflicten.push(c);
        delen.push(Deel::Van(O));
    }
    Besluit::Samen(delen)
}

/// Beide kanten wijzigden attribuut `j`, een verwijzing naar een knoop die alleen bij dit object hoort (bijv. de
/// IfcTaskTime van een taak: ons de duur, hun de naam). Dan per attribuut van die knoop, zoals bij een object: twee
/// andere waarden voor hetzelfde attribuut zijn een conflict (`TaskTime.ScheduleDuration`). Geeft per attribuut van
/// de knoop de kant, of None als de regel niet past: een ander type, een vluchtig veld, of een attribuut van hun met
/// een verwijzing (de schrijver neemt de tekst van hun letterlijk over). Dan blijft het een conflict op `j`.
fn eigen_knoop_samenvoegen(
    m: &[Model; 3],
    s: &Sleutel,
    i: [usize; 3],
    j: usize,
    naam: &str,
    conflicten: &mut Vec<Conflict>,
) -> Option<Vec<usize>> {
    let c = [m[B].eigen_knoop(i[B], j)?, m[O].eigen_knoop(i[O], j)?, m[H].eigen_knoop(i[H], j)?];
    let entiteit = m[O].entiteit(c[O])?;
    let n = entiteit.attributen.len();
    let type_naam = m[O].bestand.type_naam(c[O]);
    let zelfde_type =
        (0..3).all(|k| m[k].bestand.type_naam(c[k]) == type_naam && m[k].bestand.attributen(c[k]).len() == n);
    if !zelfde_type || entiteit.attributen.iter().any(|a| a.vluchtig) {
        return None;
    }
    let mut kanten = Vec::with_capacity(n);
    let mut botsingen = Vec::new();
    for l in 0..n {
        let h = [m[B].attribuut_hash(c[B], l), m[O].attribuut_hash(c[O], l), m[H].attribuut_hash(c[H], l)];
        let kant = if h[O] == h[H] || h[H] == h[B] {
            O
        } else if h[O] == h[B] {
            H
        } else {
            botsingen.push(l);
            O
        };
        let tekst = attribuut_tekst(&m[H], c[H], l);
        if kant == H && Tokens::nieuw(tekst).any(|(_, t)| matches!(t, Token::Verwijzing(_))) {
            return None;
        }
        kanten.push(kant);
    }
    for l in botsingen {
        let veld = format!("{naam}.{}", entiteit.attributen[l].naam);
        let toon = |k: usize| m[k].toon(attribuut_tekst(&m[k], c[k], l), false, MAX_TEKST);
        let mut x =
            Conflict::nieuw(ConflictSoort::BeideGewijzigd, format!("beide kanten wijzigden {veld} anders")).over(m, s);
        x.attribuut = Some(veld);
        x.basis = Some(toon(B));
        x.ons = Some(toon(O));
        x.hun = Some(toon(H));
        conflicten.push(x);
    }
    Some(kanten)
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
/// waarden); bij een botsing staat in de lijst de groep van ons (er komt dan geen resultaat).
fn verzameling(m: &[Model; 3], i: [usize; 3], j: usize, tas: bool) -> Option<(LijstDelen, Vec<Botsing>)> {
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
            (O, None)
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

/// Groepen losse knopen: per sleutel de groep van ons of van hun (bij een conflict van ons; er komt dan geen
/// resultaat).
fn besluit_groepen(m: &[Model; 3], conflicten: &mut Vec<Conflict>) -> Vec<(Hash, usize)> {
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
            conflicten.push(c);
            O
        };
        uit.push((s, kant));
    }
    uit
}

/// De HEADER: per veld three-way. De tijdstempel is vluchtig ([`besluit`]); FILE_SCHEMA is al gelijk
/// ([`niet_ondersteund`]).
fn besluit_header(m: &[Model; 3], conflicten: &mut Vec<Conflict>) -> HashMap<(&'static str, usize), usize> {
    let mut uit = HashMap::new();
    for &(regel, velden) in HEADER_VELDEN {
        for (j, veld) in velden.iter().enumerate() {
            if header_vluchtig(regel, j) || regel == "FILE_SCHEMA" {
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
            conflicten.push(c);
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
                    // De knoop van ons; wat van hun komt, heeft geen verwijzingen.
                    Deel::Knoop(_) => {
                        let i = m[O].objecten[s.obj[O].unwrap()].inst as usize;
                        uit.push((O, j, attribuut_tekst(&m[O], i, j)));
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
    for s in &plan.sleutels {
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
                uit.push(c);
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
                uit.push(c);
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
    volgorde(m, plan, r, uit);
    plaatsingskring(m, plan, r, uit);
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
                        let stukken = elementen
                            .iter()
                            .map(|&(k, (b, e))| {
                                let i = m[k].objecten[s.obj[k].unwrap()].inst as usize;
                                let mut stuk = Vec::new();
                                m[k].canoniek(&attribuut_tekst(&m[k], i, j)[b..e], true, &mut stuk);
                                stuk
                            })
                            .collect();
                        ongeordend_hash(stukken)
                    }
                    Deel::Knoop(kanten) => {
                        let knoop = |k: usize| {
                            let i = m[k].objecten[s.obj[k].unwrap()].inst as usize;
                            m[k].eigen_knoop(i, j).expect("gecontroleerd in eigen_knoop_samenvoegen")
                        };
                        let c = [0, knoop(O), knoop(H)];
                        let delen: Vec<(&Model, usize)> = kanten.iter().map(|&k| (&m[k], c[k])).collect();
                        Model::samengestelde_knoop_hash(m[O].bestand.type_naam(c[O]), &delen)
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
    for s in &plan.sleutels {
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
            uit.push(Conflict::nieuw(ConflictSoort::Zelfcontrole, melding).over(m, s));
        }
    }
    for object in &r.objecten {
        if !plan.op_sleutel.contains_key(&object.sleutel) {
            uit.push(Conflict::nieuw(
                ConflictSoort::Zelfcontrole,
                format!("onbekend object {} in het resultaat", object.globalid),
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
        uit.push(Conflict::nieuw(
            ConflictSoort::Zelfcontrole,
            "de losse knopen in het resultaat kloppen niet na het schrijven (bijv. een stijl op de geometrie van een \
             ander object)",
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
                uit.push(Conflict::nieuw(ConflictSoort::Zelfcontrole, format!("{regel} klopt niet na het schrijven")));
            }
        }
    }
    // Vluchtige velden: de waarde volgens de regel ([`vluchtige_kant`]).
    // Een veld dat in het resultaat ontbreekt terwijl zijn object er is, klopt ook niet.
    let velden_r = vluchtige_velden(r);
    let mut fout: Vec<String> = plan
        .vluchtig_verwacht
        .iter()
        .filter(|(veld, h)| match veld {
            Vluchtig::Object(sleutel, _) if !plan.in_resultaat(sleutel) => false,
            _ => velden_r.get(*veld).is_none_or(|(x, _)| x != *h),
        })
        .map(|(veld, _)| match veld {
            Vluchtig::Object(sleutel, j) => format!("{sleutel} attribuut {}", j + 1),
            Vluchtig::Header(regel, j) => format!("{regel} parameter {}", j + 1),
        })
        .collect();
    // De vluchtige velden van knopen, per anker.
    let knopen_r = vluchtige_knopen(r);
    for vk in &plan.vluchtige_knopen {
        for &(s, j, p) in &vk.ankers {
            let sl = &plan.sleutels[s];
            if sl.besluit == Besluit::Weg {
                continue;
            }
            let k = in_r.get(sl.sleutel.as_str()).and_then(|&o| vluchtige_knoop_op(r, &knopen_r, o, j, p));
            for &(f, _, hash) in &vk.velden {
                if k.and_then(|k| knopen_r[&k].iter().find(|x| x.0 == f).map(|x| x.1)) != Some(hash) {
                    fout.push(format!("{} attribuut {}, veld {} van de knoop", sl.sleutel, j + 1, f + 1));
                }
            }
        }
    }
    fout.sort();
    if let Some(eerste) = fout.first() {
        uit.push(Conflict::nieuw(
            ConflictSoort::Zelfcontrole,
            format!("een vluchtig veld klopt niet na het schrijven ({eerste})"),
        ));
    }
}

/// Tellingen voor de INVERSE-bovengrenzen: per object (sleutel) en inverse het aantal en de grens, en per type en
/// inverse het aantal knopen dat over de grens gaat (bijv. twee IfcStyledItem op één vorm: `StyledByItem`).
#[derive(Default)]
struct InverseTellingen {
    objecten: HashMap<(String, &'static str), (usize, u32)>,
    knopen_over: HashMap<(&'static str, &'static str), (usize, u32)>,
}

fn inverse_tellingen(m: &Model) -> InverseTellingen {
    // Per type: de attributen die het FOR-attribuut van een begrensde inverse kunnen zijn.
    let mut per_type: HashMap<u32, Vec<(usize, &'static str)>> = HashMap::new();
    let mut knopen: HashMap<(u32, &'static str), (usize, u32)> = HashMap::new();
    let mut uit = InverseTellingen::default();
    for i in 0..m.bestand.instanties.len() {
        let Some(e) = m.entiteit(i) else { continue };
        let type_id = m.bestand.instanties[i].type_id;
        let velden = per_type.entry(type_id).or_insert_with(|| {
            e.attributen
                .iter()
                .enumerate()
                .filter(|(_, a)| m.schema().is_some_and(|s| s.is_inverse_voor(e, &a.naam)))
                .map(|(j, a)| (j, a.naam.as_str()))
                .collect()
        });
        for &(j, veld) in velden.iter() {
            for (_, token) in Tokens::nieuw(attribuut_tekst(m, i, j)) {
                let Token::Verwijzing(n) = token else { continue };
                let Some(&d) = m.bestand.index.get(&n) else { continue };
                let Some(doel) = m.entiteit(d as usize) else { continue };
                for inv in &doel.inverses {
                    let Some(max) = inv.max else { continue };
                    if inv.voor != veld || !e.is_een(&inv.bron) {
                        continue;
                    }
                    match m.soort[d as usize] {
                        Soort::Object(od) => {
                            let sleutel = m.objecten[od as usize].sleutel.clone();
                            uit.objecten.entry((sleutel, inv.naam.as_str())).or_insert((0, max)).0 += 1;
                        }
                        Soort::Knoop => knopen.entry((d, inv.naam.as_str())).or_insert((0, max)).0 += 1,
                    }
                }
            }
        }
    }
    for ((d, inverse), (n, max)) in knopen {
        if n > max as usize {
            let type_naam = m.entiteit(d as usize).map_or("", |e| e.naam.as_str());
            uit.knopen_over.entry((type_naam, inverse)).or_insert((0, max)).0 += 1;
        }
    }
    uit
}

/// Meer relaties dan een INVERSE-bovengrens toestaat, terwijl dat in ons en in hun elk klopte. Bij knopen (die
/// geen vaste identiteit hebben): meer knopen over de grens dan in ons en in hun.
fn inverse_grenzen(m: &[Model; 3], plan: &Plan, r: &Model, uit: &mut Gevonden) {
    let tr = inverse_tellingen(r);
    let to = inverse_tellingen(&m[O]);
    let th = inverse_tellingen(&m[H]);
    let mut lijst: Vec<_> = tr.objecten.iter().filter(|(_, &(n, max))| n > max as usize).collect();
    lijst.sort_by(|a, b| a.0.cmp(b.0));
    for ((sleutel, inverse), &(n, max)) in lijst {
        let k = (sleutel.clone(), *inverse);
        let binnen = |t: &InverseTellingen| t.objecten.get(&k).map_or(0, |x| x.0) <= max as usize;
        if !(binnen(&to) && binnen(&th)) {
            continue;
        }
        let Some(&si) = plan.op_sleutel.get(sleutel) else { continue };
        let mut c = Conflict::nieuw(
            ConflictSoort::TeVeelRelaties,
            format!(
                "na het samenvoegen heeft dit object {n} relaties voor {inverse}, terwijl het schema hoogstens {max} \
                 toestaat"
            ),
        )
        .over(m, &plan.sleutels[si]);
        c.attribuut = Some(inverse.to_string());
        uit.push(c);
    }
    let mut knopen: Vec<_> = tr.knopen_over.iter().collect();
    knopen.sort();
    for (&(type_naam, inverse), &(n, max)) in knopen {
        let aantal = |t: &InverseTellingen| t.knopen_over.get(&(type_naam, inverse)).map_or(0, |x| x.0);
        let (o, h) = (aantal(&to), aantal(&th));
        if n <= o.max(h) {
            continue;
        }
        let mut c = Conflict::nieuw(
            ConflictSoort::TeVeelRelaties,
            format!(
                "na het samenvoegen hebben {n} knopen van type {type_naam} meer dan {max} relaties voor {inverse} \
                 (ons: {o}, hun: {h}); bijv. twee stijlen op dezelfde vorm"
            ),
        );
        c.type_naam = Some(type_naam.to_string());
        c.attribuut = Some(inverse.to_string());
        uit.push(c);
    }
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
            uit.push(c);
        }
    }
}

/// De boomrelaties: (ouder, kind) per relatie, met de sleutel van de relatie.
fn boomranden(m: &Model) -> Vec<(String, String, String)> {
    randen(
        m,
        &["IFCRELAGGREGATES", "IFCRELNESTS", "IFCRELCONTAINEDINSPATIALSTRUCTURE"],
        &["RelatingObject", "RelatingStructure"],
        &["RelatedObjects", "RelatedElements"],
    )
}

/// De volgorde van taken (`IfcRelSequence`): (voorganger, opvolger) per relatie, met de sleutel van de relatie.
fn volgranden(m: &Model) -> Vec<(String, String, String)> {
    randen(m, &["IFCRELSEQUENCE"], &["RelatingProcess"], &["RelatedProcess"])
}

/// (van, naar, relatie) voor elke relatie van een van de `typen`: van elk object in een attribuut uit `van` naar elk
/// object in een attribuut uit `naar`.
fn randen(m: &Model, typen: &[&str], van: &[&str], naar: &[&str]) -> Vec<(String, String, String)> {
    let mut uit = Vec::new();
    for object in &m.objecten {
        let i = object.inst as usize;
        let Some(e) = m.entiteit(i) else { continue };
        if !typen.iter().any(|t| e.is_een(t)) {
            continue;
        }
        let mut ouders = Vec::new();
        let mut kinderen = Vec::new();
        for (j, a) in e.attributen.iter().enumerate() {
            let rol = match a.naam.as_str() {
                x if van.contains(&x) => &mut ouders,
                x if naar.contains(&x) => &mut kinderen,
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

/// De kringen in de boomrelaties: per kring het lid waar we hem vonden.
fn kringen(randen: &[(String, String, String)]) -> Vec<String> {
    let mut kinderen: HashMap<&str, Vec<&str>> = HashMap::new();
    for (ouder, kind, _) in randen {
        kinderen.entry(ouder).or_default().push(kind);
    }
    // Diepte-eerst zoeken zonder recursie; 1 = op de stapel, 2 = klaar.
    let mut status: HashMap<&str, u8> = HashMap::new();
    let mut knopen: Vec<&str> = kinderen.keys().copied().collect();
    knopen.sort();
    let mut uit = Vec::new();
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
                    Some(1) => uit.push(kind.to_string()),
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
    for eerste in in_kring {
        let Some(&si) = plan.op_sleutel.get(&eerste) else { continue };
        let c = Conflict::nieuw(
            ConflictSoort::Boom,
            "na het samenvoegen zit er een kring in de boom (IfcRelAggregates, IfcRelNests of \
             IfcRelContainedInSpatialStructure)",
        )
        .over(m, &plan.sleutels[si]);
        uit.push(c);
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
        uit.push(c);
    }
}

/// Geen nieuwe kring in de volgorde van taken (`IfcRelSequence`): ons legt A -> B, hun B -> A. Elke kant apart is een
/// goede planning, samen niet; OPS kan zo'n planning niet doorrekenen. Een relatie van A naar B ligt in een kring als
/// A en B elkaar bereiken. Nieuw is zo'n relatie als A en B elkaar in ons en in hun niet bereikten. Een kring die een
/// kant elders al had, verbergt zo geen nieuwe. Per nieuwe kring één conflict.
fn volgorde(m: &[Model; 3], plan: &Plan, r: &Model, uit: &mut Gevonden) {
    let randen = [volgranden(&m[O]), volgranden(&m[H])];
    let delen = [samenhang(&randen[0]), samenhang(&randen[1])];
    let al_in_kring = |k: usize, a: &str, b: &str| {
        if a == b {
            randen[k].iter().any(|(x, y, _)| x == a && y == a)
        } else {
            delen[k].get(a).is_some_and(|d| delen[k].get(b) == Some(d))
        }
    };
    let rr = volgranden(r);
    let dr = samenhang(&rr);
    let mut nieuw: Vec<&(String, String, String)> = rr
        .iter()
        .filter(|(a, b, _)| dr[a.as_str()] == dr[b.as_str()] && !al_in_kring(0, a, b) && !al_in_kring(1, a, b))
        .collect();
    nieuw.sort();
    let in_r = op_sleutel(r);
    let naam = |s: &str| in_r.get(s).map_or(s.to_string(), |&o| r.object_weergave(o));
    let mut gemeld = HashSet::new();
    for (a, b, relatie) in nieuw {
        if !gemeld.insert(dr[a.as_str()]) {
            continue;
        }
        let Some(&si) = plan.op_sleutel.get(relatie) else { continue };
        let mut c = Conflict::nieuw(
            ConflictSoort::VolgordeKring,
            format!(
                "na het samenvoegen zit de relatie van {} naar {} in een kring in de volgorde van taken \
                 (IfcRelSequence); ons en hun hadden die kring elk niet",
                naam(a),
                naam(b)
            ),
        )
        .over(m, &plan.sleutels[si]);
        c.attribuut = Some("RelatedProcess".into());
        uit.push(c);
    }
}

/// Per knoop (sleutel) van de graaf `randen` het nummer van zijn sterk samenhangende deel: twee knopen met hetzelfde
/// nummer bereiken elkaar. Tarjan, zonder recursie.
fn samenhang(randen: &[(String, String, String)]) -> HashMap<&str, usize> {
    let mut opvolgers: HashMap<&str, Vec<&str>> = HashMap::new();
    for (a, b, _) in randen {
        opvolgers.entry(a).or_default().push(b);
        opvolgers.entry(b).or_default();
    }
    let mut knopen: Vec<&str> = opvolgers.keys().copied().collect();
    knopen.sort_unstable();
    // Per knoop: (volgnummer, laagste bereikbare volgnummer op de stapel).
    let mut nummer: HashMap<&str, (usize, usize)> = HashMap::new();
    let mut stapel: Vec<&str> = Vec::new();
    let mut op_stapel: HashSet<&str> = HashSet::new();
    let mut uit = HashMap::new();
    for start in knopen {
        if nummer.contains_key(start) {
            continue;
        }
        let mut pad: Vec<(&str, usize)> = vec![(start, 0)];
        nummer.insert(start, (nummer.len(), nummer.len()));
        stapel.push(start);
        op_stapel.insert(start);
        while let Some(&mut (x, ref mut volgende)) = pad.last_mut() {
            if let Some(&y) = opvolgers[x].get(*volgende) {
                *volgende += 1;
                match nummer.get(y) {
                    None => {
                        nummer.insert(y, (nummer.len(), nummer.len()));
                        stapel.push(y);
                        op_stapel.insert(y);
                        pad.push((y, 0));
                    }
                    Some(&(ny, _)) if op_stapel.contains(y) => {
                        let nx = nummer.get_mut(x).expect("bezocht");
                        nx.1 = nx.1.min(ny);
                    }
                    Some(_) => {}
                }
                continue;
            }
            pad.pop();
            let (eigen, laagste) = nummer[x];
            if let Some(&(ouder, _)) = pad.last() {
                let no = nummer.get_mut(ouder).expect("bezocht");
                no.1 = no.1.min(laagste);
            }
            if eigen == laagste {
                while let Some(y) = stapel.pop() {
                    op_stapel.remove(y);
                    uit.insert(y, eigen);
                    if y == x {
                        break;
                    }
                }
            }
        }
    }
    uit
}

/// De plaatsingen (`PlacementRelTo`): per instantie de plaatsing waar hij relatief aan staat.
fn plaatsing_ouders(m: &Model) -> HashMap<u32, u32> {
    let mut uit = HashMap::new();
    let mut per_type: HashMap<u32, Option<usize>> = HashMap::new();
    for i in 0..m.bestand.instanties.len() {
        let type_id = m.bestand.instanties[i].type_id;
        let j = *per_type.entry(type_id).or_insert_with(|| {
            m.entiteit(i).and_then(|e| e.attributen.iter().position(|a| a.naam == "PlacementRelTo"))
        });
        let Some(j) = j else { continue };
        if let Some(&d) = enkele_verwijzing(attribuut_tekst(m, i, j)).and_then(|n| m.bestand.index.get(&n)) {
            uit.insert(i as u32, d);
        }
    }
    uit
}

/// Een plaatsing in een kring van `PlacementRelTo`, als die er is.
fn plaatsing_in_kring(ouders: &HashMap<u32, u32>) -> Option<u32> {
    // 1 = op het huidige pad, 2 = klaar.
    let mut status: HashMap<u32, u8> = HashMap::new();
    let mut starts: Vec<u32> = ouders.keys().copied().collect();
    starts.sort_unstable();
    for start in starts {
        let mut pad = Vec::new();
        let mut x = start;
        loop {
            match status.get(&x) {
                Some(1) => return Some(x),
                Some(_) => break,
                None => {}
            }
            status.insert(x, 1);
            pad.push(x);
            match ouders.get(&x) {
                Some(&y) => x = y,
                None => break,
            }
        }
        for p in pad {
            status.insert(p, 2);
        }
    }
    None
}

/// Geen kring in de plaatsingen: ons zet P relatief aan Q, hun Q relatief aan P. Een kring die ons of hun zelf al
/// had, melden we niet.
fn plaatsingskring(m: &[Model; 3], plan: &Plan, r: &Model, uit: &mut Gevonden) {
    if plaatsing_in_kring(&plaatsing_ouders(&m[O])).is_some() || plaatsing_in_kring(&plaatsing_ouders(&m[H])).is_some()
    {
        return;
    }
    let ouders = plaatsing_ouders(r);
    let Some(eerste) = plaatsing_in_kring(&ouders) else { return };
    let mut leden = vec![eerste];
    let mut x = ouders[&eerste];
    while x != eerste && leden.len() <= ouders.len() {
        leden.push(x);
        x = ouders[&x];
    }
    // De objecten waarvan de plaatsingen in de kring zitten.
    let mut namen: Vec<String> =
        leden.iter().flat_map(|&k| r.ankers(k as usize).iter().map(|a| r.object_weergave(a.object as usize))).collect();
    namen.sort();
    namen.dedup();
    let mut c = Conflict::nieuw(
        ConflictSoort::PlaatsingKring,
        format!(
            "na het samenvoegen staan plaatsingen in een kring relatief aan elkaar (PlacementRelTo): {}",
            namen.join(", ")
        ),
    );
    let eerste_object = leden.iter().find_map(|&k| r.ankers(k as usize).first().map(|a| a.object as usize));
    if let Some(&si) = eerste_object.and_then(|o| plan.op_sleutel.get(r.objecten[o].sleutel.as_str())) {
        c = c.over(m, &plan.sleutels[si]);
    }
    c.attribuut = Some("ObjectPlacement.PlacementRelTo".into());
    uit.push(c);
}
