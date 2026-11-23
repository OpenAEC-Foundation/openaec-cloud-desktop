//! Snelheid en geheugen op een groot gegenereerd bestand. Niet in de gewone `cargo test`:
//!
//! ```text
//! cargo test --release --test meting -- --ignored --nocapture
//! ```
//!
//! De test schrijft de bestanden ook naar `target/meting/`, zodat je het opdrachtregelprogramma erop kunt meten
//! (bijv. met `/usr/bin/time -v`).

use std::fmt::Write;
use std::path::PathBuf;
use std::time::Instant;

use openaec_samenvoegen::verschil;

/// Zoveel elementen, elk met een eigen plaatsing en geometrie (8 instanties per element).
const AANTAL: usize = 200_000;

fn guid(soort: u8, i: usize) -> String {
    const TEKENS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    let mut uit = vec![b'0'; 22];
    uit[1] = soort;
    let mut n = i;
    for plek in (2..22).rev() {
        uit[plek] = TEKENS[n % 64];
        n /= 64;
    }
    String::from_utf8(uit).unwrap()
}

/// Een gebouw met `aantal` elementen; `naam` en `x` bepalen per element de naam en de plek.
fn genereer(
    aantal: usize,
    naam: impl Fn(usize) -> String,
    x: impl Fn(usize) -> f64,
    weg: impl Fn(usize) -> bool,
) -> String {
    let mut s = String::with_capacity(aantal * 600);
    s.push_str("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [DesignTransferView]'),'2;1');\nFILE_NAME('groot.ifc','2026-11-05T10:00:00',('Meting'),('OpenAEC'),'meting','meting','');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n");
    s.push_str("#1=IFCPERSON($,'Meting',$,$,$,$,$,$);\n#2=IFCORGANIZATION($,'OpenAEC',$,$,$);\n#3=IFCPERSONANDORGANIZATION(#1,#2,$);\n#4=IFCAPPLICATION(#2,'1','Meting','M');\n#5=IFCOWNERHISTORY(#3,#4,$,.NOCHANGE.,$,$,$,1700000000);\n");
    s.push_str("#6=IFCCARTESIANPOINT((0.,0.,0.));\n#7=IFCAXIS2PLACEMENT3D(#6,$,$);\n#8=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#7,$);\n#9=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n#10=IFCUNITASSIGNMENT((#9));\n");
    let _ = writeln!(s, "#11=IFCPROJECT('{}',#5,'Groot',$,$,$,$,(#8),#10);", guid(b'P', 0));
    s.push_str("#12=IFCLOCALPLACEMENT($,#7);\n");
    let _ = writeln!(s, "#13=IFCBUILDINGSTOREY('{}',#5,'Verdieping',$,$,#12,$,$,.ELEMENT.,0.);", guid(b'S', 0));
    let _ = writeln!(s, "#14=IFCRELAGGREGATES('{}',#5,$,$,#11,(#13));", guid(b'A', 0));
    s.push_str("#15=IFCDIRECTION((0.,0.,1.));\n#16=IFCCARTESIANPOINT((0.,0.));\n#17=IFCAXIS2PLACEMENT2D(#16,$);\n");
    let mut elementen = Vec::with_capacity(aantal);
    let mut n = 100usize;
    for i in 0..aantal {
        if weg(i) {
            continue;
        }
        let _ = writeln!(s, "#{}=IFCCARTESIANPOINT(({}.,{}.,0.));", n, x(i), i / 1000);
        let _ = writeln!(s, "#{}=IFCAXIS2PLACEMENT3D(#{},$,$);", n + 1, n);
        let _ = writeln!(s, "#{}=IFCLOCALPLACEMENT(#12,#{});", n + 2, n + 1);
        let _ = writeln!(s, "#{}=IFCRECTANGLEPROFILEDEF(.AREA.,$,#17,1.,0.2);", n + 3);
        let _ = writeln!(s, "#{}=IFCEXTRUDEDAREASOLID(#{},#7,#15,3.);", n + 4, n + 3);
        let _ = writeln!(s, "#{}=IFCSHAPEREPRESENTATION(#8,'Body','SweptSolid',(#{}));", n + 5, n + 4);
        let _ = writeln!(s, "#{}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{}));", n + 6, n + 5);
        let _ = writeln!(
            s,
            "#{}=IFCBUILDINGELEMENTPROXY('{}',#5,'{}',$,$,#{},#{},$,$);",
            n + 7,
            guid(b'E', i),
            naam(i),
            n + 2,
            n + 6
        );
        elementen.push(n + 7);
        n += 8;
    }
    let lijst: Vec<String> = elementen.iter().map(|e| format!("#{e}")).collect();
    let _ =
        writeln!(s, "#{}=IFCRELCONTAINEDINSPATIALSTRUCTURE('{}',#5,$,$,({}),#13);", n, guid(b'C', 0), lijst.join(","));
    s.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    s
}

pub fn map() -> PathBuf {
    let map = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/meting");
    std::fs::create_dir_all(&map).unwrap();
    map
}

/// Basis, ons (andere namen voor 1 op 1000) en hun (andere plek voor 1 op 1000, 1 op 4000 verwijderd).
pub fn versies(aantal: usize) -> (String, String, String) {
    let basis = genereer(aantal, |i| format!("Element {i}"), |i| (i % 1000) as f64, |_| false);
    let ons = genereer(
        aantal,
        |i| if i % 1000 == 1 { format!("Element {i} (ons)") } else { format!("Element {i}") },
        |i| (i % 1000) as f64,
        |_| false,
    );
    let hun = genereer(
        aantal,
        |i| format!("Element {i}"),
        |i| if i % 1000 == 2 { 5000.0 } else { (i % 1000) as f64 },
        |i| i % 4000 == 3,
    );
    (basis, ons, hun)
}

/// Piekgeheugen van dit proces (Linux).
pub fn piekgeheugen() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find(|r| r.starts_with("VmHWM")).map(|r| r.split_whitespace().nth(1).unwrap_or("?").to_string())
        })
        .map(|kb| format!("{} MB", kb.parse::<u64>().unwrap_or(0) / 1024))
        .unwrap_or_else(|| "onbekend".into())
}

#[test]
#[ignore]
fn meet_verschil() {
    let (basis, ons, hun) = versies(AANTAL);
    let map = map();
    std::fs::write(map.join("basis.ifc"), &basis).unwrap();
    std::fs::write(map.join("ons.ifc"), &ons).unwrap();
    std::fs::write(map.join("hun.ifc"), &hun).unwrap();
    println!("bestand: {:.0} MB, {} elementen", basis.len() as f64 / 1e6, AANTAL);
    let start = Instant::now();
    let v = verschil(basis.as_bytes(), ons.as_bytes()).unwrap();
    println!(
        "verschil(basis, ons): {:.2} s, {} gewijzigd, piek {}",
        start.elapsed().as_secs_f64(),
        v.gewijzigd.len(),
        piekgeheugen()
    );
    assert_eq!(v.gewijzigd.len(), AANTAL / 1000);
}
