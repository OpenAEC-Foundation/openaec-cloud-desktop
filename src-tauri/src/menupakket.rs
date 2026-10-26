//! "OpenAEC" in het moderne rechtermuismenu van Windows 11 (de DLL uit `crates/openaec-shellmenu`).
//!
//! Windows 11 toont in het korte menu alleen een `IExplorerCommand` uit een pakket met identiteit. De installer zet
//! daarvoor een ondertekend *sparse package* in `shellmenu\` (alleen het manifest; de DLL en het logo staan ernaast in
//! de installatiemap). Deze module:
//!
//! - schrijft `menu.json` (gekoppelde mappen, of Versturen aan staat, het pad van de app) voor de DLL, die zo geen
//!   netwerk of instellingen nodig heeft;
//! - registreert het pakket voor de gebruiker (bij elke start als de versie niet klopt, en via `--menupakket
//!   registreer` vanuit de NSIS-installer), met de installatiemap als *external location*;
//! - meldt het af bij het verwijderen (`--menupakket afmelden`, vanuit de uninstaller).
//!
//! Terugval: lukt het registreren niet (Windows 10, geen geldig ondertekend pakket, een ontwikkelversie zonder
//! pakket), dan blijft het klassieke werkwoord "Versturen met OpenAEC" (`rechtermuis`) staan. Is het pakket er wel,
//! dan gaat dat werkwoord weg, zodat Versturen niet twee keer in het menu staat. Het klassieke submenu in de
//! gekoppelde mappen (`verkenner`) blijft altijd, onder "Meer opties weergeven", met alle acties.

use crate::instellingen::Instellingen;
use std::sync::atomic::{AtomicBool, Ordering};

/// Staat het pakket (en daarmee het moderne menu) er voor deze gebruiker?
static ACTIEF: AtomicBool = AtomicBool::new(false);

/// `menu.json` bijwerken (bij het starten, na elke (ont)koppeling en als Versturen aan- of uitgaat).
pub fn schrijf_menu(inst: &Instellingen) {
    #[cfg(windows)]
    {
        use openaec_shellmenu::logica::{menu_pad, Menu};
        let Some(pad) = menu_pad() else { return };
        let menu = Menu {
            exe: std::env::current_exe().ok().map(|e| e.to_string_lossy().to_string()),
            mappen: inst.koppelingen.iter().map(|k| k.lokaal.clone()).collect(),
            versturen: inst.menu_versturen_aan(),
        };
        if let Err(e) = menu.bewaar(&pad) {
            log::warn!("menu.json voor het rechtermuismenu: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = inst;
}

/// Het klassieke werkwoord "Versturen met OpenAEC" bijwerken: alleen als Versturen aan staat en het moderne menu er
/// niet is.
pub fn werk_klassiek_bij(versturen_aan: bool) -> std::io::Result<()> {
    crate::rechtermuis::werk_bij(versturen_aan && !ACTIEF.load(Ordering::SeqCst))
}

/// Bij het starten (op de achtergrond): het pakket in orde maken en daarna het klassieke werkwoord bijwerken.
pub fn bij_start(versturen_aan: bool) {
    #[cfg(windows)]
    {
        let actief = match pakket::zorg_voor_pakket() {
            Ok(a) => a,
            Err(e) => {
                log::warn!("modern rechtermuismenu niet beschikbaar, het klassieke blijft: {e}");
                false
            }
        };
        ACTIEF.store(actief, Ordering::SeqCst);
    }
    if let Err(e) = werk_klassiek_bij(versturen_aan) {
        log::warn!("Versturen met OpenAEC in het rechtermuismenu: {e}");
    }
}

/// `--menupakket registreer|afmelden` (vanuit de installer): uitvoeren en de exitcode geven; anders `None`.
pub fn opdrachtregel(argv: &[String]) -> Option<i32> {
    let i = argv.iter().position(|a| a == "--menupakket")?;
    #[cfg(windows)]
    {
        let r = match argv.get(i + 1).map(String::as_str) {
            Some("registreer") => pakket::zorg_voor_pakket().map(|a| {
                if a {
                    // Het moderne menu is er: het klassieke werkwoord niet ook nog.
                    let _ = crate::rechtermuis::werk_bij(false);
                }
            }),
            Some("afmelden") => pakket::meld_af(openaec_shellmenu::logica::PAKKETNAAM),
            _ => Err("gebruik: --menupakket registreer|afmelden".into()),
        };
        Some(match r {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        })
    }
    #[cfg(not(windows))]
    {
        let _ = i;
        Some(0)
    }
}

#[cfg(windows)]
mod pakket {
    use openaec_shellmenu::logica::{self, Architectuur, PAKKETNAAM};
    use std::path::Path;
    use windows::core::HSTRING;
    use windows::Foundation::Uri;
    use windows::Management::Deployment::{AddPackageOptions, PackageManager};

    /// Windows 11 (build 22000 of hoger): alleen daar is er een modern menu. Op Windows 10 blijft het klassieke.
    fn windows_11() -> bool {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
            .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber"))
            .ok()
            .and_then(|b| b.parse::<u32>().ok())
            .is_some_and(|b| b >= 22000)
    }

    /// De architectuur van Windows zelf (de Verkenner), niet die van de app: op een ARM-pc draait de x64-app
    /// geëmuleerd, maar laadt de Verkenner alleen een ARM64-DLL.
    pub fn architectuur() -> Architectuur {
        use windows::Win32::System::SystemInformation::{IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_ARM64};
        use windows::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
        let (mut proces, mut systeem) = (IMAGE_FILE_MACHINE::default(), IMAGE_FILE_MACHINE::default());
        let ok = unsafe { IsWow64Process2(GetCurrentProcess(), &mut proces, Some(&mut systeem)) }.is_ok();
        if ok && systeem == IMAGE_FILE_MACHINE_ARM64 {
            Architectuur::Arm64
        } else {
            Architectuur::X64
        }
    }

    fn fout(e: windows::core::Error) -> String {
        e.message().to_string()
    }

    /// Versie en volledige naam van het geregistreerde pakket `naam` voor deze gebruiker.
    pub fn geregistreerd(naam: &str) -> Result<Option<(String, String)>, String> {
        let pm = PackageManager::new().map_err(fout)?;
        for p in pm.FindPackagesByUserSecurityId(&HSTRING::new()).map_err(fout)? {
            let Ok(id) = p.Id() else { continue };
            if id.Name().is_ok_and(|n| n == naam) {
                let v = id.Version().map_err(fout)?;
                let versie = format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision);
                return Ok(Some((versie, id.FullName().map_err(fout)?.to_string())));
            }
        }
        Ok(None)
    }

    /// Een ondertekend pakket registreren met `extern` als *external location* (vervangt een andere versie).
    pub fn registreer(pakket: &Path, extern_map: &Path) -> Result<(), String> {
        let pm = PackageManager::new().map_err(fout)?;
        let opties = AddPackageOptions::new().map_err(fout)?;
        opties.SetExternalLocationUri(&Uri::CreateUri(&HSTRING::from(extern_map.as_os_str())).map_err(fout)?).map_err(fout)?;
        opties.SetForceUpdateFromAnyVersion(true).map_err(fout)?;
        let uri = Uri::CreateUri(&HSTRING::from(pakket.as_os_str())).map_err(fout)?;
        let bewerking = pm.AddPackageByUriAsync(&uri, &opties).map_err(fout)?;
        let uitkomst = bewerking.get();
        // Mislukt: de bewerking eindigt met een fout, of de uitkomst draagt een foutcode.
        let mislukt = uitkomst.as_ref().map_or(true, |r| r.ExtendedErrorCode().is_ok_and(|c| c.is_err()));
        if mislukt {
            let tekst = bewerking.GetResults().and_then(|r| r.ErrorText()).map(|t| t.to_string()).unwrap_or_default();
            let code = bewerking.ErrorCode().map(|c| format!("{:#010x}", c.0)).unwrap_or_default();
            return Err(format!("registreren van {} mislukt ({code}): {tekst}", pakket.display()));
        }
        Ok(())
    }

    /// Pakket `naam` voor deze gebruiker afmelden (bestaat het niet, dan is dat ook goed).
    pub fn meld_af(naam: &str) -> Result<(), String> {
        let Some((_, volledig)) = geregistreerd(naam)? else { return Ok(()) };
        let pm = PackageManager::new().map_err(fout)?;
        pm.RemovePackageAsync(&HSTRING::from(volledig)).map_err(fout)?.get().map_err(fout)?;
        Ok(())
    }

    /// Het pakket van deze versie geregistreerd hebben. Geeft of het moderne menu er is.
    pub fn zorg_voor_pakket() -> Result<bool, String> {
        if !windows_11() {
            return Ok(false);
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let map = exe.parent().ok_or("geen installatiemap")?;
        let arch = architectuur();
        let (pakket, dll) = (map.join(arch.pakket()), map.join(arch.dll()));
        let huidig = geregistreerd(PAKKETNAAM)?;
        if !pakket.is_file() || !dll.is_file() {
            // Een ontwikkelversie (of een installatie zonder het pakket): niets registreren.
            return Ok(huidig.is_some());
        }
        let gewenst = logica::pakketversie(env!("CARGO_PKG_VERSION"));
        if huidig.as_ref().is_some_and(|(v, _)| *v == gewenst) {
            return Ok(true);
        }
        registreer(&pakket, map)?;
        log::info!("modern rechtermuismenu geregistreerd ({gewenst}, {})", arch.naam());
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    /// De bouwstraat voegt tauri.verkenner.conf.json samen met tauri.conf.json; een lijst wordt dan vervangen, dus
    /// de resources van de gewone config moeten er ook in staan (WebView2Loader.dll).
    #[test]
    fn verkenner_config_houdt_de_gewone_resources() {
        let lees = |t: &str| serde_json::from_str::<serde_json::Value>(t).unwrap()["bundle"]["resources"].clone();
        let gewoon = lees(include_str!("../tauri.conf.json"));
        let verkenner = lees(include_str!("../tauri.verkenner.conf.json"));
        for r in gewoon.as_array().unwrap() {
            assert!(verkenner.as_array().unwrap().contains(r), "{r} ontbreekt in tauri.verkenner.conf.json");
        }
        assert!(verkenner.as_array().unwrap().iter().any(|r| r == "shellmenu/**/*"));
    }

    #[test]
    fn alleen_met_menupakket_op_de_opdrachtregel() {
        assert_eq!(opdrachtregel(&argv(&["openaec-cloud"])), None);
        assert_eq!(opdrachtregel(&argv(&["openaec-cloud", "--actie", "versturen", "a"])), None);
    }

    #[cfg(windows)]
    #[test]
    fn onbekende_menupakket_opdracht() {
        assert_eq!(opdrachtregel(&argv(&["openaec-cloud", "--menupakket", "iets"])), Some(1));
        assert_eq!(opdrachtregel(&argv(&["openaec-cloud", "--menupakket"])), Some(1));
    }

    /// Een pakket zonder geldige handtekening registreren mislukt met een foutmelding (en dan blijft het klassieke
    /// menu). Niet standaard: `OPENAEC_TEST_MSIX=<ongetekend .msix> cargo test --lib ongetekend -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn ongetekend_pakket_registreren_mislukt_netjes() {
        let msix = std::path::PathBuf::from(std::env::var("OPENAEC_TEST_MSIX").expect("OPENAEC_TEST_MSIX"));
        let fout = pakket::registreer(&msix, msix.parent().unwrap()).unwrap_err();
        println!("{fout}");
        assert!(fout.contains("mislukt"));
        assert_eq!(pakket::geregistreerd("OpenAEC.Test.Bestaatniet").unwrap(), None);
    }

    #[cfg(windows)]
    #[test]
    fn architectuur_van_windows() {
        // Op een x64- of ARM64-pc altijd een van beide; de DLL staat in shellmenu\<arch>\.
        let a = pakket::architectuur();
        assert!(a.dll().starts_with("shellmenu\\") && a.dll().ends_with("\\openaec_shellmenu.dll"));
    }
}
