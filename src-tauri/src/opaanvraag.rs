//! De sync-motor voor één koppeling, met wat de Verkenner erbij vraagt. [`Verkenneropslag`] zit tussen de motor en de
//! echte opslag:
//!
//! - **Nieuwe mappen niet meenemen** ([`NieuweMappen::AlleenGekozen`]): bestanden in een map die er bij het kiezen nog
//!   niet was, laat de schil uit de lijst van de server weg (zie `openaec_sync::selectie::nieuw_niet_gekozen`).
//! - **Bestanden op aanvraag** (een ruimte in de hoofdmap, alleen Windows): een download wordt geen volledig bestand
//!   maar een placeholder die alleen online staat, behalve in een vastgezette map ("Altijd op dit apparaat
//!   behouden"). Een nieuwe versie van een bestand dat alleen online staat, past alleen de placeholder aan. Wat lokaal
//!   staat, wordt gewoon gedownload, en dan via de veilige routine van de motor. Kan de placeholder niet (meer), omdat
//!   er net een bestand van de gebruiker verscheen of de gebruiker het bestand ophaalde en wijzigde, dan zegt de schil
//!   dat (`plaats_zelf` geeft None) en gaat de download door die routine; nooit rechtstreeks op het pad.
//!
//! Elke methode van `Opslag` gaat door naar de echte opslag; komt er een methode bij, dan moet hij hier ook door.
//!
//! Daarnaast de ophaler: wat Windows vraagt als een bestand dat alleen online staat geopend wordt ([`ophaler`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use openaec_sync::motor::{bekende_bestanden, Batchgrenzen, KleinUpload, Opslag, OpslagBestand, Upload, Voortgang};
use openaec_sync::selectie::{mappen_boven, nieuw_niet_gekozen, NieuweMappen};
use openaec_sync::Fout;

use placeholder::Placeholders;

pub struct Verkenneropslag<'a, O> {
    binnen: &'a O,
    root: PathBuf,
    /// Een ruimte in de hoofdmap: bestanden op aanvraag, met de Cloud Filter API (alleen Windows).
    placeholders: Option<&'a dyn Placeholders>,
    /// Alleen gekozen mappen: de mappen van de server bij het kiezen.
    alleen: Option<BTreeSet<String>>,
}

/// De echte placeholders: op Windows de Cloud Filter API, elders geen.
#[cfg(windows)]
const ECHT: Option<&dyn Placeholders> = Some(&placeholder::Cloudfilter);
#[cfg(not(windows))]
const ECHT: Option<&dyn Placeholders> = None;

impl<'a, O: Opslag> Verkenneropslag<'a, O> {
    pub fn new(binnen: &'a O, root: &Path, op_aanvraag: bool, nieuwe: NieuweMappen, bekend: &[String]) -> Self {
        let alleen = (nieuwe == NieuweMappen::AlleenGekozen).then(|| bekend.iter().cloned().collect());
        Self { binnen, root: root.to_path_buf(), placeholders: if op_aanvraag { ECHT } else { None }, alleen }
    }

    /// Bestanden op aanvraag met andere placeholders dan de echte (de proeven bootsen de Cloud Filter API na).
    #[cfg(test)]
    fn met_placeholders(mut self, p: &'a dyn Placeholders) -> Self {
        self.placeholders = Some(p);
        self
    }
}

/// Mappen (met '/') die hier bestaan of bestanden in de toestand hebben: die zijn nooit nieuw.
fn mappen_hier(root: &Path) -> BTreeSet<String> {
    bekende_bestanden(root).iter().flat_map(|b| mappen_boven(&b.pad).into_iter().map(str::to_string).collect::<Vec<_>>()).collect()
}

impl<O: Opslag> Opslag for Verkenneropslag<'_, O> {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
        let lijst = self.binnen.lijst().await?;
        let Some(bekend) = &self.alleen else { return Ok(lijst) };
        let in_toestand = mappen_hier(&self.root);
        let hier = |m: &str| in_toestand.contains(m) || m.split('/').fold(self.root.clone(), |p, d| p.join(d)).is_dir();
        Ok(lijst.into_iter().filter(|b| !nieuw_niet_gekozen(&b.pad, bekend, &hier)).collect())
    }

    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout> {
        self.binnen.upload(pad, lokaal, basis_versie).await
    }

    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.binnen.download(bestand, lokaal).await
    }

    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.binnen.verwijder(bestand).await
    }

    /// Een map die alleen te lezen met mij gedeeld is: ook in de hoofdmap gaat er niets naar de server.
    fn alleen_lezen(&self) -> bool {
        self.binnen.alleen_lezen()
    }

    /// Een placeholder die alleen online staat (of wordt), zet de schil zelf neer: daar is geen lokaal werk te
    /// beschermen. Geeft het kenmerk dat de placeholder kreeg. Kan dat niet (meer), dan None: de motor downloadt dan via
    /// zijn veilige routine.
    async fn plaats_zelf(&self, bestand: &OpslagBestand, lokaal: &Path) -> Option<Result<(u64, u128), Fout>> {
        if let Some(r) = self.placeholders.and_then(|p| placeholder::plaats(p, &self.root, bestand, lokaal)) {
            return Some(r);
        }
        self.binnen.plaats_zelf(bestand, lokaal).await
    }

    async fn upload_met_voortgang(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>, voortgang: Voortgang<'_>) -> Result<Upload, Fout> {
        self.binnen.upload_met_voortgang(pad, lokaal, basis_versie, voortgang).await
    }

    async fn download_met_voortgang(&self, bestand: &OpslagBestand, lokaal: &Path, voortgang: Voortgang<'_>) -> Result<(), Fout> {
        self.binnen.download_met_voortgang(bestand, lokaal, voortgang).await
    }

    async fn bereid_mappen(&self, mappen: &[String]) -> Result<(), Fout> {
        self.binnen.bereid_mappen(mappen).await
    }

    async fn klein_grenzen(&self) -> Option<Batchgrenzen> {
        self.binnen.klein_grenzen().await
    }

    async fn upload_klein(&self, bestanden: &[KleinUpload<'_>]) -> Result<Vec<Result<Upload, Fout>>, Fout> {
        self.binnen.upload_klein(bestanden).await
    }

    async fn download_klein(&self, bestanden: &[(&OpslagBestand, &Path)]) -> Result<Vec<Result<(), Fout>>, Fout> {
        self.binnen.download_klein(bestanden).await
    }
}

/// Bestanden op aanvraag: de regels van de schil, los van de Cloud Filter API (die alleen op Windows bestaat), zodat ze
/// ook elders te testen zijn.
mod placeholder {
    use std::path::Path;

    use openaec_sync::motor::OpslagBestand;
    use openaec_sync::Fout;

    // Attributen die de Cloud Files API op bestanden zet; gelijk aan die in `crate::cloudfilter` (daar gecontroleerd).
    pub const OFFLINE: u32 = 0x0000_1000;
    pub const VASTGEZET: u32 = 0x0008_0000;
    pub const LOSGEMAAKT: u32 = 0x0010_0000;
    pub const OPHALEN_BIJ_OPENEN: u32 = 0x0004_0000;
    pub const OPHALEN_BIJ_LEZEN: u32 = 0x0040_0000;

    #[cfg(windows)]
    const _: () = {
        use crate::cloudfilter as cf;
        assert!(OFFLINE == cf::ATTR_OFFLINE && VASTGEZET == cf::ATTR_VASTGEZET && LOSGEMAAKT == cf::ATTR_LOSGEMAAKT);
        assert!(OPHALEN_BIJ_OPENEN == cf::ATTR_OPHALEN_BIJ_OPENEN && OPHALEN_BIJ_LEZEN == cf::ATTR_OPHALEN_BIJ_LEZEN);
    };

    /// Wat de schil van de Cloud Filter API gebruikt.
    pub trait Placeholders: Sync {
        /// Attributen van een pad (zonder op te halen); None als het niet bestaat.
        fn attributen(&self, pad: &Path) -> Option<u32>;
        /// Maak een bestand dat alleen online staat. Weigert als er al iets op `pad` staat. Geeft de grootte en de
        /// wijzigingstijd die het bestand kreeg (zoals de sync-motor ze meet), niet gemeten achteraf.
        fn maak(&self, pad: &Path, grootte: u64, bestand: &str) -> anyhow::Result<(u64, u128)>;
        /// Werk een bestand dat alleen online staat bij naar een nieuwe versie. Weigert als het niet meer in sync is
        /// (opgehaald en gewijzigd). Geeft het kenmerk zoals [`Placeholders::maak`].
        fn werk_bij(&self, pad: &Path, grootte: u64, bestand: &str) -> anyhow::Result<(u64, u128)>;
    }

    /// De echte: de Cloud Filter API.
    #[cfg(windows)]
    pub struct Cloudfilter;

    #[cfg(windows)]
    impl Placeholders for Cloudfilter {
        fn attributen(&self, pad: &Path) -> Option<u32> {
            crate::cloudfilter::attributen(pad)
        }
        fn maak(&self, pad: &Path, grootte: u64, bestand: &str) -> anyhow::Result<(u64, u128)> {
            crate::cloudfilter::maak_alleen_online(pad, grootte, bestand)
        }
        fn werk_bij(&self, pad: &Path, grootte: u64, bestand: &str) -> anyhow::Result<(u64, u128)> {
            crate::cloudfilter::werk_alleen_online_bij(pad, grootte, bestand)
        }
    }

    fn is_ontwaterd(a: u32) -> bool {
        a & (OFFLINE | OPHALEN_BIJ_LEZEN | OPHALEN_BIJ_OPENEN) != 0
    }

    /// Staat er boven `pad` (tot en met `root`) een vastgezette map, en geen losgemaakte die dichterbij is?
    fn vastgezet_boven(p: &dyn Placeholders, root: &Path, pad: &Path) -> bool {
        let mut map = pad.parent();
        while let Some(m) = map.filter(|m| m.starts_with(root)) {
            if let Some(a) = p.attributen(m) {
                if a & VASTGEZET != 0 {
                    return true;
                }
                if a & LOSGEMAAKT != 0 {
                    return false;
                }
            }
            map = m.parent();
        }
        false
    }

    /// Wordt een download naar `lokaal` (met attributen `a`) een placeholder zonder inhoud op deze computer? Een bestand
    /// dat alleen online staat (niet vastgezet), of een nieuw bestand buiten een vastgezette map.
    fn zonder_inhoud(p: &dyn Placeholders, root: &Path, lokaal: &Path, a: Option<u32>) -> bool {
        match a {
            Some(a) => is_ontwaterd(a) && a & VASTGEZET == 0,
            None => !vastgezet_boven(p, root, lokaal),
        }
    }

    fn fout(e: anyhow::Error) -> Fout {
        Fout::Bestand(std::io::Error::other(e.to_string()))
    }

    /// Een download in de hoofdmap: een nieuw bestand wordt een placeholder die alleen online staat (behalve onder een
    /// vastgezette map), een bestand dat alleen online staat krijgt alleen de nieuwe grootte. None: geen placeholder,
    /// ook als de bewerking geweigerd werd omdat er intussen een bestand van de gebruiker staat (of het opgehaald en
    /// gewijzigd is); de motor downloadt dan via zijn veilige routine. Bij succes het kenmerk dat de placeholder kreeg.
    pub fn plaats(p: &dyn Placeholders, root: &Path, bestand: &OpslagBestand, lokaal: &Path) -> Option<Result<(u64, u128), Fout>> {
        let a = p.attributen(lokaal);
        if !zonder_inhoud(p, root, lokaal, a) {
            return None;
        }
        let r = match a {
            Some(_) => p.werk_bij(lokaal, bestand.grootte, &bestand.bestand),
            None => std::fs::create_dir_all(lokaal.parent().unwrap_or(root))
                .map_err(anyhow::Error::from)
                .and_then(|()| p.maak(lokaal, bestand.grootte, &bestand.bestand)),
        };
        match r {
            Ok(k) => Some(Ok(k)),
            Err(e) if !zonder_inhoud(p, root, lokaal, p.attributen(lokaal)) => {
                log::info!("{}: geen placeholder ({e}); gewone download", lokaal.display());
                None
            }
            Err(e) => Some(Err(fout(e))),
        }
    }
}

/// Welk bestand op de server hoort bij een bestand dat alleen online staat: (bestand-id, versie) uit de toestand van de
/// laatste ronde, anders het id dat in de placeholder staat (net aangemaakt in een lopende ronde; dan de huidige
/// versie).
#[cfg(windows)]
pub fn serverbestand(root: &Path, pad: &Path) -> Option<(String, Option<u64>)> {
    let rel = pad.strip_prefix(root).ok()?.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/");
    if let Some((id, versie)) = openaec_sync::motor::bekend_bestand(root, &rel) {
        return Some((id, Some(versie)));
    }
    crate::cloudfilter::bestandsidentiteit(pad).map(|id| (id, None))
}

/// De ophaler voor bestanden op aanvraag: zoek de koppeling bij het pad (`koppelingen` geeft de gekoppelde mappen in
/// de hoofdmap), vraag een downloadlink voor precies die versie en stuur het gevraagde bereik door. Draait op een
/// callback-thread van Windows (geen async runtime), dus blocking. `api` verstuurt het verzoek naar de API met de
/// autorisatie van de sessie (en vernieuwt na een 401 één keer; zie `crate::api_blocking`); de downloadlink zelf gaat
/// zonder Authorization-header.
#[cfg(windows)]
pub fn ophaler<K, A>(basis: impl Fn() -> Option<String> + Send + Sync + 'static, koppelingen: K, api: A) -> crate::cloudfilter::Ophaler
where
    K: Fn() -> Vec<String> + Send + Sync + 'static,
    A: Fn(&str, &dyn Fn(&str) -> reqwest::Result<reqwest::blocking::Response>) -> Result<reqwest::blocking::Response, String>
        + Send
        + Sync
        + 'static,
{
    Box::new(move |pad, begin, lengte, schrijf| {
        use std::io::Read;
        let root = koppelingen().into_iter().map(PathBuf::from).find(|r| pad.starts_with(r)).ok_or("dit bestand hoort bij geen gekoppelde ruimte")?;
        let (bestand, versie) = serverbestand(&root, pad).ok_or("dit bestand is niet bekend op de server")?;
        let basis = basis().ok_or("niet verbonden")?;
        let http = reqwest::blocking::Client::builder().timeout(None).build().map_err(|e| e.to_string())?;
        let mut verzoek = serde_json::json!({ "bestand": bestand });
        if let Some(v) = versie {
            verzoek["versie"] = v.into();
        }
        let url = format!("{basis}/api/method/openaec_storage.api.download");
        let link: serde_json::Value = api(&basis, &|autorisatie| http.post(&url).header("Authorization", autorisatie).json(&verzoek).send())
            .map_err(|e| format!("downloadlink: {e}"))?
            .json()
            .map_err(|e| format!("downloadlink: {e}"))?;
        let url = link["message"]["url"].as_str().ok_or("geen downloadlink van de server")?.to_string();
        if lengte <= 0 {
            return Ok(());
        }
        let mut antwoord = http
            .get(url)
            .header("Range", format!("bytes={}-{}", begin, begin + lengte - 1))
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("ophalen: {e}"))?;
        // Zonder Range-ondersteuning komt het hele bestand: dan het begin overslaan.
        let mut overslaan = if antwoord.status() == reqwest::StatusCode::PARTIAL_CONTENT { 0 } else { begin as u64 };
        let mut nog = lengte as u64;
        let mut buf = vec![0u8; 256 * 1024];
        while nog > 0 {
            let n = antwoord.read(&mut buf).map_err(|e| format!("ophalen: {e}"))?;
            if n == 0 {
                return Err(format!("verbinding gesloten met nog {nog} bytes te gaan"));
            }
            let mut stuk = &buf[..n];
            let weg = overslaan.min(stuk.len() as u64) as usize;
            overslaan -= weg as u64;
            stuk = &stuk[weg..];
            let stuk = &stuk[..stuk.len().min(nog as usize)];
            nog -= stuk.len() as u64;
            if !stuk.is_empty() && !schrijf(stuk) {
                return Err("afgebroken".into());
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Opslag in het geheugen: alleen de lijst en downloads die worden bijgehouden.
    #[derive(Default)]
    struct Proef {
        lijst: Vec<OpslagBestand>,
        gedownload: Mutex<Vec<String>>,
        geupload: Mutex<Vec<String>>,
        alleen_lezen: bool,
    }

    impl Opslag for Proef {
        async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
            Ok(self.lijst.clone())
        }
        fn alleen_lezen(&self) -> bool {
            self.alleen_lezen
        }
        async fn upload(&self, pad: &str, _lokaal: &Path, _basis: Option<u64>) -> Result<Upload, Fout> {
            self.geupload.lock().unwrap().push(pad.to_string());
            Ok(Upload { pad: pad.into(), bestand: "x".into(), versie: 1, conflict: false })
        }
        async fn download(&self, bestand: &OpslagBestand, _lokaal: &Path) -> Result<(), Fout> {
            self.gedownload.lock().unwrap().push(bestand.pad.clone());
            Ok(())
        }
        async fn verwijder(&self, _bestand: &OpslagBestand) -> Result<(), Fout> {
            Ok(())
        }
        async fn download_klein(&self, bestanden: &[(&OpslagBestand, &Path)]) -> Result<Vec<Result<(), Fout>>, Fout> {
            Ok(bestanden.iter().map(|(b, _)| self.gedownload.lock().unwrap().push(b.pad.clone())).map(Ok).collect())
        }
    }

    fn b(pad: &str) -> OpslagBestand {
        OpslagBestand { pad: pad.into(), bestand: format!("id-{pad}"), versie: 1, grootte: 3 }
    }

    /// Een server in het geheugen voor een hele ronde: downloadt rechtstreeks naar het gevraagde pad (met een rename die
    /// een bestaand bestand vervangt, zoals de echte download), en maakt een conflictkopie bij een verouderde basis.
    #[derive(Default)]
    struct Server {
        bestanden: Mutex<std::collections::BTreeMap<String, (u64, Vec<u8>)>>,
        /// Elke versie die ooit op de server kwam (pad, inhoud).
        versies: Mutex<Vec<(String, Vec<u8>)>>,
    }

    impl Server {
        fn zet(&self, pad: &str, inhoud: &[u8]) {
            let mut b = self.bestanden.lock().unwrap();
            let versie = b.get(pad).map_or(1, |x| x.0 + 1);
            b.insert(pad.into(), (versie, inhoud.to_vec()));
            self.versies.lock().unwrap().push((pad.into(), inhoud.to_vec()));
        }
        fn inhoud(&self, pad: &str) -> Option<Vec<u8>> {
            self.bestanden.lock().unwrap().get(pad).map(|x| x.1.clone())
        }
    }

    impl Opslag for Server {
        async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
            let b = self.bestanden.lock().unwrap();
            Ok(b.iter()
                .map(|(pad, (versie, inhoud))| OpslagBestand { pad: pad.clone(), bestand: format!("id-{pad}"), versie: *versie, grootte: inhoud.len() as u64 })
                .collect())
        }
        async fn upload(&self, pad: &str, lokaal: &Path, basis: Option<u64>) -> Result<Upload, Fout> {
            let inhoud = std::fs::read(lokaal)?;
            let huidig = self.bestanden.lock().unwrap().get(pad).map(|x| x.0);
            let doel = match huidig {
                Some(h) if Some(h) != basis => pad.replace(".txt", " (conflict).txt"),
                _ => pad.to_string(),
            };
            self.zet(&doel, &inhoud);
            let versie = self.bestanden.lock().unwrap()[&doel].0;
            Ok(Upload { pad: doel.clone(), bestand: format!("id-{doel}"), versie, conflict: doel != pad })
        }
        async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
            let tijdelijk = lokaal.with_extension("download");
            std::fs::write(&tijdelijk, self.inhoud(&bestand.pad).unwrap())?;
            std::fs::rename(tijdelijk, lokaal)?;
            Ok(())
        }
        async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
            self.bestanden.lock().unwrap().remove(&bestand.pad);
            Ok(())
        }
    }

    type Haak = Box<dyn FnOnce(&NepWolk) + Send>;

    /// De Cloud Filter API nagebootst: attributen per pad (een gewoon bestand zonder vermelding heeft
    /// FILE_ATTRIBUTE_ARCHIVE). `maak` weigert als er al iets staat, `werk_bij` als het bestand niet meer alleen online
    /// is. Een placeholder krijgt de grootte van de versie en "nu" als wijzigingstijd, zoals de echte. `tussendoor` loopt
    /// één keer, direct na de eerste toets van de attributen: zo verschijnt er een bestand van de gebruiker tussen de
    /// toets en de bewerking. `na_bewerking` loopt één keer, direct na een geslaagde bewerking: zo opent en bewaart de
    /// gebruiker het bestand voordat de motor iets doet.
    #[derive(Default)]
    struct NepWolk {
        attributen: Mutex<std::collections::HashMap<PathBuf, u32>>,
        tussendoor: Mutex<Option<Haak>>,
        na_bewerking: Mutex<Option<Haak>>,
    }

    impl NepWolk {
        /// Grootte en wijzigingstijd zetten zoals de Cloud Filter API, en dat kenmerk teruggeven; daarna `na_bewerking`.
        fn zet(&self, pad: &Path, grootte: u64) -> anyhow::Result<(u64, u128)> {
            let f = std::fs::File::options().write(true).open(pad)?;
            f.set_len(grootte)?;
            let nu = std::time::SystemTime::now();
            f.set_modified(nu)?;
            drop(f);
            let k = (grootte, nu.duration_since(std::time::UNIX_EPOCH)?.as_nanos());
            let haak = self.na_bewerking.lock().unwrap().take();
            if let Some(h) = haak {
                h(self);
            }
            Ok(k)
        }
    }

    impl Placeholders for NepWolk {
        fn attributen(&self, pad: &Path) -> Option<u32> {
            let a = std::fs::symlink_metadata(pad)
                .ok()
                .map(|m| self.attributen.lock().unwrap().get(pad).copied().unwrap_or(if m.is_dir() { 0x10 } else { 0x20 }));
            let haak = self.tussendoor.lock().unwrap().take();
            if let Some(h) = haak {
                h(self);
            }
            a
        }
        fn maak(&self, pad: &Path, grootte: u64, _bestand: &str) -> anyhow::Result<(u64, u128)> {
            std::fs::OpenOptions::new().write(true).create_new(true).open(pad)?;
            self.attributen.lock().unwrap().insert(pad.to_path_buf(), placeholder::OFFLINE | placeholder::OPHALEN_BIJ_OPENEN);
            self.zet(pad, grootte)
        }
        fn werk_bij(&self, pad: &Path, grootte: u64, _bestand: &str) -> anyhow::Result<(u64, u128)> {
            let alleen_online = self.attributen.lock().unwrap().get(pad).is_some_and(|a| a & placeholder::OFFLINE != 0);
            if !alleen_online {
                return Err(anyhow::anyhow!("niet in sync"));
            }
            self.zet(pad, grootte)
        }
    }

    /// Een bestand van de gebruiker, met een duidelijk andere wijzigingstijd.
    fn sla_op(pad: &Path, inhoud: &str) {
        std::fs::write(pad, inhoud).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
        std::fs::File::options().write(true).open(pad).unwrap().set_modified(later).unwrap();
    }

    fn proefmap(naam: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openaec-opaanvraag-{naam}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Proef (review Sol, PR #8, punt 2): het pad is leeg als de schil kijkt, maar vóór de placeholder er staat, slaat de
    /// gebruiker daar een bestand op. Dat bestand blijft staan; de download gaat door de veilige routine (die het niet
    /// vervangt), en de volgende ronde maakt er een conflictkopie van.
    #[tokio::test]
    async fn placeholder_wijkt_voor_een_bestand_dat_net_verschijnt() {
        let dir = proefmap("verschijnt");
        let server = Server::default();
        server.zet("nieuw.txt", b"van de server");
        let wolk = NepWolk::default();
        let pad = dir.join("nieuw.txt");
        *wolk.tussendoor.lock().unwrap() = Some(Box::new(move |_| sla_op(&pad, "van de gebruiker")));
        let schil = Verkenneropslag::new(&server, &dir, true, NieuweMappen::Automatisch, &[]).met_placeholders(&wolk);

        openaec_sync::motor::sync_eenmalig(&schil, &dir).await.unwrap();
        assert!(wolk.tussendoor.lock().unwrap().is_none(), "de gebruiker sloeg op tussen de toets en de bewerking");
        assert_eq!(std::fs::read_to_string(dir.join("nieuw.txt")).unwrap(), "van de gebruiker");

        let r = openaec_sync::motor::sync_eenmalig(&schil, &dir).await.unwrap();
        assert_eq!(r.conflicten, 1);
        assert_eq!(server.inhoud("nieuw (conflict).txt").unwrap(), b"van de gebruiker");
        assert_eq!(std::fs::read_to_string(dir.join("nieuw (conflict).txt")).unwrap(), "van de gebruiker");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Proef (review Sol, PR #8, punt 2): het bestand staat alleen online als de schil kijkt, maar vóór het bijwerken
    /// haalt de gebruiker het op en wijzigt het. Dat werk blijft staan en gaat daarna als conflictkopie naar de server.
    #[tokio::test]
    async fn placeholder_wijkt_voor_een_opgehaald_en_gewijzigd_bestand() {
        let dir = proefmap("opgehaald");
        let server = Server::default();
        server.zet("a.txt", b"versie 1");
        let wolk = NepWolk::default();
        let schil = Verkenneropslag::new(&server, &dir, true, NieuweMappen::Automatisch, &[]).met_placeholders(&wolk);
        openaec_sync::motor::sync_eenmalig(&schil, &dir).await.unwrap();
        let pad = dir.join("a.txt");
        assert_eq!(wolk.attributen.lock().unwrap().get(&pad).copied(), Some(placeholder::OFFLINE | placeholder::OPHALEN_BIJ_OPENEN));

        server.zet("a.txt", b"versie 2");
        let p = pad.clone();
        *wolk.tussendoor.lock().unwrap() = Some(Box::new(move |w: &NepWolk| {
            sla_op(&p, "opgehaald + mijn werk");
            w.attributen.lock().unwrap().insert(p.clone(), 0x20);
        }));
        openaec_sync::motor::sync_eenmalig(&schil, &dir).await.unwrap();
        assert!(wolk.tussendoor.lock().unwrap().is_none());
        assert_eq!(std::fs::read_to_string(&pad).unwrap(), "opgehaald + mijn werk");

        let r = openaec_sync::motor::sync_eenmalig(&schil, &dir).await.unwrap();
        assert_eq!(r.conflicten, 1);
        assert_eq!(server.inhoud("a (conflict).txt").unwrap(), b"opgehaald + mijn werk");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Proef (review Sol ronde 2 op bbdfad1, punt 1): de placeholder voor serverinhoud A verschijnt; de gebruiker opent het
    /// bestand en bewaart B voordat de motor iets meet. Daarna schrijft een collega C. B moet als versie op de server
    /// staan voordat C hier komt: het mag niet als "al op de server" gelden.
    #[tokio::test]
    async fn opslaan_na_een_geplaatste_placeholder_gaat_niet_verloren() {
        let dir = proefmap("na-placeholder");
        let server = Server::default();
        server.zet("a.txt", b"A van de server");
        let wolk = NepWolk::default();
        let pad = dir.join("a.txt");
        let p = pad.clone();
        *wolk.na_bewerking.lock().unwrap() = Some(Box::new(move |w: &NepWolk| {
            w.attributen.lock().unwrap().insert(p.clone(), 0x20);
            sla_op(&p, "B van de gebruiker");
        }));
        let schil = Verkenneropslag::new(&server, &dir, true, NieuweMappen::Automatisch, &[]).met_placeholders(&wolk);
        openaec_sync::motor::sync_eenmalig(&schil, &dir).await.unwrap();
        assert!(wolk.na_bewerking.lock().unwrap().is_none(), "de gebruiker bewaarde B na de placeholder");

        openaec_sync::motor::sync_eenmalig(&schil, &dir).await.unwrap();
        server.zet("a.txt", b"C van een collega");
        openaec_sync::motor::sync_eenmalig(&schil, &dir).await.unwrap();

        assert!(
            server.versies.lock().unwrap().iter().any(|(pad, inhoud)| pad == "a.txt" && inhoud == b"B van de gebruiker"),
            "B staat als versie op de server"
        );
        assert_eq!(std::fs::read_to_string(&pad).unwrap(), "C van een collega");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn nieuwe_mappen_alleen_gekozen() {
        let dir = std::env::temp_dir().join(format!("openaec-opaanvraag-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Eigen")).unwrap();
        let proef = Proef { lijst: vec![b("los.txt"), b("Oud/a.txt"), b("Nieuw/b.txt"), b("Eigen/c.txt"), b("Oud/Sub/d.txt")], ..Default::default() };
        let paden = |l: Vec<OpslagBestand>| l.into_iter().map(|b| b.pad).collect::<Vec<_>>();

        let automatisch = Verkenneropslag::new(&proef, &dir, false, NieuweMappen::Automatisch, &[]);
        assert_eq!(paden(automatisch.lijst().await.unwrap()).len(), 5);

        let gekozen = Verkenneropslag::new(&proef, &dir, false, NieuweMappen::AlleenGekozen, &["Oud".to_string()]);
        // Nieuw en Oud/Sub waren er bij het kiezen niet; Eigen bestaat hier al.
        assert_eq!(paden(gekozen.lijst().await.unwrap()), vec!["los.txt", "Oud/a.txt", "Eigen/c.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn zonder_hoofdmap_gaat_alles_door() {
        let dir = std::env::temp_dir();
        let proef = Proef::default();
        let schil = Verkenneropslag::new(&proef, &dir, false, NieuweMappen::Automatisch, &[]);
        let (x, y) = (b("a.txt"), b("b.txt"));
        schil.download(&x, &dir.join("a.txt")).await.unwrap();
        let r = schil.download_klein(&[(&x, dir.join("a.txt").as_path()), (&y, dir.join("b.txt").as_path())]).await.unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(*proef.gedownload.lock().unwrap(), vec!["a.txt", "a.txt", "b.txt"]);
    }

    #[tokio::test]
    async fn alleen_lezen_gaat_door_en_uploadt_niets() {
        let dir = std::env::temp_dir().join(format!("openaec-opaanvraag-lezen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Map")).unwrap();
        std::fs::write(dir.join("Map").join("nieuw.txt"), "lokaal werk").unwrap();
        let proef = Proef { alleen_lezen: true, ..Default::default() };
        for op_aanvraag in [false, true] {
            let schil = Verkenneropslag::new(&proef, &dir, op_aanvraag, NieuweMappen::Automatisch, &[]);
            assert!(schil.alleen_lezen());
            let r = openaec_sync::motor::sync_eenmalig(&schil, &dir).await.unwrap();
            assert_eq!(r.geupload, 0);
            assert!(r.overgeslagen.iter().any(|o| o.pad == "Map/nieuw.txt"));
        }
        assert!(proef.geupload.lock().unwrap().is_empty());
        let schrijven = Proef::default();
        assert!(!Verkenneropslag::new(&schrijven, &dir, true, NieuweMappen::Automatisch, &[]).alleen_lezen());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
