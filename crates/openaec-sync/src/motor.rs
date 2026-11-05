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
//!
//! Samenvoegen (ontwerp samenwerken §5.6): kent de opslag vergelijk-en-schrijf ([`Opslag::vergelijk_en_schrijf`]), dan
//! gaat een lokaal gewijzigd `.ifc`-bestand door de opslaan-lus van `openaec_samenvoegen`. Maakte een ander intussen een
//! nieuwe versie, dan voegt de lus samen en schrijft opnieuw; lukt dat niet, dan een conflictkopie zoals altijd, met het
//! rapport ernaast. Het lokale bestand schrijft de lus niet: de toestand legt vast "server op N, lokaal afgeleid van
//! momentopname S" ([`Afgeleid`]), en het resultaat komt via de veilige routine van [`crate::vervang`] op zijn plek.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::stream::{self, StreamExt};
use openaec_samenvoegen::{kan_samenvoegen, sla_op, voeg_samen, LusFout, LusOpties, Reden, Samenvoeging, Uitkomst, Weigering};
use serde::{Deserialize, Serialize};

pub use crate::api::Batchgrenzen;
use crate::blokken::{blokgrootte, hash_bestand, hash_bytes};
use crate::namen::{self, Negeerlijst};
use crate::onderweg::{self, Onderweg};
// Mapselectie: één filter voor uploaden en downloaden (zie `selectie`).
use crate::selectie::uitgesloten as uitgesloten_pad;
use crate::vervang;
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
    /// Het id van de versieregel van deze versie (een server met vergelijk-en-schrijf), anders None.
    pub versie_id: Option<String>,
}

impl OpslagBestand {
    /// Deze versie als [`Versieregel`], als de opslag het id kent.
    pub fn versieregel(&self) -> Option<Versieregel> {
        let versie_id = self.versie_id.clone()?;
        Some(Versieregel { bestand: self.bestand.clone(), versie: self.versie, versie_id })
    }
}

/// Resultaat van een upload: het pad kan een conflictkopie zijn.
#[derive(Debug, Clone)]
pub struct Upload {
    pub pad: String,
    pub bestand: String,
    pub versie: u64,
    pub conflict: bool,
    /// Het id van de nieuwe versieregel, als de opslag het kent.
    pub versie_id: Option<String>,
}

/// Een versie op de server, aangewezen met het id van haar versieregel (ontwerp §5.5): dat wijst altijd naar dezelfde
/// inhoud of naar niets. Het volgnummer is voor mensen, en voor `samengevoegd_uit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versieregel {
    pub bestand: String,
    pub versie: u64,
    pub versie_id: String,
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

    /// Zet deze opslag `bestand` zelf op `lokaal` neer, zonder inhoud op deze computer (een placeholder die alleen online
    /// staat)? Dan Some met de uitkomst: daar is geen lokaal werk te beschermen. Bij succes het kenmerk (grootte,
    /// wijzigingstijd) dat de bewerking zelf zette: dat hoort bij de versie op de server. Nooit een meting achteraf: wie
    /// het bestand intussen opent en bewaart, heeft dan een lokale wijziging. None: niet van toepassing (meer); dan
    /// downloadt de motor naar een tijdelijk bestand en zet hij het veilig op zijn plek (zie `crate::vervang`). De
    /// controle en het neerzetten horen bij elkaar: een bewerking die een bestand van de gebruiker kan raken, mag hier
    /// niet; is de placeholder niet (meer) mogelijk, dan None, en nooit een gewone download op het pad. Standaard None.
    async fn plaats_zelf(&self, bestand: &OpslagBestand, lokaal: &Path) -> Option<Result<(u64, u128), Fout>> {
        let _ = (bestand, lokaal);
        None
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

    /// Kent deze opslag vergelijk-en-schrijf ([`Opslag::schrijf_als`], [`Opslag::lees_versie`])? Dan gaat een lokaal
    /// gewijzigd IFC-bestand door de opslaan-lus (ontwerp §5.6). Standaard niet: dan blijft alles zoals het was.
    async fn vergelijk_en_schrijf(&self) -> bool {
        false
    }

    /// Upload `lokaal` als nieuwe versie van `pad`, alleen als `basis` nog de huidige versie van dat bestand is. Anders
    /// een weigering ([`Fout::foutcode`]): `verouderde_basis` (met de huidige versie, zie [`Fout::huidige`]),
    /// `onbekende_basis` of `bestand_vergrendeld`; nooit een conflictkopie. `samengevoegd_uit`: (basis, hun), de
    /// volgnummers waaruit deze inhoud is samengevoegd.
    async fn schrijf_als(&self, pad: &str, lokaal: &Path, basis: &Versieregel, samengevoegd_uit: Option<(u64, u64)>) -> Result<Upload, Fout> {
        let _ = (pad, lokaal, basis, samengevoegd_uit);
        Err(Fout::Controle("deze opslag kent geen vergelijk-en-schrijf".into()))
    }

    /// De inhoud van precies deze versie; een weigering met `onbekende_basis` als ze niet (meer) bij het bestand hoort.
    async fn lees_versie(&self, versie: &Versieregel) -> Result<Vec<u8>, Fout> {
        let _ = versie;
        Err(Fout::Controle("deze opslag kent geen versieregels".into()))
    }

    /// Blokgrootte en blokhashes van deze versie, als de opslag ze kent: zo krijgt een bestand van vóór de versieregels
    /// zijn versieregel, maar alleen als het hier precies die inhoud heeft. Standaard None.
    async fn blokhashes(&self, versie: &Versieregel) -> Result<Option<(u64, Vec<String>)>, Fout> {
        let _ = versie;
        Ok(None)
    }

    /// Hernoem of verplaats `bestand` (het id op de server) naar `naar` (pad met '/'); een ontbrekende map komt er eerst
    /// bij. Staat het bestand al op `naar`, dan is dat goed (een herhaling na een verloren antwoord). Voor een hernoeming
    /// die hier gebeurde, ook offline (zie `docs/samenwerken/offline.md`). Standaard kan het niet: dan wordt een
    /// hernoeming een nieuw bestand plus een verwijdering, zoals altijd.
    async fn verplaats(&self, bestand: &str, naar: &str) -> Result<(), Fout> {
        let _ = (bestand, naar);
        Err(Fout::Controle("deze opslag kent geen verplaatsen".into()))
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
    /// De sessie verliep onderweg en vernieuwen lukte niet ([`Fout::SessieVerlopen`]): de ronde is netjes gestopt. Wat
    /// al gedaan was, is bewaard; de rest volgt in een ronde na opnieuw inloggen. Die bestanden staan niet in
    /// [`Rapport::fouten`].
    pub sessie_verlopen: bool,
    /// Bestanden die eerder steeds mislukten en deze ronde wachten (geen nieuwe poging, geen nieuwe fout).
    pub wachtend: Vec<Mislukt>,
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
    /// Het id van de versieregel (een server met vergelijk-en-schrijf); None bij een bestand van daarvóór.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    versie_id: Option<String>,
    /// De server staat op deze versie (een samenvoeging), maar het bestand hier (met dit kenmerk) is nog de eigen invoer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    afgeleid: Option<Afgeleid>,
}

/// "Server op N, lokaal afgeleid van S" (ontwerp §5.6, "Terugzetten in de lokale map", punt 1 en 2): de opslaan-lus
/// schreef N, het lokale bestand is nog momentopname S. Is het dat nog, dan haalt de volgende ronde N op; anders voegt
/// hij S, het lokale bestand en de huidige versie samen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Afgeleid {
    /// Momentopname S (de eigen invoer) in `.openaec/eigen/`.
    pub eigen: String,
    /// Het volgnummer waarvan S een bewerking is. Wat nog op S gebaseerd is en zonder samenvoegen naar de server gaat
    /// (een bron die toch nog wijzigde, een server zonder vergelijk-en-schrijf), gaat met deze basis: dat wordt een
    /// conflictkopie, nooit een versie over de samenvoeging heen.
    pub van: u64,
}

impl Bekend {
    /// De basis voor een upload zonder samenvoegen (zie [`Afgeleid::van`]).
    fn basis(&self) -> u64 {
        self.afgeleid.as_ref().map_or(self.versie, |a| a.van)
    }
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
    /// Per pad op de server: een overdracht die steeds mislukt. Pas na de wachttijd opnieuw (zie [`wachttijd`]), of
    /// eerder als het bestand hier of op de server veranderde.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    mislukt: BTreeMap<String, Poging>,
    /// Hier hernoemd of verplaatst, nog niet op de server: nieuw pad (de sleutel in `bestanden`) -> pad op de server
    /// (zie [`zoek_verplaatsingen`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    verplaatst: BTreeMap<String, String>,
}

/// Een mislukte overdracht van één bestand: hoe vaak, tot wanneer er gewacht wordt, en hoe het bestand er toen bij stond.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Poging {
    keer: u32,
    /// Unix-tijd in seconden.
    tot: u64,
    reden: String,
    /// Grootte en wijzigingstijd hier, en de versie op de server, bij de laatste poging.
    lokaal: Option<(u64, u128)>,
    server: Option<u64>,
}

/// Een bestand dat steeds niet lukt en deze ronde niet opnieuw geprobeerd is (wacht tot [`Mislukt::opnieuw_om`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mislukt {
    /// Pad hier (met '/').
    pub pad: String,
    pub reden: String,
    /// Zo vaak mislukt.
    pub keer: u32,
    /// Unix-tijd in seconden van de volgende poging.
    pub opnieuw_om: u64,
}

/// Wachttijd na de `keer`-ste mislukte poging: 2 minuten, daarna steeds twee keer zo lang, hoogstens 6 uur.
pub fn wachttijd(keer: u32) -> u64 {
    (120u64 << keer.saturating_sub(1).min(16)).min(6 * 3600)
}

fn nu_seconden() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
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
    let mut f = std::fs::File::create(&tijdelijk)?;
    std::io::Write::write_all(&mut f, &serde_json::to_vec_pretty(toestand).unwrap())?;
    // Duurzaam: het journaal van een vervanging gaat pas weg als de toestand die het verwerkt, echt op schijf staat. Ook
    // de map: anders kan na een stroomstoring de oude toestand terugkomen.
    f.sync_all()?;
    std::fs::rename(tijdelijk, &pad)?;
    vervang::sync_map(pad.parent().unwrap())?;
    Ok(())
}

fn lokaal_pad(root: &Path, pad: &str) -> PathBuf {
    pad.split('/').fold(root.to_path_buf(), |p, deel| p.join(deel))
}

/// Het bestand `rel` (met '/') in `root`; met een lastige naam letterlijk (`\\?\` op Windows), anders zou Windows
/// er een apparaat of een andere naam van maken.
pub(crate) fn lokaal_bestand(root: &Path, rel: &str) -> PathBuf {
    let pad = lokaal_pad(root, rel);
    if namen::padprobleem(rel).is_some() || rel.split('/').any(namen::letterlijk_nodig) { namen::lang_pad(&pad) } else { pad }
}

pub(crate) fn kenmerk(pad: &Path) -> std::io::Result<(u64, u128)> {
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
            let letterlijk = namen::naamprobleem(&naam).is_some() || namen::letterlijk_nodig(&naam);
            let pad = if letterlijk { namen::lang_pad(&item.path()) } else { item.path() };
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
        .filter(|(hier, b)| b.afgeleid.is_none() && kenmerk(&lokaal_bestand(root, hier)).is_ok_and(|k| k == (b.grootte, b.mtime_ns)))
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
            Some(b) if b.afgeleid.is_none() && (b.grootte, b.mtime_ns) == k => in_sync.push(BekendBestand {
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
    /// Lokaal gewijzigd IFC-bestand naar een opslag met vergelijk-en-schrijf: door de opslaan-lus.
    Samenvoegen { pad: String, server: OpslagBestand },
    /// Een bekend bestand zonder versieregel (van daarvóór), hier en op de server ongewijzigd: neem de versieregel van
    /// de server over als de inhoud hier precies die versie is.
    Overnemen { pad: String, server: OpslagBestand },
}

impl Taak {
    fn pad(&self) -> &str {
        match self {
            Taak::Upload { pad, .. }
            | Taak::Download { pad, .. }
            | Taak::VerwijderLokaal { pad }
            | Taak::VerwijderServer { pad, .. }
            | Taak::Samenvoegen { pad, .. }
            | Taak::Overnemen { pad, .. } => pad,
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
    /// Lokale bestanden en de bestanden op de server bij het begin van de ronde (voor [`Poging`]).
    lokaal: &'a BTreeMap<String, (u64, u128)>,
    server: &'a BTreeMap<String, OpslagBestand>,
    /// De sessie is verlopen: geen nieuwe taken meer, en geen fout per bestand.
    gestopt: AtomicBool,
    /// De opslag kent versieregels en vergelijk-en-schrijf: dan legt de ronde elke upload eerst vast (zie
    /// [`crate::onderweg`]), want alleen dan is een eigen upload later terug te vinden.
    samen: bool,
    /// Uploads onderweg waarvan de uitkomst in de toestand komt: weg zodra die op schijf staat.
    afgehandeld: Mutex<Vec<String>>,
}

impl<O: Opslag> Ronde<'_, O> {
    /// Nog nieuwe taken beginnen? Niet bij pauzeren, en niet als de sessie verlopen is.
    fn stoppen(&self) -> bool {
        self.gestopt.load(Ordering::SeqCst) || self.waarnemer.gepauzeerd()
    }

    /// Het pad hier (met '/') voor een pad op de server; meldingen en het rapport gebruiken dat pad.
    fn rel(&self, sleutel: &str) -> String {
        self.toestand.lock().unwrap().hier(sleutel)
    }

    /// Het lokale bestand voor een pad op de server.
    fn hier(&self, sleutel: &str) -> PathBuf {
        lokaal_bestand(self.root, &self.rel(sleutel))
    }

    /// Neem `pad` op in de toestand: lokaal en op de server zijn nu gelijk (komt in [`Rapport::bijgewerkt`]). Met het
    /// kenmerk (grootte, wijzigingstijd) dat bij die versie hoort, gemeten vóór het bestand op zijn plek kwam (of gezet
    /// door de bewerking zelf): een programma dat er daarna meteen in schrijft, geeft zo een lokale wijziging.
    fn onthoud_als(&self, pad: &str, bestand: &str, versie: u64, versie_id: Option<&str>, (grootte, mtime_ns): (u64, u128)) {
        let rel = self.rel(pad);
        let versie_id = versie_id.map(str::to_string);
        self.toestand
            .lock()
            .unwrap()
            .bestanden
            .insert(pad.to_string(), Bekend { bestand: bestand.to_string(), versie, grootte, mtime_ns, versie_id, afgeleid: None });
        self.rapport.lock().unwrap().bijgewerkt.push(rel);
    }

    fn vergeet(&self, pad: &str) {
        let mut toestand = self.toestand.lock().unwrap();
        toestand.bestanden.remove(pad);
        toestand.lokale_namen.remove(pad);
    }

    fn tel(&self, f: impl FnOnce(&mut Rapport)) {
        f(&mut self.rapport.lock().unwrap());
    }

    /// Upload met meldingen (start en bytevoortgang). Eerst vastgelegd (zie [`Ronde::noteer_bestand`]).
    async fn upload(&self, pad: &str, lokaal: &Path, basis: Option<u64>) -> Result<Upload, Fout> {
        let totaal = std::fs::metadata(lokaal).map(|m| m.len()).unwrap_or(0);
        let rel = self.rel(pad);
        let id = self.noteer_bestand(pad, lokaal).await?;
        self.waarnemer.melding(Gebeurtenis::Start { pad: rel.clone(), richting: Richting::Upload, totaal });
        let teller = Teller::new(self.waarnemer, &rel, Richting::Upload);
        let up = self.opslag.upload_met_voortgang(pad, lokaal, basis, &|gedaan, totaal| teller.stand(gedaan, totaal)).await?;
        self.afgehandeld(id);
        Ok(up)
    }

    /// Leg vóór de upload van `lokaal` naar `sleutel` vast welke inhoud er gaat (zie [`crate::onderweg`]): het lokale
    /// bestand zelf is dan de momentopname S. Veranderde het tijdens het lezen, dan niets: er is geen vaste inhoud om
    /// later terug te vinden (in het ergste geval wordt het één versie te veel). Geeft het id.
    async fn noteer_bestand(&self, sleutel: &str, lokaal: &Path) -> Result<Option<String>, Fout> {
        if !self.samen {
            return Ok(None);
        }
        let k = kenmerk(lokaal)?;
        let hashes = hash_bestand(lokaal, blokgrootte(k.0)).await?;
        if kenmerk(lokaal)? != k {
            return Ok(None);
        }
        self.noteer(Onderweg::nieuw(sleutel, k.0, hashes, k, None)).map(Some)
    }

    fn noteer(&self, o: Onderweg) -> Result<String, Fout> {
        onderweg::schrijf(self.root, &o)?;
        Ok(o.id)
    }

    /// De upload is aangekomen: de uitkomst komt met de toestand op schijf, en dan mag het bestand onderweg weg.
    fn afgehandeld(&self, id: Option<String>) {
        if let Some(id) = id {
            self.afgehandeld.lock().unwrap().push(id);
        }
    }

    /// Download met meldingen (start en bytevoortgang); `pad` is het pad op de server waarop het bestand hier
    /// bijgehouden wordt (bij een conflict niet per se dat van `server`).
    async fn download(&self, pad: &str, server: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
        let rel = self.rel(pad);
        self.waarnemer.melding(Gebeurtenis::Start { pad: rel.clone(), richting: Richting::Download, totaal: server.grootte });
        let teller = Teller::new(self.waarnemer, &rel, Richting::Download);
        self.opslag.download_met_voortgang(server, lokaal, &|gedaan, totaal| teller.stand(gedaan, totaal)).await
    }

    /// Wat er na een geslaagde upload moet gebeuren: opnemen, of bij een conflict de serverversie terughalen. `verstuurd`
    /// is het kenmerk (grootte, wijzigingstijd) van het lokale bestand vlak vóór de upload begon: alleen dat hoort bij de
    /// inhoud op de server. Sloeg de gebruiker tijdens de upload op, dan is het bestand nu anders, en ziet de volgende
    /// ronde dat als lokale wijziging (nooit het kenmerk van nu: dan zou dat werk als "op de server" gelden).
    async fn na_upload(&self, pad: &str, server: Option<&OpslagBestand>, up: Upload, verstuurd: (u64, u128)) -> Result<Actie, Fout> {
        match server {
            Some(server) if up.conflict => {
                // Mijn versie staat nu als conflictkopie op de server: lokaal hernoemen (zonder iets te vervangen), en
                // de serverversie op de oorspronkelijke plek zetten. Niets gaat verloren. Hernoemen houdt het kenmerk.
                vervang::plaats(&self.hier(pad), &self.hier(&up.pad))?;
                self.onthoud_als(&up.pad, &up.bestand, up.versie, up.versie_id.as_deref(), verstuurd);
                let gedownload = self.haal_op(pad, server).await?;
                self.tel(|r| {
                    r.conflicten += 1;
                    r.gedownload += u32::from(gedownload);
                });
                Ok(Actie::Conflict)
            }
            Some(_) => {
                self.onthoud_als(pad, &up.bestand, up.versie, up.versie_id.as_deref(), verstuurd);
                self.tel(|r| r.geupload += 1);
                Ok(Actie::Geupload)
            }
            None => {
                self.onthoud_als(&up.pad, &up.bestand, up.versie, up.versie_id.as_deref(), verstuurd);
                self.tel(|r| r.geupload += 1);
                Ok(Actie::Geupload)
            }
        }
    }

    /// Haal `server` op naar het pad hier van `pad`, zonder lokaal werk te vervangen. Geeft false als het niet
    /// geplaatst is (lokaal net gewijzigd, of er stond intussen een ander bestand): dat ziet de volgende ronde als
    /// lokale wijziging.
    async fn haal_op(&self, pad: &str, server: &OpslagBestand) -> Result<bool, Fout> {
        let lokaal = self.hier(pad);
        if self.placeholder(pad, server, &lokaal).await? {
            return Ok(true);
        }
        let nieuw = vervang::nieuw_pad(self.root)?;
        // Blokken die hier al kloppen, hoeven niet over de lijn: de download begint met een kopie van het lokale bestand.
        let _ = std::fs::copy(&lokaal, &nieuw);
        if let Err(fout) = self.download(pad, server, &nieuw).await {
            let _ = std::fs::remove_file(&nieuw);
            return Err(fout);
        }
        self.plaats_download(pad, server, nieuw).await
    }

    /// Laat de opslag `server` zelf als placeholder op `lokaal` zetten (zie [`Opslag::plaats_zelf`]); true als dat
    /// gebeurde, false als het niet van toepassing is (dan gaat de download door de veilige routine).
    async fn placeholder(&self, pad: &str, server: &OpslagBestand, lokaal: &Path) -> Result<bool, Fout> {
        let Some(r) = self.opslag.plaats_zelf(server, lokaal).await else { return Ok(false) };
        let k = r?;
        self.waarnemer.melding(Gebeurtenis::Start { pad: self.rel(pad), richting: Richting::Download, totaal: 0 });
        self.onthoud_als(pad, &server.bestand, server.versie, server.versie_id.as_deref(), k);
        Ok(true)
    }

    /// Zet een download (het tijdelijke bestand `nieuw`) veilig op het pad van `pad`; als [`Ronde::haal_op`].
    async fn plaats_download(&self, pad: &str, server: &OpslagBestand, nieuw: PathBuf) -> Result<bool, Fout> {
        let versie = vervang::Versie { bestand: server.bestand.clone(), versie: server.versie, versie_id: server.versie_id.clone() };
        match self.vervang_hier(pad, Some((nieuw, versie))).await? {
            vervang::Uitkomst::Geplaatst(k) => {
                self.onthoud_als(pad, &server.bestand, server.versie, server.versie_id.as_deref(), k);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Verwijder het bestand hier van `pad` veilig. Geeft false als het lokaal net gewijzigd is: dan blijft het staan.
    async fn verwijder_hier(&self, pad: &str) -> Result<bool, Fout> {
        Ok(self.vervang_hier(pad, None).await? == vervang::Uitkomst::Verwijderd)
    }

    /// De veilige vervanging of verwijdering van het bestand hier van `pad` (zie [`vervang::vervang`]), met wat de
    /// toestand ervan weet. Bij een afgeleid bestand is de basis de versie waarvan het een bewerking is (zie
    /// [`Afgeleid::van`]).
    async fn vervang_hier(&self, pad: &str, nieuw: Option<(PathBuf, vervang::Versie)>) -> Result<vervang::Uitkomst, Fout> {
        let rel = self.rel(pad);
        let bekend = self.toestand.lock().unwrap().bestanden.get(pad).map(|b| {
            let versie_id = b.versie_id.clone().filter(|_| b.afgeleid.is_none());
            (vervang::Versie { bestand: b.bestand.clone(), versie: b.basis(), versie_id }, (b.grootte, b.mtime_ns))
        });
        let opdracht = vervang::Opdracht {
            root: self.root.to_path_buf(),
            lokaal: lokaal_bestand(self.root, &rel),
            pad: rel,
            sleutel: pad.to_string(),
            bekend,
            nieuw,
        };
        // Een groot bestand lezen en hashen: niet op de thread van de ronde.
        Ok(tokio::task::spawn_blocking(move || vervang::vervang(opdracht)).await.map_err(std::io::Error::other)??)
    }

    /// Rond de bronnen af, ook die van eerdere rondes (zie [`vervang::rond_af`]). Een afwijkende bron is een lokale
    /// wijziging, afgeleid van de verwerkte inhoud: zie [`Ronde::lever_in`]. Geeft true als de toestand veranderde.
    async fn rond_af(&self) -> bool {
        let root = self.root.to_path_buf();
        let afwijkend =
            tokio::task::spawn_blocking(move || vervang::rond_af(&root, |rel| lokaal_bestand(&root, rel), false)).await.unwrap_or_default();
        let mut veranderd = false;
        for v in afwijkend {
            if self.stoppen() || self.opslag.alleen_lezen() {
                log::info!("{}: de bron wijkt af en blijft bewaakt (gestopt, of alleen lezen)", v.pad);
                continue;
            }
            veranderd |= self.lever_in(&v).await;
        }
        veranderd
    }

    /// Een afwijkende bron naar de server, langs de gewone uploadweg. Bij een vervanging met de verwerkte versie als
    /// basis: de server is intussen verder, dus dat wordt een conflictkopie, en die komt hier naast het bestand. Bij
    /// een verwijdering als nieuw bestand (een conflictkopie als er weer een bestand met die naam staat), en dan terug
    /// op het pad. Pas daarna gaat de bron weg; geeft true als er iets in de toestand kwam.
    async fn lever_in(&self, v: &vervang::Vervanging) -> bool {
        let bron = vervang::bron(self.root, &v.id);
        let k = match kenmerk(&bron) {
            Ok(k) => k,
            Err(e) => {
                log::warn!("{}: bron niet te lezen: {e}", v.pad);
                return false;
            }
        };
        let basis = v.nieuw.as_ref().map(|_| v.basis.versie);
        let up = match self.upload(&v.sleutel, &bron, basis).await {
            Ok(up) => up,
            Err(fout) if fout.sessie_verlopen() => {
                self.gestopt.store(true, Ordering::SeqCst);
                return false;
            }
            Err(fout) => {
                let pad = self.rel(&v.sleutel);
                self.waarnemer.melding(Gebeurtenis::Fout { pad: pad.clone(), fout: fout.to_string() });
                self.rapport.lock().unwrap().fout(&pad, fout);
                return false;
            }
        };
        // De inhoud staat nu op de server: eerst vastleggen dat de bron niet meer nodig is. Lukt het plaatsen hier niet
        // (er staat al iets), dan haalt een volgende ronde hem op; de bron mag dus hoe dan ook weg.
        let v = &vervang::afgerond(self.root, v).unwrap_or_else(|e| {
            log::warn!("{}: afronding niet vast te leggen: {e}", v.pad);
            v.clone()
        });
        let plek = (up.conflict || v.nieuw.is_none()).then(|| self.hier(&up.pad));
        let geplaatst = plek.is_some_and(|p| {
            p.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| vervang::plaats(&bron, &p)).is_ok()
        });
        if geplaatst {
            self.onthoud_als(&up.pad, &up.bestand, up.versie, up.versie_id.as_deref(), k);
        }
        if let Err(e) = vervang::klaar(self.root, v) {
            log::warn!("{}: bron niet op te ruimen: {e}", v.pad);
        }
        let actie = if up.conflict { Actie::Conflict } else { Actie::Geupload };
        self.tel(|r| match actie {
            Actie::Conflict => r.conflicten += 1,
            _ => r.geupload += 1,
        });
        self.waarnemer.melding(Gebeurtenis::Klaar { pad: self.rel(&up.pad), actie });
        geplaatst
    }

    /// Een lokaal gewijzigd IFC-bestand naar een opslag met vergelijk-en-schrijf, via de opslaan-lus (ontwerp §5.6).
    /// Maakte een ander intussen een nieuwe versie, dan voegt de lus samen. Lukt dat niet, dan een conflictkopie zoals
    /// altijd, met het rapport ernaast. Een fout die een nieuwe poging verdient (netwerk, sessie), is gewoon een fout.
    async fn voeg_samen_op(&self, pad: &str, server: &OpslagBestand) -> Result<Option<Actie>, Fout> {
        let bekend = self.toestand.lock().unwrap().bestanden.get(pad).cloned();
        let (Some(bekend), Some(huidig)) = (bekend, server.versieregel()) else { return self.als_kopie(pad, server, None).await };
        let Some(bekend_id) = bekend.versie_id.clone() else { return self.als_kopie(pad, server, None).await };
        // De momentopname: de inhoud, met hetzelfde kenmerk vóór en na het lezen. Anders slaat de gebruiker net op: dan
        // de volgende ronde.
        let lokaal = self.hier(pad);
        let k = kenmerk(&lokaal)?;
        let ons = tokio::fs::read(&lokaal).await?;
        if kenmerk(&lokaal)? != k {
            return Ok(None);
        }
        let (basis, inhoud, van) = match &bekend.afgeleid {
            None => (LusBasis { bestand: bekend.bestand.clone(), versie: LusVersie::van(&bekend_id, bekend.versie) }, ons.clone(), bekend.versie),
            Some(a) => {
                // Afgeleid van momentopname S en daarna gewijzigd: eerst S, ons en de huidige versie samenvoegen (punt 2
                // van "Terugzetten in de lokale map"). S is als basis veilig: de server bevat S helemaal.
                if huidig.bestand != bekend.bestand {
                    let r = Samenvoegrapport::nieuw("onbekende_basis", "op deze plek staat nu een ander bestand", &bekend, Some(&huidig));
                    return self.als_kopie(pad, server, Some(r)).await;
                }
                let s = match lees_eigen(self.root, &a.eigen) {
                    Ok(s) => s,
                    Err(e) => {
                        let r = Samenvoegrapport::nieuw("basis_niet_te_lezen", &format!("momentopname niet te lezen: {e}"), &bekend, Some(&huidig));
                        return self.als_kopie(pad, server, Some(r)).await;
                    }
                };
                let hun = match self.opslag.lees_versie(&huidig).await {
                    Ok(h) => h,
                    Err(f) if f.foutcode().as_deref() == Some("onbekende_basis") => {
                        let r = Samenvoegrapport::nieuw("onbekende_basis", &f.to_string(), &bekend, Some(&huidig));
                        return self.als_kopie(pad, server, Some(r)).await;
                    }
                    Err(f) => return Err(f),
                };
                let l = ons.clone();
                let samen = tokio::task::spawn_blocking(move || voeg_samen(&s, &l, &hun)).await.map_err(std::io::Error::other)?;
                match samen {
                    Ok(Samenvoeging { inhoud: Some(r), .. }) => (LusBasis { bestand: huidig.bestand.clone(), versie: LusVersie::van(&huidig.versie_id, huidig.versie) }, r, a.van),
                    Ok(sv) => {
                        let mut r = Samenvoegrapport::nieuw("conflicten", "beide kanten wijzigden hetzelfde", &bekend, Some(&huidig));
                        r.samenvoeging = Some(sv);
                        return self.als_kopie(pad, server, Some(r)).await;
                    }
                    Err(f) => {
                        let r = Samenvoegrapport::nieuw("niet_samen_te_voegen", &f.to_string(), &bekend, Some(&huidig));
                        return self.als_kopie(pad, server, Some(r)).await;
                    }
                }
            }
        };
        self.waarnemer.melding(Gebeurtenis::Start { pad: self.rel(pad), richting: Richting::Upload, totaal: inhoud.len() as u64 });
        let lijst = LusBasis { bestand: huidig.bestand.clone(), versie: LusVersie::van(&huidig.versie_id, huidig.versie) };
        let mut eigen = None;
        let (uitkomst, laatste) = self.lus(pad, basis, lijst, inhoud, (&ons, k, van), &mut eigen).await?;
        let (reden, melding, tegen, samenvoeging) = match uitkomst {
            Ok(Uitkomst::Opgeslagen { nieuw, inhoud, .. }) => return self.na_samenvoegen(pad, nieuw, inhoud, &ons, k, van, eigen).await.map(Some),
            Ok(Uitkomst::Conflict { huidige, reden, .. }) => match reden {
                Reden::Samenvoegen(sv) => ("conflicten", "beide kanten wijzigden hetzelfde".to_string(), huidige, Some(*sv)),
                Reden::OnbekendeBasis => ("onbekende_basis", "de basis hoort niet (meer) bij het bestand op deze plek".to_string(), huidige, None),
                // Niet te lezen door een fout die een nieuwe poging verdient: geen kopie, de volgende ronde opnieuw.
                Reden::BasisNietTeLezen(_) if laatste.as_ref().is_some_and(tijdelijk) => return Err(laatste.unwrap()),
                Reden::BasisNietTeLezen(m) => ("basis_niet_te_lezen", m, huidige, None),
            },
            Err(LusFout::Opslag(m)) => return Err(laatste.unwrap_or(Fout::Controle(m))),
            Err(LusFout::TeVaakVerouderd { pogingen, huidige }) => {
                ("te_vaak_verouderd", format!("na {pogingen} pogingen was er steeds een nieuwere versie"), Some(huidige), None)
            }
            Err(f @ LusFout::Vergrendeld { .. }) => ("vergrendeld", f.to_string(), None, None),
            Err(f @ LusFout::Fout(_)) => ("niet_samen_te_voegen", f.to_string(), None, None),
        };
        let tegen = tegen.map(|b| Versieregel { bestand: b.bestand, versie: b.versie.nummer, versie_id: b.versie.id });
        let mut r = Samenvoegrapport::nieuw(reden, &melding, &bekend, tegen.as_ref());
        r.samenvoeging = samenvoeging;
        self.als_kopie(pad, server, Some(r)).await
    }

    /// Draai de opslaan-lus van `openaec_samenvoegen` voor `pad`. De lus draait op een eigen thread (hij voegt samen en
    /// wacht op de server); elk verzoek aan de server komt via een kanaal hier terug, en de ronde voert het uit met de
    /// echte opslag. `lijst`: de versie uit de lijst van deze ronde. Geeft ook de laatste fout van de opslag: een
    /// weigering draagt alleen tekst. `s` en `eigen`: zie [`Ronde::schrijf_inhoud`].
    async fn lus(
        &self,
        pad: &str,
        basis: LusBasis,
        lijst: LusBasis,
        ons: Vec<u8>,
        s: Momentopname<'_>,
        eigen: &mut Option<String>,
    ) -> Result<(LusUitkomst, Option<Fout>), Fout> {
        let (vragen, mut ontvang) = tokio::sync::mpsc::channel(1);
        let mut opslag = Lus { vragen, lijst };
        let draad = tokio::task::spawn_blocking(move || sla_op(&mut opslag, basis, &ons, &LusOpties::default()));
        let mut laatste = None;
        // De basis van de laatst geweigerde poging (verouderd): de volgende schrijfpoging is daaruit samengevoegd.
        let mut verouderd: Option<LusVersie> = None;
        while let Some(vraag) = ontvang.recv().await {
            match vraag {
                Vraag::Lees(b, terug) => {
                    let r = self.opslag.lees_versie(&regel(&b)).await.map_err(|f| als_weigering(f, &mut laatste));
                    let _ = terug.send(r);
                }
                Vraag::Schrijf(b, inhoud, terug) => {
                    let uit = verouderd.as_ref().filter(|v| **v != b.versie && 1 <= v.nummer && v.nummer < b.versie.nummer);
                    let samengevoegd_uit = uit.map(|v| (v.nummer, b.versie.nummer));
                    let r = match self.schrijf_inhoud(pad, &regel(&b), &inhoud, samengevoegd_uit, s, eigen).await {
                        Ok(up) if !up.conflict && up.versie_id.is_some() => {
                            Ok(LusBasis { bestand: up.bestand, versie: LusVersie { nummer: up.versie, id: up.versie_id.unwrap_or_default() } })
                        }
                        Ok(up) => Err(Weigering::Fout(format!("onverwacht antwoord op vergelijk-en-schrijf: {up:?}"))),
                        Err(f) => Err(als_weigering(f, &mut laatste)),
                    };
                    if let Err(Weigering::Verouderd { .. }) = &r {
                        verouderd = Some(b.versie.clone());
                    }
                    let _ = terug.send(r);
                }
            }
        }
        let uitkomst = draad.await.map_err(std::io::Error::other)?;
        Ok((uitkomst, laatste))
    }

    /// Schrijf `inhoud` met vergelijk-en-schrijf: eerst vastgelegd (zie [`crate::onderweg`]), dan naar een tijdelijk
    /// bestand in `.openaec/nieuw/`, dan de upload. `s` is de momentopname S van het lokale bestand. Is `inhoud` een
    /// samenvoeging van S, dan gaat S eerst naar `.openaec/eigen/` (één keer per lus; de naam komt in `eigen`): zo weet
    /// een volgende ronde na een onderbreking dat het lokale bestand nog de eigen invoer is (zie [`Afgeleid`]).
    async fn schrijf_inhoud(
        &self,
        pad: &str,
        basis: &Versieregel,
        inhoud: &[u8],
        samengevoegd_uit: Option<(u64, u64)>,
        (ons, k, van): Momentopname<'_>,
        eigen: &mut Option<String>,
    ) -> Result<Upload, Fout> {
        let afgeleid = match (inhoud == ons, eigen.clone()) {
            (true, _) => None,
            (false, Some(naam)) => Some(Afgeleid { eigen: naam, van }),
            (false, None) => {
                let (root, sleutel, s) = (self.root.to_path_buf(), pad.to_string(), ons.to_vec());
                let naam = tokio::task::spawn_blocking(move || bewaar_eigen(&root, &sleutel, &s)).await.map_err(std::io::Error::other)??;
                *eigen = Some(naam.clone());
                Some(Afgeleid { eigen: naam, van })
            }
        };
        let grootte = inhoud.len() as u64;
        let id = self.noteer(Onderweg::nieuw(pad, grootte, hash_bytes(inhoud, blokgrootte(grootte)), k, afgeleid))?;
        let tijdelijk = vervang::nieuw_pad(self.root)?;
        tokio::fs::write(&tijdelijk, inhoud).await?;
        let r = self.opslag.schrijf_als(pad, &tijdelijk, basis, samengevoegd_uit).await;
        let _ = std::fs::remove_file(&tijdelijk);
        if r.is_ok() {
            self.afgehandeld(Some(id));
        }
        r
    }

    /// Na een geslaagde opslaan-lus. Is de inhoud op de server de eigen inhoud, dan is het een gewone upload. Anders is
    /// er samengevoegd: de eigen invoer gaat eerst naar `.openaec/eigen/` (ontwerp §5.6, "Wat er bewaard blijft"), de
    /// toestand legt duurzaam vast "server op N, lokaal afgeleid van S", en pas dan komt het resultaat via de veilige
    /// routine op het pad. Lukt dat niet (de gebruiker sloeg net op), dan beslist de volgende ronde. `eigen`: waar de lus
    /// de eigen invoer al bewaarde (zie [`Ronde::schrijf_inhoud`]).
    #[allow(clippy::too_many_arguments)]
    async fn na_samenvoegen(
        &self,
        pad: &str,
        nieuw: LusBasis,
        inhoud: Vec<u8>,
        ons: &[u8],
        k: (u64, u128),
        van: u64,
        eigen: Option<String>,
    ) -> Result<Actie, Fout> {
        let (bestand, versie, id) = (nieuw.bestand, nieuw.versie.nummer, nieuw.versie.id);
        Teller::new(self.waarnemer, &self.rel(pad), Richting::Upload).stand(k.0, k.0);
        self.tel(|r| r.geupload += 1);
        if inhoud == ons {
            self.onthoud_als(pad, &bestand, versie, Some(&id), k);
            return Ok(Actie::Geupload);
        }
        log::info!("{}: samengevoegd met het werk van een ander; nu versie {versie} op de server", self.rel(pad));
        // De lus schreef een andere inhoud dan de eigen invoer, dus die bewaarde hij al.
        let naam = eigen.ok_or_else(|| Fout::Controle("de eigen invoer van de samenvoeging is niet bewaard".into()))?;
        {
            let mut toestand = self.toestand.lock().unwrap();
            let afgeleid = Some(Afgeleid { eigen: naam, van });
            let b = Bekend { bestand: bestand.clone(), versie, grootte: k.0, mtime_ns: k.1, versie_id: Some(id.clone()), afgeleid };
            toestand.bestanden.insert(pad.to_string(), b);
            bewaar(self.root, &toestand)?;
        }
        let tijdelijk = vervang::nieuw_pad(self.root)?;
        tokio::fs::write(&tijdelijk, &inhoud).await?;
        let regel = vervang::Versie { bestand: bestand.clone(), versie, versie_id: Some(id.clone()) };
        if let vervang::Uitkomst::Geplaatst(k) = self.vervang_hier(pad, Some((tijdelijk, regel))).await? {
            self.onthoud_als(pad, &bestand, versie, Some(&id), k);
        }
        Ok(Actie::Geupload)
    }

    /// Niet samen te voegen: het lokale bestand als conflictkopie naar de server, zoals altijd (zie [`Ronde::na_upload`]),
    /// met het rapport ernaast (`<conflictkopie>.samenvoegrapport.json`; de volgende ronde neemt het mee naar de server).
    async fn als_kopie(&self, pad: &str, server: &OpslagBestand, rapport: Option<Samenvoegrapport>) -> Result<Option<Actie>, Fout> {
        if let Some(r) = &rapport {
            log::info!("{}: niet samengevoegd ({}): {}", self.rel(pad), r.reden, r.melding);
        }
        let lokaal = self.hier(pad);
        let verstuurd = kenmerk(&lokaal)?;
        let up = self.upload(pad, &lokaal, None).await?;
        let kopie = up.conflict.then(|| up.pad.clone());
        let actie = self.na_upload(pad, Some(server), up, verstuurd).await?;
        if let (Some(mut r), Some(kopie)) = (rapport, kopie) {
            r.bestand = self.rel(pad);
            r.conflictkopie = self.rel(&kopie);
            let doel = lokaal_bestand(self.root, &format!("{}.samenvoegrapport.json", r.conflictkopie));
            let geplaatst = vervang::nieuw_pad(self.root).and_then(|tijdelijk| {
                std::fs::write(&tijdelijk, serde_json::to_vec_pretty(&r).map_err(std::io::Error::other)?)?;
                vervang::plaats(&tijdelijk, &doel).inspect_err(|_| {
                    let _ = std::fs::remove_file(&tijdelijk);
                })
            });
            if let Err(e) = geplaatst {
                log::warn!("{}: rapport niet neer te zetten: {e}", doel.display());
            }
        }
        Ok(Some(actie))
    }

    /// Een bekend bestand zonder versieregel krijgt die van de server, als de inhoud hier precies die versie is
    /// (gelijke blokhashes) en het bestand intussen niet veranderde. Een fout is geen fout van het bestand: de volgende
    /// ronde probeert het opnieuw (alleen een verlopen sessie stopt de ronde).
    async fn neem_regel_over(&self, pad: &str, server: &OpslagBestand) -> Result<Option<Actie>, Fout> {
        let Some(regel) = server.versieregel() else { return Ok(None) };
        let lokaal = self.hier(pad);
        let gelijk = async {
            let Some((blok, hashes)) = self.opslag.blokhashes(&regel).await? else { return Ok(None) };
            let k = kenmerk(&lokaal)?;
            let hier = crate::blokken::hash_bestand(&lokaal, blok).await?;
            Ok::<_, Fout>((hier == hashes && kenmerk(&lokaal)? == k).then_some(k))
        };
        match gelijk.await {
            Ok(Some(k)) => {
                let mut toestand = self.toestand.lock().unwrap();
                if let Some(b) = toestand.bestanden.get_mut(pad).filter(|b| (b.grootte, b.mtime_ns) == k && b.versie == regel.versie && b.bestand == regel.bestand) {
                    b.versie_id = Some(regel.versie_id);
                }
            }
            Ok(None) => {}
            Err(f) if f.sessie_verlopen() => return Err(f),
            Err(f) => log::info!("{}: versieregel niet over te nemen: {f}", self.rel(pad)),
        }
        Ok(None)
    }

    /// Eén taak uitvoeren; geeft terug wat er gebeurd is (None: niets, de volgende ronde neemt het mee).
    async fn voer_uit(&self, taak: &Taak) -> Result<Option<Actie>, Fout> {
        match taak {
            Taak::Upload { pad, basis, server } => {
                // Basis: de versie waarop de lokale wijziging gebaseerd is; de server beslist over een conflict.
                let lokaal = self.hier(pad);
                let verstuurd = kenmerk(&lokaal)?;
                let up = self.upload(pad, &lokaal, *basis).await?;
                self.na_upload(pad, server.as_ref(), up, verstuurd).await.map(Some)
            }
            Taak::Download { pad, server } => {
                if !self.haal_op(pad, server).await? {
                    return Ok(None);
                }
                self.tel(|r| r.gedownload += 1);
                Ok(Some(Actie::Gedownload))
            }
            Taak::VerwijderLokaal { pad } => {
                // Op de server verwijderd (staat daar in de prullenbak) en lokaal ongewijzigd: lokaal weg, maar alleen
                // de inhoud die de sync verwerkte.
                if !self.verwijder_hier(pad).await? {
                    return Ok(None);
                }
                self.vergeet(pad);
                self.tel(|r| r.verwijderd += 1);
                Ok(Some(Actie::Verwijderd))
            }
            Taak::VerwijderServer { pad, server } => {
                self.opslag.verwijder(server).await?;
                self.vergeet(pad);
                self.tel(|r| r.verwijderd += 1);
                Ok(Some(Actie::Verwijderd))
            }
            Taak::Samenvoegen { pad, server } => self.voeg_samen_op(pad, server).await,
            Taak::Overnemen { pad, server } => self.neem_regel_over(pad, server).await,
        }
    }

    /// Uitkomst van een taak melden; een fout komt in het rapport. Een verlopen sessie is geen fout van dit bestand:
    /// de ronde stopt (zie [`Rapport::sessie_verlopen`]).
    /// Een mislukte overdracht wacht daarna een tijd (zie [`wachttijd`]); een gelukte wist de eerdere pogingen.
    fn meld(&self, sleutel: &str, uitkomst: Result<Option<Actie>, Fout>) {
        let pad = &self.rel(sleutel);
        match uitkomst {
            // Niets gedaan (lokaal net gewijzigd): geen melding; de volgende ronde neemt het mee.
            Ok(None) => {}
            Ok(Some(actie)) => {
                self.toestand.lock().unwrap().mislukt.remove(sleutel);
                self.waarnemer.melding(Gebeurtenis::Klaar { pad: pad.to_string(), actie })
            }
            Err(fout) if fout.sessie_verlopen() => self.gestopt.store(true, Ordering::SeqCst),
            Err(fout) => {
                {
                    let mut toestand = self.toestand.lock().unwrap();
                    let keer = toestand.mislukt.get(sleutel).map_or(0, |p| p.keer) + 1;
                    let poging = Poging {
                        keer,
                        tot: nu_seconden() + wachttijd(keer),
                        reden: fout.to_string(),
                        lokaal: self.lokaal.get(sleutel).copied(),
                        server: self.server.get(sleutel).map(|s| s.versie),
                    };
                    toestand.mislukt.insert(sleutel.to_string(), poging);
                }
                self.waarnemer.melding(Gebeurtenis::Fout { pad: pad.to_string(), fout: fout.to_string() });
                self.rapport.lock().unwrap().fout(pad, fout);
            }
        }
    }

    /// Taak uitvoeren en melden; een fout komt in het rapport en houdt de andere taken niet op. Bij pauzeren
    /// begint de taak niet meer (de rest volgt in een volgende ronde).
    async fn verwerk(&self, taak: &Taak) {
        if self.stoppen() {
            return;
        }
        let uitkomst = self.voer_uit(taak).await;
        self.meld(taak.pad(), uitkomst);
    }

    /// Een groep kleine uploads in één verzoek. Weigert de opslag het verzoek als geheel, dan gaat elk bestand apart.
    async fn verwerk_kleine_uploads(&self, taken: &[Taak]) {
        if self.stoppen() {
            return;
        }
        // Per bestand het kenmerk vlak vóór de upload: alleen dat hoort straks bij de inhoud op de server (zie
        // `Ronde::na_upload`). Niet te lezen: dan elk bestand apart (dat meldt de fout per bestand).
        let mut paden: Vec<(&str, PathBuf, Option<u64>, Option<&OpslagBestand>, (u64, u128))> = Vec::new();
        for t in taken {
            let Taak::Upload { pad, basis, server } = t else { continue };
            let lokaal = self.hier(pad);
            let Ok(k) = kenmerk(&lokaal) else {
                for taak in taken {
                    self.verwerk(taak).await;
                }
                return;
            };
            paden.push((pad.as_str(), lokaal, *basis, server.as_ref(), k));
        }
        // Elk bestand eerst vastgelegd (zie `Ronde::noteer_bestand`); lukt dat niet, dan elk bestand apart.
        let mut ids = Vec::with_capacity(paden.len());
        for (pad, lokaal, _, _, _) in &paden {
            match self.noteer_bestand(pad, lokaal).await {
                Ok(id) => ids.push(id),
                Err(_) => {
                    for taak in taken {
                        self.verwerk(taak).await;
                    }
                    return;
                }
            }
        }
        for (pad, _, _, _, (grootte, _)) in &paden {
            self.waarnemer.melding(Gebeurtenis::Start { pad: self.rel(pad), richting: Richting::Upload, totaal: *grootte });
        }
        let items: Vec<KleinUpload> =
            paden.iter().map(|(pad, lokaal, basis, _, _)| KleinUpload { pad, lokaal, basis_versie: *basis }).collect();
        let resultaten = match self.opslag.upload_klein(&items).await {
            Ok(r) if r.len() == paden.len() => r,
            Err(fout) if fout.sessie_verlopen() => return self.gestopt.store(true, Ordering::SeqCst),
            _ => {
                for taak in taken {
                    self.verwerk(taak).await;
                }
                return;
            }
        };
        for (((pad, _, _, server, verstuurd), resultaat), id) in paden.iter().zip(resultaten).zip(ids) {
            let uitkomst = match resultaat {
                Ok(up) => {
                    self.afgehandeld(id);
                    Teller::new(self.waarnemer, &self.rel(pad), Richting::Upload).stand(verstuurd.0, verstuurd.0);
                    self.na_upload(pad, *server, up, *verstuurd).await.map(Some)
                }
                Err(fout) => Err(fout),
            };
            self.meld(pad, uitkomst);
        }
    }

    /// Een groep kleine downloads in één verzoek; als [`Ronde::verwerk_kleine_uploads`].
    async fn verwerk_kleine_downloads(&self, taken: &[Taak]) {
        if self.stoppen() {
            return;
        }
        // Een placeholder zet de opslag zelf neer; de rest gaat naar een tijdelijk bestand dat daarna veilig op zijn plek
        // komt (zie `Ronde::haal_op`).
        let mut paden: Vec<(&str, &OpslagBestand, PathBuf)> = Vec::new();
        let mut placeholders: Vec<&str> = Vec::new();
        for taak in taken {
            let Taak::Download { pad, server } = taak else { continue };
            match self.placeholder(pad, server, &self.hier(pad)).await {
                Ok(true) => {
                    placeholders.push(pad);
                    self.tel(|r| r.gedownload += 1);
                    self.meld(pad, Ok(Some(Actie::Gedownload)));
                    continue;
                }
                Ok(false) => {}
                Err(fout) => {
                    placeholders.push(pad);
                    self.meld(pad, Err(fout));
                    continue;
                }
            }
            match vervang::nieuw_pad(self.root) {
                Ok(p) => paden.push((pad.as_str(), server, p)),
                // Geen plek voor tijdelijke bestanden: de rest elk apart (dat meldt de fout per bestand).
                Err(_) => {
                    for (_, _, tijdelijk) in &paden {
                        let _ = std::fs::remove_file(tijdelijk);
                    }
                    for taak in taken.iter().filter(|t| !placeholders.contains(&t.pad())) {
                        self.verwerk(taak).await;
                    }
                    return;
                }
            }
        }
        if paden.is_empty() {
            return;
        }
        for (pad, server, _) in &paden {
            self.waarnemer.melding(Gebeurtenis::Start { pad: self.rel(pad), richting: Richting::Download, totaal: server.grootte });
        }
        let items: Vec<(&OpslagBestand, &Path)> = paden.iter().map(|(_, server, tijdelijk)| (*server, tijdelijk.as_path())).collect();
        let resultaten = match self.opslag.download_klein(&items).await {
            Ok(r) if r.len() == paden.len() => r,
            fout => {
                for (_, _, tijdelijk) in &paden {
                    let _ = std::fs::remove_file(tijdelijk);
                }
                if let Err(f) = &fout {
                    if f.sessie_verlopen() {
                        return self.gestopt.store(true, Ordering::SeqCst);
                    }
                }
                for taak in taken.iter().filter(|t| paden.iter().any(|(p, _, _)| *p == t.pad())) {
                    self.verwerk(taak).await;
                }
                return;
            }
        };
        for ((pad, server, tijdelijk), resultaat) in paden.into_iter().zip(resultaten) {
            let geplaatst = match resultaat {
                Err(fout) => {
                    let _ = std::fs::remove_file(tijdelijk);
                    Err(fout)
                }
                Ok(()) => {
                    Teller::new(self.waarnemer, &self.rel(pad), Richting::Download).stand(server.grootte, server.grootte);
                    self.plaats_download(pad, server, tijdelijk).await
                }
            };
            let uitkomst = geplaatst.map(|g| {
                if g {
                    self.tel(|r| r.gedownload += 1);
                }
                g.then_some(Actie::Gedownload)
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

/// Een versie op de server zoals de opslaan-lus haar kent: het id van de versieregel, en het volgnummer (voor mensen en
/// voor `samengevoegd_uit`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct LusVersie {
    id: String,
    nummer: u64,
}

impl LusVersie {
    fn van(id: &str, nummer: u64) -> Self {
        Self { id: id.to_string(), nummer }
    }
}

type LusBasis = openaec_samenvoegen::Basis<String, LusVersie>;
/// De momentopname S van het lokale bestand voor de opslaan-lus: de inhoud, het kenmerk (grootte, wijzigingstijd) en
/// het volgnummer waarvan S een bewerking is (zie [`Afgeleid::van`]).
type Momentopname<'a> = (&'a [u8], (u64, u128), u64);
type LusWeigering = Weigering<String, LusVersie>;
type LusUitkomst = openaec_samenvoegen::LusUitkomst<Lus>;
type Terug<T> = tokio::sync::oneshot::Sender<Result<T, LusWeigering>>;

fn regel(b: &LusBasis) -> Versieregel {
    Versieregel { bestand: b.bestand.clone(), versie: b.versie.nummer, versie_id: b.versie.id.clone() }
}

/// Een verzoek van de opslaan-lus aan de ronde (zie [`Ronde::lus`]).
enum Vraag {
    Lees(LusBasis, Terug<Vec<u8>>),
    Schrijf(LusBasis, Vec<u8>, Terug<LusBasis>),
}

/// De opslag zoals de opslaan-lus haar ziet, voor één bestand: elk verzoek gaat via een kanaal naar de ronde, en de lus
/// wacht op het antwoord (hij draait op een eigen thread).
struct Lus {
    vragen: tokio::sync::mpsc::Sender<Vraag>,
    /// De versie uit de lijst van deze ronde.
    lijst: LusBasis,
}

impl Lus {
    fn vraag<T>(&self, maak: impl FnOnce(Terug<T>) -> Vraag) -> Result<T, LusWeigering> {
        let gestopt = || Weigering::Fout("de ronde is gestopt".into());
        let (terug, antwoord) = tokio::sync::oneshot::channel();
        self.vragen.blocking_send(maak(terug)).map_err(|_| gestopt())?;
        antwoord.blocking_recv().map_err(|_| gestopt())?
    }
}

impl openaec_samenvoegen::Opslag for Lus {
    type Bestand = String;
    type Versie = LusVersie;

    /// De versie uit de lijst; geen eigen vraag aan de server. De lus vraagt dit niet: de server geeft de huidige
    /// versie bij een weigering (zie `sla_op`).
    fn huidige(&mut self) -> Result<LusBasis, LusWeigering> {
        Ok(self.lijst.clone())
    }

    fn lees(&mut self, versie: &LusBasis) -> Result<Vec<u8>, LusWeigering> {
        self.vraag(|terug| Vraag::Lees(versie.clone(), terug))
    }

    fn schrijf_als(&mut self, basis: &LusBasis, inhoud: &[u8]) -> Result<LusBasis, LusWeigering> {
        self.vraag(|terug| Vraag::Schrijf(basis.clone(), inhoud.to_vec(), terug))
    }
}

/// Een fout van de opslag als weigering voor de opslaan-lus; de fout zelf gaat naar `laatste` (een weigering draagt
/// alleen tekst, en een verlopen sessie moet de ronde stoppen).
fn als_weigering(f: Fout, laatste: &mut Option<Fout>) -> LusWeigering {
    let w = match f.foutcode().as_deref() {
        Some("verouderde_basis") => match f.huidige() {
            (Some(bestand), Some(nummer), Some(id)) => Weigering::Verouderd { huidige: LusBasis { bestand, versie: LusVersie { id, nummer } } },
            // Geen bestand meer op de plek: niets om mee samen te voegen.
            _ => Weigering::OnbekendeBasis,
        },
        Some("onbekende_basis") => Weigering::OnbekendeBasis,
        Some("bestand_vergrendeld") => Weigering::Vergrendeld { door: f.servermelding() },
        _ => Weigering::Fout(f.to_string()),
    };
    *laatste = Some(f);
    w
}

/// Een fout die een nieuwe poging verdient (netwerk, sessie, de server even niet), geen inhoudelijke weigering.
fn tijdelijk(f: &Fout) -> bool {
    matches!(f, Fout::Netwerk(_) | Fout::Bestand(_) | Fout::Link { .. } | Fout::SessieVerlopen)
        || matches!(f, Fout::Server { status, .. } if *status >= 500)
}

/// Het rapport naast een conflictkopie van een IFC-bestand dat niet samen te voegen was (ontwerp §5.6).
#[derive(Debug, Serialize)]
struct Samenvoegrapport {
    /// Het bestand hier (pad met '/'), en de conflictkopie met het eigen werk.
    bestand: String,
    conflictkopie: String,
    /// `conflicten`, `onbekende_basis`, `basis_niet_te_lezen`, `te_vaak_verouderd`, `vergrendeld` of
    /// `niet_samen_te_voegen` (geen geldig IFC-bestand).
    reden: &'static str,
    melding: String,
    /// De versie waarop het eigen werk gebaseerd was, en die waartegen het botste (als bekend).
    basis: u64,
    huidige: Option<u64>,
    /// Per object en attribuut de waarden van basis, ons en hun (bij `conflicten`).
    #[serde(skip_serializing_if = "Option::is_none")]
    samenvoeging: Option<Samenvoeging>,
}

impl Samenvoegrapport {
    fn nieuw(reden: &'static str, melding: &str, bekend: &Bekend, tegen: Option<&Versieregel>) -> Self {
        Samenvoegrapport {
            bestand: String::new(),
            conflictkopie: String::new(),
            reden,
            melding: melding.to_string(),
            basis: bekend.basis(),
            huidige: tegen.map(|v| v.versie),
            samenvoeging: None,
        }
    }
}

/// Map in `.openaec/` met de eigen invoer van samenvoegingen.
const EIGEN: &str = "eigen";
/// Zoveel keer eigen invoer blijft per bestand bewaard.
const EIGEN_BEWAARD: usize = 3;

/// Bewaar de eigen invoer van een samenvoeging duurzaam in `.openaec/eigen/` (ontwerp §5.6, "Wat er bewaard blijft"):
/// die heeft nooit als losse versie op de server gestaan. Per bestand de laatste [`EIGEN_BEWAARD`]. Geeft de naam.
fn bewaar_eigen(root: &Path, sleutel: &str, inhoud: &[u8]) -> std::io::Result<String> {
    let map = root.join(TOESTANDSMAP).join(EIGEN);
    std::fs::create_dir_all(&map)?;
    let soort = blake3::hash(sleutel.as_bytes()).to_hex()[..16].to_string();
    let nu = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let ext = Path::new(sleutel).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let naam = format!("{soort}-{nu:024}{ext}");
    let tijdelijk = map.join(format!("{naam}.tmp"));
    let mut f = std::fs::File::create(&tijdelijk)?;
    f.write_all(inhoud)?;
    f.sync_all()?;
    std::fs::rename(&tijdelijk, map.join(&naam))?;
    vervang::sync_map(&map)?;
    let mut van_bestand: Vec<String> = std::fs::read_dir(&map)?
        .flatten()
        .map(|i| i.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with(&format!("{soort}-")))
        .collect();
    van_bestand.sort();
    // Nieuwste eerst: die blijven (een afgeleid bestand wijst altijd naar de nieuwste).
    for oud in van_bestand.iter().rev().filter(|n| !n.ends_with(".tmp")).skip(EIGEN_BEWAARD) {
        let _ = std::fs::remove_file(map.join(oud));
    }
    Ok(naam)
}

fn lees_eigen(root: &Path, naam: &str) -> std::io::Result<Vec<u8>> {
    std::fs::read(root.join(TOESTANDSMAP).join(EIGEN).join(naam))
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
    // Eerst onafgemaakte vervangingen afmaken of terugzetten, vóór de vergelijking (zie `crate::vervang`).
    // Ook offline (vóór het eerste verzoek aan de server): werk in de bron van een verwijdering komt zo terug op het pad.
    let herstel = vervang::herstel(
        root,
        |rel| lokaal_bestand(root, rel),
        // Afgeleid: de toestand kent de versie, maar niet dat het bestand hier haar inhoud heeft.
        |sleutel, n| toestand.bestanden.get(sleutel).is_some_and(|b| b.bestand == n.bestand && b.versie == n.versie && b.afgeleid.is_none()),
    )?;
    for (sleutel, n, (grootte, mtime_ns)) in herstel.bijwerken {
        let b = Bekend { bestand: n.bestand, versie: n.versie, grootte, mtime_ns, versie_id: n.versie_id, afgeleid: None };
        toestand.bestanden.insert(sleutel, b);
    }
    // Een verwijdering staat, ook als de toestand van die ronde niet meer bewaard werd. Anders zou de vergelijking het
    // lege pad als "lokaal verwijderd" zien en het bestand op de server verwijderen. Eerst duurzaam (ook zonder server),
    // dan pas mag het journaal weg; zie `vervang::leg_vast`.
    for sleutel in &herstel.vergeten {
        toestand.bestanden.remove(sleutel);
        toestand.lokale_namen.remove(sleutel);
    }
    if !herstel.vergeten.is_empty() {
        bewaar(root, &toestand)?;
        vervang::leg_vast(root, |s| toestand.bestanden.contains_key(s))?;
    }
    // Werk in de bron van een verwijdering komt zo ook offline terug op het pad.
    vervang::rond_af(root, |rel| lokaal_bestand(root, rel), true);
    // De map aflopen kost bij duizenden bestanden even: op een aparte thread, zodat de runtime doorloopt.
    let ruw = {
        let root = root.to_path_buf();
        tokio::task::spawn_blocking(move || lokale_bestanden(&root)).await.map_err(std::io::Error::other)?
    };
    let negeer = Negeerlijst::laad(root);
    // Hernoemingen hier vastleggen, ook offline, en ze op de server doen vóór de lijst: dan ziet de vergelijking het
    // bestand op zijn nieuwe pad, met zijn basis (zie `docs/samenwerken/offline.md`).
    if !opslag.alleen_lezen() {
        let lopend: BTreeSet<String> = vervang::lees(root)?.into_iter().map(|v| v.sleutel).collect();
        let overslaan = |p: &str| {
            negeer.negeert(p) || uitgesloten_pad(p, uitgesloten) || lopend.contains(p) || herstel.geblokkeerd.contains_key(p)
        };
        if zoek_verplaatsingen(&mut toestand, &ruw, overslaan) {
            bewaar(root, &toestand)?;
        }
    }
    verplaats_op_server(opslag, root, &mut toestand).await?;
    let server: BTreeMap<String, OpslagBestand> =
        opslag.lijst().await?.into_iter().map(|b| (b.pad.clone(), b)).collect();
    // Uploads van een vorige ronde met een onbekende uitkomst (zie `crate::onderweg`): eerst kijken of ze aankwamen,
    // vóór er iets opnieuw gaat.
    let onderweg = onderweg::lees(root);
    for o in &onderweg {
        if let Some(b) = eigen_upload(opslag, &toestand, &server, o).await? {
            log::info!("{}: de upload van een vorige ronde kwam aan als versie {}", o.sleutel, b.versie);
            toestand.bestanden.insert(o.sleutel.clone(), b);
            toestand.mislukt.remove(&o.sleutel);
        }
    }
    let mut rapport = Rapport::default();
    for (sleutel, reden) in &herstel.geblokkeerd {
        rapport.fout(&toestand.hier(sleutel), reden);
    }
    let buiten = koppel_servernamen(&mut toestand, &server, &ruw, &negeer, opties, &mut rapport);
    let lokaal = vertaal_lokaal(root, &mut toestand, &ruw, &server, &negeer, opties, &mut rapport);
    toestand.overgeslagen = rapport.overgeslagen.clone();

    let paden: BTreeSet<String> = toestand
        .bestanden
        .keys()
        .chain(server.keys())
        .chain(lokaal.keys())
        .filter(|p| !buiten.contains(*p) && !negeer.negeert(p) && !herstel.geblokkeerd.contains_key(*p))
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
            // Afgeleid: de server heeft de samenvoeging, het bestand hier nog niet. Kennen beide de versieregel, dan telt
            // die (een volgnummer kan terugkomen, ontwerp §5.5). Zo krijgt ook een verwijdering op de server precies de
            // basis mee waarvan de toestand uitging (`Taak::VerwijderServer`).
            let server_gewijzigd = match (server.get(&pad), bekend) {
                (Some(s), Some(b)) => {
                    let andere_regel = matches!((&s.versie_id, &b.versie_id), (Some(x), Some(y)) if x != y);
                    s.versie != b.versie || s.bestand != b.bestand || andere_regel || b.afgeleid.is_some()
                }
                (Some(_), None) => true,
                _ => false,
            };
            (pad, lokaal_gewijzigd, server_gewijzigd)
        })
        .collect();

    // Wat eerder steeds mislukte en sindsdien hier en op de server gelijk bleef, wacht tot de wachttijd om is: niet elke
    // ronde dezelfde fout. Pogingen voor bestanden die nergens meer zijn, vervallen.
    toestand.mislukt.retain(|pad, _| lokaal.contains_key(pad) || server.contains_key(pad));
    let nu = nu_seconden();
    let wacht = |pad: &str| {
        toestand
            .mislukt
            .get(pad)
            .filter(|p| p.tot > nu && p.lokaal == lokaal.get(pad).copied() && p.server == server.get(pad).map(|s| s.versie))
    };
    let (wachtend, wijzigingen): (Vec<_>, Vec<_>) = wijzigingen.into_iter().partition(|(pad, _, _)| wacht(pad).is_some());
    rapport.wachtend = wachtend
        .iter()
        .filter_map(|(pad, _, _)| {
            let p = wacht(pad)?;
            Some(Mislukt { pad: toestand.hier(pad), reden: p.reden.clone(), keer: p.keer, opnieuw_om: p.tot })
        })
        .collect();

    // Vooraf melden hoeveel er over de lijn gaat (een verwijdering op de server telt niet mee). Alleen lezen: geen
    // uploads, en een lokaal verwijderd bestand komt terug.
    let alleen_lezen = opslag.alleen_lezen();
    let samen = !alleen_lezen && opslag.vergelijk_en_schrijf().await;
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

    let ronde = Ronde {
        opslag,
        root,
        waarnemer,
        toestand: Mutex::new(toestand),
        rapport: Mutex::new(rapport),
        lokaal: &lokaal,
        server: &server,
        gestopt: AtomicBool::new(false),
        samen,
        afgehandeld: Mutex::new(onderweg.into_iter().map(|o| o.id).collect()),
    };

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
                // Uitgevinkt en lokaal ongewijzigd: de kopie hier mag weg (veilig, zie `crate::vervang`), op de server
                // blijft hij staan. Net gewijzigd: blijft staan en gaat de volgende ronde naar de server.
                match ronde.verwijder_hier(&pad).await {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(fout) => {
                        ronde.rapport.lock().unwrap().fout(&pad, fout);
                        continue;
                    }
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
                // Een IFC-bestand op een opslag met vergelijk-en-schrijf: samenvoegen (ontwerp §5.6).
                let ifc = samen && kan_samenvoegen(&pad);
                let regel = bekend.as_ref().and_then(|b| b.versie_id.as_ref());
                if lokaal_gewijzigd {
                    match (ifc, regel, &server.versie_id) {
                        (true, Some(_), Some(_)) => overdrachten.push(Taak::Samenvoegen { pad, server: server.clone() }),
                        // Een bekend bestand zonder versieregel (van daarvóór): samenvoegen kan niet veilig, dus een
                        // conflictkopie.
                        (true, None, _) => overdrachten.push(Taak::Upload { pad, basis: None, server: Some(server.clone()) }),
                        _ => {
                            let basis = bekend.as_ref().map(Bekend::basis);
                            overdrachten.push(Taak::Upload { pad, basis, server: Some(server.clone()) });
                        }
                    }
                } else if server_gewijzigd {
                    overdrachten.push(Taak::Download { pad, server: server.clone() });
                } else if ifc && regel.is_none() && server.versie_id.is_some() {
                    overdrachten.push(Taak::Overnemen { pad, server: server.clone() });
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
    // Ook als de sessie onderweg verliep: wat gelukt is, blijft bekend. De verwijderingen van deze ronde staan daarmee
    // vast.
    {
        let toestand = ronde.toestand.lock().unwrap();
        bewaar(root, &toestand)?;
        vervang::leg_vast(root, |s| toestand.bestanden.contains_key(s))?;
    }
    // De uitkomst van deze uploads staat nu in de bewaarde toestand.
    for id in ronde.afgehandeld.lock().unwrap().drain(..) {
        onderweg::klaar(root, &id);
    }
    // Pas nu de toestand op schijf staat, de bronnen afronden (ook die van eerdere rondes); een journaal verdwijnt zo
    // nooit vóór de toestand die het verwerkt.
    if ronde.rond_af().await {
        bewaar(root, &ronde.toestand.lock().unwrap())?;
    }
    let mut rapport = ronde.rapport.into_inner().unwrap();
    rapport.sessie_verlopen = ronde.gestopt.into_inner();
    Ok(rapport)
}

/// Hier hernoemd of verplaatst, ook offline: een bekend bestand is van zijn pad verdwenen, en precies één nieuw bestand
/// heeft dezelfde grootte en wijzigingstijd (hernoemen laat die staan). Dan verhuist het in de toestand mee, met zijn
/// basis, en onthoudt de toestand het pad op de server tot [`verplaats_op_server`] de hernoeming daar deed. Een wijziging
/// daarna is zo een wijziging van hetzelfde bestand (samenvoegen vanuit de basis), geen nieuw bestand. Bij twijfel (twee
/// kandidaten, een lastige naam, `overslaan`) blijft het zoals altijd: een nieuw bestand en een verwijdering. Geeft true
/// als de toestand veranderde.
fn zoek_verplaatsingen(toestand: &mut Toestand, ruw: &BTreeMap<String, (u64, u128)>, overslaan: impl Fn(&str) -> bool) -> bool {
    // Per kenmerk: de bekende bestanden die weg zijn, en de nieuwe.
    let mut paren: BTreeMap<(u64, u128), (Vec<String>, Vec<String>)> = BTreeMap::new();
    for (sleutel, b) in &toestand.bestanden {
        if !toestand.lokale_namen.contains_key(sleutel) && !ruw.contains_key(sleutel) && !overslaan(sleutel) {
            paren.entry((b.grootte, b.mtime_ns)).or_default().0.push(sleutel.clone());
        }
    }
    for (rel, k) in ruw {
        let gewoon = namen::padprobleem(rel).is_none() && namen::serverpad(rel) == *rel;
        let nieuw = !toestand.bestanden.contains_key(rel) && !toestand.lokale_namen.values().any(|l| l == rel);
        if let Some(p) = paren.get_mut(k).filter(|_| gewoon && nieuw && !overslaan(rel)) {
            p.1.push(rel.clone());
        }
    }
    let mut veranderd = false;
    for (weg, nieuw) in paren.into_values() {
        let ([van], [naar]) = (weg.as_slice(), nieuw.as_slice()) else { continue };
        let Some(b) = toestand.bestanden.remove(van) else { continue };
        toestand.bestanden.insert(naar.clone(), b);
        // Twee keer hernoemd vóór de server het zag: op de server staat het nog op het eerste pad.
        let op_server = toestand.verplaatst.remove(van).unwrap_or_else(|| van.clone());
        if op_server != *naar {
            toestand.verplaatst.insert(naar.clone(), op_server);
        }
        log::info!("{van}: hier hernoemd naar {naar}");
        veranderd = true;
    }
    veranderd
}

/// Doe de hernoemingen van [`zoek_verplaatsingen`] op de server, vóór de lijst: zo komt een hernoeming vóór een
/// wijziging van het hernoemde bestand, en de opslag maakt een ontbrekende map eerst aan. Weigert de server (het bestand
/// is weg, vergrendeld, of er staat al iets op het nieuwe pad), dan wordt het zoals altijd: het nieuwe pad een nieuw
/// bestand, het oude een verwijdering (niets gaat verloren). Een fout die een nieuwe poging verdient (offline), stopt de
/// ronde; de hernoeming blijft bewaard voor de volgende.
async fn verplaats_op_server<O: Opslag>(opslag: &O, root: &Path, toestand: &mut Toestand) -> Result<(), Fout> {
    let mut veranderd = false;
    for (naar, van) in toestand.verplaatst.clone() {
        let bestand = toestand.bestanden.get(&naar).map(|b| b.bestand.clone());
        let r = match &bestand {
            None => Err(Fout::Controle("niet meer in de toestand".into())),
            Some(_) if opslag.alleen_lezen() => Err(Fout::Controle("alleen lezen".into())),
            Some(bestand) => opslag.verplaats(bestand, &naar).await,
        };
        match r {
            Ok(()) => log::info!("{van}: op de server hernoemd naar {naar}"),
            Err(f) if tijdelijk(&f) => {
                if veranderd {
                    bewaar(root, toestand)?;
                }
                return Err(f);
            }
            Err(f) => {
                log::info!("{van}: hernoemen naar {naar} kon niet op de server ({f}); wordt een nieuw bestand");
                if let Some(b) = toestand.bestanden.remove(&naar) {
                    toestand.bestanden.entry(van.clone()).or_insert(b);
                }
            }
        }
        toestand.verplaatst.remove(&naar);
        veranderd = true;
    }
    if veranderd {
        bewaar(root, toestand)?;
    }
    Ok(())
}

/// Kwam upload `o` van een vorige ronde aan? Ja als de huidige versie op zijn pad precies de verstuurde inhoud heeft
/// (grootte en blokhashes) en de toestand die versie nog niet kent. Dan de versie zoals de toestand haar moet kennen:
/// met het kenmerk van de momentopname S, en afgeleid als de verstuurde inhoud een samenvoeging van S was. Gelijke
/// inhoud is genoeg: ook als een ander precies dit opsloeg, is het lokale bestand gelijk aan die versie.
async fn eigen_upload<O: Opslag>(
    opslag: &O,
    toestand: &Toestand,
    server: &BTreeMap<String, OpslagBestand>,
    o: &Onderweg,
) -> Result<Option<Bekend>, Fout> {
    let Some(regel) = server.get(&o.sleutel).filter(|s| s.grootte == o.grootte).and_then(OpslagBestand::versieregel) else {
        return Ok(None);
    };
    if toestand.bestanden.get(&o.sleutel).is_some_and(|b| b.versie_id.as_deref() == Some(regel.versie_id.as_str())) {
        return Ok(None);
    }
    match opslag.blokhashes(&regel).await {
        Ok(Some((blok, hashes))) if blok == blokgrootte(o.grootte) && hashes == o.hashes => Ok(Some(Bekend {
            bestand: regel.bestand,
            versie: regel.versie,
            grootte: o.kenmerk.0,
            mtime_ns: o.kenmerk.1,
            versie_id: Some(regel.versie_id),
            afgeleid: o.afgeleid.clone(),
        })),
        // Niet na te gaan nu (offline): niets opnieuw versturen in deze ronde.
        Err(f) if tijdelijk(&f) => Err(f),
        _ => Ok(None),
    }
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
            // Een naam met een spatie vooraan kan hier, maar de server haalt die spatie weg: op de server heet het
            // bestand dus anders. Dat leggen we vast; anders zoekt de motor na de upload een bestand dat hier niet is.
            let op_server = namen::serverpad(rel);
            if op_server == *rel {
                uit.insert(rel.clone(), *kenmerk);
            } else if ruw.contains_key(&op_server) || uit.contains_key(&op_server) || toestand.lokale_namen.contains_key(&op_server) {
                let reden = format!("op de server heet dit {op_server}, en die naam bestaat hier al");
                rapport.overgeslagen.push(Overgeslagen { pad: rel.clone(), reden, opruimbaar: false });
            } else {
                toestand.lokale_namen.insert(op_server.clone(), rel.clone());
                uit.insert(op_server, *kenmerk);
            }
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
