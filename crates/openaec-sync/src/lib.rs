//! Kern van de OpenAEC Storage-sync (ontwerp `2026-10-20-openaec-storage-design.md`, §5 en §8):
//! API-client, blokhashes en delta-upload/-download. Geen UI; de Tauri-app gebruikt deze crate,
//! en later ook de WebDAV-gateway en een macOS/Linux-client.

pub mod api;
pub mod bewaking;
pub mod blokken;
pub mod importeer;
pub mod motor;
pub mod namen;
pub mod oauth;
pub mod opslag;
pub mod selectie;
pub mod voortgang;

use std::io::SeekFrom;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::stream::{self, StreamExt};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

pub use api::{Client, UploadResultaat};
use motor::Voortgang;
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
    /// De server weigerde één bestand uit een batch; `foutcode` is de vaste reden (zie de README van de server).
    #[error("geweigerd: {melding}")]
    Geweigerd { foutcode: Option<String>, melding: String },
}

/// Foutcodes van de server waarna een gedeelde map niet meer te syncen is (zie [`Fout::deling_gestopt`]).
pub const DELING_GESTOPT: [&str; 4] = ["deling_ingetrokken", "deling_verlopen", "deling_weg", "deling_onbekend"];

/// Standaard aantal overdrachten tegelijk (bestanden, en delen van grote bestanden samen).
pub const STANDAARD_GELIJKTIJDIG: usize = 8;
/// Hoogste instelbare aantal overdrachten tegelijk.
pub const MAX_GELIJKTIJDIG: usize = 32;

/// Begrensd aantal verzoeken tegelijk naar de opslag: elk deel van een upload en elk blok van een download neemt
/// één plek, zodat de delen van een groot bestand binnen dezelfde limiet blijven als de kleine bestanden.
pub type Rem = tokio::sync::Semaphore;

/// Een rem met `n` plekken (begrensd op 1..=[`MAX_GELIJKTIJDIG`]).
pub fn rem(n: usize) -> Rem {
    Rem::new(n.clamp(1, MAX_GELIJKTIJDIG))
}

async fn plek(rem: &Rem) -> Result<tokio::sync::SemaphorePermit<'_>, Fout> {
    rem.acquire().await.map_err(|_| Fout::Controle("de overdracht is gestopt".into()))
}

impl Fout {
    /// HTTP-status van een weigering door de server.
    pub fn status(&self) -> Option<u16> {
        match self {
            Fout::Server { status, .. } => Some(*status),
            _ => None,
        }
    }

    fn antwoord(&self) -> Option<serde_json::Value> {
        match self {
            Fout::Server { tekst, .. } => serde_json::from_str(tekst).ok(),
            _ => None,
        }
    }

    /// Vaste foutcode van de server (`foutcode` naast de melding), bijv. `ruimte_gearchiveerd`; bij een bestand uit
    /// een batch die van dat bestand.
    pub fn foutcode(&self) -> Option<String> {
        match self {
            Fout::Geweigerd { foutcode, .. } => foutcode.clone(),
            _ => self.antwoord()?.get("foutcode")?.as_str().map(str::to_string),
        }
    }

    /// Waarom een gedeelde map niet meer te syncen is: de deling is ingetrokken, verlopen, weg (de map is verwijderd)
    /// of onbekend. Geeft de foutcode van de server, of None bij elke andere fout (bijv. geen verbinding).
    pub fn deling_gestopt(&self) -> Option<String> {
        self.foutcode().filter(|c| DELING_GESTOPT.contains(&c.as_str()))
    }

    /// De melding van de server voor mensen: Frappe zet die in `_server_messages` (een JSON-lijst van JSON-teksten),
    /// anders in `exception` (`frappe.exceptions.X: melding`).
    pub fn servermelding(&self) -> Option<String> {
        if let Fout::Geweigerd { melding, .. } = self {
            return Some(melding.clone());
        }
        let antwoord = self.antwoord()?;
        let berichten = antwoord.get("_server_messages").and_then(|v| v.as_str()).and_then(|t| serde_json::from_str::<Vec<String>>(t).ok());
        let eerste = berichten.into_iter().flatten().find_map(|b| {
            let v: serde_json::Value = serde_json::from_str(&b).ok()?;
            v.get("message").and_then(|m| m.as_str()).map(str::to_string)
        });
        eerste.or_else(|| {
            let e = antwoord.get("exception")?.as_str()?;
            Some(e.split_once(": ").map_or(e, |(_, m)| m).to_string())
        })
    }
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
    upload_met_voortgang(client, ruimte, map, naam, pad, basis_versie, &|_, _| {}).await
}

/// Als [`upload_bestand`]; `voortgang` krijgt (overgezet, totaal) in bytes, voor de delen die echt over de lijn
/// gaan: eerst (0, totaal) zodra bekend is welke blokken de server hergebruikt, daarna na elk deel. Het verschil
/// tussen de bestandsgrootte en `totaal` is wat blokhergebruik bespaart. De delen gaan parallel
/// ([`STANDAARD_GELIJKTIJDIG`] tegelijk).
pub async fn upload_met_voortgang(
    client: &Client,
    ruimte: &str,
    map: Option<&str>,
    naam: &str,
    pad: &Path,
    basis_versie: Option<u64>,
    voortgang: Voortgang<'_>,
) -> Result<UploadResultaat, Fout> {
    let rem = rem(STANDAARD_GELIJKTIJDIG);
    upload_met_rem(client, ruimte, map, naam, pad, basis_versie, voortgang, &rem).await
}

/// Als [`upload_met_voortgang`], met een gedeelde [`Rem`]: elk deel wacht op een vrije plek.
#[allow(clippy::too_many_arguments)]
pub async fn upload_met_rem(
    client: &Client,
    ruimte: &str,
    map: Option<&str>,
    naam: &str,
    pad: &Path,
    basis_versie: Option<u64>,
    voortgang: Voortgang<'_>,
    rem: &Rem,
) -> Result<UploadResultaat, Fout> {
    let grootte = tokio::fs::metadata(pad).await?.len();
    let blok = blokgrootte(grootte);
    let hashes = hash_bestand(pad, blok).await?;
    let start = client
        .upload_start(ruimte, map, naam, grootte, basis_versie, &hashes, "sync")
        .await?;

    let lengte_van = |nummer: u64| {
        let begin = (nummer - 1) * start.deelgrootte;
        (begin, start.deelgrootte.min(grootte.saturating_sub(begin)))
    };
    let totaal: u64 = start.delen.iter().map(|d| lengte_van(d.nummer).1).sum();
    voortgang(0, totaal);
    // De futures eerst in een lijst (in plaats van een closure in de stroom): zo blijft de hele upload `Send`.
    let delen: Vec<_> = start
        .delen
        .iter()
        .map(|deel| {
            let (begin, lengte) = lengte_van(deel.nummer);
            upload_deel(client, &deel.url, deel.nummer, pad, begin, lengte, rem)
        })
        .collect();
    let mut stroom = stream::iter(delen).buffer_unordered(MAX_GELIJKTIJDIG);
    let (mut gedaan, mut etags) = (0u64, Vec::with_capacity(start.delen.len()));
    while let Some(deel) = stroom.next().await {
        let (etag, lengte) = deel?;
        gedaan += lengte;
        voortgang(gedaan, totaal);
        etags.push(etag);
    }
    client.upload_klaar(&start.upload, &etags).await
}

/// Eén deel van een upload naar zijn ondertekende URL; geeft de ETag en het aantal bytes.
async fn upload_deel(
    client: &Client,
    url: &str,
    nummer: u64,
    pad: &Path,
    begin: u64,
    lengte: u64,
    rem: &Rem,
) -> Result<(Etag, u64), Fout> {
    let _plek = plek(rem).await?;
    let stuk = lees_stuk(pad, begin, lengte).await?;
    // De opslag weigert een PUT zonder `Content-Length` (411, MissingContentLength). Bij een leeg lichaam laat de
    // HTTP-laag die kop weg; daarom altijd zelf zetten, ook bij 0 bytes (een leeg bestand).
    let antwoord = client
        .http
        .put(url)
        .header(reqwest::header::CONTENT_LENGTH, stuk.len())
        .body(stuk)
        .send()
        .await?;
    if !antwoord.status().is_success() {
        let status = antwoord.status().as_u16();
        return Err(Fout::Server { status, tekst: antwoord.text().await.unwrap_or_default() });
    }
    let etag = antwoord
        .headers()
        .get("etag")
        .and_then(|w| w.to_str().ok())
        .ok_or_else(|| Fout::Controle(format!("geen ETag voor deel {nummer}")))?
        .to_string();
    Ok((Etag { nummer, etag }, lengte))
}

/// Download de huidige versie naar `pad`. Bestaat het bestand al, dan worden alleen de blokken opgehaald die
/// lokaal anders zijn; de rest komt uit het lokale bestand. Het resultaat wordt gecontroleerd en pas daarna
/// atomair op de plek van het oude bestand gezet.
pub async fn download_bestand(client: &Client, bestand: &str, pad: &Path) -> Result<(), Fout> {
    download_met_voortgang(client, bestand, pad, &|_, _| {}).await
}

/// Als [`download_bestand`]; `voortgang` krijgt (overgezet, totaal) in bytes, voor de blokken die echt over de lijn
/// gaan: eerst (0, totaal) zodra bekend is welke blokken lokaal al kloppen, daarna tijdens het ophalen. Blokken
/// komen parallel binnen ([`STANDAARD_GELIJKTIJDIG`] tegelijk) en worden op volgorde weggeschreven.
pub async fn download_met_voortgang(client: &Client, bestand: &str, pad: &Path, voortgang: Voortgang<'_>) -> Result<(), Fout> {
    let rem = rem(STANDAARD_GELIJKTIJDIG);
    download_met_rem(client, bestand, pad, voortgang, &rem).await
}

/// Als [`download_met_voortgang`], met een gedeelde [`Rem`]: elk blok dat over de lijn moet, wacht op een vrije plek.
pub async fn download_met_rem(client: &Client, bestand: &str, pad: &Path, voortgang: Voortgang<'_>, rem: &Rem) -> Result<(), Fout> {
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

    let totaal: u64 = ophalen.iter().map(|&i| blok.min(info.grootte.saturating_sub(i as u64 * blok))).sum();
    let gedaan = AtomicU64::new(0);
    voortgang(0, totaal);
    let blokken: Vec<_> = (0..aantal)
        .map(|index| {
            let begin = index as u64 * blok;
            let lengte = blok.min(info.grootte.saturating_sub(begin));
            let bron = if ophalen.contains(&index) { Some(link.url.as_str()) } else { None };
            download_blok(client, bron, pad, index, begin, lengte, rem, &gedaan, totaal, voortgang)
        })
        .collect();
    // Op volgorde: zo kan elk blok direct achter het vorige in het tijdelijke bestand.
    let mut blokken = stream::iter(blokken).buffered(MAX_GELIJKTIJDIG);

    let tijdelijk = pad.with_extension("openaec-download");
    let geschreven = async {
        let mut uit = tokio::fs::File::create(&tijdelijk).await?;
        while let Some(stuk) = blokken.next().await {
            uit.write_all(&stuk?).await?;
        }
        uit.flush().await?;
        Ok::<_, Fout>(())
    }
    .await;
    if let Err(fout) = geschreven {
        tokio::fs::remove_file(&tijdelijk).await.ok();
        return Err(fout);
    }

    if let Some(verwacht) = &info.hashes {
        if &hash_bestand(&tijdelijk, blok).await? != verwacht {
            tokio::fs::remove_file(&tijdelijk).await.ok();
            return Err(Fout::Controle("blokhashes van de download kloppen niet".into()));
        }
    }
    tokio::fs::rename(&tijdelijk, pad).await?;
    Ok(())
}

/// Eén blok van een download: met `bron` (de downloadlink) over de lijn, anders uit het lokale bestand `pad`.
#[allow(clippy::too_many_arguments)]
async fn download_blok(
    client: &Client,
    bron: Option<&str>,
    pad: &Path,
    index: usize,
    begin: u64,
    lengte: u64,
    rem: &Rem,
    gedaan: &AtomicU64,
    totaal: u64,
    voortgang: Voortgang<'_>,
) -> Result<Vec<u8>, Fout> {
    if lengte == 0 {
        return Ok(Vec::new());
    }
    let stuk = match bron {
        Some(url) => {
            let _plek = plek(rem).await?;
            let mut antwoord = client
                .http
                .get(url)
                .header("Range", format!("bytes={}-{}", begin, begin + lengte - 1))
                .send()
                .await?;
            if !antwoord.status().is_success() {
                let status = antwoord.status().as_u16();
                return Err(Fout::Server { status, tekst: antwoord.text().await.unwrap_or_default() });
            }
            // In stukjes binnenhalen, zodat de voortgang ook binnen een blok doorloopt.
            let mut stuk = Vec::with_capacity(lengte as usize);
            while let Some(brok) = antwoord.chunk().await? {
                let nu = gedaan.fetch_add(brok.len() as u64, Ordering::SeqCst) + brok.len() as u64;
                stuk.extend_from_slice(&brok);
                voortgang(nu.min(totaal), totaal);
            }
            stuk
        }
        None => lees_stuk(pad, begin, lengte).await?,
    };
    if stuk.len() as u64 != lengte {
        return Err(Fout::Controle(format!("blok {index} heeft {} bytes, verwacht {lengte}", stuk.len())));
    }
    Ok(stuk)
}
