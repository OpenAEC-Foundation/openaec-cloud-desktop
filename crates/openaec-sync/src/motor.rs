//! Sync-motor (ontwerp §6 en §8): één ronde vergelijkt de lokale map, de server en de toestand van de vorige
//! ronde, en beslist per bestand: uploaden, downloaden, conflict of verwijderen (naar de prullenbak).
//!
//! Uitgangspunten: de server beslist over conflicten (basisversie); lokaal wordt nooit iets verwijderd dat niet
//! als versie op de server staat; bij een conflict blijven beide versies bestaan.
//!
//! Een ronde gaat in drie stappen: eerst de mappen op de server die de uploads nodig hebben, dan alle uploads en
//! downloads (begrensd parallel, zie [`Opties::gelijktijdig`]), en pas daarna het verwijderen. Een verplaatsing
//! (lokaal hernoemd) is zo eerst als nieuw bestand op de server, voordat het oude naar de prullenbak gaat.
//!
//! Lastige namen (zie [`crate::namen`]): een lokaal bestand met een naam die niet kan (bijv. `nul`), gaat niet naar de
//! server maar volgt [`Opties::gereserveerd`]: overslaan (in [`Rapport::overgeslagen`], zonder fout), opruimen of
//! onder een veilige naam (`nul_`). Komt zo'n naam van de server (een andere computer), dan staat hij hier onder de
//! veilige naam; de toestand onthoudt de koppeling, zodat een wijziging terug gaat naar de oorspronkelijke naam.
//! Bestanden op de negeerlijst ([`Negeerlijst`]) doen niet mee.
//!
//! Alleen lezen ([`Opslag::alleen_lezen`], een map die met mij gedeeld is om te bekijken): er gaat niets naar de
//! server. Een lokaal nieuw of gewijzigd bestand blijft staan en komt in [`Rapport::overgeslagen`] met de reden
//! [`ALLEEN_LEZEN`] (elke ronde, zonder fout en zonder opnieuw te proberen); een lokaal verwijderd bestand komt terug
//! van de server. Wijzigingen op de server komen gewoon binnen, behalve over een lokaal gewijzigd bestand.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};

pub use crate::api::Batchgrenzen;
use crate::namen::{self, Negeerlijst};
// Mapselectie: één filter voor uploaden en downloaden (zie `selectie`).
use crate::selectie::uitgesloten as uitgesloten_pad;
use crate::voortgang::{Actie, Gebeurtenis, Richting, Stil, Teller, Waarnemer};
use crate::{Fout, MAX_GELIJKTIJDIG, STANDAARD_GELIJKTIJDIG};

/// Map in de gesynchroniseerde map waarin de motor zijn toestand bewaart; wordt zelf niet gesynchroniseerd.
pub const TOESTANDSMAP: &str = ".openaec";
/// Begin van de reden bij een overgeslagen bestand in een map die alleen te lezen is (zie [`Opslag::alleen_lezen`]).
pub const ALLEEN_LEZEN: &str = "alleen lezen gedeeld";
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

/// Eén klein bestand voor [`Opslag::upload_klein`].
#[derive(Debug, Clone, Copy)]
pub struct KleinUpload<'a> {
    pub pad: &'a str,
    pub lokaal: &'a Path,
    pub basis_versie: Option<u64>,
}

/// Wat de motor van de server nodig heeft. De echte implementatie is [`crate::opslag::ClientOpslag`].
#[allow(async_fn_in_trait)]
pub trait Opslag {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout>;
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout>;
    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout>;
    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout>;

    /// Is deze opslag alleen te lezen (een map die met mij gedeeld is om te bekijken)? Dan stuurt de motor niets naar
    /// de server. Standaard niet.
    fn alleen_lezen(&self) -> bool {
        false
    }

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

    /// Grenzen voor kleine bestanden in één verzoek, als de opslag dat kent; None (standaard): elk bestand apart.
    async fn klein_grenzen(&self) -> Option<Batchgrenzen> {
        None
    }

    /// Kleine bestanden in één verzoek uploaden (alleen als [`Opslag::klein_grenzen`] iets geeft). Per bestand het
    /// resultaat, in dezelfde volgorde; een fout voor het hele verzoek laat de motor elk bestand apart proberen.
    async fn upload_klein(&self, bestanden: &[KleinUpload<'_>]) -> Result<Vec<Result<Upload, Fout>>, Fout> {
        let _ = bestanden;
        Err(Fout::Controle("deze opslag kent geen kleine bestanden in één verzoek".into()))
    }

    /// Kleine bestanden in één verzoek downloaden naar hun lokale pad; als [`Opslag::upload_klein`].
    async fn download_klein(&self, bestanden: &[(&OpslagBestand, &Path)]) -> Result<Vec<Result<(), Fout>>, Fout> {
        let _ = bestanden;
        Err(Fout::Controle("deze opslag kent geen kleine bestanden in één verzoek".into()))
    }
}

/// Wat de motor doet met een lokaal bestand waarvan de naam niet kan (zie [`crate::namen`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gereserveerd {
    /// Niet uploaden; in de lijst met overgeslagen bestanden (standaard).
    #[default]
    Overslaan,
    /// Opruimen als het bestand kleiner is dan [`OPRUIMGRENS`] (eerst naar de prullenbak waar dat kan); een groter
    /// bestand wordt overgeslagen.
    Verwijderen,
    /// Toch uploaden, onder een veilige naam (`nul` wordt `nul_`); lokaal blijft de naam gelijk.
    VeiligeNaam,
}

/// Bestanden met een lastige naam die kleiner zijn dan dit, mag de motor zelf opruimen ([`Gereserveerd::Verwijderen`]).
pub const OPRUIMGRENS: u64 = 64 * 1024;

/// Instellingen van een ronde.
#[derive(Debug, Clone)]
pub struct Opties {
    /// Zoveel bestanden tegelijk over de lijn (1..=32).
    pub gelijktijdig: usize,
    /// Wat er gebeurt met een lokaal bestand waarvan de naam niet kan.
    pub gereserveerd: Gereserveerd,
    /// Naar de prullenbak (geeft `true` als het lukte); zonder wordt opruimen meteen verwijderen.
    pub prullenbak: Option<fn(&Path) -> bool>,
    /// Kunnen namen die niet kunnen hier ook niet (Windows, standaard daar aan)? Dan staat zo'n naam van de server
    /// hier onder een veilige naam. Elders kan een bestand dat al op de server staat gewoon blijven heten zoals het
    /// heet.
    pub lokale_naamregels: bool,
}

impl Default for Opties {
    fn default() -> Self {
        Self {
            gelijktijdig: STANDAARD_GELIJKTIJDIG,
            gereserveerd: Gereserveerd::default(),
            prullenbak: None,
            lokale_naamregels: cfg!(windows),
        }
    }
}

/// Een bestand dat niet gesynchroniseerd wordt, met de reden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overgeslagen {
    /// Pad met '/' in de gekoppelde map (of op de server, als het daar vandaan kwam).
    pub pad: String,
    pub reden: String,
    /// Een lokaal bestand met een lastige naam: kan met "Opruimen" veilig weg ([`ruim_op`]).
    pub opruimbaar: bool,
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
    /// Bestanden die niet gesynchroniseerd zijn (bijv. een gereserveerde naam). Geen fout: ze komen elke ronde
    /// terug zonder dat er iets opnieuw geprobeerd wordt.
    pub overgeslagen: Vec<Overgeslagen>,
    /// Wat de motor zelf deed met een lastige naam (opgeruimd, onder een veilige naam), voor een melding.
    pub meldingen: Vec<String>,
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
    /// Per pad op de server: wat er bij de vorige ronde gelijk was.
    bestanden: BTreeMap<String, Bekend>,
    /// Pad op de server -> pad hier, voor bestanden die hier een andere (veilige) naam hebben.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    lokale_namen: BTreeMap<String, String>,
    /// Overgeslagen bestanden van de laatste ronde.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    overgeslagen: Vec<Overgeslagen>,
}

impl Toestand {
    /// Het pad hier (met '/') voor een pad op de server.
    fn hier(&self, sleutel: &str) -> String {
        self.lokale_namen.get(sleutel).cloned().unwrap_or_else(|| sleutel.to_string())
    }

    /// Het pad op de server voor een pad hier.
    fn sleutel(&self, hier: &str) -> String {
        self.lokale_namen.iter().find(|(_, l)| *l == hier).map(|(s, _)| s.clone()).unwrap_or_else(|| hier.to_string())
    }
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

/// Het bestand `rel` (met '/') in `root`; met een lastige naam letterlijk (`\\?\` op Windows), anders zou Windows
/// er een apparaat of een andere naam van maken.
fn lokaal_bestand(root: &Path, rel: &str) -> PathBuf {
    let pad = lokaal_pad(root, rel);
    if namen::padprobleem(rel).is_some() { namen::lang_pad(&pad) } else { pad }
}

fn kenmerk(pad: &Path) -> std::io::Result<(u64, u128)> {
    let meta = std::fs::metadata(pad)?;
    let mtime = meta.modified()?.duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    Ok((meta.len(), mtime))
}

fn lokale_bestanden(root: &Path) -> BTreeMap<String, (u64, u128)> {
    let mut uit = BTreeMap::new();
    let mut te_doen = vec![(root.to_path_buf(), String::new())];
    while let Some((map, rel_map)) = te_doen.pop() {
        let Ok(items) = std::fs::read_dir(&map) else { continue };
        for item in items.flatten() {
            let naam = item.file_name().to_string_lossy().to_string();
            if rel_map.is_empty() && naam == TOESTANDSMAP {
                continue;
            }
            let rel = if rel_map.is_empty() { naam.clone() } else { format!("{rel_map}/{naam}") };
            // Een lastige naam (ook van een map onderweg) letterlijk openen.
            let pad = if namen::naamprobleem(&naam).is_some() { namen::lang_pad(&item.path()) } else { item.path() };
            match item.file_type() {
                Ok(t) if t.is_dir() => te_doen.push((pad, rel)),
                Ok(t) if t.is_file() => {
                    if pad.extension().is_some_and(|e| e == TIJDELIJK_EXT) {
                        continue;
                    }
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
    let toestand = laad(root);
    toestand.bestanden.get(&toestand.sleutel(pad)).map(|b| (b.bestand.clone(), b.versie))
}

/// Alle paden (met '/') die in sync zijn: in de toestand van de laatste ronde en lokaal ongewijzigd sinds die ronde
/// (zelfde grootte en wijzigingstijd); leeg als de map nog nooit is gesynchroniseerd. Voor de statusvinkjes in de
/// Verkenner: een bestand dat gewijzigd is terwijl de app niet draaide, krijgt zo geen vinkje.
pub fn bekende_paden(root: &Path) -> Vec<String> {
    let toestand = laad(root);
    toestand
        .bestanden
        .iter()
        .map(|(pad, b)| (toestand.hier(pad), b))
        .filter(|(hier, b)| kenmerk(&lokaal_bestand(root, hier)).is_ok_and(|k| k == (b.grootte, b.mtime_ns)))
        .map(|(hier, _)| hier)
        .collect()
}

/// Een bestand in de toestand van de laatste ronde: het pad hier (met '/'), het bestand-id en de versie op de server,
/// en grootte en wijzigingstijd zoals ze hier waren.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BekendBestand {
    pub pad: String,
    pub bestand: String,
    pub versie: u64,
    pub grootte: u64,
    pub mtime_ns: u128,
}

/// Alle bestanden in de toestand van de laatste ronde (leeg als de map nog nooit is gesynchroniseerd). Leest alleen
/// het toestandsbestand; voor de Verkenner (vergrendelingen bij een bestand-id zoeken, en zien of een gebeurtenis van
/// de bestandswachter een echte wijziging is).
pub fn bekende_bestanden(root: &Path) -> Vec<BekendBestand> {
    let toestand = laad(root);
    toestand
        .bestanden
        .iter()
        .map(|(sleutel, b)| BekendBestand {
            pad: toestand.hier(sleutel),
            bestand: b.bestand.clone(),
            versie: b.versie,
            grootte: b.grootte,
            mtime_ns: b.mtime_ns,
        })
        .collect()
}

/// Hoe `pad` (een bestand of map, met '/' ten opzichte van `root`) er hier voor staat ten opzichte van de laatste
/// ronde: de bestanden die in sync zijn (in de toestand en lokaal ongewijzigd sinds die ronde) en de lokale bestanden
/// die dat niet zijn (nieuw of gewijzigd); bestanden op de negeerlijst tellen niet mee. Leest alleen de toestand en de
/// bestanden onder `pad`. Voor versturen: wat helemaal in sync is, staat al op de server.
pub fn stand_onder(root: &Path, pad: &str) -> (Vec<BekendBestand>, Vec<String>) {
    stand_van(root, &[pad]).pop().unwrap_or_default()
}

/// Als [`stand_onder`], voor een aantal paden in dezelfde gekoppelde map (de toestand wordt één keer gelezen).
pub fn stand_van(root: &Path, paden: &[&str]) -> Vec<(Vec<BekendBestand>, Vec<String>)> {
    let toestand = laad(root);
    let negeer = Negeerlijst::laad(root);
    paden.iter().map(|pad| stand_in(root, &toestand, &negeer, pad)).collect()
}

fn stand_in(root: &Path, toestand: &Toestand, negeer: &Negeerlijst, pad: &str) -> (Vec<BekendBestand>, Vec<String>) {
    let pad = pad.trim_matches('/');
    let plek = lokaal_pad(root, pad);
    let lokaal: Vec<(String, (u64, u128))> = match std::fs::metadata(&plek) {
        Ok(m) if m.is_dir() => lokale_bestanden(&plek)
            .into_iter()
            .map(|(rel, k)| (if pad.is_empty() { rel } else { format!("{pad}/{rel}") }, k))
            .collect(),
        Ok(_) => kenmerk(&plek).map(|k| vec![(pad.to_string(), k)]).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let (mut in_sync, mut anders) = (Vec::new(), Vec::new());
    for (hier, k) in lokaal {
        let sleutel = toestand.sleutel(&hier);
        if negeer.negeert(&sleutel) || hier.split('/').next() == Some(TOESTANDSMAP) {
            continue;
        }
        match toestand.bestanden.get(&sleutel) {
            Some(b) if (b.grootte, b.mtime_ns) == k => in_sync.push(BekendBestand {
                pad: hier,
                bestand: b.bestand.clone(),
                versie: b.versie,
                grootte: b.grootte,
                mtime_ns: b.mtime_ns,
            }),
            _ => anders.push(hier),
        }
    }
    (in_sync, anders)
}

/// De overgeslagen bestanden van de laatste ronde (leeg als er geen waren of de map nog nooit is gesynchroniseerd).
pub fn overgeslagen(root: &Path) -> Vec<Overgeslagen> {
    laad(root).overgeslagen
}

/// Overgeslagen bestanden met een lastige naam opruimen (na bevestiging in de app): veilig verwijderd met
/// [`namen::verwijder_veilig`], dus via de prullenbak waar dat kan. Alleen paden die als opruimbaar in de lijst van
/// de laatste ronde staan; wat lukt, verdwijnt uit die lijst. Per pad de uitkomst. Niet tegelijk met een ronde.
pub fn ruim_op(root: &Path, paden: &[String], prullenbak: Option<fn(&Path) -> bool>) -> Vec<(String, Result<(), String>)> {
    let mut toestand = laad(root);
    let mut uit = Vec::new();
    for pad in paden {
        let mag = toestand.overgeslagen.iter().any(|o| &o.pad == pad && o.opruimbaar);
        let r = if mag {
            namen::verwijder_veilig(root, pad, prullenbak).map_err(|e| e.to_string())
        } else {
            Err("staat niet (meer) in de lijst met overgeslagen bestanden".into())
        };
        if r.is_ok() {
            toestand.overgeslagen.retain(|o| &o.pad != pad);
        }
        uit.push((pad.clone(), r));
    }
    let _ = bewaar(root, &toestand);
    uit
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
    /// Het pad hier (met '/') voor een pad op de server; meldingen en het rapport gebruiken dat pad.
    fn rel(&self, sleutel: &str) -> String {
        self.toestand.lock().unwrap().hier(sleutel)
    }

    /// Het lokale bestand voor een pad op de server.
    fn hier(&self, sleutel: &str) -> PathBuf {
        lokaal_bestand(self.root, &self.rel(sleutel))
    }

    /// Neem `pad` op in de toestand: lokaal en op de server zijn nu gelijk (komt in [`Rapport::bijgewerkt`]).
    fn onthoud(&self, pad: &str, bestand: &str, versie: u64) -> Result<(), Fout> {
        let rel = self.rel(pad);
        let (grootte, mtime_ns) = kenmerk(&lokaal_bestand(self.root, &rel))?;
        self.toestand
            .lock()
            .unwrap()
            .bestanden
            .insert(pad.to_string(), Bekend { bestand: bestand.to_string(), versie, grootte, mtime_ns });
        self.rapport.lock().unwrap().bijgewerkt.push(rel);
        Ok(())
    }

    fn vergeet(&self, pad: &str) {
        let mut toestand = self.toestand.lock().unwrap();
        toestand.bestanden.remove(pad);
        toestand.lokale_namen.remove(pad);
    }

    fn tel(&self, f: impl FnOnce(&mut Rapport)) {
        f(&mut self.rapport.lock().unwrap());
    }

    /// Upload met meldingen (start en bytevoortgang).
    async fn upload(&self, pad: &str, lokaal: &Path, basis: Option<u64>) -> Result<Upload, Fout> {
        let totaal = std::fs::metadata(lokaal).map(|m| m.len()).unwrap_or(0);
        let rel = self.rel(pad);
        self.waarnemer.melding(Gebeurtenis::Start { pad: rel.clone(), richting: Richting::Upload, totaal });
        let teller = Teller::new(self.waarnemer, &rel, Richting::Upload);
        self.opslag.upload_met_voortgang(pad, lokaal, basis, &|gedaan, totaal| teller.stand(gedaan, totaal)).await
    }

    /// Download met meldingen (start en bytevoortgang); `pad` is het pad op de server waarop het bestand hier
    /// bijgehouden wordt (bij een conflict niet per se dat van `server`).
    async fn download(&self, pad: &str, server: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        let rel = self.rel(pad);
        self.waarnemer.melding(Gebeurtenis::Start { pad: rel.clone(), richting: Richting::Download, totaal: server.grootte });
        let teller = Teller::new(self.waarnemer, &rel, Richting::Download);
        self.opslag.download_met_voortgang(server, lokaal, &|gedaan, totaal| teller.stand(gedaan, totaal)).await
    }

    /// Wat er na een geslaagde upload moet gebeuren: opnemen, of bij een conflict de serverversie terughalen.
    async fn na_upload(&self, pad: &str, server: Option<&OpslagBestand>, up: Upload) -> Result<Actie, Fout> {
        match server {
            Some(server) if up.conflict => {
                // Mijn versie staat nu als conflictkopie op de server: lokaal hernoemen, en de
                // serverversie op de oorspronkelijke plek zetten. Niets gaat verloren.
                let lokaal = self.hier(pad);
                let kopie = self.hier(&up.pad);
                std::fs::rename(&lokaal, &kopie)?;
                self.onthoud(&up.pad, &up.bestand, up.versie)?;
                self.download(pad, server, &lokaal).await?;
                self.onthoud(pad, &server.bestand, server.versie)?;
                self.tel(|r| {
                    r.conflicten += 1;
                    r.gedownload += 1;
                });
                Ok(Actie::Conflict)
            }
            Some(_) => {
                self.onthoud(pad, &up.bestand, up.versie)?;
                self.tel(|r| r.geupload += 1);
                Ok(Actie::Geupload)
            }
            None => {
                self.onthoud(&up.pad, &up.bestand, up.versie)?;
                self.tel(|r| r.geupload += 1);
                Ok(Actie::Geupload)
            }
        }
    }

    /// Eén taak uitvoeren; geeft terug wat er gebeurd is.
    async fn voer_uit(&self, taak: &Taak) -> Result<Actie, Fout> {
        let lokaal = self.hier(taak.pad());
        match taak {
            Taak::Upload { pad, basis, server } => {
                // Basis: de versie waarop de lokale wijziging gebaseerd is; de server beslist over een conflict.
                let up = self.upload(pad, &lokaal, *basis).await?;
                self.na_upload(pad, server.as_ref(), up).await
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

    /// Uitkomst van een taak melden; een fout komt in het rapport.
    fn meld(&self, pad: &str, uitkomst: Result<Actie, Fout>) {
        let pad = &self.rel(pad);
        match uitkomst {
            Ok(actie) => self.waarnemer.melding(Gebeurtenis::Klaar { pad: pad.to_string(), actie }),
            Err(fout) => {
                self.waarnemer.melding(Gebeurtenis::Fout { pad: pad.to_string(), fout: fout.to_string() });
                self.rapport.lock().unwrap().fout(pad, fout);
            }
        }
    }

    /// Taak uitvoeren en melden; een fout komt in het rapport en houdt de andere taken niet op. Bij pauzeren
    /// begint de taak niet meer (de rest volgt in een volgende ronde).
    async fn verwerk(&self, taak: &Taak) {
        if self.waarnemer.gepauzeerd() {
            return;
        }
        let uitkomst = self.voer_uit(taak).await;
        self.meld(taak.pad(), uitkomst);
    }

    /// Een groep kleine uploads in één verzoek. Weigert de opslag het verzoek als geheel, dan gaat elk bestand apart.
    async fn verwerk_kleine_uploads(&self, taken: &[Taak]) {
        if self.waarnemer.gepauzeerd() {
            return;
        }
        let paden: Vec<(&str, PathBuf, Option<u64>, Option<&OpslagBestand>, u64)> = taken
            .iter()
            .filter_map(|t| match t {
                Taak::Upload { pad, basis, server } => {
                    let lokaal = self.hier(pad);
                    let grootte = std::fs::metadata(&lokaal).map(|m| m.len()).unwrap_or(0);
                    Some((pad.as_str(), lokaal, *basis, server.as_ref(), grootte))
                }
                _ => None,
            })
            .collect();
        for (pad, _, _, _, grootte) in &paden {
            self.waarnemer.melding(Gebeurtenis::Start { pad: self.rel(pad), richting: Richting::Upload, totaal: *grootte });
        }
        let items: Vec<KleinUpload> =
            paden.iter().map(|(pad, lokaal, basis, _, _)| KleinUpload { pad, lokaal, basis_versie: *basis }).collect();
        let resultaten = match self.opslag.upload_klein(&items).await {
            Ok(r) if r.len() == paden.len() => r,
            _ => {
                for taak in taken {
                    self.verwerk(taak).await;
                }
                return;
            }
        };
        for ((pad, _, _, server, grootte), resultaat) in paden.iter().zip(resultaten) {
            let uitkomst = match resultaat {
                Ok(up) => {
                    Teller::new(self.waarnemer, &self.rel(pad), Richting::Upload).stand(*grootte, *grootte);
                    self.na_upload(pad, *server, up).await
                }
                Err(fout) => Err(fout),
            };
            self.meld(pad, uitkomst);
        }
    }

    /// Een groep kleine downloads in één verzoek; als [`Ronde::verwerk_kleine_uploads`].
    async fn verwerk_kleine_downloads(&self, taken: &[Taak]) {
        if self.waarnemer.gepauzeerd() {
            return;
        }
        let paden: Vec<(&str, &OpslagBestand, PathBuf)> = taken
            .iter()
            .filter_map(|t| match t {
                Taak::Download { pad, server } => Some((pad.as_str(), server, self.hier(pad))),
                _ => None,
            })
            .collect();
        for (pad, server, _) in &paden {
            self.waarnemer.melding(Gebeurtenis::Start { pad: self.rel(pad), richting: Richting::Download, totaal: server.grootte });
        }
        let items: Vec<(&OpslagBestand, &Path)> = paden.iter().map(|(_, server, lokaal)| (*server, lokaal.as_path())).collect();
        let resultaten = match self.opslag.download_klein(&items).await {
            Ok(r) if r.len() == paden.len() => r,
            _ => {
                for taak in taken {
                    self.verwerk(taak).await;
                }
                return;
            }
        };
        for ((pad, server, _), resultaat) in paden.iter().zip(resultaten) {
            let uitkomst = resultaat.and_then(|()| {
                Teller::new(self.waarnemer, &self.rel(pad), Richting::Download).stand(server.grootte, server.grootte);
                self.onthoud(pad, &server.bestand, server.versie)?;
                self.tel(|r| r.gedownload += 1);
                Ok(Actie::Gedownload)
            });
            self.meld(pad, uitkomst);
        }
    }

    async fn verwerk_werk(&self, werk: &Werk) {
        match werk {
            Werk::Los(taak) => self.verwerk(taak).await,
            Werk::KleineUploads(taken) => self.verwerk_kleine_uploads(taken).await,
            Werk::KleineDownloads(taken) => self.verwerk_kleine_downloads(taken).await,
        }
    }

    /// Werk begrensd parallel uitvoeren (hoogstens `n` tegelijk).
    async fn verwerk_alle(&self, werk: &[Werk], n: usize) {
        // De futures eerst in een lijst (geen closure in de stroom): zo blijft de ronde `Send` voor de app.
        let futures: Vec<_> = werk.iter().map(|w| self.verwerk_werk(w)).collect();
        stream::iter(futures).buffer_unordered(n).collect::<Vec<()>>().await;
    }
}

/// Eén eenheid werk voor de parallelle stap: een losse taak, of een groep kleine bestanden in één verzoek.
#[derive(Debug)]
enum Werk {
    Los(Taak),
    KleineUploads(Vec<Taak>),
    KleineDownloads(Vec<Taak>),
}

/// Verdeel de overdrachten in werk: kleine bestanden in groepen binnen de grenzen, de rest los (vooraan, zodat grote
/// bestanden meteen beginnen). Een groep van één wordt gewoon een losse taak.
fn verdeel(overdrachten: Vec<Taak>, grenzen: Option<Batchgrenzen>, grootte_van: impl Fn(&Taak) -> u64) -> Vec<Werk> {
    let Some(g) = grenzen.filter(|g| g.max_aantal > 1) else {
        return overdrachten.into_iter().map(Werk::Los).collect();
    };
    let mut werk = Vec::new();
    let (mut ups, mut downs) = (Vec::new(), Vec::new());
    for taak in overdrachten {
        let grootte = grootte_van(&taak);
        match &taak {
            Taak::Upload { .. } if grootte <= g.max_bestand => ups.push((taak, grootte)),
            Taak::Download { .. } if grootte <= g.max_bestand => downs.push((taak, grootte)),
            _ => werk.push(Werk::Los(taak)),
        }
    }
    let groepen = |taken: Vec<(Taak, u64)>| {
        let mut uit: Vec<Vec<Taak>> = Vec::new();
        let mut bytes = 0u64;
        for (taak, grootte) in taken {
            if uit.last().is_none_or(|l| l.len() >= g.max_aantal || bytes + grootte > g.max_bytes) {
                uit.push(Vec::new());
                bytes = 0;
            }
            bytes += grootte;
            uit.last_mut().unwrap().push(taak);
        }
        uit
    };
    for (groep, soort) in groepen(ups).into_iter().map(|g| (g, true)).chain(groepen(downs).into_iter().map(|g| (g, false))) {
        werk.push(match (groep.len(), soort) {
            (1, _) => Werk::Los(groep.into_iter().next().unwrap()),
            (_, true) => Werk::KleineUploads(groep),
            (_, false) => Werk::KleineDownloads(groep),
        });
    }
    werk
}

/// Als [`sync_met_voortgang`], met [`Opties`] (aantal overdrachten tegelijk).
pub async fn sync_met_opties<O: Opslag>(
    opslag: &O,
    root: &Path,
    uitgesloten: &[String],
    waarnemer: &dyn Waarnemer,
    opties: &Opties,
) -> Result<Rapport, Fout> {
    let mut toestand = laad(root);
    let server: BTreeMap<String, OpslagBestand> =
        opslag.lijst().await?.into_iter().map(|b| (b.pad.clone(), b)).collect();
    // De map aflopen kost bij duizenden bestanden even: op een aparte thread, zodat de runtime doorloopt.
    let ruw = {
        let root = root.to_path_buf();
        tokio::task::spawn_blocking(move || lokale_bestanden(&root)).await.map_err(std::io::Error::other)?
    };
    let negeer = Negeerlijst::laad(root);
    let mut rapport = Rapport::default();
    let buiten = koppel_servernamen(&mut toestand, &server, &ruw, &negeer, opties, &mut rapport);
    let lokaal = vertaal_lokaal(root, &mut toestand, &ruw, &server, &negeer, opties, &mut rapport);
    toestand.overgeslagen = rapport.overgeslagen.clone();

    let paden: BTreeSet<String> = toestand
        .bestanden
        .keys()
        .chain(server.keys())
        .chain(lokaal.keys())
        .filter(|p| !buiten.contains(*p) && !negeer.negeert(p))
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

    // Vooraf melden hoeveel er over de lijn gaat (een verwijdering op de server telt niet mee). Alleen lezen: geen
    // uploads, en een lokaal verwijderd bestand komt terug.
    let alleen_lezen = opslag.alleen_lezen();
    let (mut uploaden, mut downloaden, mut bytes) = (0u32, 0u32, 0u64);
    for (pad, lokaal_gewijzigd, server_gewijzigd) in &wijzigingen {
        if uitgesloten_pad(pad, uitgesloten) && !lokaal_gewijzigd {
            continue;
        }
        if *lokaal_gewijzigd {
            if !alleen_lezen {
                uploaden += 1;
                bytes += lokaal.get(pad).map_or(0, |(g, _)| *g);
            }
        } else if let Some(s) = server.get(pad).filter(|_| *server_gewijzigd || (alleen_lezen && !lokaal.contains_key(pad))) {
            downloaden += 1;
            bytes += s.grootte;
        }
    }
    waarnemer.melding(Gebeurtenis::Ronde { uploaden, downloaden, bytes });

    let ronde = Ronde { opslag, root, waarnemer, toestand: Mutex::new(toestand), rapport: Mutex::new(rapport) };

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
                if let Err(fout) = std::fs::remove_file(ronde.hier(&pad)) {
                    ronde.rapport.lock().unwrap().fout(&pad, fout);
                    continue;
                }
            }
            ronde.vergeet(&pad);
            continue;
        }
        match (hier, op_server) {
            (true, _) if lokaal_gewijzigd && alleen_lezen => {
                // Niets naar de server: het lokale werk blijft staan, met een melding (geen fout, geen nieuwe poging).
                let reden = match (bekend.is_some(), op_server.is_some()) {
                    (true, true) => "lokaal gewijzigd; gaat niet naar de server",
                    (true, false) => "lokaal gewijzigd en op de server verwijderd; blijft hier staan",
                    (false, _) => "nieuw op deze computer; gaat niet naar de server",
                };
                let pad = ronde.rel(&pad);
                ronde.rapport.lock().unwrap().overgeslagen.push(Overgeslagen { pad, reden: format!("{ALLEEN_LEZEN}: {reden}"), opruimbaar: false });
            }
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
                if bekend.is_some() && !server_gewijzigd && !alleen_lezen {
                    verwijderingen.push(Taak::VerwijderServer { pad, server: server.clone() });
                } else {
                    overdrachten.push(Taak::Download { pad, server: server.clone() });
                }
            }
            (false, None) => ronde.vergeet(&pad),
        }
    }

    let n = opties.gelijktijdig.clamp(1, MAX_GELIJKTIJDIG);
    let grenzen = opslag.klein_grenzen().await;
    let grootte_van = |taak: &Taak| match taak {
        Taak::Upload { pad, .. } => lokaal.get(pad).map_or(u64::MAX, |(g, _)| *g),
        Taak::Download { server, .. } => server.grootte,
        _ => u64::MAX,
    };
    // Mappen vóór de bestanden erin. Lukt het niet, dan probeert elke upload het zelf nog (en meldt dan de fout).
    let mappen: BTreeSet<String> = overdrachten
        .iter()
        .filter_map(|t| match t {
            Taak::Upload { pad, .. } => pad.rsplit_once('/').map(|(map, _)| map.to_string()),
            _ => None,
        })
        .collect();
    let werk = verdeel(overdrachten, grenzen, grootte_van);
    if !mappen.is_empty() {
        let _ = opslag.bereid_mappen(&mappen.into_iter().collect::<Vec<_>>()).await;
    }
    ronde.verwerk_alle(&werk, n).await;
    // Verwijderen pas na de uploads en downloads.
    ronde.verwerk_alle(&verwijderingen.into_iter().map(Werk::Los).collect::<Vec<_>>(), n).await;

    for u in uitgesloten.iter().map(|u| u.trim_matches('/')).filter(|u| !u.is_empty()) {
        ruim_lege_mappen_op(&lokaal_pad(root, u));
    }
    // Ook wat bij alleen lezen niet naar de server ging, staat in de lijst met overgeslagen bestanden.
    ronde.toestand.lock().unwrap().overgeslagen = ronde.rapport.lock().unwrap().overgeslagen.clone();
    bewaar(root, &ronde.toestand.lock().unwrap())?;
    Ok(ronde.rapport.into_inner().unwrap())
}

/// Paden op de server die hier niet onder hun eigen naam kunnen (Windows, zie [`Opties::lokale_naamregels`]) krijgen
/// een veilige naam hier (`nul` → `nul_`), vastgelegd in de toestand. Staat die veilige naam al ergens, dan doet het
/// bestand niet mee (overgeslagen). Geeft de paden op de server die buiten de ronde blijven.
fn koppel_servernamen(
    toestand: &mut Toestand,
    server: &BTreeMap<String, OpslagBestand>,
    ruw: &BTreeMap<String, (u64, u128)>,
    negeer: &Negeerlijst,
    opties: &Opties,
    rapport: &mut Rapport,
) -> BTreeSet<String> {
    let mut buiten = BTreeSet::new();
    if !opties.lokale_naamregels {
        return buiten;
    }
    for sleutel in server.keys() {
        if toestand.lokale_namen.contains_key(sleutel) || negeer.negeert(sleutel) {
            continue;
        }
        let Some(probleem) = namen::padprobleem(sleutel).filter(namen::Probleem::in_de_naam) else { continue };
        let veilig = namen::veilig_pad(sleutel);
        let bezet = server.contains_key(&veilig)
            || ruw.contains_key(&veilig)
            || toestand.lokale_namen.values().any(|l| *l == veilig)
            || toestand.bestanden.contains_key(&veilig);
        if bezet {
            let reden = format!("{probleem}; kan hier niet als {veilig}, want die naam bestaat al");
            rapport.overgeslagen.push(Overgeslagen { pad: sleutel.clone(), reden, opruimbaar: false });
            buiten.insert(sleutel.clone());
        } else {
            rapport.meldingen.push(format!("{sleutel}: {probleem}; staat hier als {veilig}"));
            toestand.lokale_namen.insert(sleutel.clone(), veilig);
        }
    }
    buiten
}

/// De lokale bestanden onder hun pad op de server. Genegeerde bestanden vallen weg; een bestand dat hier onder een
/// veilige naam staat, krijgt zijn pad op de server terug; een naam die niet kan, volgt [`Opties::gereserveerd`].
fn vertaal_lokaal(
    root: &Path,
    toestand: &mut Toestand,
    ruw: &BTreeMap<String, (u64, u128)>,
    server: &BTreeMap<String, OpslagBestand>,
    negeer: &Negeerlijst,
    opties: &Opties,
    rapport: &mut Rapport,
) -> BTreeMap<String, (u64, u128)> {
    let omgekeerd: HashMap<String, String> = toestand.lokale_namen.iter().map(|(s, l)| (l.clone(), s.clone())).collect();
    let mut uit = BTreeMap::new();
    for (rel, kenmerk) in ruw {
        if negeer.negeert(rel) {
            continue;
        }
        if let Some(sleutel) = omgekeerd.get(rel) {
            uit.insert(sleutel.clone(), *kenmerk);
            continue;
        }
        let Some(probleem) = namen::padprobleem(rel) else {
            uit.insert(rel.clone(), *kenmerk);
            continue;
        };
        // Waar de naam hier wel kan, en het bestand al op de server staat (van vóór de naamregels): gewoon bijhouden,
        // de server neemt nieuwe versies van een bestaand bestand aan.
        if !opties.lokale_naamregels && (server.contains_key(rel) || toestand.bestanden.contains_key(rel)) {
            uit.insert(rel.clone(), *kenmerk);
            continue;
        }
        let opruimbaar = probleem.in_de_naam();
        let sla_over = |rapport: &mut Rapport, reden: String| rapport.overgeslagen.push(Overgeslagen { pad: rel.clone(), reden, opruimbaar });
        match opties.gereserveerd {
            Gereserveerd::VeiligeNaam if opruimbaar => {
                let veilig = namen::veilig_pad(rel);
                if ruw.contains_key(&veilig) || uit.contains_key(&veilig) || toestand.lokale_namen.contains_key(&veilig) {
                    sla_over(rapport, format!("{probleem}; {veilig} bestaat al"));
                } else {
                    rapport.meldingen.push(format!("{rel}: {probleem}; staat op de server als {veilig}"));
                    toestand.lokale_namen.insert(veilig.clone(), rel.clone());
                    uit.insert(veilig, *kenmerk);
                }
            }
            Gereserveerd::Verwijderen if opruimbaar && kenmerk.0 < OPRUIMGRENS => {
                match namen::verwijder_veilig(root, rel, opties.prullenbak) {
                    Ok(()) => rapport.meldingen.push(format!("{rel}: {probleem}; opgeruimd")),
                    Err(fout) => sla_over(rapport, format!("{probleem}; opruimen lukte niet ({fout})")),
                }
            }
            Gereserveerd::Verwijderen if opruimbaar => sla_over(rapport, format!("{probleem}; te groot om vanzelf op te ruimen")),
            _ => sla_over(rapport, probleem.to_string()),
        }
    }
    uit
}
