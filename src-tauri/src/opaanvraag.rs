//! De sync-motor voor één koppeling, met wat de Verkenner erbij vraagt. [`Verkenneropslag`] zit tussen de motor en de
//! echte opslag:
//!
//! - **Nieuwe mappen niet meenemen** ([`NieuweMappen::AlleenGekozen`]): bestanden in een map die er bij het kiezen nog
//!   niet was, laat de schil uit de lijst van de server weg (zie `openaec_sync::selectie::nieuw_niet_gekozen`).
//! - **Bestanden op aanvraag** (een ruimte in de hoofdmap, alleen Windows): een download wordt geen volledig bestand
//!   maar een placeholder die alleen online staat, behalve in een vastgezette map ("Altijd op dit apparaat
//!   behouden"). Een nieuwe versie van een bestand dat alleen online staat, past alleen de placeholder aan. Wat lokaal
//!   staat, wordt gewoon gedownload.
//!
//! Elke methode van `Opslag` gaat door naar de echte opslag; komt er een methode bij, dan moet hij hier ook door.
//!
//! Daarnaast de ophaler: wat Windows vraagt als een bestand dat alleen online staat geopend wordt ([`ophaler`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use openaec_sync::motor::{bekende_bestanden, Batchgrenzen, KleinUpload, Opslag, OpslagBestand, Upload, Voortgang};
use openaec_sync::selectie::{mappen_boven, nieuw_niet_gekozen, NieuweMappen};
use openaec_sync::Fout;

pub struct Verkenneropslag<'a, O> {
    binnen: &'a O,
    root: PathBuf,
    /// Een ruimte in de hoofdmap: bestanden op aanvraag.
    op_aanvraag: bool,
    /// Alleen gekozen mappen: de mappen van de server bij het kiezen.
    alleen: Option<BTreeSet<String>>,
}

impl<'a, O: Opslag> Verkenneropslag<'a, O> {
    pub fn new(binnen: &'a O, root: &Path, op_aanvraag: bool, nieuwe: NieuweMappen, bekend: &[String]) -> Self {
        let alleen = (nieuwe == NieuweMappen::AlleenGekozen).then(|| bekend.iter().cloned().collect());
        Self { binnen, root: root.to_path_buf(), op_aanvraag: op_aanvraag && cfg!(windows), alleen }
    }

    /// Wat de schil doet met een download naar `lokaal`: zelf afhandelen als placeholder, of doorgeven.
    fn als_placeholder(&self, bestand: &OpslagBestand, lokaal: &Path) -> Option<Result<(), Fout>> {
        if !self.op_aanvraag {
            return None;
        }
        placeholder::download(&self.root, bestand, lokaal)
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
        match self.als_placeholder(bestand, lokaal) {
            Some(r) => r,
            None => self.binnen.download(bestand, lokaal).await,
        }
    }

    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
        self.binnen.verwijder(bestand).await
    }

    /// Een map die alleen te lezen met mij gedeeld is: ook in de hoofdmap gaat er niets naar de server.
    fn alleen_lezen(&self) -> bool {
        self.binnen.alleen_lezen()
    }

    async fn upload_met_voortgang(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>, voortgang: Voortgang<'_>) -> Result<Upload, Fout> {
        self.binnen.upload_met_voortgang(pad, lokaal, basis_versie, voortgang).await
    }

    async fn download_met_voortgang(&self, bestand: &OpslagBestand, lokaal: &Path, voortgang: Voortgang<'_>) -> Result<(), Fout> {
        match self.als_placeholder(bestand, lokaal) {
            Some(r) => r,
            None => self.binnen.download_met_voortgang(bestand, lokaal, voortgang).await,
        }
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
        // Placeholders hier; de rest in één verzoek naar de opslag, daarna weer op volgorde.
        let mut uit: Vec<Option<Result<(), Fout>>> = bestanden.iter().map(|(b, l)| self.als_placeholder(b, l)).collect();
        let rest: Vec<(&OpslagBestand, &Path)> = bestanden.iter().zip(&uit).filter(|(_, u)| u.is_none()).map(|(b, _)| *b).collect();
        if !rest.is_empty() {
            let mut van_opslag = self.binnen.download_klein(&rest).await?.into_iter();
            for plek in uit.iter_mut().filter(|u| u.is_none()) {
                *plek = Some(van_opslag.next().unwrap_or_else(|| Err(Fout::Controle("antwoord te kort".into()))));
            }
        }
        Ok(uit.into_iter().map(|u| u.unwrap()).collect())
    }
}

#[cfg(windows)]
mod placeholder {
    use std::path::Path;

    use openaec_sync::motor::OpslagBestand;
    use openaec_sync::Fout;

    use crate::cloudfilter;

    /// Staat er boven `pad` (tot en met `root`) een vastgezette map, en geen losgemaakte die dichterbij is?
    fn vastgezet_boven(root: &Path, pad: &Path) -> bool {
        let mut map = pad.parent();
        while let Some(m) = map.filter(|m| m.starts_with(root)) {
            if let Some(a) = cloudfilter::attributen(m) {
                if a & cloudfilter::ATTR_VASTGEZET != 0 {
                    return true;
                }
                if a & cloudfilter::ATTR_LOSGEMAAKT != 0 {
                    return false;
                }
            }
            map = m.parent();
        }
        false
    }

    fn fout(e: anyhow::Error) -> Fout {
        Fout::Bestand(std::io::Error::other(e.to_string()))
    }

    /// Een download in de hoofdmap: een nieuw bestand wordt een placeholder die alleen online staat (behalve onder een
    /// vastgezette map), een bestand dat alleen online staat krijgt alleen de nieuwe grootte. None: gewoon downloaden.
    pub fn download(root: &Path, bestand: &OpslagBestand, lokaal: &Path) -> Option<Result<(), Fout>> {
        match cloudfilter::attributen(lokaal) {
            Some(a) if cloudfilter::is_ontwaterd(a) && a & cloudfilter::ATTR_VASTGEZET == 0 => {
                Some(cloudfilter::werk_alleen_online_bij(lokaal, bestand.grootte, &bestand.bestand).map_err(fout))
            }
            Some(_) => None,
            None if vastgezet_boven(root, lokaal) => None,
            None => Some((|| {
                std::fs::create_dir_all(lokaal.parent().unwrap_or(root))?;
                cloudfilter::maak_alleen_online(lokaal, bestand.grootte, &bestand.bestand).map_err(fout)
            })()),
        }
    }
}

#[cfg(not(windows))]
mod placeholder {
    use std::path::Path;

    use openaec_sync::motor::OpslagBestand;
    use openaec_sync::Fout;

    pub fn download(_root: &Path, _bestand: &OpslagBestand, _lokaal: &Path) -> Option<Result<(), Fout>> {
        None
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
