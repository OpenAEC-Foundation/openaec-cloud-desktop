//! Installer van een tool downloaden (met voortgang), controleren en starten; geïnstalleerde tools starten en
//! verwijderen.
//!
//! Overgenomen uit de OpenAEC Installer (`src-tauri/src/installer.rs`, zie NOTICE) en aangepast:
//! - alleen release-downloads van de organisatie [`ORGANISATIE`](super::github::ORGANISATIE);
//! - de download moet overeenkomen met de SHA-256 die GitHub publiceert;
//! - op Windows wordt een installer alleen gestart als hij een geldige Authenticode-handtekening heeft;
//! - verwijderen (de uninstaller van de tool zelf, of op Linux de beheerde AppImage weghalen).
//!
//! Windows: de gedownloade setup wordt gestart en de gebruiker doorloopt de setup zelf; de setup schrijft zijn eigen
//! registersleutels, net als bij installeren met de losse OpenAEC Installer. Linux: de AppImage wordt direct in de
//! beheerde map gezet (zie `linux.rs`). Het verschil gaat via `InstallOutcome.mode` naar de frontend.

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::Emitter;

use super::github::ORGANISATIE;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    pub path: String,
    /// "ranInstaller" — setup gestart (Windows); "installed" — direct geïnstalleerd (Linux).
    pub mode: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    id: String,
    downloaded: u64,
    total: Option<u64>,
    done: bool,
}

/// Naam van het voortgangsbericht naar de frontend.
pub const VOORTGANG: &str = "tool-download";

/// Alleen `https://github.com/<ORGANISATIE>/<repo>/releases/download/<tag>/<bestandsnaam>`, waarbij de bestandsnaam
/// overeenkomt met het laatste deel van de URL en geen padscheiders bevat.
pub(crate) fn controleer_download(url: &str, bestandsnaam: &str) -> Result<(), String> {
    let fout = || Err("Alleen downloads uit de releases van OpenAEC-Foundation op GitHub zijn toegestaan.".to_string());
    let Some(rest) = url.strip_prefix("https://github.com/") else { return fout() };
    let delen: Vec<&str> = rest.split('/').collect();
    if delen.len() != 6 || !delen[0].eq_ignore_ascii_case(ORGANISATIE) || delen[2] != "releases" || delen[3] != "download" {
        return fout();
    }
    if delen.iter().any(|d| d.is_empty() || *d == "." || *d == ".." || d.contains(['?', '#', '\\'])) {
        return fout();
    }
    if bestandsnaam.is_empty()
        || bestandsnaam.contains(['/', '\\'])
        || bestandsnaam.contains("..")
        || delen[5].replace("%20", " ") != bestandsnaam.replace("%20", " ")
    {
        return Err("Ongeldige bestandsnaam.".into());
    }
    Ok(())
}

/// Vergelijk de berekende SHA-256 met de waarde van GitHub ("sha256:<hex>").
pub(crate) fn controleer_digest(verwacht: Option<&str>, berekend: &[u8]) -> Result<(), String> {
    let Some(hex_verwacht) = verwacht.and_then(|d| d.strip_prefix("sha256:")) else {
        return Err("GitHub geeft voor dit bestand geen controlegetal (SHA-256); de download wordt niet gebruikt.".into());
    };
    if hex::encode(berekend).eq_ignore_ascii_case(hex_verwacht.trim()) {
        Ok(())
    } else {
        Err("De download komt niet overeen met het controlegetal van GitHub en is verwijderd.".into())
    }
}

/// Download het asset naar een eigen tijdelijke map, controleer het en installeer het zoals bij het platform past.
pub async fn download_and_run(
    app: tauri::AppHandle,
    id: String,
    url: String,
    file_name: String,
    digest: Option<String>,
    display_name: Option<String>,
) -> Result<InstallOutcome, String> {
    controleer_download(&url, &file_name)?;
    // Het id komt uit de catalogus; op Linux wordt het een mapnaam.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Ongeldig tool-id.".into());
    }

    // Eigen map (niet die van de losse installer), zodat twee downloads van dezelfde tool elkaar niet raken.
    let dir = std::env::temp_dir().join("openaec-supercloud-tools");
    std::fs::create_dir_all(&dir).map_err(|e| format!("tijdelijke map: {e}"))?;
    let target = dir.join(&file_name);

    let client = reqwest::Client::builder()
        .user_agent("OpenAEC-SuperCloud-Desktop")
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("download: {e}"))?;

    let total = resp.content_length();
    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::create(&target).map_err(|e| format!("bestand: {e}"))?;

    use futures_util::StreamExt;
    use std::io::Write;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                drop(file);
                let _ = std::fs::remove_file(&target);
                return Err(format!("download: {e}"));
            }
        };
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(|e| format!("bestand: {e}"))?;
        downloaded += chunk.len() as u64;
        // Niet elk stukje een bericht: hooguit eens per 512 kB.
        if downloaded - last_emit >= 512 * 1024 {
            last_emit = downloaded;
            let _ = app.emit(VOORTGANG, DownloadProgress { id: id.clone(), downloaded, total, done: false });
        }
    }
    drop(file);
    let _ = app.emit(VOORTGANG, DownloadProgress { id: id.clone(), downloaded, total, done: true });

    let gecontroleerd = controleer_digest(digest.as_deref(), &hasher.finalize()).and_then(|()| controleer_handtekening(&target));
    if let Err(e) = gecontroleerd {
        let _ = std::fs::remove_file(&target);
        return Err(e);
    }

    let mode = run_installer(&id, display_name.as_deref(), &target)?;
    Ok(InstallOutcome { path: target.to_string_lossy().into_owned(), mode })
}

/// Windows: alleen installers met een geldige Authenticode-handtekening starten.
#[cfg(windows)]
fn controleer_handtekening(path: &std::path::Path) -> Result<(), String> {
    super::ondertekening::controleer(path)
}

/// Linux: een AppImage heeft geen handtekening die hier te controleren is; de SHA-256 van GitHub is de controle.
#[cfg(not(windows))]
fn controleer_handtekening(_path: &std::path::Path) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn run_installer(_id: &str, _display_name: Option<&str>, path: &std::path::Path) -> Result<&'static str, String> {
    let is_msi = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("msi"));
    if is_msi {
        // msiexec vraagt zelf om verhoogde rechten voor een installatie voor alle gebruikers.
        shell_execute("msiexec.exe", Some(&format!("/i \"{}\"", path.display())))?;
    } else {
        // Via de shell starten in plaats van CreateProcess: dan volgt Windows het manifest van de setup en toont het zo
        // nodig de UAC-vraag (CreateProcess faalt dan met os error 740).
        shell_execute(&path.to_string_lossy(), None)?;
    }
    Ok("ranInstaller")
}

/// Start een bestand via de Windows-shell (ShellExecuteW) met de standaardactie; het programma krijgt zo nodig
/// verhoogde rechten volgens zijn manifest.
#[cfg(windows)]
fn shell_execute(file: &str, params: Option<&str>) -> Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let file_w = HSTRING::from(file);
    let params_w = params.map(HSTRING::from);
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR::null(),
            &file_w,
            params_w.as_ref().map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW geeft bij succes een waarde groter dan 32.
    let code = result.0 as isize;
    if code > 32 {
        Ok(())
    } else {
        // 1223 = ERROR_CANCELLED (UAC geweigerd), 5 = SE_ERR_ACCESSDENIED.
        Err(match code {
            5 | 1223 => "geannuleerd (geen beheerdersrechten gegeven)".to_string(),
            other => format!("starten mislukt (code {other})"),
        })
    }
}

#[cfg(target_os = "linux")]
fn run_installer(id: &str, display_name: Option<&str>, path: &std::path::Path) -> Result<&'static str, String> {
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("appimage")) {
        return Err("Geen ondersteund installatiebestand voor Linux (AppImage verwacht).".into());
    }
    super::linux::install_appimage(id, display_name.unwrap_or(id), path)?;
    Ok("installed")
}

#[cfg(not(any(windows, target_os = "linux")))]
fn run_installer(_id: &str, _display_name: Option<&str>, _path: &std::path::Path) -> Result<&'static str, String> {
    Err("Installeren wordt op dit platform nog niet ondersteund.".into())
}

/// Start een geïnstalleerde tool.
#[cfg(windows)]
pub fn launch(exe_path: &str) -> Result<(), String> {
    let path = std::path::Path::new(exe_path);
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) || !path.exists() {
        return Err("Programma niet gevonden.".into());
    }
    let mut cmd = std::process::Command::new(path);
    if let Some(parent) = path.parent() {
        cmd.current_dir(parent);
    }
    cmd.spawn().map_err(|e| format!("starten: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn launch(exe_path: &str) -> Result<(), String> {
    super::linux::launch(exe_path)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn launch(_exe_path: &str) -> Result<(), String> {
    Err("Starten wordt op dit platform nog niet ondersteund.".into())
}

#[cfg_attr(not(windows), allow(dead_code))]
/// Splits een UninstallString uit het register in programma en parameters: `"C:\pad\uninstall.exe" /x` of
/// `MsiExec.exe /X{GUID}`.
pub(crate) fn splits_opdracht(opdracht: &str) -> Option<(String, Option<String>)> {
    let o = opdracht.trim();
    let (programma, rest) = if let Some(na) = o.strip_prefix('"') {
        let eind = na.find('"')?;
        (na[..eind].to_string(), na[eind + 1..].trim())
    } else if let Some(i) = o.to_ascii_lowercase().find(".exe") {
        (o[..i + 4].to_string(), o[i + 4..].trim())
    } else {
        return None;
    };
    if programma.is_empty() || !programma.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    Some((programma, (!rest.is_empty()).then(|| rest.to_string())))
}

/// Verwijder een tool: Windows start de uninstaller van de tool zelf (de gebruiker doorloopt die); Linux haalt de
/// AppImage en de menu-snelkoppeling weg. Geeft "ranUninstaller" of "removed".
#[cfg(windows)]
pub fn uninstall(query: &super::registry::ToolQuery) -> Result<&'static str, String> {
    let opdracht = super::registry::scan(std::slice::from_ref(query))
        .into_iter()
        .next()
        .and_then(|t| t.uninstall)
        .ok_or("Geen uninstaller gevonden voor deze tool.")?;
    let (programma, parameters) = splits_opdracht(&opdracht).ok_or("Onbekende uninstall-opdracht in het register.")?;
    shell_execute(&programma, parameters.as_deref())?;
    Ok("ranUninstaller")
}

#[cfg(target_os = "linux")]
pub fn uninstall(query: &super::registry::ToolQuery) -> Result<&'static str, String> {
    super::linux::uninstall(&query.id)?;
    Ok("removed")
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn uninstall(_query: &super::registry::ToolQuery) -> Result<&'static str, String> {
    Err("Verwijderen wordt op dit platform nog niet ondersteund.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alleen_releases_van_de_organisatie() {
        let goed = "https://github.com/OpenAEC-Foundation/open-pdf-studio/releases/download/v2026.43.4/Open.PDF.Studio_2026.43.4_x64-setup.exe";
        assert!(controleer_download(goed, "Open.PDF.Studio_2026.43.4_x64-setup.exe").is_ok());
        // Andere organisatie, andere host, geen release-download.
        assert!(controleer_download(&goed.replace("OpenAEC-Foundation", "iemand-anders"), "Open.PDF.Studio_2026.43.4_x64-setup.exe").is_err());
        assert!(controleer_download(&goed.replace("https://github.com/", "https://example.com/"), "Open.PDF.Studio_2026.43.4_x64-setup.exe").is_err());
        assert!(controleer_download("https://github.com/OpenAEC-Foundation/open-pdf-studio/archive/main.zip", "main.zip").is_err());
        // Bestandsnaam moet bij de URL horen en mag geen pad zijn.
        assert!(controleer_download(goed, "ander.exe").is_err());
        assert!(controleer_download(goed, "..\\x.exe").is_err());
        assert!(controleer_download(
            "https://github.com/OpenAEC-Foundation/x/releases/download/v1/../../../evil/x.exe",
            "x.exe"
        )
        .is_err());
        // Spaties in de naam (AppImages naar een productnaam met spaties).
        assert!(controleer_download(
            "https://github.com/OpenAEC-Foundation/open-3d-studio/releases/download/v0.8.0/Open%203D%20Studio_0.8.0_amd64.AppImage",
            "Open 3D Studio_0.8.0_amd64.AppImage"
        )
        .is_ok());
    }

    #[test]
    fn digest_wordt_gecontroleerd() {
        let hash = Sha256::digest(b"inhoud");
        let hex = hex::encode(hash);
        assert!(controleer_digest(Some(&format!("sha256:{hex}")), &hash).is_ok());
        assert!(controleer_digest(Some(&format!("sha256:{}", hex.to_uppercase())), &hash).is_ok());
        assert!(controleer_digest(Some("sha256:00"), &hash).is_err());
        assert!(controleer_digest(None, &hash).is_err());
        assert!(controleer_digest(Some(&format!("md5:{hex}")), &hash).is_err());
    }

    #[test]
    fn uninstall_opdracht_splitsen() {
        assert_eq!(
            splits_opdracht("\"C:\\Users\\a\\AppData\\Local\\Open PDF Studio\\uninstall.exe\""),
            Some(("C:\\Users\\a\\AppData\\Local\\Open PDF Studio\\uninstall.exe".into(), None))
        );
        assert_eq!(
            splits_opdracht("MsiExec.exe /X{12345678-1234-1234-1234-123456789012}"),
            Some(("MsiExec.exe".into(), Some("/X{12345678-1234-1234-1234-123456789012}".into())))
        );
        assert_eq!(
            splits_opdracht("\"C:\\Program Files\\Tool\\unins000.exe\" /SILENT"),
            Some(("C:\\Program Files\\Tool\\unins000.exe".into(), Some("/SILENT".into())))
        );
        assert_eq!(splits_opdracht("rundll32 iets"), None);
        assert_eq!(splits_opdracht("\"onafgesloten"), None);
    }
}
