//! Een vast item "OpenAEC" in het navigatiepaneel van de Verkenner (Windows), met het app-icoon, boven "Deze pc";
//! daaronder bladert de gebruiker door de mappen. Het item staat in HKCU (geen beheerdersrechten nodig) als
//! shell-namespace-item: een eigen CLSID die een gewone map toont (`TargetFolderPath`), vastgezet in de boom
//! (`System.IsPinnedToNameSpaceTree`), en verborgen op het bureaublad (`HideDesktopIcons\NewStartPanel`).
//!
//! Een map die bij Windows als eigen sync-root geregistreerd is, krijgt van Windows zelf al een item in het
//! navigatiepaneel (`StorageProviderSyncRootManager`, zie `cloudfilter`). Dit eigen item vult alleen aan wat zo'n
//! item mist, zodat er nooit twee voor dezelfde map staan. Welke map het toont ([`doel`]):
//! - **Met een hoofdmap**: is die als sync-root geregistreerd, dan geen eigen item (Windows toont "OpenAEC" al, zie
//!   `hoofdmap`); lukte registreren niet, dan toont het eigen item de hoofdmap.
//! - **Zonder hoofdmap** telt elke losse koppeling die geen eigen sync-root is. Dat komt veel voor: een map binnen
//!   een map die al door een andere sync-app beheerd wordt, kan geen tweede sync-root worden.
//!   - **Eén** zo'n koppeling: het item toont die map zelf.
//!   - **Meer**: het item toont een eigen map met per koppeling een mapsnelkoppeling (een map met `desktop.ini` en
//!     `target.lnk`, zoals "Netwerklocaties" die maakt). In de boom klapt elke snelkoppeling uit als de echte map; de
//!     bestanden blijven waar ze staan.
//!   - **Geen**: geen item.
//!
//! Er wordt niets verplaatst: de overstap naar de hoofdmap ("Naar de hoofdmap…" in het Overzicht, na een voorstel
//! bij de eerste start) blijft een keuze van de gebruiker.
//!
//! De app werkt het item bij bij het starten en na elke (ont)koppeling; de NSIS-uninstaller ruimt het op
//! (`nsis/hooks.nsh`, met dezelfde [`CLSID`]).
//!
//! Dit bestand hangt bewust niet van de rest van de app af: de rooktest (`examples/navigatie_smoke.rs`) gebruikt het
//! met een tijdelijke CLSID en testnaam, zonder de echte registratie aan te raken.

use std::path::{Path, PathBuf};

/// Vaste, eigen CLSID van het item. Niet veranderen: de uninstaller (`nsis/hooks.nsh`) en oudere versies gebruiken
/// dezelfde, zodat een update het item vervangt in plaats van er een tweede naast te zetten.
pub const CLSID: &str = "{A1046B79-4B13-4993-B03B-4D94B67DDB65}";

/// Plek in het navigatiepaneel: dezelfde waarde als die van de andere sync-apps (boven "Deze pc").
pub const SORTERING: u32 = 0x42;

/// Wat het item moet tonen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Doel {
    /// Geen eigen item (geen koppelingen, of de hoofdmap heeft het item van zijn sync-root).
    Geen,
    /// Deze map.
    Map(PathBuf),
    /// Een map met mapsnelkoppelingen: (naam in het navigatiepaneel, echte map).
    Snelkoppelingen(Vec<(String, PathBuf)>),
}

/// Een map voor [`doel`]: (naam, lokale map, heeft al een eigen item als sync-root bij Windows).
pub type Koppeling = (String, String, bool);

/// Wat het item toont, bij deze hoofdmap (pad, als sync-root geregistreerd) en deze koppelingen. Zie de uitleg
/// bovenaan.
pub fn doel(hoofdmap: Option<(&str, bool)>, koppelingen: &[Koppeling]) -> Doel {
    if let Some((pad, geregistreerd)) = hoofdmap {
        return if geregistreerd { Doel::Geen } else { Doel::Map(PathBuf::from(pad)) };
    }
    let koppelingen: Vec<(&String, &String)> = koppelingen.iter().filter(|(_, _, eigen)| !eigen).map(|(n, l, _)| (n, l)).collect();
    match koppelingen[..] {
        [] => Doel::Geen,
        [(_, lokaal)] => Doel::Map(PathBuf::from(lokaal)),
        _ => {
            let mut namen: Vec<String> = Vec::new();
            let lijst = koppelingen
                .iter()
                .map(|(naam, lokaal)| {
                    let naam = unieke_naam(naam, lokaal, &namen);
                    namen.push(naam.clone());
                    (naam, PathBuf::from(lokaal))
                })
                .collect();
            Doel::Snelkoppelingen(lijst)
        }
    }
}

/// Naam voor een mapsnelkoppeling: de naam van de koppeling (of anders de mapnaam), zonder tekens die in een mapnaam
/// niet kunnen, met " (2)", " (3)" … als die naam al bestaat.
fn unieke_naam(naam: &str, lokaal: &str, bezet: &[String]) -> String {
    let basis = if naam.trim().is_empty() {
        Path::new(lokaal).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Map".into())
    } else {
        naam.to_string()
    };
    let schoon: String = basis.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    let schoon = schoon.trim().trim_end_matches(['.', ' ']).to_string();
    let schoon = if schoon.is_empty() { "Map".to_string() } else { schoon };
    let vrij = |n: &str| !bezet.iter().any(|b| b.eq_ignore_ascii_case(n));
    if vrij(&schoon) {
        return schoon;
    }
    (2..).map(|i| format!("{schoon} ({i})")).find(|n| vrij(n)).unwrap()
}

/// Inhoud van de `desktop.ini` die van een gewone map een mapsnelkoppeling maakt (de map volgt dan `target.lnk`).
pub const SNELKOPPELING_INI: &str = "[.ShellClassInfo]\r\nCLSID2={0AFACED1-E828-11D1-9187-B532F1E9575D}\r\nFlags=2\r\n";

#[cfg(windows)]
pub use echt::*;

#[cfg(windows)]
mod echt {
    use std::path::Path;

    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};
    use winreg::{RegKey, RegValue};

    use super::{Doel, SNELKOPPELING_INI, SORTERING};

    const KLASSEN: &str = "Software\\Classes\\CLSID";
    const NAAMRUIMTE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Desktop\\NameSpace";
    const BUREAUBLAD: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\HideDesktopIcons\\NewStartPanel";
    /// De standaard shell-map die een gewone map toont (de "delegate folder" voor `TargetFolderPath`).
    const DOELMAP_CLSID: &str = "{0E5AAE11-A475-4c5b-AB00-C66DE400274E}";

    fn uitbreidbaar(tekst: &str) -> RegValue {
        let mut bytes: Vec<u8> = tekst.encode_utf16().chain(std::iter::once(0)).flat_map(|c| c.to_le_bytes()).collect();
        bytes.shrink_to_fit();
        RegValue { bytes, vtype: winreg::enums::RegType::REG_EXPAND_SZ }
    }

    /// Zet (of vervang) het item `clsid` met deze naam en dit icoon (`"pad\\naar.exe,0"`) op map `map`.
    pub fn registreer(clsid: &str, naam: &str, icoon: &str, map: &Path) -> std::io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (k, _) = hkcu.create_subkey(format!("{KLASSEN}\\{clsid}"))?;
        k.set_value("", &naam)?;
        k.set_value("System.IsPinnedToNameSpaceTree", &1u32)?;
        k.set_value("SortOrderIndex", &SORTERING)?;
        k.create_subkey("DefaultIcon")?.0.set_value("", &icoon)?;
        k.create_subkey("InProcServer32")?.0.set_raw_value("", &uitbreidbaar("%SystemRoot%\\system32\\shell32.dll"))?;
        let (instantie, _) = k.create_subkey("Instance")?;
        instantie.set_value("CLSID", &DOELMAP_CLSID)?;
        let (zak, _) = instantie.create_subkey("InitPropertyBag")?;
        // 0x11: FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_READONLY, zoals de andere items.
        zak.set_value("Attributes", &0x11u32)?;
        zak.set_value("TargetFolderPath", &map.to_string_lossy().to_string())?;
        let (shell, _) = k.create_subkey("ShellFolder")?;
        // Bladerbaar, met de gewone mapopdrachten (dezelfde waarden als de andere sync-apps gebruiken).
        shell.set_value("FolderValueFlags", &0x28u32)?;
        shell.set_value("Attributes", &0xF080_004Du32)?;
        hkcu.create_subkey(format!("{NAAMRUIMTE}\\{clsid}"))?.0.set_value("", &naam)?;
        hkcu.create_subkey(BUREAUBLAD)?.0.set_value(clsid, &1u32)?;
        ververs();
        Ok(())
    }

    /// Haal het item `clsid` weg (alle drie de plekken); niets aan de hand als het er niet was.
    pub fn verwijder(clsid: &str) -> std::io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let weg = |r: std::io::Result<()>| match r {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
        let mut was_er = false;
        if let Ok(k) = hkcu.open_subkey_with_flags(NAAMRUIMTE, KEY_ALL_ACCESS) {
            was_er |= k.open_subkey(clsid).is_ok();
            weg(k.delete_subkey_all(clsid))?;
        }
        if let Ok(k) = hkcu.open_subkey_with_flags(KLASSEN, KEY_ALL_ACCESS) {
            was_er |= k.open_subkey(clsid).is_ok();
            weg(k.delete_subkey_all(clsid))?;
        }
        if let Ok(k) = hkcu.open_subkey_with_flags(BUREAUBLAD, KEY_ALL_ACCESS) {
            weg(k.delete_value(clsid))?;
        }
        if was_er {
            ververs();
        }
        Ok(())
    }

    /// Wat er nu geregistreerd is voor `clsid`: (naam, doelmap, vastgezet, icoon, op het bureaublad verborgen).
    pub fn lees(clsid: &str) -> Option<(String, String, bool, String, bool)> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let k = hkcu.open_subkey(format!("{KLASSEN}\\{clsid}")).ok()?;
        hkcu.open_subkey(format!("{NAAMRUIMTE}\\{clsid}")).ok()?;
        let naam: String = k.get_value("").ok()?;
        let vast = k.get_value::<u32, _>("System.IsPinnedToNameSpaceTree").ok() == Some(1);
        let icoon: String = k.open_subkey("DefaultIcon").and_then(|d| d.get_value("")).unwrap_or_default();
        let map: String = k.open_subkey("Instance\\InitPropertyBag").and_then(|z| z.get_value("TargetFolderPath")).ok()?;
        let verborgen = hkcu.open_subkey(BUREAUBLAD).and_then(|b| b.get_value::<u32, _>(clsid)).ok() == Some(1);
        Some((naam, map, vast, icoon, verborgen))
    }

    /// Laat de Verkenner het navigatiepaneel opnieuw opbouwen.
    fn ververs() {
        use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    }

    /// Zet de map `map` vol met mapsnelkoppelingen (en niets anders): wat er niet (meer) bij hoort, gaat weg.
    /// Alleen mappen die zelf een mapsnelkoppeling zijn worden verwijderd; zo raakt dit nooit echte bestanden.
    pub fn vul_snelkoppelingen(map: &Path, lijst: &[(String, std::path::PathBuf)]) -> std::io::Result<()> {
        std::fs::create_dir_all(map)?;
        for item in std::fs::read_dir(map)?.flatten() {
            let pad = item.path();
            let naam = item.file_name().to_string_lossy().to_string();
            if pad.is_dir() && is_snelkoppeling(&pad) && !lijst.iter().any(|(n, _)| n.eq_ignore_ascii_case(&naam)) {
                verwijder_snelkoppeling(&pad)?;
            }
        }
        for (naam, doel) in lijst {
            maak_snelkoppeling(&map.join(naam), doel)?;
        }
        Ok(())
    }

    /// Is `pad` een mapsnelkoppeling (een map met precies `desktop.ini` en `target.lnk`)?
    pub fn is_snelkoppeling(pad: &Path) -> bool {
        let Ok(items) = std::fs::read_dir(pad) else { return false };
        let mut namen: Vec<String> = items.flatten().map(|i| i.file_name().to_string_lossy().to_lowercase()).collect();
        namen.sort();
        namen == ["desktop.ini", "target.lnk"]
    }

    fn verwijder_snelkoppeling(pad: &Path) -> std::io::Result<()> {
        for bestand in ["desktop.ini", "target.lnk"] {
            let p = pad.join(bestand);
            zet_attributen(&p, 0x80); // FILE_ATTRIBUTE_NORMAL, anders weigert verwijderen
            std::fs::remove_file(&p)?;
        }
        zet_attributen(pad, 0x80);
        std::fs::remove_dir(pad)
    }

    /// Een mapsnelkoppeling `pad` naar `doel`: de map met een `target.lnk` en een `desktop.ini` die hem laat volgen.
    pub fn maak_snelkoppeling(pad: &Path, doel: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(pad)?;
        let ini = pad.join("desktop.ini");
        zet_attributen(&ini, 0x80);
        std::fs::write(&ini, SNELKOPPELING_INI)?;
        // desktop.ini verborgen en systeem; de map alleen-lezen, anders leest de Verkenner de desktop.ini niet.
        zet_attributen(&ini, 0x2 | 0x4);
        let lnk = pad.join("target.lnk");
        zet_attributen(&lnk, 0x80);
        schrijf_lnk(&lnk, doel)?;
        zet_attributen(pad, 0x10 | 0x1);
        Ok(())
    }

    fn zet_attributen(pad: &Path, attributen: u32) {
        use windows::core::HSTRING;
        use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_FLAGS_AND_ATTRIBUTES};
        if pad.exists() {
            let _ = unsafe { SetFileAttributesW(&HSTRING::from(pad.as_os_str()), FILE_FLAGS_AND_ATTRIBUTES(attributen)) };
        }
    }

    /// Een .lnk-bestand `lnk` naar `doel` (via de shell, op een eigen COM-thread).
    fn schrijf_lnk(lnk: &Path, doel: &Path) -> std::io::Result<()> {
        let (lnk, doel) = (lnk.to_path_buf(), doel.to_path_buf());
        std::thread::spawn(move || -> windows::core::Result<()> {
            use windows::core::{Interface, HSTRING};
            use windows::Win32::System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
            };
            use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
            unsafe {
                let init = CoInitializeEx(None, COINIT_MULTITHREADED);
                let uitkomst = (|| {
                    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                    link.SetPath(&HSTRING::from(doel.as_os_str()))?;
                    link.cast::<IPersistFile>()?.Save(&HSTRING::from(lnk.as_os_str()), true)
                })();
                if init.is_ok() {
                    CoUninitialize();
                }
                uitkomst
            }
        })
        .join()
        .map_err(|_| std::io::Error::other("snelkoppeling maken: de thread viel om"))?
        .map_err(|e| std::io::Error::other(format!("snelkoppeling maken: {e}")))
    }

    /// Zet het item `clsid` zoals [`Doel`] zegt; `snelkoppelingen` is de map voor [`Doel::Snelkoppelingen`].
    pub fn pas_toe(clsid: &str, naam: &str, icoon: &str, doel: &Doel, snelkoppelingen: &Path) -> std::io::Result<()> {
        match doel {
            Doel::Geen => {
                verwijder(clsid)?;
                if snelkoppelingen.is_dir() {
                    vul_snelkoppelingen(snelkoppelingen, &[])?;
                    let _ = std::fs::remove_dir(snelkoppelingen);
                }
                Ok(())
            }
            Doel::Map(map) => {
                if snelkoppelingen.is_dir() {
                    vul_snelkoppelingen(snelkoppelingen, &[])?;
                    let _ = std::fs::remove_dir(snelkoppelingen);
                }
                if lees(clsid).is_some_and(|(n, m, vast, i, _)| n == naam && Path::new(&m) == map && vast && i == icoon) {
                    return Ok(());
                }
                registreer(clsid, naam, icoon, map)
            }
            Doel::Snelkoppelingen(lijst) => {
                vul_snelkoppelingen(snelkoppelingen, lijst)?;
                if lees(clsid).is_some_and(|(n, m, vast, i, _)| n == naam && Path::new(&m) == snelkoppelingen && vast && i == icoon) {
                    return Ok(());
                }
                registreer(clsid, naam, icoon, snelkoppelingen)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(naam: &str, lokaal: &str) -> Koppeling {
        (naam.to_string(), lokaal.to_string(), false)
    }

    const H: &str = "C:\\Users\\a\\OpenAEC";

    #[test]
    fn met_hoofdmap_geen_eigen_item() {
        // De sync-root van de hoofdmap heeft zijn eigen item; een tweede zou dubbel staan.
        assert_eq!(doel(Some((H, true)), &[k("A", "C:\\Users\\a\\OpenAEC\\A")]), Doel::Geen);
        assert_eq!(doel(Some((H, true)), &[k("Los", "D:\\Werk\\Los")]), Doel::Geen);
        assert_eq!(doel(Some((H, true)), &[]), Doel::Geen);
    }

    #[test]
    fn hoofdmap_zonder_registratie_krijgt_het_eigen_item() {
        assert_eq!(doel(Some((H, false)), &[k("Los", "D:\\Werk\\Los")]), Doel::Map(PathBuf::from(H)));
    }

    #[test]
    fn koppelingen_met_een_eigen_sync_root_tellen_niet_mee() {
        let eigen = ("Eigen".to_string(), "D:\\Eigen".to_string(), true);
        assert_eq!(doel(None, std::slice::from_ref(&eigen)), Doel::Geen);
        assert_eq!(doel(None, &[eigen.clone(), k("Los", "E:\\Los")]), Doel::Map(PathBuf::from("E:\\Los")));
    }

    #[test]
    fn zonder_koppelingen_geen_item() {
        assert_eq!(doel(None, &[]), Doel::Geen);
    }

    #[test]
    fn een_losse_koppeling_toont_de_map_zelf() {
        assert_eq!(doel(None, &[k("OpenAEC Studio", "D:\\Werk\\Studio")]), Doel::Map(PathBuf::from("D:\\Werk\\Studio")));
    }

    #[test]
    fn meer_losse_koppelingen_met_unieke_namen() {
        let d = doel(None, &[k("Project A", "D:\\A"), k("project a", "E:\\A"), k("", "F:\\Archief"), k("a/b:c", "G:\\X")]);
        assert_eq!(
            d,
            Doel::Snelkoppelingen(vec![
                ("Project A".into(), PathBuf::from("D:\\A")),
                ("project a (2)".into(), PathBuf::from("E:\\A")),
                ("Archief".into(), PathBuf::from("F:\\Archief")),
                ("a_b_c".into(), PathBuf::from("G:\\X")),
            ])
        );
    }

    #[test]
    fn vaste_clsid_staat_ook_in_de_uninstaller() {
        let nsh = include_str!("../nsis/hooks.nsh");
        assert!(nsh.contains(CLSID), "nsis/hooks.nsh moet het item met {CLSID} opruimen");
    }
}
