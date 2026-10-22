//! Tabblad "Tools": de OpenAEC-tools ontdekken, installeren, bijwerken, starten en verwijderen.
//!
//! De logica komt uit de OpenAEC Installer (zie NOTICE). Die losse installer blijft voorlopig bestaan; deze module is
//! daarom zo gebouwd dat beide naast elkaar kunnen werken:
//! - detectie leest alleen wat de setups van de tools zelf schrijven (Windows-register) of de gedeelde AppImage-map
//!   (Linux), dus beide apps zien dezelfde installaties;
//! - installeren start de setup van de tool zelf (Windows) of zet de AppImage in die gedeelde map (Linux), zodat er
//!   nooit een tweede installatie of een eigen registerregel naast komt;
//! - de installer zelf wordt hier niet bijgewerkt of beheerd.

mod github;
mod installer;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod ondertekening;
mod registry;

pub use github::{ReleaseInfo, RepoRef};
pub use installer::InstallOutcome;
pub use registry::{InstalledTool, ToolQuery};

/// Geïnstalleerde tools (Windows-register / Linux-appmap).
#[tauri::command]
pub async fn tools_geinstalleerd(queries: Vec<ToolQuery>) -> Result<Vec<InstalledTool>, String> {
    tauri::async_runtime::spawn_blocking(move || registry::scan(&queries)).await.map_err(|e| e.to_string())
}

/// De laatste release van elke tool, parallel opgehaald.
#[tauri::command]
pub async fn tools_releases(repos: Vec<RepoRef>) -> Vec<ReleaseInfo> {
    github::fetch_all(repos).await
}

/// Installer downloaden, controleren (SHA-256, op Windows ook de handtekening) en starten.
#[tauri::command]
pub async fn tool_installeren(
    app: tauri::AppHandle,
    id: String,
    url: String,
    bestandsnaam: String,
    digest: Option<String>,
    naam: Option<String>,
) -> Result<InstallOutcome, String> {
    installer::download_and_run(app, id, url, bestandsnaam, digest, naam).await
}

/// Een geïnstalleerde tool starten.
#[tauri::command]
pub fn tool_starten(exe_pad: String) -> Result<(), String> {
    installer::launch(&exe_pad)
}

/// Een tool verwijderen: "ranUninstaller" (Windows, de uninstaller van de tool loopt) of "removed" (Linux).
#[tauri::command]
pub async fn tool_verwijderen(query: ToolQuery) -> Result<&'static str, String> {
    tauri::async_runtime::spawn_blocking(move || installer::uninstall(&query)).await.map_err(|e| e.to_string())?
}
