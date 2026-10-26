//! Overdrachten: bestanden versturen met een downloadlink (en eventueel een e-mail aan de ontvangers), ook bestanden
//! die niet in een ruimte staan. Client voor `openaec_storage.overdracht.*` op de server (zie de README van de server,
//! "Overdrachten").
//!
//! Verloop: [`Client::maak_overdracht`] geeft een concept; per bestand [`verstuur_bestand`] (`bestand_start`, de
//! delen met een PUT naar hun ondertekende URL, `bestand_klaar`); tot slot [`Client::voltooi_overdracht`], dat de
//! link maakt en de ontvangers mailt. Valt de verbinding weg, dan gaat [`verstuur_bestand`] met het id van het bestand
//! verder waar hij was: `bestand_stand` zegt welke delen de opslag al heeft en geeft nieuwe links voor de rest.
//!
//! Een naam op de server kan geen '/' bevatten; een map gaat daarom mee als zip ([`pak_map`]), zonder compressie,
//! net als "Alles downloaden" op de ontvangstpagina.

use std::io::SeekFrom;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::api::{Client, Deel};
use crate::motor::Voortgang;
use crate::{plek, Fout, Rem, MAX_GELIJKTIJDIG};

const OVERDRACHT: &str = "/api/method/openaec_storage.overdracht.";

/// Zo vaak probeert de app een deel opnieuw na een netwerkfout of een tijdelijke fout van de opslag.
const POGINGEN: u32 = 4;

/// Grenzen van de server voor een overdracht (`grenzen`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grenzen {
    /// Alle bestanden samen, in bytes (GB = 10^9).
    pub max_bytes: u64,
    pub standaard_dagen: u32,
    pub max_dagen: u32,
    pub max_ontvangers: usize,
    #[serde(default)]
    pub concept_uren: u32,
    #[serde(default)]
    pub zip_max_bytes: u64,
    /// Opslaggebruik van de omgeving (`gebruikt_bytes`, `limiet_bytes`, `overdrachten_bytes`).
    #[serde(default)]
    pub quotum: Option<Value>,
}

/// Wat er bij het maken van een overdracht meegaat; leeg = niet ingevuld.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NieuweOverdracht {
    pub naam: String,
    #[serde(default)]
    pub bericht: Option<String>,
    /// `JJJJ-MM-DD` (tot het eind van die dag); None = de standaardtermijn van de server.
    #[serde(default)]
    pub verloopt_op: Option<String>,
    #[serde(default)]
    pub wachtwoord: Option<String>,
    #[serde(default)]
    pub ontvangers: Vec<String>,
}

/// Antwoord van `maak`: een concept-overdracht.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Concept {
    pub overdracht: String,
    pub naam: String,
    pub status: String,
    #[serde(default)]
    pub verloopt_op: Option<String>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
    #[serde(default)]
    pub concept_geldig_tot: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BestandStart {
    pub bestand: String,
    pub naam: String,
    pub grootte: u64,
    pub deelgrootte: u64,
    pub delen: Vec<Deel>,
}

/// Een deel dat de opslag al volledig heeft.
#[derive(Debug, Clone, Deserialize)]
pub struct AanwezigDeel {
    pub nummer: u64,
    #[serde(default)]
    pub grootte: u64,
    #[serde(default)]
    pub etag: Option<String>,
}

/// Antwoord van `bestand_stand`: wat er al is en nieuwe links voor wat ontbreekt.
#[derive(Debug, Deserialize)]
pub struct BestandStand {
    pub bestand: String,
    pub naam: String,
    pub grootte: u64,
    pub klaar: bool,
    #[serde(default)]
    pub deelgrootte: u64,
    #[serde(default)]
    pub aanwezig: Vec<AanwezigDeel>,
    #[serde(default)]
    pub ontbrekend: Vec<u64>,
    #[serde(default)]
    pub delen: Vec<Deel>,
}

/// Eén bestand van een overdracht (`bestand_klaar`, en de bestanden in `info`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverdrachtBestand {
    pub bestand: String,
    pub naam: String,
    #[serde(default)]
    pub grootte: u64,
    #[serde(default)]
    pub klaar: bool,
    #[serde(default)]
    pub aantal_downloads: u64,
}

/// Antwoord van `voltooi`: de link en naar wie de mail in de wachtrij staat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voltooid {
    pub overdracht: String,
    pub status: String,
    #[serde(default)]
    pub token: Option<String>,
    pub url: String,
    #[serde(default)]
    pub verloopt_op: Option<String>,
    #[serde(default)]
    pub aantal_bestanden: u64,
    #[serde(default)]
    pub totale_grootte: u64,
    #[serde(default)]
    pub mail: Vec<String>,
}

/// Eén eigen overdracht met zijn bestanden (`info`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverdrachtInfo {
    pub overdracht: String,
    pub naam: String,
    pub status: String,
    #[serde(default)]
    pub bericht: Option<String>,
    #[serde(default)]
    pub ontvangers: Vec<String>,
    #[serde(default)]
    pub verloopt_op: Option<String>,
    #[serde(default)]
    pub beveiligd: bool,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub totale_grootte: u64,
    #[serde(default)]
    pub aantal_downloads: u64,
    #[serde(default)]
    pub concept_geldig_tot: Option<String>,
    #[serde(default)]
    pub bestanden: Vec<OverdrachtBestand>,
}

/// Een eigen overdracht in de lijst (`mijn`), nieuwste eerst; `url` alleen als hij actief is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverdrachtRegel {
    pub overdracht: String,
    pub naam: String,
    /// `Concept`, `Actief`, `Verlopen` of `Ingetrokken`.
    pub status: String,
    #[serde(default)]
    pub verloopt_op: Option<String>,
    #[serde(default)]
    pub totale_grootte: u64,
    #[serde(default)]
    pub aantal_bestanden: u64,
    #[serde(default)]
    pub aantal_downloads: u64,
    #[serde(default)]
    pub laatst_gedownload: Option<String>,
    #[serde(default)]
    pub ontvangers: Vec<String>,
    #[serde(default)]
    pub beveiligd: bool,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub gemaakt: Option<String>,
    #[serde(default)]
    pub voltooid_op: Option<String>,
}

/// Lege tekst telt als niet ingevuld (een oudere server kan een lege vervaldatum niet lezen).
fn ingevuld(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|t| !t.is_empty())
}

impl Client {
    async fn overdracht<T: serde::de::DeserializeOwned>(&self, methode: &str, gegevens: Value) -> Result<T, Fout> {
        self.roep_op(OVERDRACHT, methode, gegevens).await
    }

    /// De grenzen voor het venster (grootste overdracht, vervaltermijn, aantal ontvangers) en het opslaggebruik. Een
    /// 403 betekent: deze gebruiker mag geen bestanden versturen.
    pub async fn overdracht_grenzen(&self) -> Result<Grenzen, Fout> {
        self.overdracht("grenzen", json!({})).await
    }

    /// Een nieuwe overdracht (concept); daarna per bestand [`verstuur_bestand`] en tot slot [`Client::voltooi_overdracht`].
    pub async fn maak_overdracht(&self, nieuw: &NieuweOverdracht) -> Result<Concept, Fout> {
        let ontvangers: Vec<&str> = nieuw.ontvangers.iter().map(|o| o.trim()).filter(|o| !o.is_empty()).collect();
        self.overdracht(
            "maak",
            json!({"naam": nieuw.naam.trim(), "bericht": ingevuld(&nieuw.bericht), "verloopt_op": ingevuld(&nieuw.verloopt_op),
                   "wachtwoord": nieuw.wachtwoord.as_deref().filter(|w| !w.is_empty()), "ontvangers": ontvangers}),
        )
        .await
    }

    /// Een bestand aankondigen: per deel een ondertekende upload-URL. `naam` krijgt " (2)" als hij al in de overdracht
    /// staat.
    pub async fn overdracht_bestand_start(&self, overdracht: &str, naam: &str, grootte: u64, sha256: Option<&str>) -> Result<BestandStart, Fout> {
        self.overdracht("bestand_start", json!({"overdracht": overdracht, "naam": naam, "grootte": grootte, "sha256": sha256})).await
    }

    /// Hervatten: welke delen de opslag al heeft, en nieuwe links voor de rest.
    pub async fn overdracht_bestand_stand(&self, overdracht: &str, bestand: &str) -> Result<BestandStand, Fout> {
        self.overdracht("bestand_stand", json!({"overdracht": overdracht, "bestand": bestand})).await
    }

    /// De upload van één bestand afronden. Zonder ETags: de server vraagt de delen zelf aan de opslag (ook na een
    /// hervatte upload). Nog eens aanroepen op een afgerond bestand kan geen kwaad.
    pub async fn overdracht_bestand_klaar(&self, overdracht: &str, bestand: &str) -> Result<OverdrachtBestand, Fout> {
        self.overdracht("bestand_klaar", json!({"overdracht": overdracht, "bestand": bestand})).await
    }

    /// Een bestand uit een concept halen (bijv. omdat het lokaal veranderd is sinds de onderbreking).
    pub async fn overdracht_bestand_weg(&self, overdracht: &str, bestand: &str) -> Result<(), Fout> {
        self.roep_leeg_op(OVERDRACHT, "bestand_weg", json!({"overdracht": overdracht, "bestand": bestand})).await
    }

    /// De link maken en de ontvangers mailen; kan pas als elk bestand afgerond is.
    pub async fn voltooi_overdracht(&self, overdracht: &str) -> Result<Voltooid, Fout> {
        self.overdracht("voltooi", json!({"overdracht": overdracht})).await
    }

    /// Eén eigen overdracht met zijn bestanden (om een onderbroken overdracht verder te zetten).
    pub async fn overdracht_info(&self, overdracht: &str) -> Result<OverdrachtInfo, Fout> {
        self.overdracht("info", json!({"overdracht": overdracht})).await
    }

    /// De eigen overdrachten, nieuwste eerst.
    pub async fn mijn_overdrachten(&self) -> Result<Vec<OverdrachtRegel>, Fout> {
        self.overdracht("mijn", json!({})).await
    }

    /// Intrekken (ook een concept): de link werkt niet meer en de bestanden gaan meteen weg. Geeft de nieuwe status.
    pub async fn trek_overdracht_in(&self, overdracht: &str) -> Result<String, Fout> {
        let r: Value = self.overdracht("trek_in", json!({"overdracht": overdracht})).await?;
        Ok(r["status"].as_str().unwrap_or("Ingetrokken").to_string())
    }
}

/// SHA-256 van een bestand (hex), in stukken gelezen.
pub async fn sha256_bestand(pad: &Path) -> Result<String, Fout> {
    let mut bestand = tokio::fs::File::open(pad).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = bestand.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Begin en lengte van deel `nummer` (vanaf 1) bij deze deelgrootte.
pub fn deelbereik(nummer: u64, deelgrootte: u64, grootte: u64) -> (u64, u64) {
    let begin = (nummer.max(1) - 1) * deelgrootte;
    (begin, deelgrootte.min(grootte.saturating_sub(begin)))
}

/// Verloopt een ondertekende link (of weigert de opslag hem), dan helpt opnieuw proberen met dezelfde link niet: dan
/// zijn er nieuwe links nodig uit `bestand_stand`.
fn link_verlopen(f: &Fout) -> bool {
    matches!(f.status(), Some(400 | 401 | 403))
}

/// Netwerkfouten en tijdelijke fouten van de opslag (5xx, 408, 429) zijn het opnieuw proberen waard.
fn tijdelijk(f: &Fout) -> bool {
    match f {
        Fout::Netwerk(_) => true,
        Fout::Server { status, .. } | Fout::Link { status, .. } => matches!(status, 408 | 429 | 500..=599),
        _ => false,
    }
}

async fn lees_stuk(pad: &Path, begin: u64, lengte: u64) -> std::io::Result<Vec<u8>> {
    let mut bestand = tokio::fs::File::open(pad).await?;
    bestand.seek(SeekFrom::Start(begin)).await?;
    let mut buffer = vec![0u8; lengte as usize];
    bestand.read_exact(&mut buffer).await?;
    Ok(buffer)
}

/// Eén deel naar zijn ondertekende URL, met `Content-Length` (ook 0 bij een leeg bestand: zonder die header weigert
/// de opslag een PUT met 411). Probeert het een paar keer opnieuw bij een tijdelijke fout.
async fn put_deel(client: &Client, url: &str, pad: &Path, begin: u64, lengte: u64, rem: &Rem) -> Result<u64, Fout> {
    let _plek = plek(rem).await?;
    let stuk = lees_stuk(pad, begin, lengte).await?;
    let mut poging = 0;
    loop {
        poging += 1;
        let resultaat = async {
            let antwoord = client
                .http
                .put(url)
                .header(reqwest::header::CONTENT_LENGTH, stuk.len())
                .body(stuk.clone())
                .send()
                .await?;
            if !antwoord.status().is_success() {
                let status = antwoord.status().as_u16();
                return Err(Fout::Link { status, tekst: antwoord.text().await.unwrap_or_default() });
            }
            Ok(lengte)
        }
        .await;
        match resultaat {
            Err(f) if poging < POGINGEN && tijdelijk(&f) => {
                tokio::time::sleep(std::time::Duration::from_millis(500 * 2u64.pow(poging - 1))).await;
            }
            r => return r,
        }
    }
}

/// Upload een lokaal bestand in een concept-overdracht en rond het af. Met `eerder` (het id van dit bestand in de
/// overdracht, van een onderbroken poging) gaat hij verder waar hij was: alleen de ontbrekende delen gaan nog over de
/// lijn. `bij_start` krijgt het id zodra het bekend is (om te bewaren voor hervatten); `voortgang` krijgt (gedaan,
/// totaal) in bytes van dit bestand. De delen gaan parallel, elk met een plek uit `rem`.
///
/// Is het bestand in de overdracht een andere grootte dan het lokale bestand, dan geeft dit een
/// [`Fout::Controle`]: haal het dan weg ([`Client::overdracht_bestand_weg`]) en begin opnieuw zonder `eerder`.
#[allow(clippy::too_many_arguments)]
pub async fn verstuur_bestand(
    client: &Client,
    overdracht: &str,
    pad: &Path,
    naam: &str,
    eerder: Option<&str>,
    rem: &Rem,
    bij_start: &(dyn Fn(&str) + Send + Sync),
    voortgang: Voortgang<'_>,
) -> Result<OverdrachtBestand, Fout> {
    let grootte = tokio::fs::metadata(pad).await?.len();
    let (bestand, deelgrootte, mut delen, al) = match eerder {
        Some(id) => {
            let stand = client.overdracht_bestand_stand(overdracht, id).await?;
            if stand.grootte != grootte {
                return Err(Fout::Controle(format!("{naam} is veranderd sinds de vorige poging")));
            }
            if stand.klaar {
                voortgang(grootte, grootte);
                return Ok(OverdrachtBestand { bestand: stand.bestand, naam: stand.naam, grootte, klaar: true, aantal_downloads: 0 });
            }
            let al: u64 = stand.aanwezig.iter().map(|d| d.grootte).sum();
            (stand.bestand, stand.deelgrootte, stand.delen, al)
        }
        None => {
            let sha = sha256_bestand(pad).await?;
            let start = client.overdracht_bestand_start(overdracht, naam, grootte, Some(&sha)).await?;
            bij_start(&start.bestand);
            (start.bestand, start.deelgrootte, start.delen, 0)
        }
    };
    if deelgrootte == 0 && grootte > 0 {
        return Err(Fout::Controle("de server gaf geen deelgrootte".into()));
    }

    let gedaan = AtomicU64::new(al.min(grootte));
    voortgang(gedaan.load(Ordering::SeqCst), grootte);
    // Ronden: verloopt een link (na 15 minuten), dan geeft `bestand_stand` nieuwe voor de delen die nog ontbreken.
    for ronde in 0..3 {
        let taken: Vec<_> = delen
            .iter()
            .map(|deel| {
                let (begin, lengte) = deelbereik(deel.nummer, deelgrootte, grootte);
                let gedaan = &gedaan;
                async move {
                    let n = put_deel(client, &deel.url, pad, begin, lengte, rem).await?;
                    let nu = gedaan.fetch_add(n, Ordering::SeqCst) + n;
                    voortgang(nu.min(grootte), grootte);
                    Ok::<_, Fout>(())
                }
            })
            .collect();
        let resultaten: Vec<Result<(), Fout>> = stream::iter(taken).buffer_unordered(MAX_GELIJKTIJDIG).collect().await;
        let fouten: Vec<Fout> = resultaten.into_iter().filter_map(Result::err).collect();
        if fouten.is_empty() {
            break;
        }
        if ronde == 2 || !fouten.iter().all(link_verlopen) {
            return Err(fouten.into_iter().find(|f| !link_verlopen(f)).unwrap_or_else(|| Fout::Controle("de uploadlinks werken niet".into())));
        }
        let stand = client.overdracht_bestand_stand(overdracht, &bestand).await?;
        gedaan.store(stand.aanwezig.iter().map(|d| d.grootte).sum::<u64>().min(grootte), Ordering::SeqCst);
        delen = stand.delen;
    }
    let klaar = client.overdracht_bestand_klaar(overdracht, &bestand).await?;
    voortgang(grootte, grootte);
    Ok(klaar)
}

/// Datum en tijd voor een zip (UTC; zip kent geen tijdzone), uit een tijdstip.
fn zipdatum(t: std::time::SystemTime) -> zip::DateTime {
    let sec = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (dagen, rest) = (sec.div_euclid(86_400), sec.rem_euclid(86_400));
    // Van dagen sinds 1970 naar jaar-maand-dag (gregoriaans).
    let z = dagen + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dag = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let maand = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let jaar = (yoe + era * 400 + i64::from(maand <= 2)) as u16;
    zip::DateTime::from_date_and_time(jaar, maand, dag, (rest / 3600) as u8, (rest % 3600 / 60) as u8, (rest % 60) as u8)
        .unwrap_or_default()
}

/// Wat er in een map zit: aantal bestanden en bytes (alles eronder; snelkoppelingen naar elders tellen niet mee).
pub fn mapomvang(map: &Path) -> std::io::Result<(u64, u64)> {
    let (mut aantal, mut bytes) = (0u64, 0u64);
    let mut te_doen = vec![map.to_path_buf()];
    while let Some(m) = te_doen.pop() {
        for item in std::fs::read_dir(&m)? {
            let item = item?;
            let soort = item.file_type()?;
            if soort.is_dir() {
                te_doen.push(item.path());
            } else if soort.is_file() {
                aantal += 1;
                bytes += item.metadata()?.len();
            }
        }
    }
    Ok((aantal, bytes))
}

/// Pak een map met alles erin in als zip in `doel`, zonder compressie: bovenaan de map zelf, daaronder de bestanden met
/// hun pad (ook lege mappen). Namen die op Windows niet kunnen, krijgen een veilige naam. Geeft het aantal bestanden.
pub fn pak_map(map: &Path, doel: &Path) -> std::io::Result<u64> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let wortel = crate::namen::veilige_naam(&map.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "map".into()));
    let tijdelijk = doel.with_extension("zip-bezig");
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(std::fs::File::create(&tijdelijk)?));
    let opties = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).unix_permissions(0o644);
    let fout = |e: zip::result::ZipError| std::io::Error::other(e.to_string());

    let mut aantal = 0u64;
    let mut te_doen = vec![(map.to_path_buf(), wortel.clone())];
    while let Some((m, naam)) = te_doen.pop() {
        let mut items: Vec<_> = std::fs::read_dir(&m)?.collect::<Result<_, _>>()?;
        items.sort_by_key(|i| i.file_name());
        let tijd = std::fs::metadata(&m).and_then(|md| md.modified()).map(zipdatum).unwrap_or_default();
        zip.add_directory(format!("{naam}/"), opties.last_modified_time(tijd).unix_permissions(0o755)).map_err(fout)?;
        for item in items {
            let soort = item.file_type()?;
            let pad = format!("{naam}/{}", crate::namen::veilige_naam(&item.file_name().to_string_lossy()));
            if soort.is_dir() {
                te_doen.push((item.path(), pad));
            } else if soort.is_file() {
                let md = item.metadata()?;
                let tijd = md.modified().map(zipdatum).unwrap_or_default();
                zip.start_file(&pad, opties.last_modified_time(tijd).large_file(md.len() >= u32::MAX as u64)).map_err(fout)?;
                std::io::copy(&mut std::fs::File::open(item.path())?, &mut zip)?;
                aantal += 1;
            }
        }
    }
    zip.finish().map_err(fout)?.flush()?;
    std::fs::rename(&tijdelijk, doel)?;
    Ok(aantal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delen_van_een_bestand() {
        let mib = 8 * 1024 * 1024;
        assert_eq!(deelbereik(1, mib, 0), (0, 0));
        assert_eq!(deelbereik(1, mib, 10), (0, 10));
        assert_eq!(deelbereik(2, mib, mib + 5), (mib, 5));
        assert_eq!(deelbereik(3, mib, mib + 5), (2 * mib, 0));
    }

    #[test]
    fn datum_in_een_zip() {
        // 2026-10-26 13:45:30 UTC
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_793_022_330);
        let d = zipdatum(t);
        assert_eq!((d.year(), d.month(), d.day(), d.hour(), d.minute(), d.second()), (2026, 10, 26, 13, 45, 30));
        let schrikkel = std::time::UNIX_EPOCH + std::time::Duration::from_secs(951_782_400); // 2000-02-29
        assert_eq!((zipdatum(schrikkel).month(), zipdatum(schrikkel).day()), (2, 29));
    }

    #[test]
    fn welke_fouten_opnieuw() {
        assert!(tijdelijk(&Fout::Server { status: 503, tekst: String::new() }));
        assert!(!tijdelijk(&Fout::Server { status: 403, tekst: String::new() }));
        assert!(link_verlopen(&Fout::Server { status: 403, tekst: String::new() }));
        assert!(link_verlopen(&Fout::Link { status: 403, tekst: String::new() }));
        assert!(tijdelijk(&Fout::Link { status: 503, tekst: String::new() }));
        assert!(!link_verlopen(&Fout::Controle("x".into())));
    }
}
