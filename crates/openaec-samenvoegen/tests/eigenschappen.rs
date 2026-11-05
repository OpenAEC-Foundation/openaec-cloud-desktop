//! Eigenschappen van samenvoegen, getoetst met property-based tests (proptest).
//!
//! Een generator maakt uit een echt testbestand willekeurige bewerkingen: een naam of beschrijving wijzigen, een
//! eigenschap wijzigen of toevoegen, een nieuwe eigenschappenset aan een object hangen, een relatie weghalen (met haar
//! eigenschappenset), een object verplaatsen, een element in een verdieping zetten of eruit halen, de tijdstempels
//! zetten zoals Open Planner Studio bij elk opslaan doet, en alles opnieuw nummeren. Twee reeksen bewerkingen op
//! dezelfde basis geven `ons` en `hun`.
//!
//! "Gelijk" (≡) betekent: het verschil volgens de bibliotheek (`verschil`) is leeg. Vluchtige velden tellen daarin
//! niet mee (ontwerp §5.6).
//!
//! De bewerkingen werken op de tekst, met een eigen kleine lezer (`Doc`); ze delen geen code met de bibliotheek. Elke
//! bewerking is na het kiezen een vaste `Stap` op GlobalId en naam. Zo kunnen we de stappen van ons en hun ook na
//! elkaar op de basis toepassen: dat is het orakel voor wat samenvoegen moet geven.
//!
//! Standaard 128 gevallen per eigenschap; `PROPTEST_CASES=<n>` zet een ander aantal.

mod gemeen;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

use openaec_samenvoegen::{
    sla_op, verschil, voeg_samen, Antwoord, AttribuutWijziging, Basis, BasisVan, Conflict, Fout, LusOpties, Opslag,
    Uitkomst, Verschil, Weigering,
};
use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::{Config, TestCaseError};

// --- Instellingen. ---

fn config() -> Config {
    let mut c = Config::default();
    if std::env::var_os("PROPTEST_CASES").is_none() {
        c.cases = 128;
    }
    // Geen bestanden met regressies naast de tests: een gevonden geval schrijven we als gewone test uit.
    c.failure_persistence = None;
    c
}

// --- De basisbestanden. ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bron {
    /// buildingSMART, IFC4: een wand met een opening en een raam (eigenschappen, materialen, een type).
    Wand,
    /// Open Planner Studio, IFC4X3: een planning met taken en eigenschappensets.
    Planning,
    /// Het kleine gebouw uit `gemeen::gebouw`, in IFC2X3.
    Gebouw2x3,
    /// buildingSMART, IFC2X3: een gebouw met geometrie, types en hoeveelheden (92 kB).
    Architectuur2x3,
}

fn tekst_van(bron: Bron) -> &'static str {
    static TEKSTEN: OnceLock<HashMap<&'static str, String>> = OnceLock::new();
    let t = TEKSTEN.get_or_init(|| {
        HashMap::from([
            ("wand", gemeen::bestand("ifc4-wall-with-opening-and-window.ifc")),
            ("planning", gemeen::bestand("showcase-groot-terrein-onderaannemer.ifc")),
            ("gebouw", gemeen::gebouw("IFC2X3")),
            ("architectuur", gemeen::bestand("ifc2x3-building-architecture.ifc")),
        ])
    });
    match bron {
        Bron::Wand => &t["wand"],
        Bron::Planning => &t["planning"],
        Bron::Gebouw2x3 => &t["gebouw"],
        Bron::Architectuur2x3 => &t["architectuur"],
    }
}

fn bron() -> impl Strategy<Value = Bron> {
    prop_oneof![
        3 => Just(Bron::Wand),
        3 => Just(Bron::Planning),
        2 => Just(Bron::Gebouw2x3),
        1 => Just(Bron::Architectuur2x3),
    ]
}

// --- Een kleine lezer en schrijver voor STEP-tekst (los van de bibliotheek). ---

/// Splitst de DATA-sectie op `;` buiten teksten; commentaar valt weg.
fn stukken(data: &str) -> Vec<String> {
    let mut uit = Vec::new();
    let mut huidig = String::new();
    let mut in_tekst = false;
    let mut tekens = data.chars().peekable();
    while let Some(c) = tekens.next() {
        if in_tekst {
            huidig.push(c);
            if c == '\'' {
                if tekens.peek() == Some(&'\'') {
                    huidig.push(tekens.next().unwrap());
                } else {
                    in_tekst = false;
                }
            }
            continue;
        }
        match c {
            '\'' => {
                in_tekst = true;
                huidig.push(c);
            }
            '/' if tekens.peek() == Some(&'*') => {
                tekens.next();
                let mut vorige = ' ';
                for d in tekens.by_ref() {
                    if vorige == '*' && d == '/' {
                        break;
                    }
                    vorige = d;
                }
            }
            ';' => {
                let stuk = huidig.trim().to_string();
                if !stuk.is_empty() {
                    uit.push(stuk);
                }
                huidig.clear();
            }
            _ => huidig.push(c),
        }
    }
    uit
}

/// Splitst op komma's op het bovenste niveau, buiten teksten en haakjes.
fn splits(tekst: &str) -> Vec<String> {
    let mut uit = Vec::new();
    let mut huidig = String::new();
    let (mut diepte, mut in_tekst) = (0i32, false);
    for c in tekst.chars() {
        match c {
            '\'' => in_tekst = !in_tekst,
            '(' if !in_tekst => diepte += 1,
            ')' if !in_tekst => diepte -= 1,
            ',' if !in_tekst && diepte == 0 => {
                uit.push(huidig.trim().to_string());
                huidig.clear();
                continue;
            }
            _ => {}
        }
        huidig.push(c);
    }
    if !huidig.trim().is_empty() || !uit.is_empty() {
        uit.push(huidig.trim().to_string());
    }
    uit
}

/// De nummers `#n` in `tekst`, buiten teksten.
fn verwijzingen(tekst: &str) -> Vec<u64> {
    let mut uit = Vec::new();
    let mut in_tekst = false;
    let tekens: Vec<char> = tekst.chars().collect();
    let mut i = 0;
    while i < tekens.len() {
        match tekens[i] {
            '\'' => in_tekst = !in_tekst,
            '#' if !in_tekst => {
                let j = (i + 1..tekens.len()).find(|&j| !tekens[j].is_ascii_digit()).unwrap_or(tekens.len());
                if let Ok(n) = tekens[i + 1..j].iter().collect::<String>().parse() {
                    uit.push(n);
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    uit
}

/// Het type van een instantie (`IFCWALL(...)`), in hoofdletters; leeg bij een complexe instantie.
fn type_van(rest: &str) -> String {
    rest.find('(').map_or(String::new(), |h| rest[..h].trim().to_ascii_uppercase())
}

fn attributen(rest: &str) -> Vec<String> {
    match (rest.find('('), rest.rfind(')')) {
        (Some(h), Some(e)) if h > 0 && e > h => splits(&rest[h + 1..e]),
        _ => Vec::new(),
    }
}

fn lijst(attr: &str) -> Vec<String> {
    attr.strip_prefix('(').and_then(|x| x.strip_suffix(')')).map(splits).unwrap_or_default()
}

fn verwijzing(attr: &str) -> Option<u64> {
    attr.strip_prefix('#')?.parse().ok()
}

fn maak_lijst(elementen: &[String]) -> String {
    format!("({})", elementen.join(","))
}

/// Een IFC-bestand als tekst: de kop tot en met `DATA;`, de instanties op nummer, en de staart vanaf `ENDSEC;`.
#[derive(Clone, Debug)]
struct Doc {
    kop: String,
    staart: String,
    inst: BTreeMap<u64, String>,
}

impl Doc {
    fn probeer(tekst: &str) -> Result<Doc, String> {
        let begin = tekst.find("DATA;").ok_or("geen DATA;")? + "DATA;".len();
        let eind = tekst.rfind("ENDSEC;").ok_or("geen ENDSEC;")?;
        if eind < begin {
            return Err("ENDSEC; vóór DATA;".into());
        }
        let mut inst = BTreeMap::new();
        for stuk in stukken(&tekst[begin..eind]) {
            let (nummer, rest) = stuk.split_once('=').ok_or_else(|| format!("geen '=' in {stuk}"))?;
            let n: u64 = nummer
                .trim()
                .strip_prefix('#')
                .and_then(|x| x.trim().parse().ok())
                .ok_or_else(|| format!("geen nummer in {stuk}"))?;
            if inst.insert(n, rest.trim().to_string()).is_some() {
                return Err(format!("#{n} staat er twee keer"));
            }
        }
        Ok(Doc { kop: tekst[..begin].to_string(), staart: tekst[eind..].to_string(), inst })
    }

    fn lees(tekst: &str) -> Doc {
        Doc::probeer(tekst).unwrap_or_else(|e| panic!("{e}"))
    }

    fn tekst(&self) -> String {
        let mut uit = self.kop.clone();
        uit.push('\n');
        for (n, rest) in &self.inst {
            uit.push_str(&format!("#{n}={rest};\n"));
        }
        uit.push_str(&self.staart);
        uit
    }

    fn ty(&self, n: u64) -> String {
        self.inst.get(&n).map(|r| type_van(r)).unwrap_or_default()
    }

    fn a(&self, n: u64) -> Vec<String> {
        self.inst.get(&n).map(|r| attributen(r)).unwrap_or_default()
    }

    fn zet(&mut self, n: u64, a: &[String]) {
        let ty = self.ty(n);
        self.inst.insert(n, format!("{ty}({})", a.join(",")));
    }

    fn nieuw(&mut self, rest: String) -> u64 {
        let n = self.inst.keys().next_back().map_or(1, |n| n + 1);
        self.inst.insert(n, rest);
        n
    }

    /// De GlobalId van `#n`, als het een object (IfcRoot) is: het eerste attribuut is een tekst van 22 tekens uit het
    /// alfabet van een GlobalId, het tweede een IfcOwnerHistory of `$`, en er zijn minstens vier attributen. (Een
    /// naam van 22 tekens, zoals `IFCMATERIAL('composite_element_roof')`, is geen GlobalId.)
    fn gid(&self, n: u64) -> Option<String> {
        let rest = self.inst.get(&n)?;
        let ty = type_van(rest);
        if ty.is_empty() || (ty.starts_with("IFCPROPERTY") && ty.ends_with("VALUE")) {
            return None;
        }
        let a = attributen(rest);
        if a.len() < 4 || !(a[1] == "$" || verwijzing(&a[1]).is_some_and(|k| self.ty(k) == "IFCOWNERHISTORY")) {
            return None;
        }
        let binnen = a[0].strip_prefix('\'')?.strip_suffix('\'')?;
        let alfabet = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
        (binnen.len() == 22 && binnen.bytes().all(alfabet)).then(|| binnen.to_string())
    }

    fn met_gid(&self, gid: &str) -> Option<u64> {
        let t = format!("'{gid}'");
        self.inst.iter().filter(|(_, r)| r.contains(&t)).map(|(&n, _)| n).find(|&n| self.gid(n).as_deref() == Some(gid))
    }

    /// Alle objecten (met GlobalId), op GlobalId; zo hangt een keuze niet af van de nummering.
    fn objecten(&self) -> Vec<(String, u64)> {
        let mut uit: Vec<(String, u64)> = self.inst.keys().filter_map(|&n| self.gid(n).map(|g| (g, n))).collect();
        uit.sort();
        uit
    }

    fn van_type(&self, ty: &str) -> Vec<u64> {
        self.inst.keys().copied().filter(|&n| self.ty(n) == ty).collect()
    }

    fn verwijzers(&self, doel: u64) -> Vec<u64> {
        let t = format!("#{doel}");
        self.inst
            .iter()
            .filter(|(_, r)| r.contains(&t) && verwijzingen(r).contains(&doel))
            .map(|(&n, _)| n)
            .collect()
    }

    /// Hoeveel objecten deze plaatsing als `ObjectPlacement` (attribuut 5) hebben.
    fn plaatsing_eigenaars(&self, p: u64) -> usize {
        self.verwijzers(p)
            .into_iter()
            .filter(|&x| self.gid(x).is_some() && self.a(x).get(5).and_then(|a| verwijzing(a)) == Some(p))
            .count()
    }

    /// Haalt knopen (zonder GlobalId) weg waar niets meer naar verwijst, vanaf `werk`.
    fn ruim_op(&mut self, mut werk: Vec<u64>) {
        while let Some(n) = werk.pop() {
            if !self.inst.contains_key(&n) || self.gid(n).is_some() || !self.verwijzers(n).is_empty() {
                continue;
            }
            let rest = self.inst.remove(&n).unwrap();
            werk.extend(verwijzingen(&rest));
        }
    }

    /// De GlobalIds die vanaf `#n` bereikbaar zijn (via alle verwijzingen), `#n` zelf ook.
    fn bereik(&self, n: u64) -> BTreeSet<String> {
        let mut gezien = HashSet::from([n]);
        let mut werk = vec![n];
        let mut uit = BTreeSet::new();
        while let Some(k) = werk.pop() {
            if let Some(g) = self.gid(k) {
                uit.insert(g);
            }
            for d in self.inst.get(&k).map(|r| verwijzingen(r)).unwrap_or_default() {
                if gezien.insert(d) {
                    werk.push(d);
                }
            }
        }
        uit
    }
}

/// Zet de tijdstempel (het tweede veld) van FILE_NAME in de kop.
fn zet_tijdstempel_in_kop(kop: &str, waarde: &str) -> String {
    let Some(i) = kop.find("FILE_NAME") else { return kop.to_string() };
    let Some(open) = kop[i..].find('(').map(|x| i + x) else { return kop.to_string() };
    let (mut diepte, mut in_tekst, mut sluit) = (0i32, false, None);
    for (k, c) in kop[open..].char_indices() {
        match c {
            '\'' => in_tekst = !in_tekst,
            '(' if !in_tekst => diepte += 1,
            ')' if !in_tekst => {
                diepte -= 1;
                if diepte == 0 {
                    sluit = Some(open + k);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(sluit) = sluit else { return kop.to_string() };
    let mut p = splits(&kop[open + 1..sluit]);
    if p.len() < 2 {
        return kop.to_string();
    }
    p[1] = format!("'{waarde}'");
    format!("{}FILE_NAME({}){}", &kop[..i], p.join(","), &kop[sluit + 1..])
}

// --- Bewerkingen. ---

/// Een bewerking zoals de generator hem kiest: een doel is een getal; `getal % aantal` kiest uit de mogelijke doelen
/// (zo krimpt proptest naar het eerste doel).
#[derive(Debug, Clone)]
enum Bewerking {
    Naam { doel: u32, waarde: u8 },
    Beschrijving { doel: u32, waarde: u8 },
    Eigenschap { doel: u32, waarde: u8 },
    EigenschapErbij { pset: u32, naam: u8, waarde: u8 },
    NieuwePset { object: u32, naam: u8, waarde: u8 },
    Verwijder { doel: u32 },
    Verplaats { doel: u32, x: i8 },
    NieuwElement { bevat: u32, naam: u8 },
    HaalUit { doel: u32 },
    Tijdstempel { waarde: u8 },
    Hernummer { zaad: u64 },
}

fn bewerking() -> impl Strategy<Value = Bewerking> {
    prop_oneof![
        3 => (any::<u32>(), 0u8..4).prop_map(|(doel, waarde)| Bewerking::Naam { doel, waarde }),
        1 => (any::<u32>(), 0u8..4).prop_map(|(doel, waarde)| Bewerking::Beschrijving { doel, waarde }),
        3 => (any::<u32>(), 0u8..4).prop_map(|(doel, waarde)| Bewerking::Eigenschap { doel, waarde }),
        2 => (any::<u32>(), 0u8..3, 0u8..4)
            .prop_map(|(pset, naam, waarde)| Bewerking::EigenschapErbij { pset, naam, waarde }),
        2 => (any::<u32>(), 0u8..3, 0u8..4)
            .prop_map(|(object, naam, waarde)| Bewerking::NieuwePset { object, naam, waarde }),
        2 => any::<u32>().prop_map(|doel| Bewerking::Verwijder { doel }),
        2 => (any::<u32>(), -3i8..4).prop_map(|(doel, x)| Bewerking::Verplaats { doel, x }),
        2 => (any::<u32>(), 0u8..4).prop_map(|(bevat, naam)| Bewerking::NieuwElement { bevat, naam }),
        1 => any::<u32>().prop_map(|doel| Bewerking::HaalUit { doel }),
        1 => (1u8..6).prop_map(|waarde| Bewerking::Tijdstempel { waarde }),
        1 => any::<u64>().prop_map(|zaad| Bewerking::Hernummer { zaad }),
    ]
}

fn reeks() -> impl Strategy<Value = Vec<Bewerking>> {
    prop::collection::vec(bewerking(), 0..=3)
}

/// Een vaste stap, op GlobalId en naam. Bestaat het doel niet (meer), dan doet de stap niets.
#[derive(Debug, Clone, PartialEq)]
enum Stap {
    /// Attribuut `attribuut` (2 = Name, 3 = Description) van het object wordt `waarde`.
    ZetAttribuut { globalid: String, attribuut: usize, waarde: String },
    /// De eigenschap `naam` in de set krijgt NominalValue `waarde`; zonder die eigenschap komt hij erbij.
    ZetEigenschap { pset: String, naam: String, waarde: String },
    /// Een nieuwe set (met één eigenschap) en een nieuwe IfcRelDefinesByProperties voor het object.
    NieuwePset { object: String, pset: String, relatie: String, naam: String, waarde: String },
    /// De relatie weg; bij IfcRelDefinesByProperties ook de set, als niets anders hem gebruikt.
    Verwijder { relatie: String },
    /// Het object krijgt een nieuwe plek: een nieuwe IfcAxis2Placement3D met een nieuw punt.
    Verplaats { object: String, punt: [i32; 3] },
    /// Een nieuwe IfcBuildingElementProxy in deze IfcRelContainedInSpatialStructure.
    NieuwElement { relatie: String, globalid: String, naam: String },
    /// Het element uit deze IfcRelContainedInSpatialStructure (alleen als er dan nog een overblijft).
    HaalUit { relatie: String, element: String },
    /// Tijdstempels zoals OPS bij elk opslaan: FILE_NAME, IfcOwnerHistory.CreationDate en CreationDate van de
    /// planning (vluchtig).
    Tijdstempel { waarde: u8 },
    /// Alles opnieuw nummeren en de regels schudden.
    Hernummer { zaad: u64 },
}

/// Een kant (ons, hun, ...): een letter voor de nieuwe GlobalIds en een teller.
struct Kant {
    letter: char,
    teller: u32,
}

impl Kant {
    fn gid(&self, extra: u32) -> String {
        format!("0{}PBT{:0>17}", self.letter, self.teller + extra)
    }
}

/// Een typewaarde met een nieuwe waarde van hetzelfde type (bijv. `IFCLABEL('w2')`, `IFCBOOLEAN(.T.)`).
fn nieuwe_waarde(oud: &str, w: u8) -> String {
    let Some(h) = oud.find('(') else { return format!("IFCLABEL('w{w}')") };
    let ty = &oud[..h];
    let binnen = oud[h + 1..].trim_end_matches(')');
    if binnen.starts_with('\'') {
        format!("{ty}('w{w}')")
    } else if binnen.starts_with('.') {
        format!("{ty}({})", [".T.", ".F."][usize::from(w % 2)])
    } else if binnen.contains('.') || binnen.contains('E') {
        format!("{ty}({w}.5)")
    } else {
        format!("{ty}({})", u32::from(w) + 1)
    }
}

/// Een object waar een eigenschappenset aan kan (IfcObject): geen relatie, set, type of stijl.
fn objectachtig(ty: &str) -> bool {
    !ty.starts_with("IFCREL")
        && !ty.ends_with("TYPE")
        && !ty.ends_with("STYLE")
        && !matches!(ty, "IFCPROPERTYSET" | "IFCELEMENTQUANTITY" | "IFCPROJECTLIBRARY")
}

/// Alle stappen die bewerking `b` op `doc` kan zijn.
fn kandidaten(doc: &Doc, b: &Bewerking, kant: &Kant) -> Vec<Stap> {
    let objecten = doc.objecten();
    let zet = |j: usize, waarde: String| {
        objecten
            .iter()
            .filter(|(_, n)| doc.a(*n).len() > j)
            .map(|(g, _)| Stap::ZetAttribuut { globalid: g.clone(), attribuut: j, waarde: waarde.clone() })
            .collect::<Vec<_>>()
    };
    let psets = || objecten.iter().filter(|(_, n)| doc.ty(*n) == "IFCPROPERTYSET");
    let bevat = || objecten.iter().filter(|(_, n)| doc.ty(*n) == "IFCRELCONTAINEDINSPATIALSTRUCTURE");
    match b {
        Bewerking::Naam { waarde, .. } => zet(2, format!("'w{waarde}'")),
        Bewerking::Beschrijving { waarde, .. } => zet(3, format!("'d{waarde}'")),
        Bewerking::Eigenschap { waarde, .. } => {
            let mut uit = Vec::new();
            for (g, n) in psets() {
                let props: Vec<u64> =
                    doc.a(*n).get(4).map(|a| lijst(a).iter().filter_map(|e| verwijzing(e)).collect()).unwrap_or_default();
                let namen: Vec<String> = props.iter().map(|&p| doc.a(p).first().cloned().unwrap_or_default()).collect();
                if namen.iter().collect::<HashSet<_>>().len() != namen.len() {
                    continue;
                }
                for &p in &props {
                    let pa = doc.a(p);
                    if doc.ty(p) != "IFCPROPERTYSINGLEVALUE" || pa.len() < 3 || !pa[2].contains('(') {
                        continue;
                    }
                    let Some(naam) = pa[0].strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) else { continue };
                    uit.push(Stap::ZetEigenschap {
                        pset: g.clone(),
                        naam: naam.to_string(),
                        waarde: nieuwe_waarde(&pa[2], *waarde),
                    });
                }
            }
            uit
        }
        Bewerking::EigenschapErbij { naam, waarde, .. } => psets()
            .map(|(g, _)| Stap::ZetEigenschap {
                pset: g.clone(),
                naam: format!("PBT{naam}"),
                waarde: format!("IFCLABEL('w{waarde}')"),
            })
            .collect(),
        Bewerking::NieuwePset { naam, waarde, .. } => objecten
            .iter()
            .filter(|(_, n)| objectachtig(&doc.ty(*n)))
            .map(|(g, _)| Stap::NieuwePset {
                object: g.clone(),
                pset: kant.gid(1),
                relatie: kant.gid(2),
                naam: format!("Pset_PBT{naam}"),
                waarde: format!("IFCLABEL('w{waarde}')"),
            })
            .collect(),
        Bewerking::Verwijder { .. } => objecten
            .iter()
            .filter(|(_, n)| doc.ty(*n).starts_with("IFCREL") && doc.verwijzers(*n).is_empty())
            .map(|(g, _)| Stap::Verwijder { relatie: g.clone() })
            .collect(),
        Bewerking::Verplaats { x, .. } => objecten
            .iter()
            .filter(|(_, n)| {
                let Some(p) = doc.a(*n).get(5).and_then(|a| verwijzing(a)) else { return false };
                doc.ty(p) == "IFCLOCALPLACEMENT"
                    && doc.plaatsing_eigenaars(p) == 1
                    && doc.a(p).get(1).and_then(|a| verwijzing(a)).is_some_and(|ax| doc.ty(ax) == "IFCAXIS2PLACEMENT3D")
            })
            .map(|(g, _)| Stap::Verplaats { object: g.clone(), punt: [100 * i32::from(*x), 50, 0] })
            .collect(),
        Bewerking::NieuwElement { naam, .. } => bevat()
            .filter(|(_, n)| doc.a(*n).len() > 5)
            .map(|(g, _)| Stap::NieuwElement { relatie: g.clone(), globalid: kant.gid(1), naam: format!("E{naam}") })
            .collect(),
        Bewerking::HaalUit { .. } => {
            let mut uit = Vec::new();
            for (g, n) in bevat() {
                let elementen = doc.a(*n).get(4).map(|a| lijst(a)).unwrap_or_default();
                if elementen.len() < 2 {
                    continue;
                }
                let mut gids: Vec<String> =
                    elementen.iter().filter_map(|e| verwijzing(e)).filter_map(|e| doc.gid(e)).collect();
                gids.sort();
                uit.extend(gids.into_iter().map(|e| Stap::HaalUit { relatie: g.clone(), element: e }));
            }
            uit
        }
        Bewerking::Tijdstempel { waarde } => vec![Stap::Tijdstempel { waarde: *waarde }],
        Bewerking::Hernummer { zaad } => vec![Stap::Hernummer { zaad: *zaad }],
    }
}

fn index_van(b: &Bewerking) -> Option<u32> {
    match b {
        Bewerking::Naam { doel, .. }
        | Bewerking::Beschrijving { doel, .. }
        | Bewerking::Eigenschap { doel, .. }
        | Bewerking::Verwijder { doel }
        | Bewerking::Verplaats { doel, .. }
        | Bewerking::HaalUit { doel } => Some(*doel),
        Bewerking::EigenschapErbij { pset, .. } => Some(*pset),
        Bewerking::NieuwePset { object, .. } => Some(*object),
        Bewerking::NieuwElement { bevat, .. } => Some(*bevat),
        Bewerking::Tijdstempel { .. } | Bewerking::Hernummer { .. } => None,
    }
}

/// Welke objecten (GlobalIds) een stap raakt. Voor het weghalen van een relatie: alles wat de relatie (ook via
/// andere objecten, bijv. een type en zijn sets) bereikt; zo valt ook de koppelregel eronder.
fn raakt(doc: &Doc, s: &Stap) -> BTreeSet<String> {
    let set = |x: &[&String]| x.iter().map(|g| g.to_string()).collect();
    match s {
        Stap::ZetAttribuut { globalid, .. } => set(&[globalid]),
        Stap::ZetEigenschap { pset, .. } => set(&[pset]),
        Stap::NieuwePset { object, pset, relatie, .. } => set(&[object, pset, relatie]),
        Stap::Verwijder { relatie } => doc.met_gid(relatie).map(|n| doc.bereik(n)).unwrap_or_default(),
        Stap::Verplaats { object, .. } => set(&[object]),
        Stap::NieuwElement { relatie, globalid, .. } => set(&[relatie, globalid]),
        Stap::HaalUit { relatie, element } => set(&[relatie, element]),
        Stap::Tijdstempel { .. } | Stap::Hernummer { .. } => BTreeSet::new(),
    }
}

fn pas_toe(doc: &mut Doc, s: &Stap) {
    match s {
        Stap::ZetAttribuut { globalid, attribuut, waarde } => {
            let Some(n) = doc.met_gid(globalid) else { return };
            let mut a = doc.a(n);
            if *attribuut < a.len() && a[*attribuut] != *waarde {
                a[*attribuut] = waarde.clone();
                doc.zet(n, &a);
            }
        }
        Stap::ZetEigenschap { pset, naam, waarde } => {
            let Some(n) = doc.met_gid(pset) else { return };
            let mut a = doc.a(n);
            if a.len() < 5 {
                return;
            }
            let mut el = lijst(&a[4]);
            let gezocht = format!("'{naam}'");
            let gevonden = el
                .iter()
                .filter_map(|e| verwijzing(e))
                .find(|&p| doc.ty(p) == "IFCPROPERTYSINGLEVALUE" && doc.a(p).first() == Some(&gezocht));
            match gevonden {
                Some(p) => {
                    let mut pa = doc.a(p);
                    if pa.len() < 3 || pa[2] == *waarde {
                        return;
                    }
                    pa[2] = waarde.clone();
                    if doc.verwijzers(p) == vec![n] {
                        doc.zet(p, &pa);
                    } else {
                        let q = doc.nieuw(format!("IFCPROPERTYSINGLEVALUE({})", pa.join(",")));
                        for e in el.iter_mut() {
                            if verwijzing(e) == Some(p) {
                                *e = format!("#{q}");
                            }
                        }
                        a[4] = maak_lijst(&el);
                        doc.zet(n, &a);
                    }
                }
                None => {
                    let q = doc.nieuw(format!("IFCPROPERTYSINGLEVALUE({gezocht},$,{waarde},$)"));
                    el.push(format!("#{q}"));
                    a[4] = maak_lijst(&el);
                    doc.zet(n, &a);
                }
            }
        }
        Stap::NieuwePset { object, pset, relatie, naam, waarde } => {
            let Some(x) = doc.met_gid(object) else { return };
            if doc.met_gid(pset).is_some() || doc.met_gid(relatie).is_some() {
                return;
            }
            let oh = doc.a(x).get(1).cloned().unwrap_or_else(|| "$".into());
            let v = doc.nieuw(format!("IFCPROPERTYSINGLEVALUE('PBT',$,{waarde},$)"));
            let s = doc.nieuw(format!("IFCPROPERTYSET('{pset}',{oh},'{naam}',$,(#{v}))"));
            doc.nieuw(format!("IFCRELDEFINESBYPROPERTIES('{relatie}',{oh},$,$,(#{x}),#{s})"));
        }
        Stap::Verwijder { relatie } => {
            let Some(r) = doc.met_gid(relatie) else { return };
            if !doc.verwijzers(r).is_empty() {
                return;
            }
            let rest = doc.inst.remove(&r).unwrap();
            let mut werk = verwijzingen(&rest);
            if type_van(&rest) == "IFCRELDEFINESBYPROPERTIES" {
                if let Some(s) = attributen(&rest).get(5).and_then(|a| verwijzing(a)) {
                    if doc.ty(s) == "IFCPROPERTYSET" && doc.verwijzers(s).is_empty() {
                        let srest = doc.inst.remove(&s).unwrap();
                        werk.extend(verwijzingen(&srest));
                    }
                }
            }
            doc.ruim_op(werk);
        }
        Stap::Verplaats { object, punt } => {
            let Some(x) = doc.met_gid(object) else { return };
            let Some(p) = doc.a(x).get(5).and_then(|a| verwijzing(a)) else { return };
            if doc.ty(p) != "IFCLOCALPLACEMENT" || doc.plaatsing_eigenaars(p) != 1 {
                return;
            }
            let mut pa = doc.a(p);
            let Some(ax) = pa.get(1).and_then(|a| verwijzing(a)) else { return };
            if doc.ty(ax) != "IFCAXIS2PLACEMENT3D" {
                return;
            }
            let axa = doc.a(ax);
            let np = doc.nieuw(format!("IFCCARTESIANPOINT(({}.,{}.,{}.))", punt[0], punt[1], punt[2]));
            let na = doc.nieuw(format!("IFCAXIS2PLACEMENT3D(#{np},{},{})", axa[1], axa[2]));
            pa[1] = format!("#{na}");
            doc.zet(p, &pa);
            doc.ruim_op(vec![ax]);
        }
        Stap::NieuwElement { relatie, globalid, naam } => {
            let Some(r) = doc.met_gid(relatie) else { return };
            if doc.met_gid(globalid).is_some() {
                return;
            }
            let mut ra = doc.a(r);
            if ra.len() < 6 {
                return;
            }
            let oh = ra[1].clone();
            let sp = verwijzing(&ra[5])
                .and_then(|s| doc.a(s).get(5).cloned())
                .filter(|x| x.starts_with('#'))
                .unwrap_or_else(|| "$".into());
            let pt = doc.nieuw("IFCCARTESIANPOINT((0.,0.,0.))".into());
            let ax = doc.nieuw(format!("IFCAXIS2PLACEMENT3D(#{pt},$,$)"));
            let lp = doc.nieuw(format!("IFCLOCALPLACEMENT({sp},#{ax})"));
            let el = doc.nieuw(format!("IFCBUILDINGELEMENTPROXY('{globalid}',{oh},'{naam}',$,$,#{lp},$,$,$)"));
            let mut l = lijst(&ra[4]);
            l.push(format!("#{el}"));
            ra[4] = maak_lijst(&l);
            doc.zet(r, &ra);
        }
        Stap::HaalUit { relatie, element } => {
            let (Some(r), Some(e)) = (doc.met_gid(relatie), doc.met_gid(element)) else { return };
            let mut ra = doc.a(r);
            let mut l = lijst(&ra[4]);
            let voor = l.len();
            l.retain(|x| verwijzing(x) != Some(e));
            if voor < 2 || l.len() == voor {
                return;
            }
            ra[4] = maak_lijst(&l);
            doc.zet(r, &ra);
        }
        Stap::Tijdstempel { waarde } => {
            doc.kop = zet_tijdstempel_in_kop(&doc.kop, &format!("2030-01-0{waarde}T00:00:00"));
            for n in doc.van_type("IFCOWNERHISTORY") {
                let mut a = doc.a(n);
                if a.len() > 7 {
                    a[7] = format!("{}", 1_900_000_000 + u64::from(*waarde));
                    doc.zet(n, &a);
                }
            }
            for n in doc.van_type("IFCWORKPLAN").into_iter().chain(doc.van_type("IFCWORKSCHEDULE")) {
                let mut a = doc.a(n);
                if a.len() > 6 && a[6].starts_with('\'') {
                    a[6] = format!("'2030-01-0{waarde}T00:00:00'");
                    doc.zet(n, &a);
                }
            }
        }
        Stap::Hernummer { zaad } => *doc = Doc::lees(&gemeen::hernummer(&doc.tekst(), *zaad)),
    }
}

/// Een versie van één kant: de bewerkingen na elkaar op de basis. Stappen die iets uit `verboden` raken, kiezen we
/// niet. Geeft de tekst, de gekozen stappen en wat ze raken.
struct Versie {
    tekst: String,
    stappen: Vec<Stap>,
    raakt: BTreeSet<String>,
}

fn maak_versie(basis: &str, bewerkingen: &[Bewerking], letter: char, verboden: &BTreeSet<String>) -> Versie {
    let mut doc = Doc::lees(basis);
    let mut kant = Kant { letter, teller: 0 };
    let mut stappen = Vec::new();
    let mut alles = BTreeSet::new();
    for b in bewerkingen {
        let mogelijk: Vec<(Stap, BTreeSet<String>)> = kandidaten(&doc, b, &kant)
            .into_iter()
            .map(|s| {
                let r = raakt(&doc, &s);
                (s, r)
            })
            .filter(|(_, r)| r.is_disjoint(verboden))
            .collect();
        kant.teller += 2;
        if mogelijk.is_empty() {
            continue;
        }
        let k = index_van(b).map_or(0, |i| i as usize % mogelijk.len());
        let (stap, r) = mogelijk[k].clone();
        pas_toe(&mut doc, &stap);
        alles.extend(r);
        stappen.push(stap);
    }
    Versie { tekst: doc.tekst(), stappen, raakt: alles }
}

fn versie(basis: &str, bewerkingen: &[Bewerking], letter: char) -> Versie {
    maak_versie(basis, bewerkingen, letter, &BTreeSet::new())
}

/// De stappen van de reeksen na elkaar op `basis`.
fn na_elkaar(basis: &str, reeksen: &[&[Stap]]) -> String {
    let mut doc = Doc::lees(basis);
    for reeks in reeksen {
        for s in *reeks {
            pas_toe(&mut doc, s);
        }
    }
    doc.tekst()
}

// --- Hulpmiddelen voor de controles. ---

type Uit = Result<(), TestCaseError>;

fn eis(ok: bool, melding: impl FnOnce() -> String) -> Uit {
    if ok {
        Ok(())
    } else {
        Err(TestCaseError::fail(melding()))
    }
}

/// De uitkomst van één samenvoeging: het resultaat (None bij een conflict) en de conflicten.
struct Uitslag {
    r: Option<String>,
    conflicten: Vec<Conflict>,
}

impl Uitslag {
    fn rapport(&self) -> String {
        self.conflicten.iter().map(|c| format!("{:?}: {}", c.soort, c.melding)).collect::<Vec<_>>().join("; ")
    }
}

/// Voegt samen. Een `Fout` is hier altijd fout: de invoer is geldig.
fn samen(b: &str, o: &str, h: &str) -> Result<Uitslag, TestCaseError> {
    match voeg_samen(b.as_bytes(), o.as_bytes(), h.as_bytes()) {
        Ok(s) => Ok(Uitslag {
            r: s.inhoud.map(|x| String::from_utf8(x).expect("het resultaat is utf-8")),
            conflicten: s.conflicten,
        }),
        Err(f) => Err(TestCaseError::fail(format!("voeg_samen gaf een fout: {f}"))),
    }
}

/// Samenvoegen moet lukken; geeft het resultaat.
fn gelukt(b: &str, o: &str, h: &str, wat: &str) -> Result<String, TestCaseError> {
    let u = samen(b, o, h)?;
    match u.r {
        Some(r) => Ok(r),
        None => Err(TestCaseError::fail(format!("{wat}: onverwacht conflict: {}", u.rapport()))),
    }
}

fn verschil_van(a: &str, b: &str) -> Result<Verschil, TestCaseError> {
    verschil(a.as_bytes(), b.as_bytes()).map_err(|f| TestCaseError::fail(format!("verschil gaf een fout: {f}")))
}

fn kort_verschil(v: &Verschil) -> String {
    let mut delen = Vec::new();
    for x in &v.toegevoegd {
        delen.push(format!("+{} {}", x.type_naam, x.globalid));
    }
    for x in &v.verwijderd {
        delen.push(format!("-{} {}", x.type_naam, x.globalid));
    }
    for w in &v.gewijzigd {
        for a in &w.attributen {
            delen.push(format!("~{} {}.{}: {} -> {}", w.type_naam, w.globalid, a.attribuut, a.oud, a.nieuw));
        }
    }
    for k in &v.losse_knopen.toegevoegd {
        delen.push(format!("+los {}", k.tekst));
    }
    for k in &v.losse_knopen.verwijderd {
        delen.push(format!("-los {}", k.tekst));
    }
    for a in &v.header {
        delen.push(format!("~HEADER {}: {} -> {}", a.attribuut, a.oud, a.nieuw));
    }
    delen.join("\n  ")
}

/// a ≡ b: het verschil is leeg.
fn gelijk(a: &str, b: &str, wat: &str) -> Uit {
    let v = verschil_van(a, b)?;
    eis(v.is_leeg(), || format!("{wat}: niet gelijk:\n  {}", kort_verschil(&v)))
}

fn verhaal(bron: Bron, kanten: &[(&str, &[Stap])]) -> String {
    let mut uit = format!("basis {bron:?}");
    for (naam, stappen) in kanten {
        uit.push_str(&format!("\n{naam}: {stappen:?}"));
    }
    uit
}

/// De stappen van ons en hun na elkaar, als de volgorde niet uitmaakt (o;h ≡ h;o). Anders None.
fn orakel(b: &str, o: &[Stap], h: &[Stap]) -> Result<Option<String>, TestCaseError> {
    let oh = na_elkaar(b, &[o, h]);
    let ho = na_elkaar(b, &[h, o]);
    Ok(verschil_van(&oh, &ho)?.is_leeg().then_some(oh))
}

/// De naam van het attribuut op het bovenste niveau: `ObjectPlacement.RelativePlacement` en
/// `ObjectPlacement (gedeeld)` worden `ObjectPlacement`.
fn hoofdattribuut(a: &AttribuutWijziging) -> String {
    let naam = a.attribuut.trim_end_matches(" (gedeeld)");
    naam.split('.').next().unwrap_or(naam).to_string()
}

/// Per object de gewijzigde hoofdattributen, met de wijzigingen.
fn per_object(v: &Verschil) -> BTreeMap<String, BTreeMap<String, Vec<AttribuutWijziging>>> {
    let mut uit: BTreeMap<String, BTreeMap<String, Vec<AttribuutWijziging>>> = BTreeMap::new();
    for w in &v.gewijzigd {
        let object = uit.entry(w.globalid.clone()).or_default();
        for a in &w.attributen {
            object.entry(hoofdattribuut(a)).or_default().push(a.clone());
        }
    }
    uit
}

/// Behoud: elke wijziging van ons en van hun zit in r, en r bevat niets wat in geen van beide zat. Vluchtige velden
/// tellen niet (die volgen de regel uit §5.6, zie `vluchtig_volgens_de_regel`).
fn behoud(b: &str, o: &str, h: &str, r: &str) -> Uit {
    let (vo, vh, vr) = (verschil_van(b, o)?, verschil_van(b, h)?, verschil_van(b, r)?);
    let gids = |l: &[openaec_samenvoegen::ObjectInfo]| l.iter().map(|x| x.globalid.clone()).collect::<BTreeSet<_>>();
    let samen = |a: BTreeSet<String>, b: BTreeSet<String>| a.union(&b).cloned().collect::<BTreeSet<_>>();
    let erbij = samen(gids(&vo.toegevoegd), gids(&vh.toegevoegd));
    let eraf = samen(gids(&vo.verwijderd), gids(&vh.verwijderd));
    eis(gids(&vr.toegevoegd) == erbij, || {
        format!("toegevoegd: ons+hun {erbij:?}, resultaat {:?}", gids(&vr.toegevoegd))
    })?;
    eis(gids(&vr.verwijderd) == eraf, || format!("verwijderd: ons+hun {eraf:?}, resultaat {:?}", gids(&vr.verwijderd)))?;

    let (po, ph, pr) = (per_object(&vo), per_object(&vh), per_object(&vr));
    for (kant, eigen, ander, naar_r) in [("ons", &po, &ph, verschil_van(o, r)?), ("hun", &ph, &po, verschil_van(h, r)?)] {
        let na = per_object(&naar_r);
        for (gid, attrs) in eigen {
            for (attr, wijzigingen) in attrs {
                let ook_ander = ander.get(gid).is_some_and(|x| x.contains_key(attr));
                if !ook_ander {
                    // Alleen deze kant: in r dezelfde waarde als bij deze kant.
                    eis(!na.get(gid).is_some_and(|x| x.contains_key(attr)), || {
                        format!(
                            "{kant} wijzigde {gid}.{attr}, maar r heeft een andere waarde: {:?}",
                            na[gid][attr].iter().map(|a| format!("{} -> {}", a.oud, a.nieuw)).collect::<Vec<_>>()
                        )
                    })?;
                    continue;
                }
                // Beide kanten wijzigden dit attribuut: bij een verzameling moet wat erbij kwam en wat eraf ging
                // in r zitten; anders dezelfde nieuwe waarde.
                let Some(in_r) = pr.get(gid).and_then(|x| x.get(attr)) else {
                    // r gelijk aan de basis voor dit attribuut, terwijl beide het wijzigden.
                    return Err(TestCaseError::fail(format!(
                        "ons en hun wijzigden {gid}.{attr}, maar r heeft de waarde van de basis"
                    )));
                };
                for w in wijzigingen {
                    if w.erbij.is_empty() && w.eraf.is_empty() {
                        continue;
                    }
                    let r_erbij: BTreeSet<&String> = in_r.iter().flat_map(|x| &x.erbij).collect();
                    let r_eraf: BTreeSet<&String> = in_r.iter().flat_map(|x| &x.eraf).collect();
                    for e in &w.erbij {
                        eis(r_erbij.contains(e), || format!("{kant} voegde {e} toe aan {gid}.{attr}; r niet"))?;
                    }
                    for e in &w.eraf {
                        eis(r_eraf.contains(e), || format!("{kant} haalde {e} uit {gid}.{attr}; r niet"))?;
                    }
                }
            }
        }
    }
    // Niets extra: elk gewijzigd attribuut in r wijzigde ons of hun.
    for (gid, attrs) in &pr {
        for attr in attrs.keys() {
            let bron = po.get(gid).is_some_and(|x| x.contains_key(attr)) || ph.get(gid).is_some_and(|x| x.contains_key(attr));
            eis(bron, || format!("r wijzigt {gid}.{attr}, maar ons en hun deden dat niet: {:?}", attrs[attr]))?;
        }
    }
    let losse = |v: &Verschil| {
        (
            v.losse_knopen.toegevoegd.iter().cloned().collect::<BTreeSet<_>>(),
            v.losse_knopen.verwijderd.iter().cloned().collect::<BTreeSet<_>>(),
        )
    };
    let (lo, lh, lr) = (losse(&vo), losse(&vh), losse(&vr));
    for k in &lr.0 {
        eis(lo.0.contains(k) || lh.0.contains(k), || format!("r heeft een losse knoop erbij die nergens vandaan komt: {k:?}"))?;
    }
    for k in &lr.1 {
        eis(lo.1.contains(k) || lh.1.contains(k), || format!("r haalde een losse knoop weg die niemand weghaalde: {k:?}"))?;
    }
    for k in lo.0.iter().chain(&lh.0) {
        eis(lr.0.contains(k), || format!("een losse knoop van ons of hun ontbreekt in r: {k:?}"))?;
    }
    for k in lo.1.iter().chain(&lh.1) {
        eis(lr.1.contains(k), || format!("een losse knoop die ons of hun weghaalde, staat nog in r: {k:?}"))?;
    }
    let kop = |v: &Verschil| v.header.iter().map(|a| a.attribuut.clone()).collect::<BTreeSet<_>>();
    let (ko, kh, kr) = (kop(&vo), kop(&vh), kop(&vr));
    eis(kr == ko.union(&kh).cloned().collect(), || format!("HEADER: ons {ko:?}, hun {kh:?}, r {kr:?}"))?;
    Ok(())
}

/// Vluchtige velden: wijzigde alleen hun ze, dan die van hun; anders die van ons (§5.6). Onze tijdstempel raakt alle
/// IfcOwnerHistory's en de planning tegelijk; het orakel is dan "hun en daarna ons", anders "ons en daarna hun".
fn vluchtig_volgens_de_regel(b: &str, o: &[Stap], h: &[Stap], r: &str) -> Uit {
    let ons_tijd = o.iter().any(|s| matches!(s, Stap::Tijdstempel { .. }));
    let verwacht = if ons_tijd { na_elkaar(b, &[h, o]) } else { na_elkaar(b, &[o, h]) };
    let v = verschil_van(&verwacht, r)?;
    eis(v.vluchtig.is_empty(), || {
        format!(
            "vluchtige velden kloppen niet (verwacht -> r): {:?}",
            v.vluchtig.iter().map(|x| format!("{} {}: {} -> {}", x.waar, x.attribuut, x.oud, x.nieuw)).collect::<Vec<_>>()
        )
    })
}

// --- 1. Identiteit. ---

proptest! {
    #![proptest_config(config())]

    /// voeg_samen(b, b, h) ≡ h en voeg_samen(b, o, b) ≡ o, zonder conflict. Ook met een hernummerde basis aan de
    /// kant die niets deed (dan is de tekst anders, maar de inhoud gelijk).
    #[test]
    fn identiteit(bron in bron(), stappen in reeks(), zaad in any::<u64>()) {
        let b = tekst_van(bron);
        let x = versie(b, &stappen, 'O');
        let wat = verhaal(bron, &[("x", &x.stappen)]);
        let b2 = gemeen::hernummer(b, zaad);
        gelijk(&gelukt(b, b, &x.tekst, &wat)?, &x.tekst, &format!("(b, b, x)\n{wat}"))?;
        gelijk(&gelukt(b, &x.tekst, b, &wat)?, &x.tekst, &format!("(b, x, b)\n{wat}"))?;
        gelijk(&gelukt(b, &b2, &x.tekst, &wat)?, &x.tekst, &format!("(b, hernummerd b, x)\n{wat}"))?;
        gelijk(&gelukt(b, &x.tekst, &b2, &wat)?, &x.tekst, &format!("(b, x, hernummerd b)\n{wat}"))?;
    }

    // --- 2. Idempotent. ---

    /// voeg_samen(b, x, x) ≡ x zonder conflict; ook als de ene x hernummerd is.
    #[test]
    fn idempotent(bron in bron(), stappen in reeks(), zaad in any::<u64>()) {
        let b = tekst_van(bron);
        let x = versie(b, &stappen, 'O');
        let wat = verhaal(bron, &[("x", &x.stappen)]);
        gelijk(&gelukt(b, &x.tekst, &x.tekst, &wat)?, &x.tekst, &format!("(b, x, x)\n{wat}"))?;
        let x2 = gemeen::hernummer(&x.tekst, zaad);
        gelijk(&gelukt(b, &x.tekst, &x2, &wat)?, &x.tekst, &format!("(b, x, hernummerd x)\n{wat}"))?;
    }

    // --- 3. Symmetrie. ---

    /// Conflict bij (b, o, h) precies als bij (b, h, o); zonder conflict zijn de twee resultaten gelijk.
    #[test]
    fn symmetrie(bron in bron(), so in reeks(), sh in reeks()) {
        let b = tekst_van(bron);
        let (o, h) = (versie(b, &so, 'O'), versie(b, &sh, 'H'));
        let wat = verhaal(bron, &[("ons", &o.stappen), ("hun", &h.stappen)]);
        let oh = samen(b, &o.tekst, &h.tekst)?;
        let ho = samen(b, &h.tekst, &o.tekst)?;
        eis(oh.r.is_some() == ho.r.is_some(), || format!(
            "conflict (b,o,h): {} [{}]; conflict (b,h,o): {} [{}]\n{wat}",
            oh.r.is_none(), oh.rapport(), ho.r.is_none(), ho.rapport()
        ))?;
        if let (Some(r1), Some(r2)) = (&oh.r, &ho.r) {
            gelijk(r1, r2, &format!("(b,o,h) tegen (b,h,o)\n{wat}"))?;
        }
    }

    // --- 4. Behoud. ---

    /// Zonder conflict: elke wijziging van ons en van hun zit in het resultaat, en er komt niets bij. Is de volgorde
    /// van de stappen om het even (o;h ≡ h;o), dan is het resultaat dat ook. Is de volgorde niet om het even, dan
    /// moet er een conflict zijn: samenvoegen mag niet stil een kant kiezen. Vluchtige velden volgen de regel.
    #[test]
    fn behoud_zonder_stil_verlies(bron in bron(), so in reeks(), sh in reeks()) {
        let b = tekst_van(bron);
        let (o, h) = (versie(b, &so, 'O'), versie(b, &sh, 'H'));
        let wat = verhaal(bron, &[("ons", &o.stappen), ("hun", &h.stappen)]);
        let u = samen(b, &o.tekst, &h.tekst)?;
        let verwacht = orakel(b, &o.stappen, &h.stappen)?;
        match (&u.r, &verwacht) {
            (Some(r), _) => {
                behoud(b, &o.tekst, &h.tekst, r).map_err(|e| TestCaseError::fail(format!("{e}\n{wat}")))?;
                let v = verwacht.as_ref().ok_or_else(|| TestCaseError::fail(format!(
                    "o;h en h;o verschillen, maar er is geen conflict\n{wat}"
                )))?;
                gelijk(r, v, &format!("resultaat tegen o;h\n{wat}"))?;
                vluchtig_volgens_de_regel(b, &o.stappen, &h.stappen, r)
                    .map_err(|e| TestCaseError::fail(format!("{e}\n{wat}")))?;
            }
            (None, _) => {}
        }
    }

    // --- 5. Stabiel in de opslaan-lus. ---

    /// Opnieuw samenvoegen verandert niets: met r = (b, o, h) geven (h, r, h), (b, r, h) en (b, r, o) weer r.
    #[test]
    fn stabiel_in_de_opslaan_lus(bron in bron(), so in reeks(), sh in reeks()) {
        let b = tekst_van(bron);
        let (o, h) = (versie(b, &so, 'O'), versie(b, &sh, 'H'));
        let wat = verhaal(bron, &[("ons", &o.stappen), ("hun", &h.stappen)]);
        let Some(r) = samen(b, &o.tekst, &h.tekst)?.r else { return Ok(()) };
        for (naam, (x, y, z)) in [
            ("(h, r, h)", (h.tekst.as_str(), r.as_str(), h.tekst.as_str())),
            ("(b, r, h)", (b, r.as_str(), h.tekst.as_str())),
            ("(b, r, o)", (b, r.as_str(), o.tekst.as_str())),
        ] {
            let r2 = gelukt(x, y, z, &format!("{naam}\n{wat}"))?;
            gelijk(&r2, &r, &format!("{naam} tegen r\n{wat}"))?;
        }
    }

    /// Een klant slaat opnieuw op terwijl zijn werk al op de server staat (bijv. het antwoord op de eerste upload
    /// ging verloren): met r = (b, o, h) geven (b, o, r) en (b, h, r) weer r.
    #[test]
    fn opnieuw_opslaan_na_gelukt(bron in bron(), so in reeks(), sh in reeks()) {
        let b = tekst_van(bron);
        let (o, h) = (versie(b, &so, 'O'), versie(b, &sh, 'H'));
        let wat = verhaal(bron, &[("ons", &o.stappen), ("hun", &h.stappen)]);
        let Some(r) = samen(b, &o.tekst, &h.tekst)?.r else { return Ok(()) };
        for (naam, (x, y, z)) in [
            ("(b, o, r)", (b, o.tekst.as_str(), r.as_str())),
            ("(b, h, r)", (b, h.tekst.as_str(), r.as_str())),
        ] {
            let r2 = gelukt(x, y, z, &format!("{naam}\n{wat}"))?;
            gelijk(&r2, &r, &format!("{naam} tegen r\n{wat}"))?;
        }
    }
}

// --- 6. Volgorde. ---

/// Een opslag in het geheugen met versies van één bestand.
struct Server {
    versies: Vec<Vec<u8>>,
}

impl Opslag for Server {
    type Bestand = u8;
    type Versie = usize;

    fn huidige(&mut self) -> Antwoord<Self, BasisVan<Self>> {
        Ok(Basis { bestand: 1, versie: self.versies.len() - 1 })
    }

    fn lees(&mut self, versie: &BasisVan<Self>) -> Antwoord<Self, Vec<u8>> {
        self.versies.get(versie.versie).cloned().ok_or_else(|| Weigering::Fout("geen versie".into()))
    }

    fn schrijf_als(&mut self, basis: &BasisVan<Self>, inhoud: &[u8]) -> Antwoord<Self, BasisVan<Self>> {
        let laatste = self.versies.len() - 1;
        if basis.versie != laatste {
            return Err(Weigering::Verouderd { huidige: Basis { bestand: 1, versie: laatste } });
        }
        self.versies.push(inhoud.to_vec());
        Ok(Basis { bestand: 1, versie: laatste + 1 })
    }
}

/// Drie klanten slaan na elkaar op, allemaal met basis b, via `sla_op`. Geeft de laatste versie op de server.
fn drie_klanten(b: &str, klanten: [&str; 3]) -> Result<String, String> {
    let mut server = Server { versies: vec![b.as_bytes().to_vec()] };
    for (i, x) in klanten.iter().enumerate() {
        match sla_op(&mut server, Basis { bestand: 1, versie: 0 }, x.as_bytes(), &LusOpties::default()) {
            Ok(Uitkomst::Opgeslagen { .. }) => {}
            Ok(Uitkomst::Conflict { reden, .. }) => return Err(format!("klant {} kreeg een conflict: {reden:?}", i + 1)),
            Err(e) => return Err(format!("klant {} kreeg een fout: {e}", i + 1)),
        }
    }
    Ok(String::from_utf8(server.versies.last().unwrap().clone()).unwrap())
}

proptest! {
    #![proptest_config(config())]

    /// Drie losse bewerkingen (o, h, z) op verschillende objecten, allemaal vanaf basis b, via de opslaan-lus in elke
    /// volgorde: geen conflict, en alle uitkomsten zijn gelijk aan o;h;z.
    #[test]
    fn volgorde_maakt_niet_uit(bron in bron(), so in reeks(), sh in reeks(), sz in reeks()) {
        let b = tekst_van(bron);
        let o = maak_versie(b, &so, 'O', &BTreeSet::new());
        let h = maak_versie(b, &sh, 'H', &o.raakt);
        let oh: BTreeSet<String> = o.raakt.union(&h.raakt).cloned().collect();
        let z = maak_versie(b, &sz, 'Z', &oh);
        let wat = verhaal(bron, &[("o", &o.stappen), ("h", &h.stappen), ("z", &z.stappen)]);
        let verwacht = na_elkaar(b, &[&o.stappen, &h.stappen, &z.stappen]);
        let x = [&o.tekst, &h.tekst, &z.tekst];
        for volgorde in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            let r = drie_klanten(b, volgorde.map(|i| x[i].as_str()))
                .map_err(|e| TestCaseError::fail(format!("volgorde {volgorde:?}: {e}\n{wat}")))?;
            gelijk(&r, &verwacht, &format!("volgorde {volgorde:?} tegen o;h;z\n{wat}"))?;
        }
    }

    // --- 7. Nummering. ---

    /// De nummers doen niet mee: verschil(x, hernummerd x) is leeg, en een hernummerde basis, ons of hun geeft
    /// hetzelfde (conflict of resultaat).
    #[test]
    fn nummering_doet_niet_mee(bron in bron(), so in reeks(), sh in reeks(), zaad in any::<u64>()) {
        let b = tekst_van(bron);
        let (o, h) = (versie(b, &so, 'O'), versie(b, &sh, 'H'));
        let wat = verhaal(bron, &[("ons", &o.stappen), ("hun", &h.stappen)]);
        for x in [b, &o.tekst, &h.tekst] {
            gelijk(x, &gemeen::hernummer(x, zaad), &format!("x tegen hernummerd x\n{wat}"))?;
        }
        let gewoon = samen(b, &o.tekst, &h.tekst)?;
        let her = |x: &str| gemeen::hernummer(x, zaad);
        for (naam, u) in [
            ("hernummerd ons", samen(b, &her(&o.tekst), &h.tekst)?),
            ("hernummerd hun", samen(b, &o.tekst, &her(&h.tekst))?),
            ("hernummerde basis", samen(&her(b), &o.tekst, &h.tekst)?),
        ] {
            eis(u.r.is_some() == gewoon.r.is_some(), || format!(
                "{naam}: conflict {} [{}], gewoon: conflict {} [{}]\n{wat}",
                u.r.is_none(), u.rapport(), gewoon.r.is_none(), gewoon.rapport()
            ))?;
            if let (Some(r1), Some(r2)) = (&u.r, &gewoon.r) {
                gelijk(r1, r2, &format!("{naam} tegen gewoon\n{wat}"))?;
            }
        }
    }

    // --- 8. Schrijven en lezen. ---

    /// Een resultaat is weer te lezen: door de bibliotheek en door een losse lezer (elk `#n` één keer, elke
    /// verwijzing bestaat, elke GlobalId één keer). Het is gelijk aan zichzelf na opnieuw lezen en schrijven (onze
    /// schrijver, hernummeren, en samenvoegen met zichzelf).
    #[test]
    fn schrijven_en_lezen(bron in bron(), so in reeks(), sh in reeks(), zaad in any::<u64>()) {
        let b = tekst_van(bron);
        let (o, h) = (versie(b, &so, 'O'), versie(b, &sh, 'H'));
        let wat = verhaal(bron, &[("ons", &o.stappen), ("hun", &h.stappen)]);
        let Some(r) = samen(b, &o.tekst, &h.tekst)?.r else { return Ok(()) };
        let doc = Doc::probeer(&r).map_err(|e| TestCaseError::fail(format!("losse lezer: {e}\n{wat}")))?;
        let mut gids = HashSet::new();
        for (&n, rest) in &doc.inst {
            for d in verwijzingen(rest) {
                eis(doc.inst.contains_key(&d), || format!("#{n} verwijst naar #{d}, dat er niet is\n{wat}"))?;
            }
            if let Some(g) = doc.gid(n) {
                eis(gids.insert(g.clone()), || format!("GlobalId {g} staat er twee keer\n{wat}"))?;
            }
        }
        gelijk(&r, &r, &format!("r tegen r\n{wat}"))?;
        gelijk(&doc.tekst(), &r, &format!("opnieuw geschreven r tegen r\n{wat}"))?;
        let r2 = gemeen::hernummer(&r, zaad);
        gelijk(&r2, &r, &format!("hernummerd r tegen r\n{wat}"))?;
        gelijk(&gelukt(&r, &r, &r2, &wat)?, &r, &format!("(r, r, hernummerd r) tegen r\n{wat}"))?;
    }
}

// --- 9. Geen paniek. ---

#[derive(Debug, Clone)]
enum Verminking {
    /// Een stuk bytes weg.
    Weg { plek: Index, lengte: u16 },
    /// Een regel twee keer.
    Dubbel { regel: Index },
    /// Twee regels omgewisseld.
    Wissel { a: Index, b: Index },
    /// Alles na deze plek weg.
    Kap { plek: Index },
    /// Eén byte anders.
    Byte { plek: Index, byte: u8 },
}

fn verminking() -> impl Strategy<Value = Verminking> {
    prop_oneof![
        (any::<Index>(), 1u16..64).prop_map(|(plek, lengte)| Verminking::Weg { plek, lengte }),
        any::<Index>().prop_map(|regel| Verminking::Dubbel { regel }),
        (any::<Index>(), any::<Index>()).prop_map(|(a, b)| Verminking::Wissel { a, b }),
        any::<Index>().prop_map(|plek| Verminking::Kap { plek }),
        (any::<Index>(), prop::sample::select(b"#=(),;'$*.\\\"0E\n".to_vec()))
            .prop_map(|(plek, byte)| Verminking::Byte { plek, byte }),
    ]
}

fn vermink(tekst: &[u8], v: &Verminking) -> Vec<u8> {
    let mut t = tekst.to_vec();
    if t.is_empty() {
        return t;
    }
    match v {
        Verminking::Weg { plek, lengte } => {
            let p = plek.index(t.len());
            let e = (p + usize::from(*lengte)).min(t.len());
            t.drain(p..e);
        }
        Verminking::Dubbel { regel } => {
            let regels: Vec<&[u8]> = tekst.split_inclusive(|&c| c == b'\n').collect();
            let k = regel.index(regels.len());
            let mut uit = Vec::new();
            for (i, r) in regels.iter().enumerate() {
                uit.extend_from_slice(r);
                if i == k {
                    uit.extend_from_slice(r);
                }
            }
            t = uit;
        }
        Verminking::Wissel { a, b } => {
            let mut regels: Vec<&[u8]> = tekst.split_inclusive(|&c| c == b'\n').collect();
            let (i, j) = (a.index(regels.len()), b.index(regels.len()));
            regels.swap(i, j);
            t = regels.concat();
        }
        Verminking::Kap { plek } => t.truncate(plek.index(t.len())),
        Verminking::Byte { plek, byte } => {
            let p = plek.index(t.len());
            t[p] = *byte;
        }
    }
    t
}

proptest! {
    #![proptest_config(config())]

    /// Bij verminkte invoer (bytes weg, regels dubbel of omgewisseld, afgekapt, een byte anders) geven `verschil` en
    /// `voeg_samen` een fout of een uitkomst, nooit een paniek. Een paniek in een lees-thread wordt `Fout::Intern`;
    /// die telt ook als paniek.
    #[test]
    fn geen_paniek(
        bron in bron(),
        so in reeks(),
        sh in reeks(),
        welke in 0usize..3,
        verminkingen in prop::collection::vec(verminking(), 1..=3),
    ) {
        let b = tekst_van(bron);
        let (o, h) = (versie(b, &so, 'O'), versie(b, &sh, 'H'));
        let mut drie = [b.as_bytes().to_vec(), o.tekst.into_bytes(), h.tekst.into_bytes()];
        for v in &verminkingen {
            drie[welke] = vermink(&drie[welke], v);
        }
        let wat = format!("basis {bron:?}, verminkt: {}, {verminkingen:?}", ["basis", "ons", "hun"][welke]);
        let [b, o, h] = &drie;
        let uitslag = std::panic::catch_unwind(|| voeg_samen(b, o, h));
        eis(uitslag.is_ok(), || format!("voeg_samen raakte in paniek\n{wat}"))?;
        eis(!matches!(uitslag, Ok(Err(Fout::Intern(_)))), || format!("voeg_samen: {:?}\n{wat}", uitslag.as_ref().ok()))?;
        let v = std::panic::catch_unwind(|| verschil(b, o));
        eis(v.is_ok(), || format!("verschil raakte in paniek\n{wat}"))?;
        eis(!matches!(v, Ok(Err(Fout::Intern(_)))), || format!("verschil: Fout::Intern\n{wat}"))?;
        // Lukt het toch, dan is het resultaat weer te lezen.
        if let Ok(Ok(s)) = uitslag {
            if let Some(r) = s.inhoud {
                eis(verschil(&r, &r).is_ok(), || format!("het resultaat is niet te lezen\n{wat}"))?;
            }
        }
    }
}

// --- 10. Het rapport klopt met de uitkomst. ---

proptest! {
    #![proptest_config(config())]

    /// `gelukt`, `inhoud` en `conflicten` zeggen hetzelfde: gelukt ⇔ er is een inhoud ⇔ er zijn geen conflicten.
    #[test]
    fn rapport_klopt(bron in bron(), so in reeks(), sh in reeks()) {
        let b = tekst_van(bron);
        let (o, h) = (versie(b, &so, 'O'), versie(b, &sh, 'H'));
        let wat = verhaal(bron, &[("ons", &o.stappen), ("hun", &h.stappen)]);
        for (x, y) in [(&o.tekst, &h.tekst), (&h.tekst, &o.tekst)] {
            let s = voeg_samen(b.as_bytes(), x.as_bytes(), y.as_bytes())
                .map_err(|f| TestCaseError::fail(format!("fout: {f}\n{wat}")))?;
            eis(s.gelukt == s.inhoud.is_some() && s.gelukt == s.conflicten.is_empty(), || format!(
                "gelukt {}, inhoud {}, conflicten {:?}\n{wat}",
                s.gelukt, s.inhoud.is_some(), s.conflicten.iter().map(|c| c.soort).collect::<Vec<_>>()
            ))?;
        }
    }
}

// --- Controle van de generator zelf. ---

/// Elke bewerking doet op de basisbestanden echt iets, en de tekst blijft voor de bibliotheek leesbaar.
#[test]
fn generator_maakt_echte_wijzigingen() {
    let i = 0;
    let alle = [
        Bewerking::Naam { doel: i, waarde: 1 },
        Bewerking::Beschrijving { doel: i, waarde: 1 },
        Bewerking::Eigenschap { doel: i, waarde: 1 },
        Bewerking::EigenschapErbij { pset: i, naam: 0, waarde: 1 },
        Bewerking::NieuwePset { object: i, naam: 0, waarde: 1 },
        Bewerking::Verwijder { doel: i },
        Bewerking::Verplaats { doel: i, x: 3 },
        Bewerking::NieuwElement { bevat: i, naam: 0 },
        Bewerking::HaalUit { doel: i },
    ];
    let mut gedaan: HashMap<String, usize> = HashMap::new();
    for bron in [Bron::Wand, Bron::Planning, Bron::Gebouw2x3, Bron::Architectuur2x3] {
        let b = tekst_van(bron);
        let v = verschil(b.as_bytes(), Doc::lees(b).tekst().as_bytes()).unwrap();
        assert!(v.is_leeg(), "{bron:?}");
        // De losse lezer ziet dezelfde objecten als de bibliotheek.
        assert_eq!(Doc::lees(b).objecten().len(), v.statistiek.a.objecten, "{bron:?}");
        for bw in &alle {
            let x = versie(b, std::slice::from_ref(bw), 'O');
            let v = verschil(b.as_bytes(), x.tekst.as_bytes()).unwrap();
            if let Some(s) = x.stappen.first() {
                assert!(!v.is_leeg(), "{bron:?}: {s:?} veranderde niets");
                *gedaan.entry(format!("{bw:?}").split(' ').next().unwrap().to_string()).or_default() += 1;
            }
        }
        // De tijdstempel is vluchtig: het verschil is leeg, maar noemt hem wel.
        let x = versie(b, &[Bewerking::Tijdstempel { waarde: 3 }], 'O');
        let v = verschil(b.as_bytes(), x.tekst.as_bytes()).unwrap();
        assert!(v.is_leeg() && !v.vluchtig.is_empty(), "{bron:?}: {v:?}");
    }
    // Elke soort bewerking kwam op minstens één bestand voor.
    assert_eq!(gedaan.len(), alle.len(), "{gedaan:?}");
}
