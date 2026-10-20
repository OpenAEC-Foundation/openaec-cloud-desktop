//! Eén sync-ronde vanaf de opdrachtregel, voor tests tegen een echte omgeving:
//!
//! `OPENAEC_AUTORISATIE="token <sleutel>:<geheim>" cargo run --example sync -- <basis-url> <ruimte> <map>`

use openaec_sync::motor::sync_eenmalig;
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::Client;

#[tokio::main]
async fn main() {
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
