// Rooktest voor de hoofdmap (één plek in het navigatiepaneel, bestanden op aanvraag), op een TIJDELIJKE map onder
// %TEMP% met een testnaam — nooit op een echte gebruikersmap en zonder de echte registratie aan te raken.
// Draai met:  cargo run --example hoofdmap_smoke
// Met OPENAEC_ROOKTEST_WACHT=<seconden> blijft de registratie zo lang staan (en opent de Verkenner de map), om in de
// Verkenner te kijken; daarna wordt alles opgeruimd.
//
// Stappen: registreren (en het item in het navigatiepaneel teruglezen uit het register) -> verbinden -> een gewoon
// bestand in sync zetten zonder vastzetten -> een bestand dat alleen online staat aanmaken en openen (ophalen via de
// callback, ook in meerdere stukken) -> ruimte vrijmaken -> "altijd op dit apparaat" en terug via de attributen ->
// een nieuwe versie op de server zonder downloaden -> eigen kolommen -> terugzetten -> afmelden -> opruimen.

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../src/cloudfilter.rs"]
mod cloudfilter;

#[cfg(not(windows))]
fn main() {
    println!("alleen voor Windows");
}

#[cfg(windows)]
const NAAM: &str = "OpenAEC Cloud (rooktest)";

/// Naam van een eerdere versie: een bestaande registratie met deze naam moet naar [`NAAM`] gaan.
#[cfg(windows)]
const OUDE_NAAM: &str = "OpenAEC (rooktest)";

#[cfg(windows)]
fn main() {
    let _ = env_logger::builder().is_test(true).try_init();
    let dir = std::env::temp_dir().join(format!("openaec-hoofdmap-rooktest-{}", std::process::id()));
    let mut sleutel = None;
    let uitkomst = proef(&dir, &mut sleutel);
    if let Some(k) = sleutel {
        let _ = cloudfilter::disconnect(k);
    }
    let pad = dir.to_string_lossy().to_string();
    let clsid = navigatie_clsid(&pad);
    if cloudfilter::eigen_sync_root(&pad) {
        let _ = cloudfilter::zet_terug(&dir);
        if let Err(e) = cloudfilter::afmelden_hoofdmap(&pad) {
            println!("afmelden: {e}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    let item_weg = clsid.as_deref().is_none_or(|c| !navigatie_item(c).is_some_and(|(_, vast, _)| vast));
    println!(
        "opgeruimd: registratie {}, item in het navigatiepaneel {}, map {}",
        if cloudfilter::eigen_sync_root(&pad) { "STAAT ER NOG" } else { "weg" },
        if item_weg { "weg" } else { "STAAT ER NOG" },
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
fn controleer(ok: bool, wat: &str) -> anyhow::Result<()> {
    println!("  {:<70} {}", wat, if ok { "OK" } else { "MISLUKT" });
    if ok {
        Ok(())
    } else {
        Err(anyhow::anyhow!("{wat}"))
    }
}

/// De registratie onder HKLM\…\SyncRootManager waarvan de map `pad` is: (sleutelnaam, NamespaceCLSID).
#[cfg(windows)]
fn registratie(pad: &str) -> Option<(String, Option<String>)> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let basis = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\SyncRootManager").ok()?;
    for naam in basis.enum_keys().flatten() {
        let Ok(k) = basis.open_subkey(&naam) else { continue };
        let mappen = k.open_subkey("UserSyncRoots").ok()?;
        let hier = mappen.enum_values().flatten().any(|(_, v)| v.to_string().trim_end_matches('\0').eq_ignore_ascii_case(pad));
        if hier {
            return Some((naam, k.get_value::<String, _>("NamespaceCLSID").ok()));
        }
    }
    None
}

#[cfg(windows)]
fn navigatie_clsid(pad: &str) -> Option<String> {
    registratie(pad).and_then(|(_, c)| c)
}

/// Het naamruimte-item: (naam, vastgemaakt in het navigatiepaneel, doelmap).
#[cfg(windows)]
fn navigatie_item(clsid: &str) -> Option<(String, bool, String)> {
    use winreg::enums::HKEY_CURRENT_USER;
    let k = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!("Software\\Classes\\CLSID\\{clsid}")).ok()?;
    let naam: String = k.get_value("").unwrap_or_default();
    let vast = k.get_value::<u32, _>("System.IsPinnedToNamespaceTree").unwrap_or(0) == 1;
    let doel: String = k.open_subkey("Instance\\InitPropertyBag").and_then(|t| t.get_value("TargetFolderPath")).unwrap_or_default();
    Some((naam, vast, doel))
}

#[cfg(windows)]
fn in_naamruimte(clsid: &str) -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Desktop\\NameSpace\\{clsid}"))
        .is_ok()
}

#[cfg(windows)]
fn inhoud(n: usize, zaad: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(zaad)).collect()
}

#[cfg(windows)]
fn attrib(args: &[&str], pad: &std::path::Path) -> anyhow::Result<()> {
    let s = std::process::Command::new("attrib").args(args).arg(pad).status()?;
    anyhow::ensure!(s.success(), "attrib {args:?} mislukt");
    Ok(())
}

#[cfg(windows)]
fn proef(dir: &std::path::Path, sleutel: &mut Option<windows::Win32::Storage::CloudFilters::CF_CONNECTION_KEY>) -> anyhow::Result<()> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    let tijdelijk = std::env::temp_dir();
    anyhow::ensure!(dir.starts_with(&tijdelijk), "weiger: {} ligt niet onder %TEMP%", dir.display());
    let _ = std::fs::remove_dir_all(dir);
    let ruimte = dir.join("Ruimte A");
    std::fs::create_dir_all(ruimte.join("Tekeningen"))?;
    std::fs::write(ruimte.join("lokaal.txt"), "gewoon lokaal")?;
    let pad = dir.to_string_lossy().to_string();
    println!("hoofdmap: {pad}");

    // --- registreren en het navigatiepaneel ---
    let icoon = format!("{},0", std::env::current_exe()?.display());
    cloudfilter::registreer_hoofdmap(&pad, OUDE_NAAM, env!("CARGO_PKG_VERSION"), &icoon)?;
    controleer(cloudfilter::eigen_sync_root(&pad), "registreer_hoofdmap (en teruggelezen als eigen sync-root)")?;
    controleer(cloudfilter::weergave(&pad).is_some_and(|(n, _)| n == OUDE_NAAM), "weergavenaam van de registratie teruggelezen")?;
    cloudfilter::registreer_hoofdmap(&pad, NAAM, env!("CARGO_PKG_VERSION"), &icoon)?;
    let weergave = cloudfilter::weergave(&pad);
    println!("  (info) na registreren met een nieuwe naam: {weergave:?}");
    controleer(weergave.is_some_and(|(n, i)| n == NAAM && i == icoon), "een andere naam werkt de bestaande registratie bij")?;
    cloudfilter::registreer_hoofdmap(&pad, NAAM, env!("CARGO_PKG_VERSION"), &icoon)?;
    controleer(true, "nogmaals registreren gaat goed")?;
    let reg = registratie(&pad);
    println!("  (info) registratie: {:?}", reg);
    let clsid = reg.as_ref().and_then(|(_, c)| c.clone());
    controleer(clsid.is_some(), "registratie heeft een NamespaceCLSID (item in het navigatiepaneel)")?;
    let item = navigatie_item(clsid.as_deref().unwrap());
    println!("  (info) naamruimte-item: {:?}, onder Desktop\\NameSpace: {}", item, in_naamruimte(clsid.as_deref().unwrap()));
    controleer(
        item.as_ref().is_some_and(|(naam, vast, doel)| naam == NAAM && *vast && doel.eq_ignore_ascii_case(&pad)),
        "naamruimte-item: naam, vastgemaakt in de boom, doelmap = hoofdmap",
    )?;

    // --- verbinden: de ophaler serveert uit het geheugen ---
    let server: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
    let verzoeken = Arc::new(Mutex::new(Vec::<(i64, i64)>::new()));
    {
        let (server, verzoeken) = (server.clone(), verzoeken.clone());
        cloudfilter::zet_ophaler(Box::new(move |pad, begin, lengte, schrijf| {
            verzoeken.lock().unwrap().push((begin, lengte));
            let id = cloudfilter::bestandsidentiteit(pad).ok_or("geen identiteit")?;
            let data = server.lock().unwrap().get(&id).cloned().ok_or("onbekend bestand")?;
            let stuk = &data[begin as usize..(begin + lengte) as usize];
            // In brokken van 300 KB, zoals een HTTP-stroom binnenkomt.
            for brok in stuk.chunks(300 * 1024) {
                if !schrijf(brok) {
                    return Err("afgebroken".into());
                }
            }
            Ok(())
        }));
    }
    *sleutel = Some(cloudfilter::verbind_hoofdmap(&pad)?);
    controleer(true, "verbind_hoofdmap (FETCH_DATA-callback)")?;

    // --- een gewoon bestand in sync, zonder vastzetten ---
    let lokaal = ruimte.join("lokaal.txt");
    controleer(cloudfilter::markeer_op_aanvraag(&ruimte, &lokaal, true)?, "markeer_op_aanvraag lokaal.txt")?;
    let st = cloudfilter::placeholder_status(&lokaal)?;
    controleer(st.is_some_and(|p| p.in_sync && !p.vastgezet), &format!("lokaal.txt in sync, niet vastgezet: {st:?}"))?;

    // --- alleen online: aanmaken en openen ---
    let klein = inhoud(10_000, 1);
    let groot = inhoud(3_500_000, 7);
    server.lock().unwrap().insert("B-klein".into(), klein.clone());
    server.lock().unwrap().insert("B-groot".into(), groot.clone());
    let p_klein = ruimte.join("Tekeningen").join("plan.pdf");
    let p_groot = ruimte.join("model.ifc");
    cloudfilter::maak_alleen_online(&p_klein, klein.len() as u64, "B-klein")?;
    cloudfilter::maak_alleen_online(&p_groot, groot.len() as u64, "B-groot")?;
    let a = cloudfilter::attributen(&p_groot).unwrap_or(0);
    controleer(cloudfilter::is_ontwaterd(a), &format!("model.ifc staat alleen online (attributen 0x{a:X})"))?;
    controleer(std::fs::metadata(&p_groot)?.len() == groot.len() as u64, "grootte van de servervrsie zichtbaar zonder ophalen")?;
    controleer(cloudfilter::bestandsidentiteit(&p_groot).as_deref() == Some("B-groot"), "bestandsidentiteit = bestand-id op de server")?;
    controleer(verzoeken.lock().unwrap().is_empty(), "nog niets opgehaald")?;
    controleer(std::fs::read(&p_klein)? == klein, "plan.pdf openen haalt de inhoud op (callback)")?;
    controleer(std::fs::read(&p_groot)? == groot, "model.ifc (3,5 MB) openen: inhoud klopt (meerdere stukken)")?;
    println!("  (info) ophaalverzoeken: {:?}", verzoeken.lock().unwrap());
    let a = cloudfilter::attributen(&p_groot).unwrap_or(0);
    controleer(!cloudfilter::is_ontwaterd(a), &format!("model.ifc daarna lokaal (attributen 0x{a:X})"))?;
    let st = cloudfilter::placeholder_status(&p_groot)?;
    controleer(st.is_some_and(|p| p.in_sync), &format!("model.ifc blijft in sync: {st:?}"))?;

    // --- ruimte vrijmaken ---
    let voor = std::fs::metadata(&p_groot)?;
    cloudfilter::ontwater(&p_groot)?;
    let na = std::fs::metadata(&p_groot)?;
    controleer(cloudfilter::is_ontwaterd(cloudfilter::attributen(&p_groot).unwrap_or(0)), "ontwater: model.ifc weer alleen online")?;
    controleer(voor.len() == na.len() && voor.modified()? == na.modified()?, "grootte en wijzigingstijd blijven gelijk (motor ziet niets)")?;
    // Een gedownload (volledig) bestand dat de app tot placeholder maakte, kan ook vrijgemaakt worden.
    let gedownload = ruimte.join("gedownload.txt");
    std::fs::write(&gedownload, "volledig gedownload")?;
    cloudfilter::markeer_op_aanvraag(&ruimte, &gedownload, true)?;
    let r = cloudfilter::ontwater(&gedownload);
    controleer(r.is_ok(), &format!("ontwater: een omgezet, gedownload bestand kan vrij: {r:?}"))?;
    controleer(cloudfilter::is_ontwaterd(cloudfilter::attributen(&gedownload).unwrap_or(0)), "gedownload.txt staat nu alleen online")?;
    std::fs::write(ruimte.join("lokaal.txt"), "gewijzigd, nog niet op de server")?;
    let st = cloudfilter::placeholder_status(&ruimte.join("lokaal.txt"))?;
    println!("  (info) lokaal.txt na schrijven: {st:?}");
    let fout = cloudfilter::markeer_op_aanvraag(&ruimte, &ruimte.join("lokaal.txt"), false)
        .and_then(|_| cloudfilter::ontwater(&ruimte.join("lokaal.txt")));
    controleer(fout.is_err(), &format!("een gewijzigd bestand (niet in sync) vrijmaken wordt geweigerd: {}", fout.err().map(|e| e.to_string()).unwrap_or_default()))?;
    controleer(std::fs::read_to_string(ruimte.join("lokaal.txt"))? == "gewijzigd, nog niet op de server", "het werk blijft staan")?;

    // --- altijd op dit apparaat / ruimte vrijmaken via de attributen (zoals het rechtermuismenu) ---
    verzoeken.lock().unwrap().clear();
    attrib(&["+P"], &p_groot)?;
    controleer(cloudfilter::volg_pinstatus(&p_groot)? == cloudfilter::Pinactie::Opgehaald, "attrib +P -> volg_pinstatus haalt op")?;
    let a = cloudfilter::attributen(&p_groot).unwrap_or(0);
    controleer(!cloudfilter::is_ontwaterd(a) && a & cloudfilter::ATTR_VASTGEZET != 0, &format!("vastgezet en lokaal (0x{a:X})"))?;
    controleer(!verzoeken.lock().unwrap().is_empty(), "ophalen liep via de callback")?;
    controleer(cloudfilter::volg_pinstatus(&p_groot)? == cloudfilter::Pinactie::Niets, "tweede keer: niets te doen")?;
    attrib(&["-P", "+U"], &p_groot)?;
    controleer(cloudfilter::volg_pinstatus(&p_groot)? == cloudfilter::Pinactie::Vrijgemaakt, "attrib -P +U -> volg_pinstatus maakt ruimte vrij")?;
    controleer(cloudfilter::is_ontwaterd(cloudfilter::attributen(&p_groot).unwrap_or(0)), "weer alleen online")?;
    attrib(&["-U"], &p_groot)?;

    // --- nieuwe versie op de server zonder downloaden ---
    let nieuw = inhoud(4_000, 9);
    server.lock().unwrap().insert("B-groot-2".into(), nieuw.clone());
    verzoeken.lock().unwrap().clear();
    cloudfilter::werk_alleen_online_bij(&p_groot, nieuw.len() as u64, "B-groot-2")?;
    controleer(std::fs::metadata(&p_groot)?.len() == nieuw.len() as u64, "werk_alleen_online_bij: nieuwe grootte")?;
    controleer(cloudfilter::is_ontwaterd(cloudfilter::attributen(&p_groot).unwrap_or(0)), "blijft alleen online")?;
    controleer(verzoeken.lock().unwrap().is_empty(), "er is niets gedownload")?;
    controleer(std::fs::read(&p_groot)? == nieuw, "openen geeft de nieuwe versie")?;

    // --- nieuw bestand in een vastgezette map erft het vastzetten ---
    cloudfilter::markeer_op_aanvraag(&ruimte, &ruimte.join("Tekeningen"), true)?;
    cloudfilter::zet_pin(&ruimte.join("Tekeningen"), Some(true))?;
    std::fs::write(ruimte.join("Tekeningen").join("nieuw.txt"), "nieuw")?;
    cloudfilter::markeer_op_aanvraag(&ruimte, &ruimte.join("Tekeningen").join("nieuw.txt"), true)?;
    let st = cloudfilter::placeholder_status(&ruimte.join("Tekeningen").join("nieuw.txt"))?;
    controleer(st.is_some_and(|p| p.vastgezet && p.in_sync), &format!("nieuw bestand in een vastgezette map is vastgezet: {st:?}"))?;

    // --- eigen kolommen (status en vergrendeling) ---
    let r = cloudfilter::zet_eigenschappen(
        &p_klein,
        &[
            (cloudfilter::EIGENSCHAP_STATUS, "Fout: de server weigerde het bestand".into(), cloudfilter::ICOON_FOUT),
            (cloudfilter::EIGENSCHAP_SLOT, "Vergrendeld door jan@voorbeeld.nl".into(), cloudfilter::ICOON_SLOT),
        ],
    );
    println!("  (info) eigen kolommen zetten: {r:?}");
    controleer(r.is_ok(), "eigen kolommen op plan.pdf (StorageProviderItemProperties)")?;

    if let Some(s) = std::env::var("OPENAEC_ROOKTEST_WACHT").ok().and_then(|s| s.parse::<u64>().ok()) {
        let _ = std::process::Command::new("explorer").arg(&ruimte).spawn();
        println!("  (info) {s} s om in de Verkenner te kijken: {}", ruimte.display());
        std::thread::sleep(std::time::Duration::from_secs(s));
    }
    let leeg = cloudfilter::zet_eigenschappen(&p_klein, &[]);
    controleer(leeg.is_ok(), "eigen kolommen weer weghalen")?;
    Ok(())
}
