//! Hulpmiddelen voor de tests: testbestanden lezen, kleine IFC-bestanden maken, tekst vervangen en opnieuw nummeren.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;

/// Een testbestand uit `tests/bestanden`.
pub fn bestand(naam: &str) -> String {
    let pad = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/bestanden").join(naam);
    std::fs::read_to_string(&pad).unwrap_or_else(|e| panic!("{}: {e}", pad.display()))
}

/// Alle testbestanden.
pub fn alle_bestanden() -> Vec<(&'static str, String)> {
    [
        "showcase-groot-terrein-onderaannemer.ifc",
        "13-supermarkt-albert-heijn.ifc",
        "05-brugvervanging-n279.ifc",
        "ifc2x3-building-architecture.ifc",
        "ifc4-wall-with-opening-and-window.ifc",
        "ifc4-tessellation-with-individual-colors.ifc",
    ]
    .into_iter()
    .map(|n| (n, bestand(n)))
    .collect()
}

/// Een klein IFC-bestand met deze DATA-regels.
pub fn ifc(schema: &str, data: &str) -> String {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [CoordinationView]'),'2;1');\nFILE_NAME('test.ifc','2026-11-05T10:00:00',('Anne'),('Bouw BV'),'test','test','');\nFILE_SCHEMA(('{schema}'));\nENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n"
    )
}

/// Vervangt `van` door `naar`; `van` moet precies één keer voorkomen.
pub fn vervang(tekst: &str, van: &str, naar: &str) -> String {
    let aantal = tekst.matches(van).count();
    assert_eq!(aantal, 1, "'{van}' komt {aantal} keer voor");
    tekst.replacen(van, naar, 1)
}

/// De regel die met `begin` begint (bijv. `#52=`), zonder regeleinde.
pub fn regel<'t>(tekst: &'t str, begin: &str) -> &'t str {
    tekst.lines().find(|r| r.starts_with(begin)).unwrap_or_else(|| panic!("geen regel {begin}"))
}

/// Voegt regels toe vlak voor de ENDSEC van de DATA-sectie.
pub fn voeg_toe(tekst: &str, regels: &str) -> String {
    let plek = tekst.rfind("ENDSEC;").unwrap();
    format!("{}{}\n{}", &tekst[..plek], regels.trim_end(), &tekst[plek..])
}

/// Verwijdert de regel die met `begin` begint.
pub fn verwijder_regel(tekst: &str, begin: &str) -> String {
    let r = regel(tekst, begin).to_string();
    vervang(tekst, &format!("{r}\n"), "")
}

/// Een kleine, voorspelbare toevalsgenerator (xorshift).
pub struct Toeval(u64);

impl Toeval {
    pub fn nieuw(zaad: u64) -> Self {
        Toeval(zaad.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn volgende(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn schud<T>(&mut self, lijst: &mut [T]) {
        for i in (1..lijst.len()).rev() {
            let j = (self.volgende() % (i as u64 + 1)) as usize;
            lijst.swap(i, j);
        }
    }
}

/// Splitst de DATA-sectie in instanties (op `;` buiten teksten); commentaar valt weg.
fn instanties(data: &str) -> Vec<String> {
    let mut uit = Vec::new();
    let mut huidig = String::new();
    let mut in_tekst = false;
    let mut tekens = data.chars().peekable();
    while let Some(c) = tekens.next() {
        huidig.push(c);
        if in_tekst {
            if c == '\'' {
                if tekens.peek() == Some(&'\'') {
                    huidig.push(tekens.next().unwrap());
                } else {
                    in_tekst = false;
                }
            }
        } else if c == '\'' {
            in_tekst = true;
        } else if c == '/' && tekens.peek() == Some(&'*') {
            // Commentaar valt weg.
            huidig.pop();
            tekens.next();
            let mut vorige = ' ';
            for d in tekens.by_ref() {
                if vorige == '*' && d == '/' {
                    break;
                }
                vorige = d;
            }
        } else if c == ';' {
            huidig.pop();
            let stuk = huidig.trim().to_string();
            if !stuk.is_empty() {
                uit.push(stuk);
            }
            huidig.clear();
        }
    }
    uit
}

/// Vervangt elk `#n` buiten teksten via `tabel`.
fn vervang_nummers(instantie: &str, tabel: &HashMap<u64, u64>) -> String {
    let mut uit = String::with_capacity(instantie.len());
    let mut in_tekst = false;
    let tekens: Vec<char> = instantie.chars().collect();
    let mut i = 0;
    while i < tekens.len() {
        let c = tekens[i];
        if in_tekst {
            uit.push(c);
            if c == '\'' {
                if tekens.get(i + 1) == Some(&'\'') {
                    uit.push('\'');
                    i += 1;
                } else {
                    in_tekst = false;
                }
            }
            i += 1;
            continue;
        }
        if c == '\'' {
            in_tekst = true;
            uit.push(c);
            i += 1;
            continue;
        }
        if c == '#' {
            let mut j = i + 1;
            while j < tekens.len() && tekens[j].is_ascii_digit() {
                j += 1;
            }
            let nummer: u64 = tekens[i + 1..j].iter().collect::<String>().parse().unwrap();
            uit.push_str(&format!("#{}", tabel[&nummer]));
            i = j;
            continue;
        }
        uit.push(c);
        i += 1;
    }
    uit
}

/// Nummert alle instanties willekeurig opnieuw en zet ze in een willekeurige volgorde. De inhoud blijft gelijk.
pub fn hernummer(tekst: &str, zaad: u64) -> String {
    let begin = tekst.find("DATA;").unwrap() + "DATA;".len();
    let eind = tekst.rfind("ENDSEC;").unwrap();
    let mut lijst = instanties(&tekst[begin..eind]);
    let mut toeval = Toeval::nieuw(zaad);
    let mut nieuw: Vec<u64> = (1..=lijst.len() as u64).map(|n| n * 3 + 7).collect();
    toeval.schud(&mut nieuw);
    let mut tabel = HashMap::new();
    for (inst, n) in lijst.iter().zip(&nieuw) {
        let nummer: u64 = inst[1..inst.find('=').unwrap()].trim().parse().unwrap();
        tabel.insert(nummer, *n);
    }
    let mut regels: Vec<String> = lijst.iter_mut().map(|i| format!("{};", vervang_nummers(i, &tabel))).collect();
    toeval.schud(&mut regels);
    format!("{}\n{}\n{}", &tekst[..begin], regels.join("\n"), &tekst[eind..])
}

/// De drie schema's die we kennen.
pub const SCHEMAS: [&str; 3] = ["IFC2X3", "IFC4", "IFC4X3_ADD2"];

/// Een GlobalId van 22 tekens uit een korte naam (aangevuld met nullen).
pub fn gid(naam: &str) -> String {
    assert!(naam.len() <= 21);
    format!("0{naam:0<21}")
}

/// Een klein gebouw dat in IFC2X3, IFC4 en IFC4X3 geldig is: verdiepingen P en Q met elk een eigen plaatsing met
/// dezelfde inhoud, wanden W en V (IfcBuildingElementProxy) die relatief aan de plaatsing van P staan en in P zitten,
/// en een eigenschappenset met Brandwerendheid = 30 voor W en V.
pub fn gebouw(schema: &str) -> String {
    let data = format!(
        "#1=IFCPERSON($,'Anne',$,$,$,$,$,$);
#2=IFCORGANIZATION($,'Bouw BV',$,$,$);
#3=IFCPERSONANDORGANIZATION(#1,#2,$);
#4=IFCAPPLICATION(#2,'1.0','Test','Test');
#5=IFCOWNERHISTORY(#3,#4,$,.NOCHANGE.,$,$,$,1700000000);
#6=IFCPROJECT('{project}',#5,'Project',$,$,$,$,(#7),#8);
#7=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#11,$);
#8=IFCUNITASSIGNMENT((#9));
#9=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#10=IFCCARTESIANPOINT((0.,0.,0.));
#11=IFCAXIS2PLACEMENT3D(#10,$,$);
#20=IFCLOCALPLACEMENT($,#21);
#21=IFCAXIS2PLACEMENT3D(#22,$,$);
#22=IFCCARTESIANPOINT((0.,0.,0.));
#23=IFCLOCALPLACEMENT($,#24);
#24=IFCAXIS2PLACEMENT3D(#25,$,$);
#25=IFCCARTESIANPOINT((0.,0.,0.));
#30=IFCBUILDINGSTOREY('{p}',#5,'P',$,$,#20,$,$,.ELEMENT.,0.);
#31=IFCBUILDINGSTOREY('{q}',#5,'Q',$,$,#23,$,$,.ELEMENT.,0.);
#32=IFCRELAGGREGATES('{aggr}',#5,$,$,#6,(#30,#31));
#40=IFCLOCALPLACEMENT(#20,#41);
#41=IFCAXIS2PLACEMENT3D(#42,$,$);
#42=IFCCARTESIANPOINT((1.,0.,0.));
#43=IFCBUILDINGELEMENTPROXY('{w}',#5,'W',$,$,#40,$,$,$);
#44=IFCBUILDINGELEMENTPROXY('{v}',#5,'V',$,$,#45,$,$,$);
#45=IFCLOCALPLACEMENT(#20,#46);
#46=IFCAXIS2PLACEMENT3D(#47,$,$);
#47=IFCCARTESIANPOINT((2.,0.,0.));
#50=IFCRELCONTAINEDINSPATIALSTRUCTURE('{bevat}',#5,$,$,(#43,#44),#30);
#60=IFCPROPERTYSINGLEVALUE('Brandwerendheid',$,IFCINTEGER(30),$);
#61=IFCPROPERTYSET('{pset}',#5,'Pset_Test',$,(#60));
#62=IFCRELDEFINESBYPROPERTIES('{defs}',#5,$,$,(#43,#44),#61);",
        project = gid("Project"),
        p = gid("StoreyP"),
        q = gid("StoreyQ"),
        aggr = gid("Aggr"),
        w = gid("WandW"),
        v = gid("WandV"),
        bevat = gid("BevatP"),
        pset = gid("Pset"),
        defs = gid("Defs"),
    );
    ifc(schema, &data)
}
