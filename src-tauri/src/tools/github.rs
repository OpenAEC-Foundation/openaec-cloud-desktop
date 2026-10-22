//! Laatste releases van de OpenAEC-tools ophalen via de GitHub API (ongeauthenticeerd, 60 aanroepen per uur — de
//! frontend bewaart het resultaat een tijd).
//!
//! Overgenomen uit de OpenAEC Installer (`src-tauri/src/github.rs`, zie NOTICE) en aangepast: alleen repo's van de
//! organisatie [`ORGANISATIE`] worden opgevraagd, en de SHA-256 die GitHub per asset publiceert gaat mee, zodat de
//! download gecontroleerd kan worden.
//!
//! De versie wordt uit de naam van het installer-asset gehaald in plaats van uit de tag: bij sommige repo's wijkt de
//! tag af van de versie van het bestand. Welk asset gekozen wordt hangt af van het platform: Windows-installers op
//! Windows, AppImages op Linux.

use serde::{Deserialize, Serialize};

const USER_AGENT: &str = "OpenAEC-Desktop-App (https://github.com/OpenAEC-Foundation)";

/// De enige GitHub-organisatie waarvan releases worden opgehaald en gedownload.
pub const ORGANISATIE: &str = "OpenAEC-Foundation";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRef {
    pub id: String,
    pub owner: String,
    pub repo: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub id: String,
    pub ok: bool,
    pub error: Option<String>,
    pub tag: Option<String>,
    /// Versie uit de naam van het installer-asset, met de tag als terugval.
    pub version: Option<String>,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: Option<u64>,
    /// SHA-256 van het asset zoals GitHub hem publiceert ("sha256:<hex>").
    pub asset_digest: Option<String>,
    pub page_url: Option<String>,
    pub published_at: Option<String>,
    /// Bij `error = "rate_limited"`: wanneer de GitHub-limiet weer vrijgeeft (epoch-seconden, uit de
    /// x-ratelimit-reset-header).
    pub rate_limit_reset: Option<u64>,
}

pub async fn fetch_all(repos: Vec<RepoRef>) -> Vec<ReleaseInfo> {
    let client = match reqwest::Client::builder().user_agent(USER_AGENT).build() {
        Ok(c) => c,
        Err(e) => return repos.into_iter().map(|r| error_info(r.id, format!("http client: {e}"))).collect(),
    };
    let futures = repos.into_iter().map(|r| fetch_one(client.clone(), r));
    futures_util::future::join_all(futures).await
}

/// Geldige repo-naam volgens GitHub: letters, cijfers, '-', '_' en '.'.
fn geldige_repo(naam: &str) -> bool {
    !naam.is_empty() && naam != "." && naam != ".." && naam.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

async fn fetch_one(client: reqwest::Client, repo: RepoRef) -> ReleaseInfo {
    if !repo.owner.eq_ignore_ascii_case(ORGANISATIE) || !geldige_repo(&repo.repo) {
        return error_info(repo.id, "not_allowed".into());
    }
    let url = format!("https://api.github.com/repos/{}/{}/releases/latest", ORGANISATIE, repo.repo);
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => return error_info(repo.id, format!("netwerk: {e}")),
    };

    match resp.status().as_u16() {
        200 => {}
        404 => return error_info(repo.id, "no_releases".into()),
        403 | 429 => {
            let reset = resp
                .headers()
                .get("x-ratelimit-reset")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            let mut info = error_info(repo.id, "rate_limited".into());
            info.rate_limit_reset = reset;
            return info;
        }
        s => return error_info(repo.id, format!("http {s}")),
    }

    let json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return error_info(repo.id, format!("json: {e}")),
    };
    release_uit_json(repo.id, &json)
}

/// Zet het antwoord van `releases/latest` om; los van het netwerk zodat het te testen is.
fn release_uit_json(id: String, json: &serde_json::Value) -> ReleaseInfo {
    let tag = json["tag_name"].as_str().map(str::to_string);
    let page_url = json["html_url"].as_str().map(str::to_string);
    let published_at = json["published_at"].as_str().map(str::to_string);

    let assets = json["assets"].as_array().cloned().unwrap_or_default();
    let best = assets
        .iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?;
            let score = score_asset(name)?;
            Some((score, name.to_string(), a))
        })
        .max_by_key(|(score, _, _)| *score);

    let (asset_name, asset_url, asset_size, asset_digest) = match &best {
        Some((_, name, a)) => (
            Some(name.clone()),
            a["browser_download_url"].as_str().map(str::to_string),
            a["size"].as_u64(),
            a["digest"].as_str().map(str::to_string),
        ),
        None => (None, None, None, None),
    };

    let version = asset_name
        .as_deref()
        .and_then(extract_version)
        .or_else(|| tag.as_deref().map(|t| t.trim_start_matches('v').to_string()));

    ReleaseInfo {
        id,
        ok: true,
        error: None,
        tag,
        version,
        asset_name,
        asset_url,
        asset_size,
        asset_digest,
        page_url,
        published_at,
        rate_limit_reset: None,
    }
}

/// Kies het juiste asset voor het huidige platform. `cfg!` in plaats van `#[cfg]`, zodat alle scorers op elk platform
/// meecompileren en getest worden.
fn score_asset(name: &str) -> Option<u32> {
    if cfg!(target_os = "linux") {
        score_linux_asset(name)
    } else if cfg!(target_os = "macos") {
        // Installeren op macOS bestaat nog niet; geen asset betekent dat de kaart "geen installer" toont.
        None
    } else {
        score_windows_asset(name)
    }
}

/// Linux: alleen x86-64-AppImages — die kan de app zelf plaatsen en starten zonder beheerdersrechten (.deb vergt
/// root en valt af).
fn score_linux_asset(name: &str) -> Option<u32> {
    let n = name.to_ascii_lowercase();
    if !n.ends_with(".appimage") {
        return None;
    }
    if ["aarch64", "arm64", "armv7", "armhf", "i386", "i686"].iter().any(|a| n.contains(a)) {
        return None;
    }
    let x64 = n.contains("x64") || n.contains("x86_64") || n.contains("amd64");
    Some(if x64 { 100 } else { 90 })
}

/// Voorkeursvolgorde voor Windows-installers; None = geen installer-asset.
fn score_windows_asset(name: &str) -> Option<u32> {
    let n = name.to_ascii_lowercase();
    if n.ends_with(".sig") || n.ends_with(".sha256") || n.ends_with(".zip") || n.ends_with(".json") {
        return None;
    }
    let x64 = n.contains("x64") || n.contains("x86_64") || n.contains("amd64");
    if n.ends_with("-setup.exe") || n.ends_with("_setup.exe") {
        // Voorkeur voor de installer per gebruiker: die vraagt geen beheerdersrechten. Binnen dezelfde soort
        // installer wint x64 altijd.
        if n.contains("user-setup") {
            return Some(if x64 { 110 } else { 95 });
        }
        return if x64 { Some(100) } else { Some(85) };
    }
    if n.ends_with(".msi") {
        return if x64 || n.contains("windows") { Some(60) } else { Some(50) };
    }
    None
}

/// Zoek een versienummer (minimaal `x.y`) in een assetnaam, bijv. "Open.Frame.Studio_0.5.2_x64-setup.exe" → "0.5.2".
/// Ook gebruikt door de Linux-detectie voor de versie van een geïnstalleerde AppImage.
pub(crate) fn extract_version(name: &str) -> Option<String> {
    name.split(['_', '-'])
        .map(|t| t.trim_start_matches('v').trim_start_matches('V'))
        .find(|t| !t.is_empty() && t.contains('.') && t.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())))
        .map(str::to_string)
}

fn error_info(id: String, error: String) -> ReleaseInfo {
    ReleaseInfo {
        id,
        ok: false,
        error: Some(error),
        tag: None,
        version: None,
        asset_name: None,
        asset_url: None,
        asset_size: None,
        asset_digest: None,
        page_url: None,
        published_at: None,
        rate_limit_reset: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versie_uit_assetnaam() {
        assert_eq!(extract_version("Open.Frame.Studio_0.5.2_x64-setup.exe").as_deref(), Some("0.5.2"));
        assert_eq!(extract_version("Open.Planner.Studio_2026.7.10_x64-setup.exe").as_deref(), Some("2026.7.10"));
        assert_eq!(extract_version("OpenCADStudio-v0.8.0-windows-x86_64-installer.msi").as_deref(), Some("0.8.0"));
        assert_eq!(extract_version("Open 3D Studio_0.8.0_amd64.AppImage").as_deref(), Some("0.8.0"));
        assert_eq!(extract_version("latest.json"), None);
    }

    #[test]
    fn windows_voorkeur_per_gebruiker() {
        let system = score_windows_asset("Open.PDF.Studio_1.67.0_x64-setup.exe");
        let user = score_windows_asset("Open.PDF.Studio_1.67.0_x64_user-setup.exe");
        let msi = score_windows_asset("Open.PDF.Studio_1.67.0_x64_en-US.msi");
        assert!(user > system && system > msi && msi.is_some());
        assert_eq!(score_windows_asset("Open.PDF.Studio_1.67.0_x64-setup.exe.sig"), None);
        assert_eq!(score_windows_asset("Open.PDF.Studio_1.67.0_x64-setup.nsis.zip"), None);
        assert_eq!(score_windows_asset("Open.2D.Studio_0.35.0_amd64.AppImage"), None);
        // x64 wint altijd binnen dezelfde soort installer.
        assert!(score_windows_asset("A_1.0.0_x64-setup.exe") > score_windows_asset("A_1.0.0_x86-setup.exe"));
    }

    #[test]
    fn linux_alleen_x86_64_appimages() {
        assert!(score_linux_asset("Open.2D.Studio_0.35.0_amd64.AppImage").is_some());
        assert_eq!(score_linux_asset("Open.2D.Studio_0.35.0_amd64.deb"), None);
        assert_eq!(score_linux_asset("Open.PDF.Studio_1.67.0_x64-setup.exe"), None);
        assert_eq!(score_linux_asset("Open.2D.Studio_0.35.0_amd64.AppImage.sig"), None);
        assert_eq!(score_linux_asset("Open.2D.Studio_0.35.0_aarch64.AppImage"), None);
        assert_eq!(score_linux_asset("Open.2D.Studio_0.35.0_i386.AppImage"), None);
    }

    #[test]
    fn alleen_eigen_organisatie() {
        assert!(geldige_repo("open-pdf-studio"));
        assert!(geldige_repo("Open-Calculations-Studio"));
        assert!(!geldige_repo("../x"));
        assert!(!geldige_repo("a/b"));
        assert!(!geldige_repo(""));
    }

    #[test]
    fn release_met_digest_en_assetkeuze() {
        let json = serde_json::json!({
            "tag_name": "v1.0.1",
            "html_url": "https://github.com/OpenAEC-Foundation/monty-ifc-viewer/releases/tag/v1.0.1",
            "published_at": "2026-01-01T00:00:00Z",
            "assets": [
                { "name": "Monty.IFC.Viewer_1.0.0_x64_en-US.msi", "browser_download_url": "https://github.com/x/msi", "size": 2, "digest": "sha256:bb" },
                { "name": "Monty.IFC.Viewer_1.0.0_x64-setup.exe", "browser_download_url": "https://github.com/x/exe", "size": 1, "digest": "sha256:aa" },
                { "name": "Monty.IFC.Viewer_1.0.0_amd64.AppImage", "browser_download_url": "https://github.com/x/appimage", "size": 3, "digest": "sha256:cc" },
                { "name": "latest.json", "browser_download_url": "https://github.com/x/json", "size": 4 }
            ]
        });
        let r = release_uit_json("monty".into(), &json);
        assert!(r.ok);
        assert_eq!(r.tag.as_deref(), Some("v1.0.1"));
        // De versie komt uit de assetnaam, niet uit de tag.
        if cfg!(windows) {
            assert_eq!(r.asset_name.as_deref(), Some("Monty.IFC.Viewer_1.0.0_x64-setup.exe"));
            assert_eq!(r.asset_digest.as_deref(), Some("sha256:aa"));
            assert_eq!(r.version.as_deref(), Some("1.0.0"));
        } else if cfg!(target_os = "linux") {
            assert_eq!(r.asset_digest.as_deref(), Some("sha256:cc"));
        }
        // Zonder bruikbaar asset valt de versie terug op de tag.
        let r = release_uit_json("x".into(), &serde_json::json!({ "tag_name": "v2.0", "assets": [] }));
        assert_eq!(r.version.as_deref(), Some("2.0"));
        assert!(r.asset_url.is_none());
    }
}
