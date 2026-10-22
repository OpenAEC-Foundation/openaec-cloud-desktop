//! Sync-motor (ontwerp §6 en §8): één ronde vergelijkt de lokale map, de server en de toestand van de vorige
//! ronde, en beslist per bestand: uploaden, downloaden, conflict of verwijderen (naar de prullenbak).
//!
//! Uitgangspunten: de server beslist over conflicten (basisversie); lokaal wordt nooit iets verwijderd dat niet
//! als versie op de server staat; bij een conflict blijven beide versies bestaan.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::Fout;

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

/// Wat de motor van de server nodig heeft. De echte implementatie is [`crate::opslag::ClientOpslag`].
#[allow(async_fn_in_trait)]
pub trait Opslag {
    async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout>;
    async fn upload(&self, pad: &str, lokaal: &Path, basis_versie: Option<u64>) -> Result<Upload, Fout>;
    async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout>;
    async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout>;
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

/// Neem `pad` op in de toestand: lokaal en op de server zijn nu gelijk (komt in [`Rapport::bijgewerkt`]).
fn onthoud(
    toestand: &mut Toestand,
    rapport: &mut Rapport,
    root: &Path,
    pad: &str,
    bestand: &str,
    versie: u64,
) -> Result<(), Fout> {
    let (grootte, mtime_ns) = kenmerk(&lokaal_pad(root, pad))?;
    toestand
        .bestanden
        .insert(pad.to_string(), Bekend { bestand: bestand.to_string(), versie, grootte, mtime_ns });
    rapport.bijgewerkt.push(pad.to_string());
    Ok(())
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
    let mut toestand = laad(root);
    let mut rapport = Rapport::default();
    let server: BTreeMap<String, OpslagBestand> =
        opslag.lijst().await?.into_iter().map(|b| (b.pad.clone(), b)).collect();
    let lokaal = lokale_bestanden(root);

    let paden: BTreeSet<String> = toestand
        .bestanden
        .keys()
        .chain(server.keys())
        .chain(lokaal.keys())
        .cloned()
        .collect();

    for pad in paden {
        let bekend = toestand.bestanden.get(&pad).cloned();
        let op_server = server.get(&pad);
        let hier = lokaal.get(&pad);
        let lokaal_gewijzigd = match (hier, &bekend) {
            (Some((g, m)), Some(b)) => *g != b.grootte || *m != b.mtime_ns,
            (Some(_), None) => true,
            _ => false,
        };
        let server_gewijzigd = match (op_server, &bekend) {
            (Some(s), Some(b)) => s.versie != b.versie || s.bestand != b.bestand,
            (Some(_), None) => true,
            _ => false,
        };
        if uitgesloten_pad(&pad, uitgesloten) && !(hier.is_some() && lokaal_gewijzigd) {
            if hier.is_some() {
                // Uitgevinkt en lokaal ongewijzigd: de kopie hier mag weg, op de server blijft hij staan.
                if let Err(fout) = std::fs::remove_file(lokaal_pad(root, &pad)) {
                    rapport.fout(&pad, fout);
                    continue;
                }
            }
            toestand.bestanden.remove(&pad);
            continue;
        }
        let uitkomst = stap(
            opslag, root, &mut toestand, &mut rapport, &pad, bekend, op_server, hier.is_some(),
            lokaal_gewijzigd, server_gewijzigd,
        )
        .await;
        if let Err(fout) = uitkomst {
            rapport.fout(&pad, fout);
        }
    }
    for u in uitgesloten.iter().map(|u| u.trim_matches('/')).filter(|u| !u.is_empty()) {
        ruim_lege_mappen_op(&lokaal_pad(root, u));
    }
    bewaar(root, &toestand)?;
    Ok(rapport)
}

#[allow(clippy::too_many_arguments)]
async fn stap<O: Opslag>(
    opslag: &O,
    root: &Path,
    toestand: &mut Toestand,
    rapport: &mut Rapport,
    pad: &str,
    bekend: Option<Bekend>,
    op_server: Option<&OpslagBestand>,
    hier: bool,
    lokaal_gewijzigd: bool,
    server_gewijzigd: bool,
) -> Result<(), Fout> {
    let lokaal = lokaal_pad(root, pad);
    match (hier, op_server) {
        (true, Some(server)) => {
            if lokaal_gewijzigd {
                // Basis: de versie waarop de lokale wijziging gebaseerd is; de server beslist over een conflict.
                let basis = bekend.as_ref().map(|b| b.versie);
                let up = opslag.upload(pad, &lokaal, basis).await?;
                if up.conflict {
                    // Mijn versie staat nu als conflictkopie op de server: lokaal hernoemen, en de
                    // serverversie op de oorspronkelijke plek zetten. Niets gaat verloren.
                    let kopie = lokaal_pad(root, &up.pad);
                    std::fs::rename(&lokaal, &kopie)?;
                    onthoud(toestand, rapport, root, &up.pad, &up.bestand, up.versie)?;
                    opslag.download(server, &lokaal).await?;
                    onthoud(toestand, rapport, root, pad, &server.bestand, server.versie)?;
                    rapport.conflicten += 1;
                    rapport.gedownload += 1;
                } else {
                    onthoud(toestand, rapport, root, pad, &up.bestand, up.versie)?;
                    rapport.geupload += 1;
                }
            } else if server_gewijzigd {
                opslag.download(server, &lokaal).await?;
                onthoud(toestand, rapport, root, pad, &server.bestand, server.versie)?;
                rapport.gedownload += 1;
            } else if bekend.is_none() {
                onthoud(toestand, rapport, root, pad, &server.bestand, server.versie)?;
            }
        }
        (true, None) => {
            if bekend.is_some() && !lokaal_gewijzigd {
                // Op de server verwijderd (staat daar in de prullenbak) en lokaal ongewijzigd: lokaal weg.
                std::fs::remove_file(&lokaal)?;
                toestand.bestanden.remove(pad);
                rapport.verwijderd += 1;
            } else {
                let up = opslag.upload(pad, &lokaal, None).await?;
                onthoud(toestand, rapport, root, &up.pad, &up.bestand, up.versie)?;
                rapport.geupload += 1;
            }
        }
        (false, Some(server)) => {
            if bekend.is_some() && !server_gewijzigd {
                // Lokaal verwijderd: op de server naar de prullenbak.
                opslag.verwijder(server).await?;
                toestand.bestanden.remove(pad);
                rapport.verwijderd += 1;
            } else {
                opslag.download(server, &lokaal).await?;
                onthoud(toestand, rapport, root, pad, &server.bestand, server.versie)?;
                rapport.gedownload += 1;
            }
        }
        (false, None) => {
            toestand.bestanden.remove(pad);
        }
    }
    Ok(())
}
