// Smoke-test van de statusvinkjes zoals de app ze gebruikt: de echte sync-motor tegen een opslag in het geheugen,
// met src/vinkjes.rs erachter, op een TIJDELIJKE map onder %TEMP% — nooit op een echte gebruikersmap.
// Draai met:  cargo run --example vinkjes_smoke
//
// Koppelen -> ronde -> alles groen; schrijven midden in een bestand (bestandswachter) -> pijlen -> ronde -> groen;
// een fout bij uploaden -> pijlen op het bestand en de mappen erboven; ontkoppelen -> alles terug, afgemeld.

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../src/cloudfilter.rs"]
mod cloudfilter;

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../src/instellingen.rs"]
mod instellingen;

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../src/vinkjes.rs"]
mod vinkjes;

#[cfg(not(windows))]
fn main() {
    println!("alleen voor Windows");
}

#[cfg(windows)]
#[tokio::main]
async fn main() {
    let dir = std::env::temp_dir().join(format!("openaec-vinkjes-smoke-{}", std::process::id()));
    let uitkomst = proef(&dir).await;
    // Altijd opruimen, ook als een stap mislukt.
    vinkjes::ontkoppel(&dir.to_string_lossy());
    let _ = std::fs::remove_dir_all(&dir);
    println!(
        "opgeruimd: registratie {}, map {}",
        if cloudfilter::eigen_sync_root(&dir.to_string_lossy()) { "STAAT ER NOG" } else { "weg" },
        if dir.exists() { "STAAT ER NOG" } else { "weg" }
    );
    match uitkomst {
        Ok(()) => println!("\nALLES OK"),
        Err(e) => {
            println!("\nMISLUKT: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(windows)]
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

#[cfg(windows)]
fn controleer(ok: bool, wat: &str) -> anyhow::Result<()> {
    println!("  {:<64} {}", wat, if ok { "OK" } else { "MISLUKT" });
    anyhow::ensure!(ok, "{wat}");
    Ok(())
}

/// "groen", "pijlen", "uitgesloten" of "geen placeholder".
#[cfg(windows)]
fn staat(pad: &std::path::Path) -> &'static str {
    match cloudfilter::placeholder_status(pad) {
        Ok(Some(p)) if p.uitgesloten => "uitgesloten",
        Ok(Some(p)) if p.in_sync && p.vastgezet => "groen",
        Ok(Some(p)) if !p.in_sync => "pijlen",
        Ok(Some(_)) => "in sync, niet vastgezet",
        Ok(None) => "geen placeholder",
        Err(_) => "onbekend",
    }
}

#[cfg(windows)]
async fn ronde(opslag: &geheugen::Geheugen, root: &std::path::Path) -> anyhow::Result<openaec_sync::motor::Rapport> {
    let mut r = openaec_sync::motor::sync_eenmalig(opslag, root).await?;
    let (bijgewerkt, foutpaden) = (std::mem::take(&mut r.bijgewerkt), std::mem::take(&mut r.foutpaden));
    println!("  (ronde) bijgewerkt {bijgewerkt:?}, foutpaden {foutpaden:?}");
    vinkjes::na_ronde(&root.to_string_lossy(), &bijgewerkt, &foutpaden);
    Ok(r)
}

#[cfg(windows)]
async fn proef(dir: &std::path::Path) -> anyhow::Result<()> {
    use std::io::Write;

    anyhow::ensure!(dir.starts_with(std::env::temp_dir()), "weiger: {} ligt niet onder %TEMP%", dir.display());
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir.join("Tekeningen"))?;
    std::fs::write(dir.join("a.txt"), "lokaal")?;
    std::fs::write(dir.join("Tekeningen/plan.pdf"), "pdf")?;
    let opslag = geheugen::Geheugen::default();
    opslag.zet("Model/gebouw.ifc", b"ifc van de server");
    let root = dir.to_path_buf();
    let lokaal = dir.to_string_lossy().to_string();
    println!("gekoppelde map: {lokaal}");

    // --- koppelen (registreren; nog niets bekend) en de eerste ronde ---
    let k = instellingen::Koppeling { ruimte: "R1".into(), ruimte_naam: "Smoke-test".into(), lokaal: lokaal.clone(), uitgesloten: vec![] };
    controleer(vinkjes::nog_te_koppelen(&lokaal), "nog te koppelen")?;
    vinkjes::koppel(&k);
    controleer(cloudfilter::eigen_sync_root(&lokaal) && !vinkjes::nog_te_koppelen(&lokaal), "koppel: geregistreerd")?;
    ronde(&opslag, &root).await?;
    for p in ["a.txt", "Tekeningen/plan.pdf", "Model/gebouw.ifc", "Tekeningen", "Model", ""] {
        let s = staat(&dir.join(p));
        controleer(s == "groen", &format!("na de eerste ronde {:<20} {s}", if p.is_empty() { "(map zelf)" } else { p }))?;
    }
    let s = staat(&dir.join(".openaec"));
    controleer(s == "uitgesloten", &format!("toestandsmap .openaec {s}"))?;

    // --- schrijven midden in een bestand: de bestandswachter zet pijlen, de ronde weer groen ---
    std::fs::OpenOptions::new().append(true).open(dir.join("Tekeningen/plan.pdf"))?.write_all(b" v2")?;
    controleer(vinkjes::gebeurtenis(&root, &dir.join("Tekeningen/plan.pdf")), "gebeurtenis telt mee voor een ronde")?;
    let (s1, s2, s3) = (staat(&dir.join("Tekeningen/plan.pdf")), staat(&dir.join("Tekeningen")), staat(dir));
    controleer(s1 == "pijlen" && s2 == "pijlen" && s3 == "pijlen", &format!("bestand {s1}, map {s2}, map zelf {s3}"))?;
    controleer(!vinkjes::gebeurtenis(&root, &dir.join("Tekeningen/plan.pdf")), "het zetten van de pijlen zelf telt niet mee")?;
    let r = ronde(&opslag, &root).await?;
    controleer(r.geupload == 1, "ronde uploadt de wijziging")?;
    let (s1, s2, s3) = (staat(&dir.join("Tekeningen/plan.pdf")), staat(&dir.join("Tekeningen")), staat(dir));
    controleer(s1 == "groen" && s2 == "groen" && s3 == "groen", &format!("daarna: bestand {s1}, map {s2}, map zelf {s3}"))?;

    // --- alleen een attribuut gewijzigd (Windows wist de status, de motor ziet niets) ---
    let mut perm = std::fs::metadata(dir.join("a.txt"))?.permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(dir.join("a.txt"), perm.clone())?;
    let voor = staat(&dir.join("a.txt"));
    vinkjes::gebeurtenis(&root, &dir.join("a.txt"));
    let r = ronde(&opslag, &root).await?;
    let na = staat(&dir.join("a.txt"));
    controleer(r.geupload == 0 && na == "groen", &format!("alleen-lezen gezet: {voor} -> na de ronde {na} (niets geüpload)"))?;
    perm.set_readonly(false);
    std::fs::set_permissions(dir.join("a.txt"), perm)?;
    let voor = staat(&dir.join("a.txt"));
    vinkjes::gebeurtenis(&root, &dir.join("a.txt"));
    ronde(&opslag, &root).await?;
    let na = staat(&dir.join("a.txt"));
    controleer(na == "groen", &format!("en weer uitgezet: {voor} -> na de ronde {na}"))?;

    // --- fout bij uploaden: pijlen op het bestand en de mappen erboven ---
    std::fs::create_dir_all(dir.join("Tekeningen/Detail"))?;
    std::fs::write(dir.join("Tekeningen/Detail/kapot.dwg"), "dwg")?;
    opslag.weiger.lock().unwrap().insert("Tekeningen/Detail/kapot.dwg".into());
    vinkjes::gebeurtenis(&root, &dir.join("Tekeningen/Detail/kapot.dwg"));
    ronde(&opslag, &root).await?;
    let s: Vec<&str> = ["Tekeningen/Detail/kapot.dwg", "Tekeningen/Detail", "Tekeningen", ""].iter().map(|p| staat(&dir.join(p))).collect();
    controleer(s.iter().all(|s| *s == "pijlen"), &format!("fout: bestand, mappen en map zelf {s:?}"))?;
    let s = (staat(&dir.join("Tekeningen/plan.pdf")), staat(&dir.join("Model")));
    controleer(s == ("groen", "groen"), &format!("de rest blijft groen {s:?}"))?;
    opslag.weiger.lock().unwrap().clear();
    ronde(&opslag, &root).await?;
    let s: Vec<&str> = ["Tekeningen/Detail/kapot.dwg", "Tekeningen/Detail", "Tekeningen", ""].iter().map(|p| staat(&dir.join(p))).collect();
    controleer(s.iter().all(|s| *s == "groen"), &format!("fout opgelost: weer groen {s:?}"))?;

    // --- zoals bij het opnieuw starten: de registratie wordt hergebruikt, markeren verandert niets ---
    vinkjes::koppel(&k);
    controleer(staat(&dir.join("Tekeningen/plan.pdf")) == "groen", "nogmaals koppelen (start): registratie hergebruikt, nog groen")?;

    // --- ontkoppelen: alles terug, afgemeld, bestanden blijven ---
    vinkjes::ontkoppel(&lokaal);
    let s: Vec<&str> = ["a.txt", "Tekeningen/plan.pdf", "Tekeningen", ".openaec"].iter().map(|p| staat(&dir.join(p))).collect();
    controleer(s.iter().all(|s| *s == "geen placeholder"), &format!("ontkoppeld: {s:?}"))?;
    controleer(!cloudfilter::eigen_sync_root(&lokaal), "afgemeld")?;
    controleer(std::fs::read_to_string(dir.join("Tekeningen/plan.pdf"))? == "pdf v2", "bestanden staan er nog")?;
    Ok(())
}
