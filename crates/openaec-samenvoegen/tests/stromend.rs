//! `verschil_stromend` geeft hetzelfde als `verschil` (de controle zit in `gemeen::verschil`): op elk testbestand
//! met wijzigingen erin, en op paren van verschillende bestanden. Met `IFC_VOORBEELDEN=<map>[:<map>…]` ook op elk
//! `.ifc`-bestand in die mappen (bijv. `examples/` van Open Planner Studio).

mod gemeen;

use gemeen::*;

/// Verandert in elke `stap`-de regel met een getal achter een punt dat cijfer (een coördinaat, een waarde of tekst).
fn ander_getal(tekst: &str, stap: usize) -> String {
    let mut uit = String::with_capacity(tekst.len());
    for (n, r) in tekst.split_inclusive('\n').enumerate() {
        let plek = (n % stap == 0 && r.starts_with('#'))
            .then(|| r.as_bytes().windows(2).position(|w| w[0] == b'.' && w[1].is_ascii_digit()))
            .flatten();
        match plek {
            Some(p) => {
                let c = r.as_bytes()[p + 1];
                uit.push_str(&r[..p + 1]);
                uit.push(if c == b'9' { '0' } else { (c + 1) as char });
                uit.push_str(&r[p + 2..]);
            }
            None => uit.push_str(r),
        }
    }
    uit
}

/// Geeft elk `stap`-de object een andere GlobalId: verwijderd en toegevoegd, of bij een relatie gekoppeld.
fn andere_globalid(tekst: &str, stap: usize) -> String {
    let mut uit = String::with_capacity(tekst.len());
    let mut objecten = 0;
    for r in tekst.split_inclusive('\n') {
        let plek = r.starts_with('#').then(|| r.find("('")).flatten().filter(|&p| {
            let id = r.as_bytes().get(p + 2..p + 24);
            id.is_some_and(|id| id.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'$'))
                && r.as_bytes().get(p + 24) == Some(&b'\'')
        });
        match plek {
            Some(p) if objecten % stap == 0 => {
                objecten += 1;
                let c = r.as_bytes()[p + 3];
                uit.push_str(&r[..p + 3]);
                uit.push(if c == b'A' { 'B' } else { 'A' });
                uit.push_str(&r[p + 4..]);
            }
            Some(_) => {
                objecten += 1;
                uit.push_str(r);
            }
            None => uit.push_str(r),
        }
    }
    uit
}

/// Wat het verschil op het corpus vond, om te zien dat de wijzigingen er echt in zitten.
#[derive(Default, Debug)]
struct Telling {
    paren: usize,
    fouten: usize,
    gewijzigd: usize,
    toegevoegd: usize,
    gekoppeld: usize,
    losse_knopen: usize,
    vluchtig: usize,
}

impl Telling {
    fn tel(&mut self, a: &str, b: &str) {
        self.paren += 1;
        match verschil(a.as_bytes(), b.as_bytes()) {
            Ok(v) => {
                self.gewijzigd += v.gewijzigd.len();
                self.toegevoegd += v.toegevoegd.len();
                self.gekoppeld += v.statistiek.relaties_gekoppeld;
                self.losse_knopen += v.losse_knopen.toegevoegd.len() + v.losse_knopen.verwijderd.len();
                self.vluchtig += v.vluchtig.len();
            }
            Err(_) => self.fouten += 1,
        }
    }
}

fn controleer(bestanden: &[(String, String)]) -> Telling {
    let mut telling = Telling::default();
    for (n, (naam, tekst)) in bestanden.iter().enumerate() {
        eprintln!("{naam}");
        let varianten = [
            hernummer(tekst, 3),
            ander_getal(tekst, 7),
            hernummer(&ander_getal(tekst, 3), 5),
            andere_globalid(tekst, 5),
            andere_globalid(&ander_getal(tekst, 11), 2),
            met_crlf(tekst),
        ];
        telling.tel(tekst, tekst);
        for v in &varianten {
            telling.tel(tekst, v);
            telling.tel(v, tekst);
        }
        // Een ander bestand: (bijna) alles toegevoegd en verwijderd.
        let (_, ander) = &bestanden[(n + 1) % bestanden.len()];
        telling.tel(tekst, ander);
    }
    telling
}

#[test]
fn zelfde_uitkomst_op_de_testbestanden() {
    let bestanden: Vec<(String, String)> = alle_bestanden().into_iter().map(|(n, t)| (n.to_string(), t)).collect();
    let telling = controleer(&bestanden);
    eprintln!("{telling:?}");
    assert!(telling.gewijzigd > 0 && telling.toegevoegd > 0 && telling.gekoppeld > 0, "{telling:?}");
}

#[test]
fn zelfde_uitkomst_op_de_voorbeelden() {
    let Some(mappen) = std::env::var_os("IFC_VOORBEELDEN") else {
        eprintln!("IFC_VOORBEELDEN staat niet; overgeslagen");
        return;
    };
    let mut bestanden = Vec::new();
    for map in std::env::split_paths(&mappen) {
        let mut paden: Vec<_> = std::fs::read_dir(&map)
            .unwrap_or_else(|e| panic!("{}: {e}", map.display()))
            .map(|x| x.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifc")))
            .collect();
        paden.sort();
        for p in paden {
            let tekst = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).into_owned();
            bestanden.push((p.display().to_string(), tekst));
        }
    }
    let telling = controleer(&bestanden);
    eprintln!("{} bestanden: {telling:?}", bestanden.len());
}

/// Fouten komen in dezelfde volgorde als bij `verschil`: eerst lezen, dan een dubbele GlobalId, dan een kring.
#[test]
fn zelfde_fouten() {
    let goed = gebouw("IFC4");
    let kapot = vervang(&goed, "ENDSEC;\nEND-ISO", "ENDSEC\nEND-ISO");
    let dubbel = voeg_toe(&goed, &format!("#900=IFCBUILDINGELEMENTPROXY('{}',$,'X',$,$,$,$,$,$);\n#901=IFCBUILDINGELEMENTPROXY('{}',$,'Y',$,$,$,$,$,$);", gid("Dubbel"), gid("Dubbel")));
    let kring = voeg_toe(&goed, "#903=IFCLOCALPLACEMENT(#904,$);\n#904=IFCLOCALPLACEMENT(#903,$);");
    let zichzelf = voeg_toe(&goed, "#905=IFCLOCALPLACEMENT(#905,$);");
    let wees = voeg_toe(&goed, "#906=IFCLOCALPLACEMENT(#999,$);");
    for x in [&goed, &kapot, &dubbel, &kring, &zichzelf, &wees] {
        for y in [&goed, &kapot, &dubbel, &kring, &zichzelf, &wees] {
            let _ = verschil(x.as_bytes(), y.as_bytes());
        }
    }
    assert!(verschil(goed.as_bytes(), zichzelf.as_bytes()).is_err());
    assert!(verschil(goed.as_bytes(), kring.as_bytes()).is_err());
}
