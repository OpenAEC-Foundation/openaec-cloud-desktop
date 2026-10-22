//! Authenticode-controle (Windows) van een gedownloade installer, met WinVerifyTrust: de handtekening moet geldig zijn
//! en het certificaat moet herleidbaar zijn tot een vertrouwde basis. Zonder handtekening, of met een ongeldige, wordt
//! de installer niet gestart.

use std::path::Path;

use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Security::WinTrust::{
    WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO, WTD_CHOICE_FILE,
    WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
};

/// TRUST_E_NOSIGNATURE, TRUST_E_SUBJECT_FORM_UNKNOWN en TRUST_E_PROVIDER_UNKNOWN: er is geen handtekening.
const GEEN_HANDTEKENING: [u32; 3] = [0x800B_0100, 0x800B_0003, 0x800B_0001];

pub fn controleer(pad: &Path) -> Result<(), String> {
    let pad_w = HSTRING::from(pad.as_os_str());
    let mut bestand = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(pad_w.as_ptr()),
        hFile: HANDLE::default(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut bestand },
        dwStateAction: WTD_STATEACTION_VERIFY,
        ..Default::default()
    };
    let mut actie: GUID = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let uitkomst = unsafe { WinVerifyTrust(HWND::default(), &mut actie, &mut data as *mut _ as *mut _) };
    // De toestand die WinVerifyTrust heeft opgebouwd altijd weer vrijgeven.
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe { WinVerifyTrust(HWND::default(), &mut actie, &mut data as *mut _ as *mut _) };

    match uitkomst as u32 {
        0 => Ok(()),
        code if GEEN_HANDTEKENING.contains(&code) => Err(
            "Deze installer is niet digitaal ondertekend en wordt daarom niet gestart. Installeer hem zo nodig via de GitHub-pagina van de tool."
                .into(),
        ),
        code => Err(format!("De digitale handtekening van deze installer is niet geldig (0x{code:08X}); hij wordt niet gestart.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ongetekend_bestand_wordt_geweigerd() {
        let pad = std::env::temp_dir().join(format!("openaec-ondertekening-{}.exe", std::process::id()));
        std::fs::write(&pad, b"MZ geen echte installer").unwrap();
        let r = controleer(&pad);
        let _ = std::fs::remove_file(&pad);
        assert!(r.is_err());
    }

    #[test]
    fn ondertekend_bestand_wordt_geaccepteerd() {
        // De meegeleverde WebView2Loader.dll draagt een handtekening van Microsoft.
        let pad = Path::new(env!("CARGO_MANIFEST_DIR")).join("WebView2Loader.dll");
        assert_eq!(controleer(&pad), Ok(()));
    }
}
