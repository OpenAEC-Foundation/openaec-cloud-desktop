fn main() {
    tauri_build::build();
    // Het resourcebestand met het app-manifest (Common Controls v6) linkt tauri-build alleen in de app zelf; de
    // unit-tests linken het via lib.rs, anders start de testbinary niet (TaskDialogIndirect ontbreekt in v5).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-search=native={}", std::env::var("OUT_DIR").unwrap());
    }
}
