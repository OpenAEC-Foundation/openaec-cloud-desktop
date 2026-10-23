//! Blokken en blokhashes (ontwerp §5): vaste blokgrootte per bestand, BLAKE3 per blok.

use std::io::Read;
use std::path::Path;

/// Standaard blokgrootte: 8 MiB (ook de partgrootte van multipart-uploads).
pub const BLOK: u64 = 8 * 1024 * 1024;
const MAX_DELEN: u64 = 9000;
const MIB: u64 = 1024 * 1024;

/// Zelfde regels als de server: 8 MiB, of groter boven 80 GiB (max. 9000 blokken, veelvoud van 1 MiB).
pub fn blokgrootte(grootte: u64) -> u64 {
    if grootte <= BLOK * MAX_DELEN {
        return BLOK;
    }
    grootte.div_ceil(MAX_DELEN).div_ceil(MIB) * MIB
}

/// Aantal blokken voor een bestand van deze grootte (minstens één, ook voor een leeg bestand).
pub fn aantal_blokken(grootte: u64, blok: u64) -> u64 {
    grootte.div_ceil(blok).max(1)
}

fn hash(stuk: &[u8]) -> String {
    blake3::hash(stuk).to_hex().to_string()
}

/// Hashes per blok van een stuk geheugen.
pub fn hash_bytes(data: &[u8], blok: u64) -> Vec<String> {
    if data.is_empty() {
        return vec![hash(&[])];
    }
    data.chunks(blok as usize).map(hash).collect()
}

/// Hashes per blok van een bestand, zonder het hele bestand in het geheugen te laden. Het lezen en rekenen gebeurt op
/// een aparte thread (`spawn_blocking`), zodat parallelle overdrachten de async-runtime niet ophouden.
pub async fn hash_bestand(pad: &Path, blok: u64) -> std::io::Result<Vec<String>> {
    let pad = pad.to_path_buf();
    tokio::task::spawn_blocking(move || hash_bestand_nu(&pad, blok)).await.map_err(std::io::Error::other)?
}

/// Als [`hash_bestand`], maar op de huidige thread.
pub fn hash_bestand_nu(pad: &Path, blok: u64) -> std::io::Result<Vec<String>> {
    let mut bestand = std::fs::File::open(pad)?;
    let mut buffer = vec![0u8; blok as usize];
    let mut hashes = Vec::new();
    loop {
        let mut gevuld = 0;
        while gevuld < buffer.len() {
            let gelezen = bestand.read(&mut buffer[gevuld..])?;
            if gelezen == 0 {
                break;
            }
            gevuld += gelezen;
        }
        if gevuld == 0 {
            break;
        }
        hashes.push(hash(&buffer[..gevuld]));
        if gevuld < buffer.len() {
            break;
        }
    }
    if hashes.is_empty() {
        hashes.push(hash(&[]));
    }
    Ok(hashes)
}

/// Indexen van blokken die in `nieuw` anders zijn dan (of ontbreken in) `lokaal`.
pub fn gewijzigde_blokken(lokaal: &[String], nieuw: &[String]) -> Vec<usize> {
    nieuw
        .iter()
        .enumerate()
        .filter(|(i, h)| lokaal.get(*i) != Some(*h))
        .map(|(i, _)| i)
        .collect()
}
