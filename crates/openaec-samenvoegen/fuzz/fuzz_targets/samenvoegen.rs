//! Samenvoegen van drie stukken uit één invoer: basis, ons en hun, gescheiden door NUL-bytes. Ontbreekt een stuk,
//! dan is het gelijk aan de basis. Nooit een paniek. Lukt het, dan leest het resultaat weer in; lukt het niet, dan
//! is er geen inhoud en wel een conflict. Wijzigde hun niets, dan is het resultaat ons, byte voor byte.
#![no_main]

use libfuzzer_sys::fuzz_target;
use openaec_samenvoegen::{verschil, voeg_samen};

fuzz_target!(|data: &[u8]| {
    let mut delen = data.splitn(3, |&b| b == 0);
    let basis = delen.next().unwrap_or_default();
    let ons = delen.next().unwrap_or(basis);
    let hun = delen.next().unwrap_or(basis);
    let Ok(s) = voeg_samen(basis, ons, hun) else { return };
    if !s.gelukt {
        assert!(s.inhoud.is_none() && !s.conflicten.is_empty(), "{s:?}");
        return;
    }
    let r = s.inhoud.expect("gelukt zonder inhoud");
    if hun == basis {
        assert_eq!(r, ons, "hun wijzigde niets, maar het resultaat is niet ons");
    }
    match verschil(&r, &r) {
        Ok(v) => assert!(v.is_leeg(), "resultaat: verschil met zichzelf {v:?}"),
        Err(e) => panic!("resultaat leest niet in: {e}\n{}", String::from_utf8_lossy(&r)),
    }
});
