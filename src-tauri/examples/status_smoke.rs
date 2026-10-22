// Smoke-test voor de statusvinkjes in gekoppelde mappen (Fase 2c), op een
// TIJDELIJKE map onder %TEMP% — nooit op een echte gebruikersmap.
// Draai met:  cargo run --example status_smoke
//
// Gebruikt hetzelfde bronbestand als de app (src/cloudfilter.rs), maar zonder
// tauri/webview2, zodat de .exe licht blijft. Stappen: registreren -> bestanden
// en mappen omzetten en in-sync zetten -> status teruglezen -> wijzigen (Windows
// zet het bestand zelf op niet-in-sync) -> terugzetten -> afmelden -> opruimen.

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../src/cloudfilter.rs"]
mod cloudfilter;

#[cfg(not(windows))]
fn main() {
    println!("alleen voor Windows");
}

#[cfg(windows)]
fn main() {
    let dir = std::env::temp_dir().join(format!("openaec-status-smoke-{}", std::process::id()));
    let opruimen = Opruimen(dir.clone());
    let uitkomst = proef(&dir);
    drop(opruimen);
    match uitkomst {
        Ok(()) => println!("\nALLES OK"),
        Err(e) => {
            println!("\nMISLUKT: {e}");
            std::process::exit(1);
        }
    }
}

/// Ruimt altijd op, ook als een stap mislukt: registratie weg en map weg.
#[cfg(windows)]
struct Opruimen(std::path::PathBuf);

#[cfg(windows)]
impl Drop for Opruimen {
    fn drop(&mut self) {
        let pad = self.0.to_string_lossy().to_string();
        if cloudfilter::eigen_sync_root(&pad) {
            let _ = cloudfilter::zet_terug(&self.0);
            let _ = cloudfilter::afmelden_volledig(&pad);
        }
        let _ = std::fs::remove_dir_all(&self.0);
        println!(
            "opgeruimd: registratie {}, map {}",
            if cloudfilter::eigen_sync_root(&pad) { "STAAT ER NOG" } else { "weg" },
            if self.0.exists() { "STAAT ER NOG" } else { "weg" }
        );
    }
}

#[cfg(windows)]
const NAAM: &str = "OpenAEC Storage (smoke-test)";

/// Policies van de sync-root zoals Windows ze heeft vastgelegd.
#[cfg(windows)]
fn policies(pad: &str) -> String {
    use windows::Win32::Storage::CloudFilters::*;
    let w: Vec<u16> = pad.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u64; (std::mem::size_of::<CF_SYNC_ROOT_STANDARD_INFO>() + 4096) / 8];
    let r = unsafe {
        CfGetSyncRootInfoByPath(windows::core::PCWSTR(w.as_ptr()), CF_SYNC_ROOT_INFO_STANDARD, buf.as_mut_ptr() as _, (buf.len() * 8) as u32, None)
    };
    let i = unsafe { &*(buf.as_ptr() as *const CF_SYNC_ROOT_STANDARD_INFO) };
    let naam = String::from_utf16_lossy(&i.ProviderName).trim_end_matches('\0').to_string();
    format!(
        "{} provider={naam} hydration={} (3 = ALWAYS_FULL) population={} (3 = ALWAYS_FULL) insync=0x{:X}",
        if r.is_ok() { "ok" } else { "FOUT" },
        i.HydrationPolicy.Primary.0,
        i.PopulationPolicy.Primary.0,
        i.InSyncPolicy.0
    )
}

#[cfg(windows)]
fn controleer(ok: bool, wat: &str) -> anyhow::Result<()> {
    println!("  {:<58} {}", wat, if ok { "OK" } else { "MISLUKT" });
    if ok {
        Ok(())
    } else {
        Err(anyhow::anyhow!("{wat}"))
    }
}

#[cfg(windows)]
fn status(pad: &std::path::Path) -> anyhow::Result<Option<cloudfilter::Placeholder>> {
    cloudfilter::placeholder_status(pad)
}

#[cfg(windows)]
fn kenmerk(pad: &std::path::Path) -> (u64, std::time::SystemTime) {
    let m = std::fs::metadata(pad).unwrap();
    (m.len(), m.modified().unwrap())
}

#[cfg(windows)]
fn proef(dir: &std::path::Path) -> anyhow::Result<()> {
    use cloudfilter::Placeholder;
    use std::io::Write;

    let tijdelijk = std::env::temp_dir();
    anyhow::ensure!(dir.starts_with(&tijdelijk), "weiger: {} ligt niet onder %TEMP%", dir.display());
    let _ = std::fs::remove_dir_all(dir);
    for (pad, inhoud) in [
        ("a.txt", "een"),
        ("Map/b.txt", "twee"),
        ("Map/Sub/c.txt", "drie"),
        (".openaec/staat.json", "{}"),
        ("Map/model.openaec-download", "half"),
    ] {
        let p = dir.join(pad);
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(p, inhoud)?;
    }
    let root = dir.to_path_buf();
    let pad = dir.to_string_lossy().to_string();
    println!("sync-root: {pad}");

    // --- registreren ---
    let icoon = format!("{},0", std::env::current_exe()?.display());
    cloudfilter::registreer_volledig(&pad, NAAM, env!("CARGO_PKG_VERSION"), &icoon)?;
    controleer(cloudfilter::eigen_sync_root(&pad), "registreer_volledig (en teruggelezen als eigen sync-root)")?;
    println!("  (info) policies: {}", policies(&pad));
    cloudfilter::registreer_volledig(&pad, NAAM, env!("CARGO_PKG_VERSION"), &icoon)?;
    controleer(true, "nogmaals registreren (bij elke start) gaat goed")?;
    let binnen = dir.join("Map").to_string_lossy().to_string();
    let geweigerd = cloudfilter::registreer_volledig(&binnen, NAAM, "1", &icoon);
    controleer(geweigerd.is_err(), &format!("map binnen een sync-root geweigerd: {}", geweigerd.err().map(|e| e.to_string()).unwrap_or_default()))?;

    // --- omzetten en in-sync zetten ---
    let groen = Some(Placeholder { in_sync: true, vastgezet: true, uitgesloten: false });
    let pijlen = Some(Placeholder { in_sync: false, vastgezet: true, uitgesloten: false });
    let voor: Vec<_> = ["a.txt", "Map/b.txt", "Map/Sub/c.txt"].iter().map(|p| kenmerk(&dir.join(p))).collect();
    controleer(status(&dir.join("a.txt"))?.is_none(), "a.txt is vooraf geen placeholder")?;
    for p in ["a.txt", "Map/b.txt", "Map/Sub/c.txt", "Map/Sub", "Map", ""] {
        let doel = if p.is_empty() { root.clone() } else { dir.join(p) };
        let veranderd = cloudfilter::markeer_in_sync(&root, &doel)?;
        let st = status(&doel)?;
        controleer(veranderd && st == groen, &format!("markeer_in_sync {:<14} -> {:?}", if p.is_empty() { "(root)" } else { p }, st))?;
    }
    let na: Vec<_> = ["a.txt", "Map/b.txt", "Map/Sub/c.txt"].iter().map(|p| kenmerk(&dir.join(p))).collect();
    controleer(voor == na, "grootte en wijzigingstijd ongewijzigd (motor ziet niets nieuws)")?;
    controleer(std::fs::read_to_string(dir.join("Map/Sub/c.txt"))? == "drie", "inhoud blijft gewoon lokaal leesbaar")?;
    controleer(!cloudfilter::markeer_in_sync(&root, &dir.join("a.txt"))?, "tweede keer markeren verandert niets")?;
    for p in [".openaec", ".openaec/staat.json", "Map/model.openaec-download"] {
        let veranderd = cloudfilter::markeer_in_sync(&root, &dir.join(p))?;
        controleer(!veranderd && status(&dir.join(p))?.is_none(), &format!("{p} overgeslagen (geen placeholder)"))?;
    }

    // --- Windows houdt wijzigingen zelf bij ---
    std::fs::write(dir.join("a.txt"), "een en meer")?; // opslaan zoals de meeste programma's: overschrijven
    let st = status(&dir.join("a.txt"))?;
    controleer(st.is_some_and(|s| !s.in_sync && s.vastgezet), &format!("a.txt opgeslagen -> Windows zet het op niet in sync: {st:?}"))?;
    std::fs::write(dir.join("Map/nieuw.txt"), "nieuw")?;
    let st_map = status(&dir.join("Map"))?;
    let st_nieuw = status(&dir.join("Map/nieuw.txt"))?;
    println!("  (info) nieuw bestand in Map: Map -> {st_map:?}, nieuw.txt -> {st_nieuw:?}");
    let st = status(&dir.join("Map/Sub/c.txt"))?;
    std::fs::OpenOptions::new().append(true).open(dir.join("Map/Sub/c.txt"))?.write_all(b" erbij")?;
    println!("  (info) toevoegen aan Map/Sub/c.txt: {st:?} -> {:?} (dit vangt de bestandswachter van de app op)", status(&dir.join("Map/Sub/c.txt"))?);
    cloudfilter::markeer_in_sync(&root, &dir.join("a.txt"))?;
    controleer(status(&dir.join("a.txt"))? == groen, "na de sync-ronde weer in sync")?;

    // --- fout: expliciet niet in sync ---
    cloudfilter::markeer_niet_in_sync(&root, &dir.join("Map/b.txt"))?;
    let st = status(&dir.join("Map/b.txt"))?;
    controleer(st == pijlen, &format!("markeer_niet_in_sync Map/b.txt -> {st:?}"))?;
    cloudfilter::markeer_niet_in_sync(&root, &dir.join("Map/nieuw.txt"))?;
    let st = status(&dir.join("Map/nieuw.txt"))?;
    controleer(st == pijlen, &format!("markeer_niet_in_sync nieuw bestand -> {st:?}"))?;

    // --- toestandsmap: uitgesloten in plaats van "wacht op upload" ---
    controleer(cloudfilter::sluit_toestandsmap_uit(&root)?, "sluit_toestandsmap_uit")?;
    let st = status(&dir.join(".openaec"))?;
    controleer(st.is_some_and(|s| s.uitgesloten), &format!(".openaec uitgesloten -> {st:?}"))?;
    controleer(!cloudfilter::sluit_toestandsmap_uit(&root)?, "tweede keer verandert niets")?;
    std::fs::write(dir.join(".openaec/staat.json.tmp"), "{\"x\":1}")?;
    std::fs::rename(dir.join(".openaec/staat.json.tmp"), dir.join(".openaec/staat.json"))?;
    controleer(std::fs::read_to_string(dir.join(".openaec/staat.json"))? == "{\"x\":1}", "motor kan zijn toestand gewoon blijven schrijven")?;

    if let Some(s) = std::env::var("STATUS_SMOKE_PAUZE").ok().and_then(|s| s.parse().ok()) {
        println!("  PAUZE {s} s: {}", dir.display());
        std::thread::sleep(std::time::Duration::from_secs(s));
    }

    // --- ruimte vrijmaken kan niet ---
    controleer(!ontwater(&dir.join("a.txt")), "ontwateren van een vastgezet bestand geweigerd")?;
    controleer(!ontwater_los(&dir.join("a.txt")), "ook na losmaken geweigerd (ALWAYS_FULL)")?;
    cloudfilter::zet_vast(&dir.join("a.txt"))?;
    controleer(std::fs::read_to_string(dir.join("a.txt"))? == "een en meer", "inhoud nog volledig lokaal")?;

    // --- wat de motor doet, werkt zonder verbinding (geen callbacks) ---
    std::fs::write(dir.join("Map/Sub/c.openaec-download"), "drie v2")?;
    std::fs::rename(dir.join("Map/Sub/c.openaec-download"), dir.join("Map/Sub/c.txt"))?;
    controleer(std::fs::read_to_string(dir.join("Map/Sub/c.txt"))? == "drie v2", "download plaatsen (hernoemen over een placeholder)")?;
    std::fs::rename(dir.join("Map/b.txt"), dir.join("Map/b (conflict).txt"))?;
    controleer(dir.join("Map/b (conflict).txt").exists(), "hernoemen (conflictkopie)")?;
    std::fs::remove_file(dir.join("Map/b (conflict).txt"))?;
    controleer(!dir.join("Map/b (conflict).txt").exists(), "verwijderen")?;

    // --- terugzetten en afmelden ---
    let n = cloudfilter::zet_terug(&root)?;
    let mut nog = Vec::new();
    for p in ["a.txt", "Map/Sub/c.txt", "Map/nieuw.txt", "Map/Sub", "Map", ".openaec"] {
        if status(&dir.join(p))?.is_some() {
            nog.push(p);
        }
    }
    controleer(nog.is_empty(), &format!("zet_terug: {n} teruggezet, geen placeholders meer over"))?;
    cloudfilter::afmelden_volledig(&pad)?;
    controleer(!cloudfilter::eigen_sync_root(&pad), "afgemeld (shell en filter)")?;
    let attr = |p: &std::path::Path| { use std::os::windows::fs::MetadataExt; std::fs::metadata(p).map(|m| m.file_attributes()).unwrap_or(0) };
    let rest: Vec<String> = [dir.to_path_buf(), dir.join("a.txt"), dir.join("Map"), dir.join("Map/nieuw.txt"), dir.join(".openaec")]
        .iter()
        .filter(|p| attr(p) & 0x0008_0400 != 0)
        .map(|p| format!("{} (0x{:X})", p.display(), attr(p)))
        .collect();
    controleer(rest.is_empty(), &format!("geen reparse-punt of vastzet-attribuut meer over {rest:?}"))?;
    controleer(std::fs::read_to_string(dir.join("a.txt"))? == "een en meer", "bestanden staan er nog, gewoon leesbaar")?;
    Ok(())
}

/// Probeer een bestand te ontwateren (wat "ruimte vrijmaken" doet); true als het lukte.
#[cfg(windows)]
fn ontwater(pad: &std::path::Path) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::CloudFilters::{CfDehydratePlaceholder, CF_DEHYDRATE_FLAG_NONE};
    let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(pad) else { return false };
    let r = unsafe { CfDehydratePlaceholder(HANDLE(f.as_raw_handle()), 0, -1, CF_DEHYDRATE_FLAG_NONE, None) };
    if let Err(e) = &r {
        println!("  (info) CfDehydratePlaceholder: {e}");
    }
    r.is_ok()
}

/// Losmaken (zoals de Verkenner bij "ruimte vrijmaken" doet) en dan ontwateren.
#[cfg(windows)]
fn ontwater_los(pad: &std::path::Path) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::CloudFilters::{CfSetPinState, CF_PIN_STATE_UNPINNED, CF_SET_PIN_FLAG_NONE};
    if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(pad) {
        let _ = unsafe { CfSetPinState(HANDLE(f.as_raw_handle()), CF_PIN_STATE_UNPINNED, CF_SET_PIN_FLAG_NONE, None) };
    }
    ontwater(pad)
}
