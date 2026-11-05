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
    // Met hun als winnaar: wel een bestand, en exitcode 1 omdat er conflicten waren.
    let (code, _) = voeg_samen(&pad, &b, &o, &h, &["--bij-conflict", "hun"]);
    assert_eq!(code, 1);
    assert!(std::fs::read_to_string(pad.join("uit.ifc")).unwrap().contains("'W-hun'"));

    let pad = map("fout");
    let (code, _) = voeg_samen(&pad, &b, "geen IFC", &h, &[]);
    assert_eq!(code, 2);
}
