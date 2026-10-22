//! Detectie van geïnstalleerde tools.
//!
//! Overgenomen uit de OpenAEC Installer (`src-tauri/src/registry.rs`, zie NOTICE), met de UninstallString erbij
//! (voor verwijderen). Alleen lezen: de registersleutels zijn van de setups van de tools zelf, dus de losse installer
//! en deze app zien dezelfde installaties en zitten elkaar niet in de weg.
//!
//! Windows: via de Uninstall-registersleutels. NSIS-installaties (Tauri
//! `x64-setup.exe`) registreren onder
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{ProductName}` met
//! DisplayIcon = pad naar de exe. MSI-installaties registreren onder HKLM,
//! vaak zonder InstallLocation — daarvoor zoeken we op bekende paden.
//!
//! Linux: via de beheerde AppImage-map (zie `linux.rs`), dezelfde map als die van de losse installer.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolQuery {
    pub id: String,
    /// DisplayName zoals die in het register staat (bijv. "Open Frame Studio").
    pub display_name: String,
    /// Verwachte exe-naam, als fallback wanneer het register geen pad geeft.
    pub exe_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledTool {
    pub id: String,
    pub display_name: String,
    pub version: Option<String>,
    pub exe_path: Option<String>,
    /// UninstallString uit het register (Windows), of None.
    pub uninstall: Option<String>,
}

#[cfg(windows)]
pub fn scan(queries: &[ToolQuery]) -> Vec<InstalledTool> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let hives: [(winreg::HKEY, &str); 3] = [
        (
            HKEY_CURRENT_USER,
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ];

    let mut found: Vec<InstalledTool> = Vec::new();

    for (hive, path) in hives {
        let root = RegKey::predef(hive);
        let Ok(uninstall) = root.open_subkey(path) else {
            continue;
        };
        for key_name in uninstall.enum_keys().flatten() {
            let Ok(sub) = uninstall.open_subkey(&key_name) else {
                continue;
            };
            let Ok(display_name) = sub.get_value::<String, _>("DisplayName") else {
                continue;
            };
            let display_trim = display_name.trim();
            let Some(query) = queries
                .iter()
                .find(|q| q.display_name.eq_ignore_ascii_case(display_trim))
            else {
                continue;
            };
            // Eerste treffer per tool wint (HKCU vóór HKLM).
            if found.iter().any(|f| f.id == query.id) {
                continue;
            }

            let version = sub.get_value::<String, _>("DisplayVersion").ok();
            let display_icon = sub.get_value::<String, _>("DisplayIcon").ok();
            let install_location = sub.get_value::<String, _>("InstallLocation").ok();
            let exe_path = resolve_exe(query, display_icon, install_location);
            let uninstall = sub.get_value::<String, _>("UninstallString").ok().filter(|u| !u.trim().is_empty());

            found.push(InstalledTool {
                id: query.id.clone(),
                display_name: display_trim.to_string(),
                version,
                exe_path,
                uninstall,
            });
        }
    }

    found
}

/// Bepaal het exe-pad: DisplayIcon → InstallLocation + exe-naam → bekende mappen.
#[cfg(windows)]
fn resolve_exe(
    query: &ToolQuery,
    display_icon: Option<String>,
    install_location: Option<String>,
) -> Option<String> {
    use std::path::{Path, PathBuf};

    // DisplayIcon kan er zo uitzien: "C:\...\app.exe", soms met ",0"-suffix.
    if let Some(icon) = display_icon {
        let cleaned = icon
            .trim()
            .trim_matches('"')
            .rsplit_once(",")
            .map(|(p, idx)| {
                if idx.trim().chars().all(|c| c.is_ascii_digit()) {
                    p.to_string()
                } else {
                    icon.trim().trim_matches('"').to_string()
                }
            })
            .unwrap_or_else(|| icon.trim().trim_matches('"').to_string());
        let cleaned = cleaned.trim().trim_matches('"');
        if cleaned.to_ascii_lowercase().ends_with(".exe") && Path::new(cleaned).exists() {
            return Some(cleaned.to_string());
        }
    }

    let exe_name = query.exe_name.as_deref()?;

    if let Some(loc) = install_location {
        let loc = loc.trim().trim_matches('"');
        if !loc.is_empty() {
            let candidate = Path::new(loc).join(exe_name);
            if candidate.exists() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }

    // Best-effort zoeken op gebruikelijke installatiepaden.
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(PathBuf::from(&local));
        roots.push(PathBuf::from(&local).join("Programs"));
    }
    if let Ok(pf) = std::env::var("ProgramFiles") {
        roots.push(PathBuf::from(pf));
    }
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        roots.push(PathBuf::from(pf86));
    }

    let exe_stem = exe_name.trim_end_matches(".exe");
    for root in roots {
        for folder in [&query.display_name, &exe_stem.to_string()] {
            let candidate = root.join(folder).join(exe_name);
            if candidate.exists() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
pub fn scan(queries: &[ToolQuery]) -> Vec<InstalledTool> {
    super::linux::scan(queries)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn scan(_queries: &[ToolQuery]) -> Vec<InstalledTool> {
    Vec::new()
}
