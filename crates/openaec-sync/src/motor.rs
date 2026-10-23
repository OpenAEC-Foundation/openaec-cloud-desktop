//! Sync-motor (ontwerp §6 en §8): één ronde vergelijkt de lokale map, de server en de toestand van de vorige
//! ronde, en beslist per bestand: uploaden, downloaden, conflict of verwijderen (naar de prullenbak).
//!
//! Uitgangspunten: de server beslist over conflicten (basisversie); lokaal wordt nooit iets verwijderd dat niet
//! als versie op de server staat; bij een conflict blijven beide versies bestaan.
//!
//! Een ronde gaat in drie stappen: eerst de mappen op de server die de uploads nodig hebben, dan alle uploads en
//! downloads (begrensd parallel, zie [`Opties::gelijktijdig`]), en pas daarna het verwijderen. Een verplaatsing
//! (lokaal hernoemd) is zo eerst als nieuw bestand op de server, voordat het oude naar de prullenbak gaat.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};

use crate::voortgang::{Actie, Gebeurtenis, Richting, Stil, Teller, Waarnemer};
use crate::{Fout, MAX_GELIJKTIJDIG, STANDAARD_GELIJKTIJDIG};

/// Map in de gesynchroniseerde map waarin de motor zijn toestand bewaart; wordt zelf niet gesynchroniseerd.
pub const TOESTANDSMAP: &str = ".openaec";
const TIJDELIJK_EXT: &str = "openaec-download";

/// Een bestand op de server, met zijn pad binnen de ruimte (met '/').
#[derive(Debug, Clone)]
pub struct OpslagBestand {
    pub pad: String,
    pub bestand: String,
    pub versie: u64,
    pub grootte: u64,
}

/// Resultaat van een upload: het pad kan een conflictkopie zijn.
#[derive(Debug, Clone)]
pub struct Upload {
    pub pad: String,
    pub bestand: String,
    pub versie: u64,
    pub conflict: bool,
}

/// Bytevoortgang tijdens een upload of download: (overgezet, totaal over de lijn).
pub type Voortgang<'a> = &'a (dyn Fn(u64, u64) + Send + Sync);

/// Wat de motor van de server nodig heeft. De echte implementatie is [`crate::opslag::ClientOpslag`].
#[allow(async_fn_in_trait)]
pub trait Opslag {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout>;
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout>;
    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout>;
    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout>;

    /// Upload met bytevoortgang; standaard zonder tussenstanden.
    async fn upload_met_voortgang(
        &self,
        pad: &str,
        lokaal: &Path,
        basis_versie: Option<u64>,
        voortgang: Voortgang<'_>,
    ) -> Result<Upload, Fout> {
        let _ = voortgang;
        self.upload(pad, lokaal, basis_versie).await
    }

    /// Download met bytevoortgang; standaard zonder tussenstanden.
    async fn download_met_voortgang(&self, bestand: &OpslagBestand, lokaal: &Path, voortgang: Voortgang<'_>) -> Result<(), Fout> {
        let _ = voortgang;
        self.download(bestand, lokaal).await
    }

    /// Zorg dat deze mappen (paden met '/') op de server bestaan, vóór de uploads erin beginnen. Standaard niets:
    /// een opslag zonder mappen, of een die ze bij de upload zelf aanmaakt.
    async fn bereid_mappen(&self, mappen: &[String]) -> Result<(), Fout> {
        let _ = mappen;
        Ok(())
    }
}

/// Instellingen van een ronde.
#[derive(Debug, Clone)]
pub struct Opties {
    /// Zoveel bestanden tegelijk over de lijn (1..=32).
    pub gelijktijdig: usize,
}

impl Default for Opties {
    fn default() -> Self {
        Self { gelijktijdig: STANDAARD_GELIJKTIJDIG }
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Rapport {
    pub geupload: u32,
    pub gedownload: u32,
    pub verwijderd: u32,
    pub conflicten: u32,
    pub fouten: Vec<String>,
    /// Paden (met '/') die deze ronde in sync zijn gekomen: geüpload, gedownload, als conflictkopie ontstaan of
    /// voor het eerst in de toestand opgenomen. Voor de statusvinkjes in de Verkenner; niet naar de UI (bij de
    /// eerste ronde kunnen dit duizenden paden zijn).
    #[serde(skip)]
    pub bijgewerkt: Vec<String>,
    /// Paden (met '/') waarvoor deze ronde een fout optrad; staan in [`Rapport::fouten`] met de reden erbij.
    #[serde(skip)]
    pub foutpaden: Vec<String>,
}

impl Rapport {
    fn fout(&mut self, pad: &str, fout: impl std::fmt::Display) {
        self.fouten.push(format!("{pad}: {fout}"));
        self.foutpaden.push(pad.to_string());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Bekend {
    bestand: String,
    versie: u64,
    grootte: u64,
    mtime_ns: u128,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Toestand {
    bestanden: BTreeMap<String, Bekend>,
}

fn toestandsbestand(root: &Path) -> PathBuf {
    root.join(TOESTANDSMAP).join("staat.json")
}

fn laad(root: &Path) -> Toestand {
    std::fs::read(toestandsbestand(root))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn bewaar(root: &Path, toestand: &Toestand) -> Result<(), Fout> {
    let pad = toestandsbestand(root);
    std::fs::create_dir_all(pad.parent().unwrap())?;
    let tijdelijk = pad.with_extension("json.tmp");
    std::fs::write(&tijdelijk, serde_json::to_vec_pretty(toestand).unwrap())?;
    std::fs::rename(tijdelijk, pad)?;
    Ok(())
}

fn lokaal_pad(root: &Path, pad: &str) -> PathBuf {
    pad.split('/').fold(root.to_path_buf(), |p, deel| p.join(deel))
}

fn kenmerk(pad: &Path) -> std::io::Result<(u64, u128)> {
    let meta = std::fs::metadata(pad)?;
    let mtime = meta.modified()?.duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    Ok((meta.len(), mtime))
}

fn lokale_bestanden(root: &Path) -> BTreeMap<String, (u64, u128)> {
    let mut uit = BTreeMap::new();
    let mut te_doen = vec![root.to_path_buf()];
    while let Some(map) = te_doen.pop() {
        let Ok(items) = std::fs::read_dir(&map) else { continue };
        for item in items.flatten() {
            let pad = item.path();
            let naam = item.file_name().to_string_lossy().to_string();
            if map == root && naam == TOESTANDSMAP {
                continue;
            }
            match item.file_type() {
                Ok(t) if t.is_dir() => te_doen.push(pad),
                Ok(t) if t.is_file() => {
                    if pad.extension().is_some_and(|e| e == TIJDELIJK_EXT) {
                        continue;
                    }
                    let rel = pad
                        .strip_prefix(root)
                        .unwrap()
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join("/");
                    if let Ok(k) = kenmerk(&pad) {
                        uit.insert(rel, k);
                    }
                }
                _ => {}
            }
        }
    }
    uit
}

/// Bestand-id en versie van een gesynchroniseerd bestand (pad met '/' ten opzichte van `root`), uit de toestand
/// van de laatste ronde. Voor vergrendelen en delen vanuit de Verkenner.
pub fn bekend_bestand(root: &Path, pad: &str) -> Option<(String, u64)> {
    laad(root).bestanden.get(pad).map(|b| (b.bestand.clone(), b.versie))
}

/// Alle paden (met '/') die in sync zijn: in de toestand van de laatste ronde en lokaal ongewijzigd sinds die ronde
/// (zelfde grootte en wijzigingstijd); leeg als de map nog nooit is gesynchroniseerd. Voor de statusvinkjes in de
/// Verkenner: een bestand dat gewijzigd is terwijl de app niet draaide, krijgt zo geen vinkje.
pub fn bekende_paden(root: &Path) -> Vec<String> {
    laad(root)
        .bestanden
        .into_iter()
        .filter(|(pad, b)| kenmerk(&lokaal_pad(root, pad)).is_ok_and(|k| k == (b.grootte, b.mtime_ns)))
        .map(|(pad, _)| pad)
        .collect()
}

/// Aantal bestanden in de toestand van de laatste ronde (0 als de map nog nooit is gesynchroniseerd). Goedkoop: leest
/// alleen het toestandsbestand, zonder de bestanden zelf te bekijken.
pub fn aantal_bekend(root: &Path) -> usize {
    laad(root).bestanden.len()
}

/// Eén sync-ronde voor de map `root` tegen `opslag`.
pub async fn sync_eenmalig<O: Opslag>(opslag: &O, root: &Path) -> Result<Rapport, Fout> {
    sync_met_selectie(opslag, root, &[]).await
}

/// Ligt `pad` in een van de uitgesloten mappen (hele mapnamen, met '/')?
fn uitgesloten_pad(pad: &str, uitgesloten: &[String]) -> bool {
    uitgesloten.iter().map(|u| u.trim_matches('/')).filter(|u| !u.is_empty()).any(|u| {
        pad == u || pad.strip_prefix(u).is_some_and(|rest| rest.starts_with('/'))
    })
}

/// Lege mappen opruimen onder `map` (van diep naar ondiep); niet-lege blijven staan.
fn ruim_lege_mappen_op(map: &Path) {
    if let Ok(items) = std::fs::read_dir(map) {
        for item in items.flatten() {
            if item.file_type().is_ok_and(|t| t.is_dir()) {
                ruim_lege_mappen_op(&item.path());
            }
        }
    }
    let _ = std::fs::remove_dir(map);
}

/// Eén sync-ronde met mapselectie: mappen in `uitgesloten` (paden met '/') blijven op de server maar komen niet
/// (meer) op deze computer. Een ongewijzigde lokale kopie verdwijnt; lokaal nieuw of gewijzigd werk erin wordt
/// toch geüpload (niets gaat verloren) en verdwijnt daarna in een volgende ronde.
pub async fn sync_met_selectie<O: Opslag>(opslag: &O, root: &Path, uitgesloten: &[String]) -> Result<Rapport, Fout> {
    sync_met_voortgang(opslag, root, uitgesloten, &Stil).await
}

/// Als [`sync_met_selectie`], met meldingen per bestand aan `waarnemer` (zie [`crate::voortgang`]). Vraagt de
/// waarnemer om te pauzeren, dan begint de ronde geen nieuwe bestanden meer; wat al klaar is, blijft bewaard.
pub async fn sync_met_voortgang<O: Opslag>(
    opslag: &O,
    root: &Path,
    uitgesloten: &[String],
    waarnemer: &dyn Waarnemer,
) -> Result<Rapport, Fout> {
    sync_met_opties(opslag, root, uitgesloten, waarnemer, &Opties::default()).await
}

/// Wat er met één pad moet gebeuren (na de vergelijking).
#[derive(Debug, Clone)]
enum Taak {
    /// Lokaal nieuw of gewijzigd. `server` is de versie die er nu staat (voor een conflict).
    Upload { pad: String, basis: Option<u64>, server: Option<OpslagBestand> },
    Download { pad: String, server: OpslagBestand },
    /// Op de server verwijderd en lokaal ongewijzigd.
    VerwijderLokaal { pad: String },
    /// Lokaal verwijderd: op de server naar de prullenbak.
    VerwijderServer { pad: String, server: OpslagBestand },
}

impl Taak {
    fn pad(&self) -> &str {
        match self {
            Taak::Upload { pad, .. } | Taak::Download { pad, .. } | Taak::VerwijderLokaal { pad } | Taak::VerwijderServer { pad, .. } => pad,
        }
    }
}

/// Toestand en rapport van een lopende ronde, gedeeld door de parallelle overdrachten.
struct Ronde<'a, O> {
    opslag: &'a O,
    root: &'a Path,
    waarnemer: &'a dyn Waarnemer,
    toestand: Mutex<Toestand>,
    rapport: Mutex<Rapport>,
}

impl<O: Opslag> Ronde<'_, O> {
    /// Neem `pad` op in de toestand: lokaal en op de server zijn nu gelijk (komt in [`Rapport::bijgewerkt`]).
    fn onthoud(&self, pad: &str, bestand: &str, versie: u64) -> Result<(), Fout> {
        let (grootte, mtime_ns) = kenmerk(&lokaal_pad(self.root, pad))?;
        self.toestand
            .lock()
            .unwrap()
            .bestanden
            .insert(pad.to_string(), Bekend { bestand: bestand.to_string(), versie, grootte, mtime_ns });
        self.rapport.lock().unwrap().bijgewerkt.push(pad.to_string());
        Ok(())
    }

    fn vergeet(&self, pad: &str) {
        self.toestand.lock().unwrap().bestanden.remove(pad);
    }

    fn tel(&self, f: impl FnOnce(&mut Rapport)) {
        f(&mut self.rapport.lock().unwrap());
    }

    /// Upload met meldingen (start en bytevoortgang).
    async fn upload(&self, pad: &str, lokaal: &Path, basis: Option<u64>) -> Result<Upload, Fout> {
        let totaal = std::fs::metadata(lokaal).map(|m| m.len()).unwrap_or(0);
        self.waarnemer.melding(Gebeurtenis::Start { pad: pad.to_string(), richting: Richting::Upload, totaal });
        let teller = Teller::new(self.waarnemer, pad, Richting::Upload);
        self.opslag.upload_met_voortgang(pad, lokaal, basis, &|gedaan, totaal| teller.stand(gedaan, totaal)).await
    }

    /// Download met meldingen (start en bytevoortgang); `pad` is het pad in de map, niet per se dat op de server.
    async fn download(&self, pad: &str, server: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        self.waarnemer.melding(Gebeurtenis::Start { pad: pad.to_string(), richting: Richting::Download, totaal: server.grootte });
        let teller = Teller::new(self.waarnemer, pad, Richting::Download);
        self.opslag.download_met_voortgang(server, lokaal, &|gedaan, totaal| teller.stand(gedaan, totaal)).await
    }

    /// Eén taak uitvoeren; geeft terug wat er gebeurd is.
    async fn voer_uit(&self, taak: &Taak) -> Result<Actie, Fout> {
        let lokaal = lokaal_pad(self.root, taak.pad());
        match taak {
            Taak::Upload { pad, basis, server: Some(server) } => {
                // Basis: de versie waarop de lokale wijziging gebaseerd is; de server beslist over een conflict.
                let up = self.upload(pad, &lokaal, *basis).await?;
                if up.conflict {
                    // Mijn versie staat nu als conflictkopie op de server: lokaal hernoemen, en de
                    // serverversie op de oorspronkelijke plek zetten. Niets gaat verloren.
                    let kopie = lokaal_pad(self.root, &up.pad);
                    std::fs::rename(&lokaal, &kopie)?;
                    self.onthoud(&up.pad, &up.bestand, up.versie)?;
                    self.download(pad, server, &lokaal).await?;
                    self.onthoud(pad, &server.bestand, server.versie)?;
                    self.tel(|r| {
                        r.conflicten += 1;
                        r.gedownload += 1;
                    });
                    Ok(Actie::Conflict)
                } else {
                    self.onthoud(pad, &up.bestand, up.versie)?;
                    self.tel(|r| r.geupload += 1);
                    Ok(Actie::Geupload)
                }
            }
            Taak::Upload { pad, basis, server: None } => {
                let up = self.upload(pad, &lokaal, *basis).await?;
                self.onthoud(&up.pad, &up.bestand, up.versie)?;
                self.tel(|r| r.geupload += 1);
                Ok(Actie::Geupload)
            }
            Taak::Download { pad, server } => {
                self.download(pad, server, &lokaal).await?;
                self.onthoud(pad, &server.bestand, server.versie)?;
                self.tel(|r| r.gedownload += 1);
                Ok(Actie::Gedownload)
            }
            Taak::VerwijderLokaal { pad } => {
                // Op de server verwijderd (staat daar in de prullenbak) en lokaal ongewijzigd: lokaal weg.
                std::fs::remove_file(&lokaal)?;
                self.vergeet(pad);
                self.tel(|r| r.verwijderd += 1);
                Ok(Actie::Verwijderd)
            }
            Taak::VerwijderServer { pad, server } => {
                self.opslag.verwijder(server).await?;
                self.vergeet(pad);
                self.tel(|r| r.verwijderd += 1);
                Ok(Actie::Verwijderd)
            }
        }
    }

    /// Taak uitvoeren en melden; een fout komt in het rapport en houdt de andere taken niet op. Bij pauzeren
    /// begint de taak niet meer (de rest volgt in een volgende ronde).
    async fn verwerk(&self, taak: &Taak) {
        if self.waarnemer.gepauzeerd() {
            return;
        }
        let pad = taak.pad().to_string();
        match self.voer_uit(taak).await {
            Ok(actie) => self.waarnemer.melding(Gebeurtenis::Klaar { pad, actie }),
            Err(fout) => {
                self.waarnemer.melding(Gebeurtenis::Fout { pad: pad.clone(), fout: fout.to_string() });
                self.rapport.lock().unwrap().fout(&pad, fout);
            }
        }
    }

    /// Taken begrensd parallel uitvoeren (hoogstens `n` tegelijk).
    async fn verwerk_alle(&self, taken: &[Taak], n: usize) {
        // De futures eerst in een lijst (geen closure in de stroom): zo blijft de ronde `Send` voor de app.
        let werk: Vec<_> = taken.iter().map(|taak| self.verwerk(taak)).collect();
        stream::iter(werk).buffer_unordered(n).collect::<Vec<()>>().await;
    }
}

/// Als [`sync_met_voortgang`], met [`Opties`] (aantal overdrachten tegelijk).
pub async fn sync_met_opties<O: Opslag>(
    opslag: &O,
    root: &Path,
    uitgesloten: &[String],
    waarnemer: &dyn Waarnemer,
    opties: &Opties,
) -> Result<Rapport, Fout> {
    let toestand = laad(root);
    let server: BTreeMap<String, OpslagBestand> =
        opslag.lijst().await?.into_iter().map(|b| (b.pad.clone(), b)).collect();
    // De map aflopen kost bij duizenden bestanden even: op een aparte thread, zodat de runtime doorloopt.
    let lokaal = {
        let root = root.to_path_buf();
        tokio::task::spawn_blocking(move || lokale_bestanden(&root)).await.map_err(std::io::Error::other)?
    };

    let paden: BTreeSet<String> = toestand
        .bestanden
        .keys()
        .chain(server.keys())
        .chain(lokaal.keys())
        .cloned()
        .collect();

    // Per pad: is het lokaal en/of op de server gewijzigd sinds de vorige ronde?
    let wijzigingen: Vec<(String, bool, bool)> = paden
        .into_iter()
        .map(|pad| {
            let bekend = toestand.bestanden.get(&pad);
            let lokaal_gewijzigd = match (lokaal.get(&pad), bekend) {
                (Some((g, m)), Some(b)) => *g != b.grootte || *m != b.mtime_ns,
                (Some(_), None) => true,
                _ => false,
            };
            let server_gewijzigd = match (server.get(&pad), bekend) {
                (Some(s), Some(b)) => s.versie != b.versie || s.bestand != b.bestand,
                (Some(_), None) => true,
                _ => false,
            };
            (pad, lokaal_gewijzigd, server_gewijzigd)
        })
        .collect();

    // Vooraf melden hoeveel er over de lijn gaat (een verwijdering op de server telt niet mee).
    let (mut uploaden, mut downloaden, mut bytes) = (0u32, 0u32, 0u64);
    for (pad, lokaal_gewijzigd, server_gewijzigd) in &wijzigingen {
        if uitgesloten_pad(pad, uitgesloten) && !lokaal_gewijzigd {
            continue;
        }
        if *lokaal_gewijzigd {
            uploaden += 1;
            bytes += lokaal.get(pad).map_or(0, |(g, _)| *g);
        } else if let Some(s) = server.get(pad).filter(|_| *server_gewijzigd) {
            downloaden += 1;
            bytes += s.grootte;
        }
    }
    waarnemer.melding(Gebeurtenis::Ronde { uploaden, downloaden, bytes });

    let ronde = Ronde { opslag, root, waarnemer, toestand: Mutex::new(toestand), rapport: Mutex::new(Rapport::default()) };

    // Beslissen per pad. Wat geen overdracht is (opnemen, vergeten, uitgesloten), gebeurt meteen.
    let mut overdrachten = Vec::new();
    let mut verwijderingen = Vec::new();
    for (pad, lokaal_gewijzigd, server_gewijzigd) in wijzigingen {
        if waarnemer.gepauzeerd() {
            break;
        }
        let bekend = ronde.toestand.lock().unwrap().bestanden.get(&pad).cloned();
        let op_server = server.get(&pad);
        let hier = lokaal.contains_key(&pad);
        if uitgesloten_pad(&pad, uitgesloten) && !(hier && lokaal_gewijzigd) {
            if hier {
                // Uitgevinkt en lokaal ongewijzigd: de kopie hier mag weg, op de server blijft hij staan.
                if let Err(fout) = std::fs::remove_file(lokaal_pad(root, &pad)) {
                    ronde.rapport.lock().unwrap().fout(&pad, fout);
                    continue;
                }
            }
            ronde.vergeet(&pad);
            continue;
        }
        match (hier, op_server) {
            (true, Some(server)) => {
                if lokaal_gewijzigd {
                    let basis = bekend.as_ref().map(|b| b.versie);
                    overdrachten.push(Taak::Upload { pad, basis, server: Some(server.clone()) });
                } else if server_gewijzigd {
                    overdrachten.push(Taak::Download { pad, server: server.clone() });
                } else if bekend.is_none() {
                    if let Err(fout) = ronde.onthoud(&pad, &server.bestand, server.versie) {
                        ronde.rapport.lock().unwrap().fout(&pad, fout);
                    }
                }
            }
            (true, None) => {
                if bekend.is_some() && !lokaal_gewijzigd {
                    verwijderingen.push(Taak::VerwijderLokaal { pad });
                } else {
                    overdrachten.push(Taak::Upload { pad, basis: None, server: None });
                }
            }
            (false, Some(server)) => {
                if bekend.is_some() && !server_gewijzigd {
                    verwijderingen.push(Taak::VerwijderServer { pad, server: server.clone() });
                } else {
                    overdrachten.push(Taak::Download { pad, server: server.clone() });
                }
            }
            (false, None) => ronde.vergeet(&pad),
        }
    }

    let n = opties.gelijktijdig.clamp(1, MAX_GELIJKTIJDIG);
    // Mappen vóór de bestanden erin. Lukt het niet, dan probeert elke upload het zelf nog (en meldt dan de fout).
    let mappen: BTreeSet<String> = overdrachten
        .iter()
        .filter_map(|t| match t {
            Taak::Upload { pad, .. } => pad.rsplit_once('/').map(|(map, _)| map.to_string()),
            _ => None,
        })
        .collect();
    if !mappen.is_empty() {
        let _ = opslag.bereid_mappen(&mappen.into_iter().collect::<Vec<_>>()).await;
    }
    ronde.verwerk_alle(&overdrachten, n).await;
    // Verwijderen pas na de uploads en downloads.
    ronde.verwerk_alle(&verwijderingen, n).await;

    for u in uitgesloten.iter().map(|u| u.trim_matches('/')).filter(|u| !u.is_empty()) {
        ruim_lege_mappen_op(&lokaal_pad(root, u));
    }
    bewaar(root, &ronde.toestand.lock().unwrap())?;
    Ok(ronde.rapport.into_inner().unwrap())
}
