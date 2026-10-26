// Rooktest voor "Versturen met OpenAEC" in het rechtermuismenu van de Verkenner, met een TESTWERKWOORD onder een
// testnaam (`OpenAECVersturenRooktest`) dat naar dit programma wijst — nooit het echte werkwoord van de app, en zonder
// de instellingen of de installatie van de app aan te raken.
// Draai met:  cargo run --example versturen_smoke
//
// Stappen: het werkwoord registreren en de waarden teruglezen uit het register -> de Verkenner (Shell.Application)
// noemt het item bij een bestand en een map -> het werkwoord aanroepen op drie bestanden tegelijk (zoals een selectie
// in de Verkenner) en op een map -> elke aanroep komt hier binnen als `--actie versturen <pad>` -> opruimen.

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../src/rechtermuis.rs"]
mod rechtermuis;

#[cfg(not(windows))]
fn main() {
    println!("alleen voor Windows");
}

#[cfg(windows)]
const SLEUTEL: &str = "OpenAECVersturenRooktest";
#[cfg(windows)]
const LABEL: &str = "Versturen met OpenAEC (rooktest)";

/// Het logbestand waarin elke aanroep vanuit de Verkenner een regel zet.
#[cfg(windows)]
fn log() -> std::path::PathBuf {
    std::env::temp_dir().join("openaec-versturen-rooktest.log")
}

#[cfg(windows)]
fn main() {
    let argv: Vec<String> = std::env::args().collect();
    // Aangeroepen door de Verkenner: de paden opschrijven en stoppen.
    if let Some(i) = argv.iter().position(|a| a == "--actie") {
        use std::io::Write;
        let regel = format!("{}\n", argv[i + 1..].join("\t"));
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(log()).expect("logbestand");
        f.write_all(regel.as_bytes()).expect("schrijven");
        return;
    }

    let dir = std::env::temp_dir().join(format!("openaec-versturen-rooktest-{}", std::process::id()));
    let _ = std::fs::remove_file(log());
    let uitkomst = proef(&dir);
    let opgeruimd = rechtermuis::verwijder_windows(SLEUTEL);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(log());
    let weg = waarde(&format!("*\\shell\\{SLEUTEL}"), "MUIVerb").is_none() && waarde(&format!("Directory\\shell\\{SLEUTEL}"), "MUIVerb").is_none();
    println!(
        "opgeruimd: testwerkwoord {}, map {}",
        if weg && opgeruimd.is_ok() { "weg" } else { "STAAT ER NOG" },
        if dir.exists() { "STAAT ER NOG" } else { "weg" }
    );
    match uitkomst {
        Ok(()) if weg => println!("\nALLES OK"),
        Ok(()) => {
            println!("\nMISLUKT: het testwerkwoord is niet opgeruimd");
            std::process::exit(1);
        }
        Err(e) => {
            println!("\nMISLUKT: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(windows)]
fn waarde(sleutel: &str, naam: &str) -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!("Software\\Classes\\{sleutel}")).ok()?.get_value(naam).ok()
}

#[cfg(windows)]
fn controleer(ok: bool, wat: &str) -> Result<(), String> {
    println!("{} {wat}", if ok { "ok  " } else { "FOUT" });
    if ok {
        Ok(())
    } else {
        Err(wat.to_string())
    }
}

/// PowerShell met de Shell van Windows (dezelfde als de Verkenner); geeft wat het script schrijft.
#[cfg(windows)]
fn powershell(script: &str) -> Result<String, String> {
    let uit = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    if !uit.status.success() {
        return Err(format!("powershell: {}", String::from_utf8_lossy(&uit.stderr)));
    }
    Ok(String::from_utf8_lossy(&uit.stdout).to_string())
}

/// Wacht tot het log `aantal` regels heeft (de Verkenner start de aanroepen los van elkaar).
#[cfg(windows)]
fn regels(aantal: usize) -> Vec<String> {
    for _ in 0..100 {
        let tekst = std::fs::read_to_string(log()).unwrap_or_default();
        let r: Vec<String> = tekst.lines().map(str::to_string).collect();
        if r.len() >= aantal {
            return r;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    std::fs::read_to_string(log()).unwrap_or_default().lines().map(str::to_string).collect()
}

#[cfg(windows)]
fn proef(dir: &std::path::Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?.to_string_lossy().to_string();
    std::fs::create_dir_all(dir.join("Tekeningen")).map_err(|e| e.to_string())?;
    for naam in ["a.pdf", "b met spatie.dwg", "leeg.txt"] {
        std::fs::write(dir.join(naam), if naam == "leeg.txt" { &b""[..] } else { &b"x"[..] }).map_err(|e| e.to_string())?;
    }

    // 1. Registreren en teruglezen.
    rechtermuis::registreer_windows(&exe, SLEUTEL, LABEL).map_err(|e| format!("registreren: {e}"))?;
    for w in rechtermuis::windows_waarden(&exe, SLEUTEL, LABEL) {
        controleer(waarde(&w.sleutel, w.naam).as_deref() == Some(w.waarde.as_str()), &format!("register {}\\{} = {}", w.sleutel, w.naam, w.waarde))?;
    }

    // 2. De Verkenner kent het item bij een bestand en een map.
    let map = dir.to_string_lossy().replace('\'', "''");
    let werkwoorden = powershell(&format!(
        "$s = New-Object -ComObject Shell.Application; $f = $s.Namespace('{map}'); \
         'bestand:' + (($f.ParseName('a.pdf').Verbs() | ForEach-Object {{ $_.Name }}) -join '|'); \
         'map:' + (($f.ParseName('Tekeningen').Verbs() | ForEach-Object {{ $_.Name }}) -join '|')"
    ))?;
    let regel = |soort: &str| werkwoorden.lines().find(|r| r.starts_with(soort)).unwrap_or("").to_string();
    controleer(regel("bestand:").split('|').any(|v| v == LABEL), "de Verkenner toont het item bij een bestand")?;
    controleer(regel("map:").split('|').any(|v| v == LABEL), "de Verkenner toont het item bij een map")?;

    // 3. Drie bestanden tegelijk (de bestanden van de map, zonder de submap), zoals een selectie in de Verkenner.
    powershell(&format!(
        "$s = New-Object -ComObject Shell.Application; $items = $s.Namespace('{map}').Items(); $items.Filter(64, '*'); \
         $items.InvokeVerbEx('{SLEUTEL}')"
    ))?;
    let binnen = regels(3);
    controleer(binnen.len() == 3, &format!("drie aanroepen voor drie bestanden (kreeg {})", binnen.len()))?;
    for naam in ["a.pdf", "b met spatie.dwg", "leeg.txt"] {
        let verwacht = format!("versturen\t{}", dir.join(naam).to_string_lossy());
        controleer(binnen.iter().any(|r| r.eq_ignore_ascii_case(&verwacht)), &format!("--actie versturen \"{}\"", dir.join(naam).display()))?;
    }

    // 4. Een map.
    powershell(&format!("$s = New-Object -ComObject Shell.Application; $s.Namespace('{map}').ParseName('Tekeningen').InvokeVerbEx('{SLEUTEL}')"))?;
    let binnen = regels(4);
    let verwacht = format!("versturen\t{}", dir.join("Tekeningen").to_string_lossy());
    controleer(binnen.iter().any(|r| r.eq_ignore_ascii_case(&verwacht)), "--actie versturen op een map")?;
    Ok(())
}
