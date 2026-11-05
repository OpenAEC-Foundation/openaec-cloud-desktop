//! De lezer op willekeurige invoer: nooit een paniek, nooit vastlopen. Leest het bestand, dan is het verschil met
//! zichzelf leeg.
#![no_main]

use libfuzzer_sys::fuzz_target;
use openaec_samenvoegen::verschil;

fuzz_target!(|data: &[u8]| {
    if let Ok(v) = verschil(data, data) {
        assert!(v.is_leeg(), "verschil met zichzelf: {v:?}");
    }
});
