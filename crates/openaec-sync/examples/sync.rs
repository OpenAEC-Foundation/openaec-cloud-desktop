//! Eén sync-ronde vanaf de opdrachtregel, voor tests tegen een echte omgeving:
//!
//! `OPENAEC_AUTORISATIE="token <sleutel>:<geheim>" cargo run --example sync -- <basis-url> <ruimte> <map>`
//!
//! Het rapport komt als JSON op stdout; de logregels van de motor (bijv. "samengevoegd", "niet samengevoegd") op
//! stderr. De simulatie in `scripts/simulatie/` telt daarmee.

use openaec_sync::motor::sync_eenmalig;
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::Client;

/// Logregels van deze crate naar stderr, vanaf niveau info.
struct NaarStderr;

impl log::Log for NaarStderr {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info && m.target().starts_with("openaec_sync")
    }

    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("{} {}", r.level(), r.args());
        }
    }

    fn flush(&self) {}
}

#[tokio::main]
async fn main() {
    if log::set_logger(&NaarStderr).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [basis, ruimte, map] = args.as_slice() else {
        eprintln!("gebruik: sync <basis-url> <ruimte> <map>");
        std::process::exit(2);
    };
    let autorisatie = std::env::var("OPENAEC_AUTORISATIE").expect("OPENAEC_AUTORISATIE ontbreekt");
    let opslag = ClientOpslag::new(Client::new(basis, &autorisatie), ruimte);
    std::fs::create_dir_all(map).unwrap();
    match sync_eenmalig(&opslag, std::path::Path::new(map)).await {
        Ok(rapport) => println!("{}", serde_json::to_string(&rapport).unwrap()),
        Err(fout) => {
            eprintln!("{fout}");
            std::process::exit(1);
        }
    }
}
