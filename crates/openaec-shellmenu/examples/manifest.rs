//! Het manifest van het pakket schrijven (voor de bouwstraat en de lokale test met een testnaam).
//!
//! ```text
//! cargo run --example manifest -- --versie 0.8.1 --arch x64 --uit pakket-x64/AppxManifest.xml
//!     [--uitgever "CN=..."] [--naam Test.Naam] [--clsid <GUID>] [--exe app.exe] [--weergavenaam "..."]
//! ```

use openaec_shellmenu::logica::{self, Architectuur, Pakketgegevens};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let optie = |naam: &str| argv.iter().position(|a| a == naam).and_then(|i| argv.get(i + 1)).cloned();
    let stop = |tekst: &str| -> ! {
        eprintln!("{tekst}");
        std::process::exit(2)
    };
    let versie = optie("--versie").unwrap_or_else(|| stop("--versie ontbreekt"));
    let arch = optie("--arch").and_then(|a| Architectuur::van(&a)).unwrap_or_else(|| stop("--arch x64 of arm64"));
    let uit = optie("--uit").unwrap_or_else(|| stop("--uit ontbreekt"));
    let (naam, uitgever, clsid, exe, weergavenaam) = (
        optie("--naam"),
        optie("--uitgever"),
        optie("--clsid"),
        optie("--exe"),
        optie("--weergavenaam"),
    );
    let mut g = Pakketgegevens::app(&versie, arch);
    if let Some(n) = naam.as_deref() {
        g.naam = n;
    }
    if let Some(u) = uitgever.as_deref() {
        g.uitgever = u;
    }
    if let Some(c) = clsid.as_deref() {
        g.clsid = c;
    }
    if let Some(e) = exe.as_deref() {
        g.exe = e;
    }
    if let Some(w) = weergavenaam.as_deref() {
        g.weergavenaam = w;
    }
    let xml = logica::manifest(&g);
    if let Some(map) = std::path::Path::new(&uit).parent() {
        let _ = std::fs::create_dir_all(map);
    }
    std::fs::write(&uit, xml).unwrap_or_else(|e| stop(&format!("{uit}: {e}")));
    println!("{uit}: {} {} {} ({})", g.naam, g.versie, g.architectuur.naam(), g.uitgever);
}
