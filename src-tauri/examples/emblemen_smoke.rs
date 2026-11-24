// Smoke-test van de statusvinkjes op Linux zoals de app ze gebruikt: de echte sync-motor tegen een opslag in het
// geheugen, met src/emblemen.rs erachter (emblemen in de metadata van GVfs), op een TIJDELIJKE map onder de
// tijdelijke map van het systeem — nooit op een echte gebruikersmap.
// Draai met:  cargo run --example emblemen_smoke
//
// Koppelen -> ronde -> alles een vinkje; een wijziging -> pijlen op het bestand en de mappen erboven -> ronde -> vinkje;
// een fout bij uploaden -> uitroepteken; overgeslagen -> streepje; een embleem van de gebruiker blijft staan;
// ontkoppelen -> de eigen emblemen weg.
//
// Controleert wat er in `metadata::emblems` staat (hetzelfde als `gio info -a metadata::emblems <pad>`); of de
// bestandsbeheerder het embleem tekent, zie je alleen in de bestandsbeheerder zelf.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "../src/itemstatus.rs"]
mod itemstatus;

#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "../src/emblemen.rs"]
mod emblemen;

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("alleen voor Linux");
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() {
    let dir = std::env::temp_dir().join(format!("openaec-emblemen-smoke-{}", std::process::id()));
    let uitkomst = proef(&dir).await;
    // Altijd opruimen, ook als een stap mislukt.
    emblemen::ontkoppel(&dir.to_string_lossy());
    emblemen::echt::wacht();
    let _ = std::fs::remove_dir_all(&dir);
    println!("opgeruimd: map {}", if dir.exists() { "STAAT ER NOG" } else { "weg" });
    match uitkomst {
        Ok(()) => println!("\nALLES OK"),
        Err(e) => {
            println!("\nMISLUKT: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "linux")]
mod geheugen {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;
    use std::sync::Mutex;

    use openaec_sync::motor::{Opslag, OpslagBestand, Upload};
    use openaec_sync::Fout;

    /// Opslag in het geheugen: pad -> (versie, inhoud); paden in `weiger` geven een fout bij uploaden.
    #[derive(Default)]
    pub struct Geheugen {
        pub bestanden: Mutex<BTreeMap<String, (u64, Vec<u8>)>>,
        pub weiger: Mutex<BTreeSet<String>>,
    }

    impl Geheugen {
        pub fn zet(&self, pad: &str, inhoud: &[u8]) {
            let mut b = self.bestanden.lock().unwrap();
            let versie = b.get(pad).map(|x| x.0 + 1).unwrap_or(1);
            b.insert(pad.to_string(), (versie, inhoud.to_vec()));
        }
    }

    impl Opslag for Geheugen {
        async fn lijst(&self) -> Result<Vec<OpslagBestand>, Fout> {
            let b = self.bestanden.lock().unwrap();
            Ok(b.iter()
                .map(|(pad, (versie, inhoud))| OpslagBestand {
                    pad: pad.clone(),
                    bestand: format!("id-{pad}"),
                    versie: *versie,
                    grootte: inhoud.len() as u64,
                })
                .collect())
        }

        async fn upload(&self, pad: &str, lokaal: &Path, _basis: Option<u64>) -> Result<Upload, Fout> {
            if self.weiger.lock().unwrap().contains(pad) {
                return Err(Fout::Controle(format!("{pad} geweigerd (proef)")));
            }
            self.zet(pad, &std::fs::read(lokaal)?);
            let versie = self.bestanden.lock().unwrap()[pad].0;
            Ok(Upload { pad: pad.to_string(), bestand: format!("id-{pad}"), versie, conflict: false })
        }

        async fn download(&self, bestand: &OpslagBestand, lokaal: &Path) -> Result<(), Fout> {
            let inhoud = self.bestanden.lock().unwrap()[&bestand.pad].1.clone();
            std::fs::create_dir_all(lokaal.parent().unwrap())?;
            // Zoals de echte download: tijdelijk bestand, daarna op zijn plek zetten.
            let tijdelijk = lokaal.with_extension("openaec-download");
            std::fs::write(&tijdelijk, inhoud)?;
            std::fs::rename(tijdelijk, lokaal)?;
            Ok(())
        }

        async fn verwijder(&self, bestand: &OpslagBestand) -> Result<(), Fout> {
            self.bestanden.lock().unwrap().remove(&bestand.pad);
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
fn controleer(ok: bool, wat: &str) -> anyhow::Result<()> {
    println!("  {:<72} {}", wat, if ok { "OK" } else { "MISLUKT" });
    anyhow::ensure!(ok, "{wat}");
    Ok(())
}

/// Het eigen embleem op een pad: "vinkje", "pijlen", "fout", "overgeslagen" of "geen".
#[cfg(target_os = "linux")]
fn staat(pad: &std::path::Path) -> &'static str {
    let namen = emblemen::echt::lees(pad).unwrap_or_default();
    let heeft = |e: emblemen::Embleem| namen.iter().any(|n| n == e.naam());
    if heeft(emblemen::Embleem::Fout) {
        "fout"
    } else if heeft(emblemen::Embleem::Bezig) {
        "pijlen"
    } else if heeft(emblemen::Embleem::Overgeslagen) {
        "overgeslagen"
    } else if heeft(emblemen::Embleem::InSync) {
        "vinkje"
    } else {
        "geen"
    }
}

#[cfg(target_os = "linux")]
async fn ronde(opslag: &geheugen::Geheugen, root: &std::path::Path, overgeslagen: &[String]) -> anyhow::Result<openaec_sync::motor::Rapport> {
    let mut r = openaec_sync::motor::sync_eenmalig(opslag, root).await?;
    let (bijgewerkt, mut foutpaden) = (std::mem::take(&mut r.bijgewerkt), std::mem::take(&mut r.foutpaden));
    // Zoals de app: wat na een mislukte poging wacht op een nieuwe, houdt zijn foutstatus.
    foutpaden.extend(r.wachtend.iter().map(|w| w.pad.clone()));
    println!("  (ronde) bijgewerkt {bijgewerkt:?}, foutpaden {foutpaden:?}");
    let lokaal = root.to_string_lossy();
    emblemen::fouten(&lokaal, &foutpaden);
    emblemen::na_ronde(&lokaal, overgeslagen);
    emblemen::echt::wacht();
    Ok(r)
}

#[cfg(target_os = "linux")]
async fn proef(dir: &std::path::Path) -> anyhow::Result<()> {
    use std::io::Write;

    use gio::prelude::*;

    anyhow::ensure!(dir.starts_with(std::env::temp_dir()), "weiger: {} ligt niet onder de tijdelijke map", dir.display());
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir.join("Tekeningen"))?;
    std::fs::write(dir.join("a.txt"), "lokaal")?;
    std::fs::write(dir.join("Tekeningen/plan.pdf"), "pdf")?;
    let opslag = geheugen::Geheugen::default();
    opslag.zet("Model/gebouw.ifc", b"ifc van de server");
    let lokaal = dir.to_string_lossy().to_string();
    println!("gekoppelde map: {lokaal}");
    let alle = ["a.txt", "Tekeningen/plan.pdf", "Model/gebouw.ifc", "Tekeningen", "Model", ""];
    let staten = |paden: &[&str]| paden.iter().map(|p| staat(&dir.join(p))).collect::<Vec<_>>();

    // --- koppelen (nog niets bekend) en de eerste ronde ---
    controleer(!emblemen::gekoppeld(&lokaal), "nog te koppelen")?;
    emblemen::koppel(&lokaal);
    emblemen::echt::wacht();
    controleer(emblemen::gekoppeld(&lokaal), "gekoppeld")?;
    let s = staten(&["a.txt", ""]);
    controleer(s.iter().all(|s| *s == "geen"), &format!("voor de eerste ronde nog geen embleem {s:?}"))?;
    let pictogram = dirs::data_dir().map(|d| d.join("icons/hicolor/scalable/emblems/emblem-openaec-synced.svg"));
    controleer(pictogram.as_deref().is_some_and(|p| p.is_file()), "pictogrammen staan in ~/.local/share/icons/hicolor")?;
    ronde(&opslag, dir, &[]).await?;
    let s = staten(&alle);
    controleer(s.iter().all(|s| *s == "vinkje"), &format!("na de eerste ronde: bestanden, mappen en de map zelf {s:?}"))?;
    controleer(staat(&dir.join(".openaec")) == "geen", "de toestandsmap .openaec krijgt geen embleem")?;

    // --- een wijziging: pijlen op het bestand en de mappen erboven, na de ronde weer een vinkje ---
    std::fs::OpenOptions::new().append(true).open(dir.join("Tekeningen/plan.pdf"))?.write_all(b" v2")?;
    emblemen::bezig(&lokaal, "Tekeningen/plan.pdf");
    emblemen::echt::wacht();
    let s = staten(&["Tekeningen/plan.pdf", "Tekeningen", ""]);
    controleer(s.iter().all(|s| *s == "pijlen"), &format!("gewijzigd: bestand, map en de map zelf {s:?}"))?;
    let s = staten(&["a.txt", "Model", "Model/gebouw.ifc"]);
    controleer(s.iter().all(|s| *s == "vinkje"), &format!("de rest houdt zijn vinkje {s:?}"))?;
    let r = ronde(&opslag, dir, &[]).await?;
    controleer(r.geupload == 1, "de ronde uploadt de wijziging")?;
    let s = staten(&alle);
    controleer(s.iter().all(|s| *s == "vinkje"), &format!("daarna alles weer een vinkje {s:?}"))?;

    // --- tijdens een ronde: bezig en klaar per bestand ---
    emblemen::bezig(&lokaal, "a.txt");
    emblemen::klaar(&lokaal, "a.txt");
    emblemen::echt::wacht();
    controleer(staat(&dir.join("a.txt")) == "vinkje", "bezig en daarna klaar: het bestand heeft zijn vinkje terug")?;
    ronde(&opslag, dir, &[]).await?;
    controleer(staat(dir) == "vinkje", "en na de ronde ook de map zelf")?;

    // --- een fout bij uploaden: uitroepteken op het bestand en de mappen erboven ---
    std::fs::create_dir_all(dir.join("Tekeningen/Detail"))?;
    std::fs::write(dir.join("Tekeningen/Detail/kapot.dwg"), "dwg")?;
    opslag.weiger.lock().unwrap().insert("Tekeningen/Detail/kapot.dwg".into());
    ronde(&opslag, dir, &[]).await?;
    let s = staten(&["Tekeningen/Detail/kapot.dwg", "Tekeningen/Detail", "Tekeningen", ""]);
    controleer(s.iter().all(|s| *s == "fout"), &format!("fout: bestand, mappen en de map zelf {s:?}"))?;
    let s = staten(&["Tekeningen/plan.pdf", "Model"]);
    controleer(s.iter().all(|s| *s == "vinkje"), &format!("de rest houdt zijn vinkje {s:?}"))?;
    // De motor wacht even met een nieuwe poging; tot die tijd blijft de fout staan. Het bestand opnieuw bewaren
    // (een wijziging) laat hem het meteen weer proberen.
    opslag.weiger.lock().unwrap().clear();
    let r = ronde(&opslag, dir, &[]).await?;
    if !r.wachtend.is_empty() {
        controleer(staat(&dir.join("Tekeningen/Detail/kapot.dwg")) == "fout", "wacht op een nieuwe poging: de fout blijft staan")?;
        std::fs::write(dir.join("Tekeningen/Detail/kapot.dwg"), "dwg, opnieuw bewaard")?;
        ronde(&opslag, dir, &[]).await?;
    }
    let s = staten(&["Tekeningen/Detail/kapot.dwg", "Tekeningen/Detail", "Tekeningen", ""]);
    controleer(s.iter().all(|s| *s == "vinkje"), &format!("na een geslaagde ronde weer een vinkje {s:?}"))?;

    // --- overgeslagen: een streepje op het bestand, de map blijft in sync ---
    std::fs::write(dir.join("Tekeningen/alleen-hier.txt"), "x")?;
    ronde(&opslag, dir, &["Model/gebouw.ifc".to_string()]).await?;
    let s = staten(&["Model/gebouw.ifc", "Model", ""]);
    controleer(s == ["overgeslagen", "vinkje", "vinkje"], &format!("overgeslagen: bestand, map, de map zelf {s:?}"))?;
    ronde(&opslag, dir, &[]).await?;
    controleer(staat(&dir.join("Model/gebouw.ifc")) == "vinkje", "niet meer overgeslagen: weer een vinkje")?;

    // --- een embleem van de gebruiker blijft staan ---
    let info = gio::FileInfo::new();
    info.set_attribute_stringv(emblemen::ATTRIBUUT, &["emblem-favorite", emblemen::Embleem::InSync.naam()][..]);
    gio::File::for_path(dir.join("a.txt")).set_attributes_from_info(&info, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)?;
    std::fs::OpenOptions::new().append(true).open(dir.join("a.txt"))?.write_all(b" meer")?;
    emblemen::bezig(&lokaal, "a.txt");
    emblemen::echt::wacht();
    let namen = emblemen::echt::lees(&dir.join("a.txt")).unwrap_or_default();
    controleer(namen == ["emblem-favorite", "emblem-openaec-syncing"], &format!("eigen embleem naast dat van de gebruiker {namen:?}"))?;

    // --- een verwijderd bestand en ontkoppelen ---
    std::fs::remove_file(dir.join("Tekeningen/alleen-hier.txt"))?;
    ronde(&opslag, dir, &[]).await?;
    emblemen::ontkoppel(&lokaal);
    emblemen::echt::wacht();
    let s = staten(&["a.txt", "Tekeningen/plan.pdf", "Tekeningen/Detail/kapot.dwg", "Model/gebouw.ifc", "Tekeningen/Detail", "Tekeningen", "Model", ""]);
    controleer(s.iter().all(|s| *s == "geen"), &format!("ontkoppeld: geen eigen emblemen meer {s:?}"))?;
    let namen = emblemen::echt::lees(&dir.join("a.txt")).unwrap_or_default();
    controleer(namen == ["emblem-favorite"], &format!("het embleem van de gebruiker staat er nog {namen:?}"))?;
    controleer(!emblemen::gekoppeld(&lokaal), "niet meer gekoppeld")?;
    Ok(())
}
