//! Kern van de OpenAEC Storage-sync (ontwerp `2026-10-20-openaec-storage-design.md`, §5 en §8):
//! API-client, blokhashes en delta-upload/-download. Geen UI; de Tauri-app gebruikt deze crate,
//! en later ook de WebDAV-gateway en een macOS/Linux-client.

pub mod api;
pub mod bewaking;
pub mod blokken;
pub mod motor;
pub mod oauth;
pub mod opslag;

use std::io::SeekFrom;
use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

pub use api::{Client, UploadResultaat};
use api::Etag;
use blokken::{aantal_blokken, blokgrootte, gewijzigde_blokken, hash_bestand};

#[derive(Debug, thiserror::Error)]
pub enum Fout {
    #[error("netwerkfout: {0}")]
    Netwerk(#[from] reqwest::Error),
    #[error("bestandsfout: {0}")]
    Bestand(#[from] std::io::Error),
    #[error("server antwoordde {status}: {tekst}")]
    Server { status: u16, tekst: String },
    #[error("controle mislukt: {0}")]
    Controle(String),
}

async fn lees_stuk(pad: &Path, begin: u64, lengte: u64) -> std::io::Result<Vec<u8>> {
    let mut bestand = tokio::fs::File::open(pad).await?;
    bestand.seek(SeekFrom::Start(begin)).await?;
    let mut buffer = vec![0u8; lengte as usize];
    bestand.read_exact(&mut buffer).await?;
    Ok(buffer)
}

/// Upload een lokaal bestand als nieuwe versie. Met `basis_versie` (de versie waarop de lokale wijziging
/// gebaseerd is) hergebruikt de server ongewijzigde blokken; alleen de gevraagde blokken gaan over de lijn.
pub async fn upload_bestand(
    client: &Client,
    ruimte: &str,
    map: Option<&str>,
    naam: &str,
    pad: &Path,
    basis_versie: Option<u64>,
) -> Result<UploadResultaat, Fout> {
    let grootte = tokio::fs::metadata(pad).await?.len();
    let blok = blokgrootte(grootte);
    let hashes = hash_bestand(pad, blok).await?;
    let start = client
        .upload_start(ruimte, map, naam, grootte, basis_versie, &hashes, "sync")
        .await?;

    let mut etags = Vec::with_capacity(start.delen.len());
    for deel in &start.delen {
        let begin = (deel.nummer - 1) * start.deelgrootte;
        let lengte = start.deelgrootte.min(grootte.saturating_sub(begin));
        let stuk = lees_stuk(pad, begin, lengte).await?;
        let antwoord = client.http.put(&deel.url).body(stuk).send().await?;
        if !antwoord.status().is_success() {
            let status = antwoord.status().as_u16();
            return Err(Fout::Server { status, tekst: antwoord.text().await.unwrap_or_default() });
        }
        let etag = antwoord
            .headers()
            .get("etag")
            .and_then(|w| w.to_str().ok())
            .ok_or_else(|| Fout::Controle(format!("geen ETag voor deel {}", deel.nummer)))?
            .to_string();
        etags.push(Etag { nummer: deel.nummer, etag });
    }
    client.upload_klaar(&start.upload, &etags).await
}

/// Download de huidige versie naar `pad`. Bestaat het bestand al, dan worden alleen de blokken opgehaald die
/// lokaal anders zijn; de rest komt uit het lokale bestand. Het resultaat wordt gecontroleerd en pas daarna
/// atomair op de plek van het oude bestand gezet.
pub async fn download_bestand(client: &Client, bestand: &str, pad: &Path) -> Result<(), Fout> {
    let info = client.blokken(bestand).await?;
    let link = client.download(bestand).await?;
    let blok = info.blokgrootte;
    let aantal = aantal_blokken(info.grootte, blok) as usize;

    let lokaal = if tokio::fs::try_exists(pad).await? {
        Some(hash_bestand(pad, blok).await?)
    } else {
        None
    };
    let ophalen: Vec<usize> = match (&lokaal, &info.hashes) {
        (Some(lokaal), Some(nieuw)) => gewijzigde_blokken(lokaal, nieuw),
        _ => (0..aantal).collect(),
    };

    let tijdelijk = pad.with_extension("openaec-download");
    let mut uit = tokio::fs::File::create(&tijdelijk).await?;
    for index in 0..aantal {
        let begin = index as u64 * blok;
        let lengte = blok.min(info.grootte.saturating_sub(begin));
        if lengte == 0 {
            continue;
        }
        let stuk = if ophalen.contains(&index) {
            let antwoord = client
                .http
                .get(&link.url)
                .header("Range", format!("bytes={}-{}", begin, begin + lengte - 1))
                .send()
                .await?;
            if !antwoord.status().is_success() {
                let status = antwoord.status().as_u16();
                return Err(Fout::Server { status, tekst: antwoord.text().await.unwrap_or_default() });
            }
            antwoord.bytes().await?.to_vec()
        } else {
            lees_stuk(pad, begin, lengte).await?
        };
        if stuk.len() as u64 != lengte {
            return Err(Fout::Controle(format!("blok {index} heeft {} bytes, verwacht {lengte}", stuk.len())));
        }
        uit.write_all(&stuk).await?;
    }
    uit.flush().await?;
    drop(uit);

    if let Some(verwacht) = &info.hashes {
        if &hash_bestand(&tijdelijk, blok).await? != verwacht {
            tokio::fs::remove_file(&tijdelijk).await.ok();
            return Err(Fout::Controle("blokhashes van de download kloppen niet".into()));
        }
    }
    tokio::fs::rename(&tijdelijk, pad).await?;
    Ok(())
}
