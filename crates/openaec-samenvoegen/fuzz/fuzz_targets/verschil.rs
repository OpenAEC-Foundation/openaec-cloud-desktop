//! Het verschil tussen twee stukken uit één invoer (gescheiden door een NUL-byte; zonder NUL is het tweede stuk
//! gelijk aan het eerste). Nooit een paniek; `verschil(a, b)` en `verschil(b, a)` lukken allebei of geen van beide, en
//! wat bij de een toegevoegd is, is bij de ander verwijderd.
#![no_main]

use libfuzzer_sys::fuzz_target;
use openaec_samenvoegen::verschil;

fuzz_target!(|data: &[u8]| {
    let mut delen = data.splitn(2, |&b| b == 0);
    let a = delen.next().unwrap_or_default();
    let b = delen.next().unwrap_or(a);
    match (verschil(a, b), verschil(b, a)) {
        (Ok(ab), Ok(ba)) => {
            assert_eq!(ab.toegevoegd.len(), ba.verwijderd.len());
            assert_eq!(ab.verwijderd.len(), ba.toegevoegd.len());
            assert_eq!(ab.gewijzigd.len(), ba.gewijzigd.len());
        }
        (Err(_), Err(_)) => {}
        (ab, ba) => panic!("verschil(a, b) = {ab:?}\nverschil(b, a) = {ba:?}"),
    }
});
