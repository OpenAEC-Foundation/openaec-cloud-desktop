//! Meting van de sync tegen een echte server (bijv. de lokale meetbench, zie `scripts/meetbench` in openaec-storage).
//!
//!     cargo run --release --example meting -- maak <map> [kleine bestanden] [grote bestanden] [MB per groot bestand]
//!     cargo run --release --example meting -- upload <basis> <ruimte> <map>
//!     cargo run --release --example meting -- download <basis> <ruimte> <map>
//!
//! `maak` zet een vaste set neer (standaard 2000 bestanden van 1–20 KB in 40 mappen en 5 bestanden van 50 MB).
//! `upload` zet de map in een lege ruimte, `download` haalt de ruimte in een lege map; beide geven de tijd. De
//! autorisatie komt uit `OPENAEC_AUTORISATIE` (bijv. `token sleutel:geheim`). Alleen de openbare API van de crate:
//! dezelfde meting werkt zo ook tegen een oudere versie van de crate.

use std::path::Path;
use std::time::Instant;

use openaec_sync::motor::sync_met_selectie;
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::Client;
use rand::{Rng, RngCore, SeedableRng};

fn maak(map: &Path, klein: usize, groot: usize, mb: usize) -> std::io::Result<()> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(2026);
    for i in 0..klein {
        let grootte = rng.gen_range(1024..=20 * 1024);
        let mut inhoud = vec![0u8; grootte];
        rng.fill_bytes(&mut inhoud);
        let pad = map.join(format!("map {:02}", i % 40)).join(format!("bestand {i:04}.dat"));
        std::fs::create_dir_all(pad.parent().unwrap())?;
        std::fs::write(pad, inhoud)?;
    }
    for i in 0..groot {
        let mut inhoud = vec![0u8; mb * 1024 * 1024];
        rng.fill_bytes(&mut inhoud);
        let pad = map.join("groot").join(format!("model {i}.bin"));
        std::fs::create_dir_all(pad.parent().unwrap())?;
        std::fs::write(pad, inhoud)?;
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let getal = |i: usize, standaard: usize| args.get(i).and_then(|a| a.parse().ok()).unwrap_or(standaard);
    match args.first().map(String::as_str) {
        Some("maak") => {
            let map = Path::new(&args[1]);
            maak(map, getal(2, 2000), getal(3, 5), getal(4, 50)).expect("set maken");
            println!("klaar: {}", map.display());
        }
        Some(richting @ ("upload" | "download")) => {
            let (basis, ruimte, map) = (&args[1], &args[2], Path::new(&args[3]));
            let autorisatie = std::env::var("OPENAEC_AUTORISATIE").expect("zet OPENAEC_AUTORISATIE");
            std::fs::create_dir_all(map).expect("map");
            let opslag = ClientOpslag::new(Client::new(basis, &autorisatie), ruimte);
            let begin = Instant::now();
            let rapport = sync_met_selectie(&opslag, map, &[]).await.expect("ronde");
            let duur = begin.elapsed().as_secs_f64();
            println!(
                "{richting}: {duur:.1} s (geüpload {}, gedownload {}, fouten {})",
                rapport.geupload,
                rapport.gedownload,
                rapport.fouten.len()
            );
            for f in rapport.fouten.iter().take(5) {
                eprintln!("  fout: {f}");
            }
        }
        _ => eprintln!("gebruik: meting maak <map> | meting upload|download <basis> <ruimte> <map>"),
    }
}
