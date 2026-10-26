// Rooktest voor het item "OpenAEC Cloud" in het navigatiepaneel (navigatie.rs), met een TIJDELIJKE CLSID en een
// testnaam, op mappen onder %TEMP% — de echte registratie van de app (navigatie::CLSID) wordt niet aangeraakt, alleen
// gelezen om dat te controleren.
// Draai met:  cargo run --example navigatie_smoke
// Met OPENAEC_ROOKTEST_WACHT=<seconden> blijft het item zo lang staan (en opent de Verkenner het), om in het
// navigatiepaneel te kijken; daarna wordt alles opgeruimd.
//
// Stappen: één koppeling (het item toont de eigen map met één mapsnelkoppeling; de weergavenaam van het item en van
// het kind, bladeren en een nieuw bestand via de shell komen in de echte map) -> meer koppelingen -> een koppeling
// binnen de map van een andere sync-app (alleen als die op deze computer bestaat; daar wordt alleen gelezen) -> de
// hoofdmap actief (geen eigen item) -> geen koppelingen (het item en de snelkoppelingen verdwijnen, de echte mappen
// blijven) -> opruimen.

#[allow(dead_code)]
#[path = "../src/navigatie.rs"]
mod navigatie;

#[cfg(not(windows))]
fn main() {
    println!("alleen voor Windows");
}

#[cfg(windows)]
const NAAM: &str = "OpenAEC Cloud (rooktest)";

#[cfg(windows)]
fn main() {
    let dir = std::env::temp_dir().join(format!("openaec-navigatie-rooktest-{}", std::process::id()));
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let clsid = format!("{{{:08X}-0000-4000-8000-{:012X}}}", std::process::id(), nanos & 0xFFFF_FFFF_FFFF);
    let proef_clsid = format!("{{{:08X}-0000-4000-8001-{:012X}}}", std::process::id(), nanos & 0xFFFF_FFFF_FFFF);
    assert_ne!(clsid, navigatie::CLSID, "de rooktest mag de echte CLSID niet gebruiken");
    assert_ne!(proef_clsid, navigatie::CLSID, "de rooktest mag de echte CLSID niet gebruiken");
    let echt_voor = navigatie::lees(navigatie::CLSID);
    println!("tijdelijke CLSID: {clsid}");

    let uitkomst = proef(&dir, &clsid, &proef_clsid);

    let _ = navigatie::verwijder(&clsid);
    let _ = navigatie::verwijder(&proef_clsid);
    let _ = navigatie::vul_snelkoppelingen(&dir.join("Snelkoppelingen"), &[]);
    let _ = std::fs::remove_dir_all(&dir);
    let echt_na = navigatie::lees(navigatie::CLSID);
    let weg = |c: &str| navigatie::lees(c).is_none() && !in_naamruimte(c) && !op_bureaublad_verborgen(c);
    println!(
        "opgeruimd: item {}, mappen {}, echte registratie {}",
        if weg(&clsid) && weg(&proef_clsid) { "weg" } else { "STAAT ER NOG" },
        if dir.exists() { "STAAN ER NOG" } else { "weg" },
        if echt_voor == echt_na { "onveranderd" } else { "VERANDERD" }
    );
    match uitkomst {
        Ok(()) if echt_voor == echt_na => println!("\nALLES OK"),
        Ok(()) => {
            println!("\nMISLUKT: de echte registratie is veranderd");
            std::process::exit(1);
        }
        Err(e) => {
            println!("\nMISLUKT: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(windows)]
fn controleer(ok: bool, wat: &str) -> anyhow::Result<()> {
    println!("  {:<84} {}", wat, if ok { "OK" } else { "MISLUKT" });
    anyhow::ensure!(ok, "{wat}");
    Ok(())
}

#[cfg(windows)]
fn in_naamruimte(clsid: &str) -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Desktop\\NameSpace\\{clsid}"))
        .is_ok()
}

#[cfg(windows)]
fn op_bureaublad_verborgen(clsid: &str) -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\HideDesktopIcons\\NewStartPanel")
        .and_then(|k| k.get_value::<u32, _>(clsid))
        .is_ok()
}

/// Een waarde onder `HKCU\Software\Classes\CLSID\{clsid}`.
#[cfg(windows)]
fn klassewaarde(clsid: &str, naam: &str) -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!("Software\\Classes\\CLSID\\{clsid}")).ok()?.get_value(naam).ok()
}

/// De map van een sync-root van een andere app op deze computer (uit de registraties bij Windows), als die er is.
#[cfg(windows)]
fn vreemde_sync_root() -> Option<std::path::PathBuf> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let basis = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\SyncRootManager").ok()?;
    basis.enum_keys().flatten().filter(|n| !n.starts_with("OpenAEC")).find_map(|n| {
        let mappen = basis.open_subkey(format!("{n}\\UserSyncRoots")).ok()?;
        mappen
            .enum_values()
            .flatten()
            .map(|(_, v)| std::path::PathBuf::from(v.to_string().trim_end_matches('\0')))
            .find(|p| p.is_dir())
    })
}

#[cfg(windows)]
fn com() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
}

#[cfg(windows)]
fn item(naam: &str) -> Option<windows::Win32::UI::Shell::IShellItem> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::SHCreateItemFromParsingName;
    com();
    unsafe { SHCreateItemFromParsingName(&HSTRING::from(naam), None).ok() }
}

/// Hoe de shell zelf een parseernaam ziet, via `IShellItem::GetDisplayName`: (weergavenaam, naam binnen de ouder
/// zoals de boom van het navigatiepaneel hem toont, bestandssysteempad als dat er is).
#[cfg(windows)]
fn shell(naam: &str) -> Option<(String, String, Option<String>)> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SIGDN, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVE};
    let item = item(naam)?;
    let lees = |s: SIGDN| unsafe {
        item.GetDisplayName(s).ok().map(|p| {
            let t = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            t
        })
    };
    Some((lees(SIGDN_NORMALDISPLAY)?, lees(SIGDN_PARENTRELATIVE)?, lees(SIGDN_FILESYSPATH)))
}

/// Maak via de shell (`IFileOperation::NewItem`, zoals de Verkenner bij "Nieuw") een leeg bestand in de map met
/// parseernaam `map`.
#[cfg(windows)]
fn nieuw_bestand(map: &str, naam: &str) -> anyhow::Result<()> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    use windows::Win32::UI::Shell::{FileOperation, IFileOperation, FOF_NO_UI};
    let doel = item(map).ok_or_else(|| anyhow::anyhow!("{map} onbekend bij de shell"))?;
    unsafe {
        let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        op.SetOperationFlags(FOF_NO_UI)?;
        op.NewItem(&doel, 0x80, &HSTRING::from(naam), PCWSTR::null(), None)?;
        op.PerformOperations()?;
    }
    Ok(())
}

#[cfg(windows)]
fn proef(dir: &std::path::Path, clsid: &str, proef_clsid: &str) -> anyhow::Result<()> {
    use navigatie::Doel;

    anyhow::ensure!(dir.starts_with(std::env::temp_dir()), "weiger: {} ligt niet onder %TEMP%", dir.display());
    let _ = std::fs::remove_dir_all(dir);
    let (a, b, snel) = (dir.join("ruimte-a-lokaal"), dir.join("ruimte-b-lokaal"), dir.join("Snelkoppelingen"));
    std::fs::create_dir_all(a.join("Tekeningen"))?;
    std::fs::create_dir_all(&b)?;
    std::fs::write(a.join("plan.txt"), "echt bestand")?;
    std::fs::write(b.join("notitie.txt"), "echt bestand")?;
    let icoon = format!("{},0", std::env::current_exe()?.display());
    let gelijk = |x: &str, y: &std::path::Path| std::path::Path::new(x) == y;
    let wortel = format!("::{clsid}");

    // --- één koppeling: het item toont de eigen map, met één mapsnelkoppeling met de naam van de ruimte ---
    let een = navigatie::doel(None, &[("Ruimte A".into(), a.to_string_lossy().to_string(), false)]);
    controleer(matches!(&een, Doel::Snelkoppelingen(l) if l.len() == 1), "doel bij één koppeling: de eigen map met één snelkoppeling")?;
    navigatie::pas_toe(clsid, NAAM, &icoon, &een, &snel)?;
    let reg = navigatie::lees(clsid);
    println!("  (info) register: {reg:?}");
    controleer(
        reg.as_ref().is_some_and(|(n, m, vast, i, verborgen)| n == NAAM && gelijk(m, &snel) && *vast && *i == icoon && *verborgen),
        "register: naam, TargetFolderPath = eigen map, vastgezet, icoon, niet op het bureaublad",
    )?;
    controleer(
        klassewaarde(clsid, "LocalizedString").as_deref() == Some(NAAM) && klassewaarde(clsid, "InfoTip").as_deref() == Some(NAAM),
        "register: LocalizedString en InfoTip",
    )?;
    controleer(in_naamruimte(clsid), "vermelding onder Desktop\\NameSpace")?;
    controleer(navigatie::is_snelkoppeling(&snel.join("Ruimte A")), "mapsnelkoppeling \"Ruimte A\" (desktop.ini + target.lnk)")?;
    let gezien = shell(&wortel);
    println!("  (info) shell, item: {gezien:?}");
    controleer(gezien.as_ref().is_some_and(|(n, m, _)| n == NAAM && m == NAAM), "weergavenaam van het item (gewoon en in de boom)")?;
    let kind = shell(&format!("{wortel}\\Ruimte A"));
    println!("  (info) shell, kind: {kind:?}");
    controleer(kind.as_ref().is_some_and(|(n, m, _)| n == "Ruimte A" && m == "Ruimte A"), "weergavenaam van het kind = naam van de ruimte, niet de mapnaam")?;
    let klein = shell(&format!("{wortel}\\Ruimte A\\Tekeningen"));
    println!("  (info) shell, kleinkind: {klein:?}");
    controleer(
        klein.as_ref().and_then(|(_, _, p)| p.as_deref()).is_some_and(|p| gelijk(p, &a.join("Tekeningen"))),
        "doorklikken in het item komt uit in de echte map",
    )?;
    let bestand = shell(&format!("{wortel}\\Ruimte A\\plan.txt"));
    controleer(
        bestand.as_ref().and_then(|(_, _, p)| p.as_deref()).is_some_and(|p| gelijk(p, &a.join("plan.txt"))),
        "een bestand openen via het item opent het echte bestand",
    )?;
    nieuw_bestand(&format!("{wortel}\\Ruimte A"), "opgeslagen.txt")?;
    controleer(
        a.join("opgeslagen.txt").is_file() && !snel.join("Ruimte A").join("opgeslagen.txt").exists(),
        "een nieuw bestand via het item (shell) komt in de echte map",
    )?;
    navigatie::pas_toe(clsid, NAAM, &icoon, &een, &snel)?;
    controleer(navigatie::lees(clsid) == reg, "nogmaals toepassen verandert niets")?;

    // --- meer koppelingen ---
    let lijst = vec![("Ruimte A".to_string(), a.clone()), ("Ruimte B".to_string(), b.clone())];
    navigatie::pas_toe(clsid, NAAM, &icoon, &Doel::Snelkoppelingen(lijst), &snel)?;
    controleer(navigatie::lees(clsid).is_some_and(|(_, m, ..)| gelijk(&m, &snel)), "TargetFolderPath blijft de eigen map")?;
    controleer(
        navigatie::is_snelkoppeling(&snel.join("Ruimte A")) && navigatie::is_snelkoppeling(&snel.join("Ruimte B")),
        "twee mapsnelkoppelingen",
    )?;
    let via = shell(&format!("{wortel}\\Ruimte B\\notitie.txt"));
    println!("  (info) shell, via snelkoppeling: {via:?}");
    controleer(
        via.as_ref().and_then(|(_, _, p)| p.as_deref()).is_some_and(|p| gelijk(p, &b.join("notitie.txt"))),
        "een bestand via de tweede snelkoppeling is het echte bestand",
    )?;

    // --- een koppeling binnen de map van een andere sync-app (daar wordt alleen gelezen) ---
    match vreemde_sync_root() {
        None => println!("  (info) geen sync-root van een andere app op deze computer; deze stap vervalt"),
        Some(vreemd) => {
            // Ter vergelijking: een item dat rechtstreeks naar die map wijst, krijgt de weergavenaam van die sync-root.
            navigatie::registreer(proef_clsid, NAAM, &icoon, &vreemd)?;
            let direct = shell(&format!("::{proef_clsid}"));
            println!("  (info) item rechtstreeks op een map van een andere sync-app heet: {:?}", direct.as_ref().map(|d| &d.0));
            navigatie::verwijder(proef_clsid)?;
            let lijst = vec![("Ruimte A".to_string(), a.clone()), ("Ruimte elders".to_string(), vreemd.clone())];
            navigatie::pas_toe(clsid, NAAM, &icoon, &Doel::Snelkoppelingen(lijst), &snel)?;
            let gezien = shell(&wortel);
            let kind = shell(&format!("{wortel}\\Ruimte elders"));
            println!("  (info) shell, item en kind: {:?} / {:?}", gezien.as_ref().map(|g| &g.0), kind.as_ref().map(|k| &k.0));
            controleer(gezien.as_ref().is_some_and(|(n, m, _)| n == NAAM && m == NAAM), "met een map van een andere sync-app: het item houdt zijn naam")?;
            controleer(kind.as_ref().is_some_and(|(n, _, _)| n == "Ruimte elders"), "met een map van een andere sync-app: het kind houdt de naam van de ruimte")?;
        }
    }

    if let Some(s) = std::env::var("OPENAEC_ROOKTEST_WACHT").ok().and_then(|s| s.parse::<u64>().ok()) {
        let _ = std::process::Command::new("explorer.exe").arg(format!("shell:::{clsid}")).spawn();
        println!("  (info) het item \"{NAAM}\" staat {s} s in het navigatiepaneel…");
        std::thread::sleep(std::time::Duration::from_secs(s));
    }

    // --- terug naar één: de overige snelkoppelingen verdwijnen ---
    navigatie::pas_toe(clsid, NAAM, &icoon, &Doel::Snelkoppelingen(vec![("Ruimte B".to_string(), b.clone())]), &snel)?;
    controleer(
        navigatie::is_snelkoppeling(&snel.join("Ruimte B")) && !snel.join("Ruimte A").exists() && !snel.join("Ruimte elders").exists(),
        "terug naar één snelkoppeling",
    )?;

    // --- de hoofdmap actief: geen eigen item (de sync-root heeft er al een) ---
    navigatie::pas_toe(clsid, NAAM, &icoon, &Doel::Hoofdmap(dir.to_path_buf()), &snel)?;
    controleer(navigatie::lees(clsid).is_none() && !in_naamruimte(clsid), "hoofdmap actief: geen eigen item")?;
    controleer(!snel.exists(), "hoofdmap actief: de map met snelkoppelingen is weg")?;

    // --- geen koppelingen: het item verdwijnt, de echte mappen blijven ---
    navigatie::pas_toe(clsid, NAAM, &icoon, &een, &snel)?;
    controleer(navigatie::lees(clsid).is_some(), "weer een koppeling: het item is terug")?;
    navigatie::pas_toe(clsid, NAAM, &icoon, &Doel::Geen, &snel)?;
    controleer(navigatie::lees(clsid).is_none(), "CLSID weg")?;
    controleer(!in_naamruimte(clsid), "vermelding onder Desktop\\NameSpace weg")?;
    controleer(!op_bureaublad_verborgen(clsid), "HideDesktopIcons-waarde weg")?;
    controleer(shell(&wortel).is_none(), "de shell kent het item niet meer")?;
    controleer(!snel.exists(), "de map met snelkoppelingen is weg")?;
    controleer(a.join("plan.txt").exists() && b.join("notitie.txt").exists(), "de echte bestanden staan er nog")?;
    Ok(())
}
