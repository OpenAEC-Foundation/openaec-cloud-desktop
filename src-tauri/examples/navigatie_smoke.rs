// Rooktest voor het item "OpenAEC" in het navigatiepaneel (navigatie.rs), met een TIJDELIJKE CLSID en een testnaam,
// op mappen onder %TEMP% — de echte registratie van de app (navigatie::CLSID) wordt niet aangeraakt, alleen gelezen
// om dat te controleren.
// Draai met:  cargo run --example navigatie_smoke
// Met OPENAEC_ROOKTEST_WACHT=<seconden> blijft het item zo lang staan (en opent de Verkenner het), om in het
// navigatiepaneel te kijken; daarna wordt alles opgeruimd.
//
// Stappen: één map (het item toont die map) -> meer mappen (een map met mapsnelkoppelingen) -> terug naar één ->
// geen koppelingen (het item en de snelkoppelingen verdwijnen, de echte mappen blijven) -> opruimen.

#[allow(dead_code)]
#[path = "../src/navigatie.rs"]
mod navigatie;

#[cfg(not(windows))]
fn main() {
    println!("alleen voor Windows");
}

#[cfg(windows)]
const NAAM: &str = "OpenAEC (rooktest)";

#[cfg(windows)]
fn main() {
    let dir = std::env::temp_dir().join(format!("openaec-navigatie-rooktest-{}", std::process::id()));
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let clsid = format!("{{{:08X}-0000-4000-8000-{:012X}}}", std::process::id(), nanos & 0xFFFF_FFFF_FFFF);
    assert_ne!(clsid, navigatie::CLSID, "de rooktest mag de echte CLSID niet gebruiken");
    let echt_voor = navigatie::lees(navigatie::CLSID);
    println!("tijdelijke CLSID: {clsid}");

    let uitkomst = proef(&dir, &clsid);

    let _ = navigatie::verwijder(&clsid);
    let _ = navigatie::vul_snelkoppelingen(&dir.join("Snelkoppelingen"), &[]);
    let _ = std::fs::remove_dir_all(&dir);
    let echt_na = navigatie::lees(navigatie::CLSID);
    println!(
        "opgeruimd: item {}, mappen {}, echte registratie {}",
        if navigatie::lees(&clsid).is_none() && !in_naamruimte(&clsid) && !op_bureaublad_verborgen(&clsid) { "weg" } else { "STAAT ER NOG" },
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
    println!("  {:<74} {}", wat, if ok { "OK" } else { "MISLUKT" });
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

/// Hoe de shell zelf een parseernaam ziet: (weergavenaam, bestandssysteempad als dat er is).
#[cfg(windows)]
fn shell(naam: &str) -> Option<(String, Option<String>)> {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(naam), None).ok()?;
        let lees = |s| {
            item.GetDisplayName(s).ok().map(|p| {
                let t = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                t
            })
        };
        Some((lees(SIGDN_NORMALDISPLAY)?, lees(SIGDN_FILESYSPATH)))
    }
}

#[cfg(windows)]
fn proef(dir: &std::path::Path, clsid: &str) -> anyhow::Result<()> {
    use navigatie::Doel;

    anyhow::ensure!(dir.starts_with(std::env::temp_dir()), "weiger: {} ligt niet onder %TEMP%", dir.display());
    let _ = std::fs::remove_dir_all(dir);
    let (a, b, snel) = (dir.join("Ruimte A"), dir.join("Ruimte B"), dir.join("Snelkoppelingen"));
    std::fs::create_dir_all(a.join("Tekeningen"))?;
    std::fs::create_dir_all(&b)?;
    std::fs::write(a.join("plan.txt"), "echt bestand")?;
    std::fs::write(b.join("notitie.txt"), "echt bestand")?;
    let icoon = format!("{},0", std::env::current_exe()?.display());
    let gelijk = |x: &str, y: &std::path::Path| std::path::Path::new(x) == y;

    // --- één koppeling: het item toont die map ---
    navigatie::pas_toe(clsid, NAAM, &icoon, &Doel::Map(a.clone()), &snel)?;
    let item = navigatie::lees(clsid);
    println!("  (info) register: {item:?}");
    controleer(
        item.as_ref().is_some_and(|(n, m, vast, i, verborgen)| n == NAAM && gelijk(m, &a) && *vast && *i == icoon && *verborgen),
        "register: naam, TargetFolderPath, vastgezet, icoon, niet op het bureaublad",
    )?;
    controleer(in_naamruimte(clsid), "vermelding onder Desktop\\NameSpace")?;
    let gezien = shell(&format!("::{clsid}"));
    println!("  (info) shell: {gezien:?}");
    controleer(gezien.as_ref().is_some_and(|(n, _)| n == NAAM), "de shell kent het item onder zijn naam")?;
    let kind = shell(&format!("::{clsid}\\Tekeningen"));
    println!("  (info) shell, submap: {kind:?}");
    controleer(
        kind.as_ref().and_then(|(_, p)| p.as_deref()).is_some_and(|p| gelijk(p, &a.join("Tekeningen"))),
        "bladeren in het item komt uit in de echte map",
    )?;
    navigatie::pas_toe(clsid, NAAM, &icoon, &Doel::Map(a.clone()), &snel)?;
    controleer(navigatie::lees(clsid) == item, "nogmaals toepassen verandert niets")?;

    // --- meer koppelingen: een map met mapsnelkoppelingen ---
    let lijst = vec![("Ruimte A".to_string(), a.clone()), ("Ruimte B".to_string(), b.clone())];
    navigatie::pas_toe(clsid, NAAM, &icoon, &Doel::Snelkoppelingen(lijst), &snel)?;
    controleer(navigatie::lees(clsid).is_some_and(|(_, m, ..)| gelijk(&m, &snel)), "TargetFolderPath = de map met snelkoppelingen")?;
    controleer(
        navigatie::is_snelkoppeling(&snel.join("Ruimte A")) && navigatie::is_snelkoppeling(&snel.join("Ruimte B")),
        "twee mapsnelkoppelingen (desktop.ini + target.lnk)",
    )?;
    let via = shell(&snel.join("Ruimte B").join("notitie.txt").to_string_lossy());
    println!("  (info) shell, via snelkoppeling: {via:?}");
    controleer(
        via.as_ref().and_then(|(_, p)| p.as_deref()).is_some_and(|p| gelijk(p, &b.join("notitie.txt"))),
        "een bestand via de snelkoppeling is het echte bestand",
    )?;

    if let Some(s) = std::env::var("OPENAEC_ROOKTEST_WACHT").ok().and_then(|s| s.parse::<u64>().ok()) {
        let _ = std::process::Command::new("explorer.exe").arg(format!("shell:::{clsid}")).spawn();
        println!("  (info) het item \"{NAAM}\" staat {s} s in het navigatiepaneel…");
        std::thread::sleep(std::time::Duration::from_secs(s));
    }

    // --- terug naar één: de snelkoppelingen verdwijnen ---
    navigatie::pas_toe(clsid, NAAM, &icoon, &Doel::Map(b.clone()), &snel)?;
    controleer(navigatie::lees(clsid).is_some_and(|(_, m, ..)| gelijk(&m, &b)), "terug naar één map")?;
    controleer(!snel.exists(), "de map met snelkoppelingen is weg")?;

    // --- geen koppelingen: het item verdwijnt, de echte mappen blijven ---
    navigatie::pas_toe(clsid, NAAM, &icoon, &Doel::Geen, &snel)?;
    controleer(navigatie::lees(clsid).is_none(), "CLSID weg")?;
    controleer(!in_naamruimte(clsid), "vermelding onder Desktop\\NameSpace weg")?;
    controleer(!op_bureaublad_verborgen(clsid), "HideDesktopIcons-waarde weg")?;
    controleer(shell(&format!("::{clsid}")).is_none(), "de shell kent het item niet meer")?;
    controleer(a.join("plan.txt").exists() && b.join("notitie.txt").exists(), "de echte bestanden staan er nog")?;
    Ok(())
}
