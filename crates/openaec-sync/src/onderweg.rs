//! Uploads onderweg (zie `docs/samenwerken/offline.md`).
//!
//! Na een fout bij het versturen weet de motor niet of de server de upload vastlegde: het antwoord kan verloren zijn
//! gegaan, of het programma stopte vlak na de upload en vóór de toestand. Een nieuwe poging zou dan een tweede versie
//! of een conflictkopie van hetzelfde werk geven. Daarom legt de motor vóór elke upload duurzaam vast welke inhoud
//! hij verstuurt (grootte en blokhashes) en van welke momentopname S van het lokale bestand die komt. De volgende
//! ronde kijkt eerst: staat precies die inhoud nu als huidige versie op dat pad, dan was het de eigen upload, en neemt
//! de toestand die versie over (zie `motor::eigen_upload`). Een wijziging die de gebruiker daarna opsloeg, is dan een
//! gewone wijziging op die versie.
//!
//! Elke upload heeft een eigen bestand in `.openaec/onderweg/`, geschreven zoals het journaal van `crate::vervang`
//! (tijdelijk bestand, fsync, rename, fsync van de map). Het gaat pas weg als de toestand die de uitkomst kent op
//! schijf staat.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::motor::{Afgeleid, TOESTANDSMAP};
use crate::vervang::{nieuw_id, sync_map};

const MAP: &str = "onderweg";

/// Eén upload die onderweg is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Onderweg {
    pub id: String,
    /// Pad op de server (de sleutel in de toestand).
    pub sleutel: String,
    /// De verstuurde inhoud: grootte en blokhashes (zie `crate::blokken`).
    pub grootte: u64,
    pub hashes: Vec<String>,
    /// Grootte en wijzigingstijd van het lokale bestand (de momentopname S) toen de motor het las.
    pub kenmerk: (u64, u128),
    /// De verstuurde inhoud is een samenvoeging van S, en S staat in `.openaec/eigen/` (zie `motor::Afgeleid`). None: de
    /// verstuurde inhoud is S zelf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub afgeleid: Option<Afgeleid>,
}

impl Onderweg {
    pub fn nieuw(sleutel: &str, grootte: u64, hashes: Vec<String>, kenmerk: (u64, u128), afgeleid: Option<Afgeleid>) -> Self {
        Onderweg { id: nieuw_id(), sleutel: sleutel.to_string(), grootte, hashes, kenmerk, afgeleid }
    }
}

fn map(root: &Path) -> PathBuf {
    root.join(TOESTANDSMAP).join(MAP)
}

/// Leg `o` duurzaam vast, vóór de upload begint.
pub(crate) fn schrijf(root: &Path, o: &Onderweg) -> io::Result<()> {
    let map = map(root);
    fs::create_dir_all(&map)?;
    let tijdelijk = map.join(format!("{}.tmp", o.id));
    let mut f = File::create(&tijdelijk)?;
    f.write_all(&serde_json::to_vec(o)?)?;
    f.sync_all()?;
    fs::rename(&tijdelijk, map.join(format!("{}.json", o.id)))?;
    sync_map(&map)
}

/// Alle uploads die onderweg waren. Een bestand dat niet te lezen is, slaat de motor over (met een logregel): dan
/// weet hij van die upload niets, en wordt het in het ergste geval één versie of conflictkopie te veel.
pub(crate) fn lees(root: &Path) -> Vec<Onderweg> {
    let mut uit = Vec::new();
    for item in fs::read_dir(map(root)).into_iter().flatten().flatten() {
        let pad = item.path();
        if pad.extension().is_some_and(|e| e == "tmp") {
            let _ = fs::remove_file(&pad);
            continue;
        }
        match fs::read(&pad).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
            Ok(o) => uit.push(o),
            Err(e) => log::warn!("{}: upload onderweg niet te lezen: {e}", pad.display()),
        }
    }
    uit
}

/// De uitkomst van de upload staat in de bewaarde toestand: het bestand mag weg.
pub(crate) fn klaar(root: &Path, id: &str) {
    if let Err(e) = fs::remove_file(map(root).join(format!("{id}.json"))) {
        if e.kind() != io::ErrorKind::NotFound {
            log::warn!("upload onderweg {id} niet op te ruimen: {e}");
        }
    }
}
