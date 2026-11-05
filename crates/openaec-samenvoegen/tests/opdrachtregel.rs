//! De opdrachtregel `openaec-samenvoegen voeg-samen` (alleen met de feature `cli`).
#![cfg(feature = "cli")]

mod gemeen;

use std::path::PathBuf;
use std::process::Command;

use gemeen::*;

fn map(naam: &str) -> PathBuf {
    let pad = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(naam);
    let _ = std::fs::remove_dir_all(&pad);
    std::fs::create_dir_all(&pad).unwrap();
    pad
}

fn voeg_samen(pad: &std::path::Path, b: &str, o: &str, h: &str, extra: &[&str]) -> (i32, String) {
    for (naam, inhoud) in [("basis.ifc", b), ("ons.ifc", o), ("hun.ifc", h)] {
        std::fs::write(pad.join(naam), inhoud).unwrap();
    }
    let uit = Command::new(env!("CARGO_BIN_EXE_openaec-samenvoegen"))
        .current_dir(pad)
        .args(["voeg-samen", "basis.ifc", "ons.ifc", "hun.ifc", "-o", "uit.ifc", "--rapport", "rapport.json"])
        .args(extra)
        .output()
        .unwrap();
    (uit.status.code().unwrap(), String::from_utf8_lossy(&uit.stdout).into_owned())
}

#[test]
fn gelukt_conflict_en_fout() {
    let b = gebouw("IFC4");
    let o = vervang(&b, "#5,'W',$", "#5,'W-ons',$");
    let h = vervang(&b, "#5,'V',$", "#5,'V-hun',$");
    let pad = map("gelukt");
    let (code, tekst) = voeg_samen(&pad, &b, &o, &h, &[]);
    assert_eq!(code, 0, "{tekst}");
    let r = std::fs::read(pad.join("uit.ifc")).unwrap();
    let verwacht = vervang(&o, "#5,'V',$", "#5,'V-hun',$");
    assert!(openaec_samenvoegen::verschil(&r, verwacht.as_bytes()).unwrap().is_leeg());

    let pad = map("conflict");
    let h = vervang(&b, "#5,'W',$", "#5,'W-hun',$");
    let (code, tekst) = voeg_samen(&pad, &b, &o, &h, &[]);
    assert_eq!(code, 1, "{tekst}");
    assert!(tekst.contains("beide_gewijzigd"), "{tekst}");
    assert!(!pad.join("uit.ifc").exists());
    let rapport: serde_json::Value = serde_json::from_slice(&std::fs::read(pad.join("rapport.json")).unwrap()).unwrap();
    assert_eq!(rapport["conflicten"][0]["attribuut"], "Name");
    // Een keuze "ons wint" of "hun wint" is er niet meer (K4).
    let uit = Command::new(env!("CARGO_BIN_EXE_openaec-samenvoegen"))
        .current_dir(&pad)
        .args(["voeg-samen", "basis.ifc", "ons.ifc", "hun.ifc", "-o", "uit.ifc", "--bij-conflict", "hun"])
        .output()
        .unwrap();
    assert_eq!(uit.status.code(), Some(2));
    assert!(!pad.join("uit.ifc").exists());

    let pad = map("fout");
    let (code, _) = voeg_samen(&pad, &b, "geen IFC", &h, &[]);
    assert_eq!(code, 2);
}

/// `verschil` leest via mmap en de stromende weg; de JSON is dezelfde als die van de bibliotheek. Een leeg bestand
/// geeft een fout, geen crash.
#[test]
fn verschil_via_mmap() {
    let a = gebouw("IFC4");
    let b = vervang(&hernummer(&a, 9), "'W',$", "'W-nieuw',$");
    let pad = map("verschil");
    std::fs::write(pad.join("a.ifc"), &a).unwrap();
    std::fs::write(pad.join("b.ifc"), &b).unwrap();
    std::fs::write(pad.join("leeg.ifc"), "").unwrap();
    let draai = |x: &str, y: &str| {
        Command::new(env!("CARGO_BIN_EXE_openaec-samenvoegen"))
            .current_dir(&pad)
            .args(["verschil", x, y, "--json"])
            .output()
            .unwrap()
    };
    let uit = draai("a.ifc", "b.ifc");
    assert_eq!(uit.status.code(), Some(0), "{}", String::from_utf8_lossy(&uit.stderr));
    let gelezen: serde_json::Value = serde_json::from_slice(&uit.stdout).unwrap();
    let verwacht = serde_json::to_value(openaec_samenvoegen::verschil(a.as_bytes(), b.as_bytes()).unwrap()).unwrap();
    assert_eq!(gelezen, verwacht);
    assert_eq!(gelezen["gewijzigd"].as_array().map(Vec::len), Some(1));
    assert_eq!(draai("a.ifc", "leeg.ifc").status.code(), Some(2));
}
