//! Overstappen naar OpenAEC Storage: een bestaande map (bijvoorbeeld van een ander opslagplatform, of een
//! netwerkschijf) in één richting naar een ruimte uploaden. De bron blijft onaangeroerd. Hervatbaar: wat al met
//! dezelfde grootte op de server staat, wordt overgeslagen; een gewijzigd bestand wordt een nieuwe versie.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use futures_util::stream::{self, StreamExt};

use crate::motor::Opslag;
use crate::Fout;

/// Systeem-, tijdelijke en verborgen bestanden blijven achter.
pub fn telt_mee_voor_import(naam: &str) -> bool {
    let klein = naam.to_lowercase();
    !(naam.starts_with('.')
        || naam.starts_with("~$")
        || klein == "thumbs.db"
        || klein == "desktop.ini"
        || klein.ends_with(".tmp"))
}

#[derive(Debug, Clone)]
pub struct Voortgang {
    pub pad: String,
    pub bytes: u64,
    pub overgeslagen: bool,
    pub fout: Option<String>,
}

#[derive(Debug, Default)]
pub struct ImportRapport {
    pub geupload: u32,
    pub overgeslagen: u32,
    pub bytes: u64,
    pub fouten: Vec<String>,
}

fn bestanden(bron: &Path) -> Vec<(String, PathBuf, u64)> {
    let mut uit = Vec::new();
    let mut te_doen = vec![bron.to_path_buf()];
    while let Some(map) = te_doen.pop() {
        let Ok(items) = std::fs::read_dir(&map) else { continue };
        for item in items.flatten() {
            let naam = item.file_name().to_string_lossy().to_string();
            if !telt_mee_voor_import(&naam) {
                continue;
            }
            let pad = item.path();
            match item.file_type() {
                Ok(t) if t.is_dir() => te_doen.push(pad),
                Ok(t) if t.is_file() => {
                    let rel = pad
                        .strip_prefix(bron)
                        .unwrap()
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join("/");
                    let grootte = item.metadata().map(|m| m.len()).unwrap_or(0);
                    uit.push((rel, pad, grootte));
                }
                _ => {}
            }
        }
    }
    uit.sort();
    uit
}

/// Importeer `bron` naar de ruimte van `opslag`, onder de map `doel` ("" = hoofdniveau), met `parallel` uploads
/// tegelijk. `voortgang` wordt per bestand aangeroepen.
pub async fn importeer<O: Opslag>(
    opslag: &O,
    bron: &Path,
    doel: &str,
    parallel: usize,
    mut voortgang: impl FnMut(&Voortgang),
) -> Result<ImportRapport, Fout> {
    let doel = doel.trim_matches('/');
    let op_server: HashMap<String, (u64, u64)> =
        opslag.lijst().await?.into_iter().map(|b| (b.pad, (b.versie, b.grootte))).collect();
    let taken = bestanden(bron).into_iter().map(|(rel, lokaal, grootte)| {
        let pad = if doel.is_empty() { rel } else { format!("{doel}/{rel}") };
        let bestaand = op_server.get(&pad).copied();
        async move {
            if let Some((_, g)) = bestaand {
                if g == grootte {
                    return Voortgang { pad, bytes: grootte, overgeslagen: true, fout: None };
                }
            }
            let fout = opslag.upload(&pad, &lokaal, bestaand.map(|(v, _)| v)).await.err().map(|f| f.to_string());
            Voortgang { pad, bytes: grootte, overgeslagen: false, fout }
        }
    });
    let mut rapport = ImportRapport::default();
    let mut stroom = stream::iter(taken).buffer_unordered(parallel.max(1));
    while let Some(v) = stroom.next().await {
        match (&v.fout, v.overgeslagen) {
            (Some(f), _) => rapport.fouten.push(format!("{}: {f}", v.pad)),
            (None, true) => rapport.overgeslagen += 1,
            (None, false) => {
                rapport.geupload += 1;
                rapport.bytes += v.bytes;
            }
        }
        voortgang(&v);
    }
    Ok(rapport)
}
