//! Wanneer is een sync-ronde nodig (ontwerp §8): bij een lokale wijziging (bestandswachter in de app) of
//! wanneer de wijzigingenlijst van de server iets nieuws heeft sinds de laatste cursor. Zo hoeft de app niet
//! elke keer de hele ruimte af te lopen.

use std::path::Path;

use crate::motor::TOESTANDSMAP;
use crate::{Client, Fout};

const TIJDELIJK: [&str; 2] = [".openaec-download", ".json.tmp"];

/// Telt een gebeurtenis op `pad` mee voor de map `root`? Niet voor de eigen toestandsmap en tijdelijke bestanden
/// van de motor (anders lokt elke ronde een volgende uit).
pub fn telt_mee(root: &Path, pad: &Path) -> bool {
    let Ok(rel) = pad.strip_prefix(root) else { return false };
    if rel.components().next().is_some_and(|c| c.as_os_str() == TOESTANDSMAP) {
        return false;
    }
    let naam = pad.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    !TIJDELIJK.iter().any(|ext| naam.ends_with(ext))
}

/// Zijn er wijzigingen in de ruimte na `sinds`? Geeft ook de nieuwe cursor (loopt alle pagina's af).
pub async fn nieuwe_wijzigingen(client: &Client, ruimte: &str, sinds: u64) -> Result<(bool, u64), Fout> {
    let mut cursor = sinds;
    let mut iets = false;
    loop {
        let pagina = client.wijzigingen(ruimte, cursor).await?;
        iets |= !pagina.wijzigingen.is_empty();
        cursor = pagina.cursor.max(cursor);
        if !pagina.meer {
            return Ok((iets, cursor));
        }
    }
}
