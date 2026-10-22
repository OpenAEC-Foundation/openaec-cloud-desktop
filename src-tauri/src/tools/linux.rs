//! Linux-ondersteuning: AppImages beheren zonder beheerdersrechten.
//!
//! Overgenomen uit de OpenAEC Installer (`src-tauri/src/linux.rs`, zie NOTICE). De beheerde map is bewust dezelfde
//! als die van de losse installer (`openaec-installer/apps`): zo zien beide apps dezelfde installaties en komt een tool
//! nooit twee keer op de computer. Zelf-update van de installer zit hier niet in; die regelt de installer zelf.
//!
//! "Installeren" betekent hier: de AppImage in een eigen beheerde map zetten
//! (`~/.local/share/openaec-installer/apps/<tool-id>/`), uitvoerbaar maken en
//! een menu-snelkoppeling (.desktop) aanmaken. Detectie scant diezelfde map en
//! haalt de versie uit de bestandsnaam (bijv. "Open.2D.Studio_0.35.0_amd64
//! .AppImage" → 0.35.0).

use std::path::{Path, PathBuf};

use super::registry::{InstalledTool, ToolQuery};

/// Basismap voor gebruikersdata volgens XDG (`~/.local/share`).
fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

/// Map waarin de app geïnstalleerde AppImages beheert, per tool-id.
fn apps_dir() -> Option<PathBuf> {
    Some(data_home()?.join("openaec-installer").join("apps"))
}

/// Alle AppImages in een map (meestal 0 of 1).
fn appimages_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .map(|e| e.eq_ignore_ascii_case("appimage"))
                    .unwrap_or(false)
        })
        .collect()
}

/// Scan de beheerde map op geïnstalleerde tools.
pub fn scan(queries: &[ToolQuery]) -> Vec<InstalledTool> {
    let Some(root) = apps_dir() else {
        return Vec::new();
    };
    scan_in(&root, queries)
}

/// Zelfde scan, maar met een expliciete map. De tests gebruiken deze variant
/// met een eigen tijdelijke map: de omgevingsvariabele die `apps_dir()` leest
/// is procesbreed, en Rust draait tests in threads binnen één proces, dus
/// tests die die variabele zetten zouden elkaar overschrijven.
fn scan_in(root: &Path, queries: &[ToolQuery]) -> Vec<InstalledTool> {
    queries
        .iter()
        .filter_map(|q| {
            let dir = root.join(&q.id);
            // Bij meerdere bestanden (hoort niet, maar kan na een mislukte
            // update) wint de hoogste versie.
            let best = appimages_in(&dir).into_iter().max_by_key(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(super::github::extract_version)
                    .map(version_key)
                    .unwrap_or_default()
            })?;
            let version = best
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(super::github::extract_version);
            Some(InstalledTool {
                id: q.id.clone(),
                display_name: q.display_name.clone(),
                version,
                exe_path: Some(best.to_string_lossy().into_owned()),
                uninstall: None,
            })
        })
        .collect()
}

/// Sorteersleutel voor versies: "0.35.0" → [0, 35, 0].
fn version_key(v: String) -> Vec<u64> {
    v.split('.').filter_map(|p| p.parse().ok()).collect()
}

/// Verplaats een bestand, met kopie-fallback voor over bestandssysteemgrenzen
/// heen (temp is vaak tmpfs). De kopie gaat via een `.part`-bestand en een
/// rename, zodat het doel nooit half geschreven is.
fn move_file(src: &Path, target: &Path) -> Result<(), String> {
    if std::fs::rename(src, target).is_ok() {
        return Ok(());
    }
    let part = target.with_extension("part");
    std::fs::copy(src, &part).map_err(|e| format!("kopiëren: {e}"))?;
    std::fs::rename(&part, target).map_err(|e| format!("verplaatsen: {e}"))?;
    let _ = std::fs::remove_file(src);
    Ok(())
}

fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("uitvoerbaar maken: {e}"))
}

/// Plaats een gedownloade AppImage in de beheerde map en maak een
/// menu-snelkoppeling. Oude versies van dezelfde tool worden opgeruimd.
pub fn install_appimage(id: &str, display_name: &str, downloaded: &Path) -> Result<(), String> {
    let data = data_home().ok_or("thuismap niet gevonden")?;
    install_appimage_in(&data, id, display_name, downloaded)
}

/// Zelfde installatie, maar met een expliciete datamap — zie `scan_in`.
fn install_appimage_in(
    data_home: &Path,
    id: &str,
    display_name: &str,
    downloaded: &Path,
) -> Result<(), String> {
    let dir = data_home.join("openaec-installer").join("apps").join(id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("appmap: {e}"))?;

    for old in appimages_in(&dir) {
        let _ = std::fs::remove_file(old);
    }

    let file_name = downloaded.file_name().ok_or("ongeldige bestandsnaam")?;
    let target = dir.join(file_name);
    move_file(downloaded, &target)?;
    make_executable(&target)?;

    // Snelkoppeling is nuttig maar niet essentieel — een fout hier mag de
    // installatie niet laten mislukken.
    let _ = write_desktop_entry(data_home, id, display_name, &target);
    Ok(())
}

/// Schrijf `~/.local/share/applications/openaec-<id>.desktop`, zodat de tool
/// ook in het applicatiemenu verschijnt. Wordt bij elke (her)installatie
/// overschreven zodat het pad naar de nieuwste versie wijst.
fn write_desktop_entry(
    data_home: &Path,
    id: &str,
    display_name: &str,
    appimage: &Path,
) -> Result<(), String> {
    let apps = data_home.join("applications");
    std::fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec=\"{}\"\nTerminal=false\nCategories=Office;Engineering;\nComment=Geïnstalleerd door OpenAEC SuperCloud Desktop\n",
        display_name,
        appimage.display(),
    );
    std::fs::write(apps.join(format!("openaec-{id}.desktop")), entry).map_err(|e| e.to_string())
}

/// Verwijder een beheerde tool: de map met de AppImage en de menu-snelkoppeling.
pub fn uninstall(id: &str) -> Result<(), String> {
    let data = data_home().ok_or("thuismap niet gevonden")?;
    uninstall_in(&data, id)
}

fn uninstall_in(data_home: &Path, id: &str) -> Result<(), String> {
    // Het id komt uit de catalogus; nooit een pad buiten de beheerde map.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("ongeldig tool-id".into());
    }
    let dir = data_home.join("openaec-installer").join("apps").join(id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("verwijderen: {e}"))?;
    }
    let _ = std::fs::remove_file(data_home.join("applications").join(format!("openaec-{id}.desktop")));
    Ok(())
}

/// Start een geïnstalleerde AppImage als losstaand programma.
pub fn launch(exe_path: &str) -> Result<(), String> {
    let path = Path::new(exe_path);
    if !path
        .extension()
        .map(|e| e.eq_ignore_ascii_case("appimage"))
        .unwrap_or(false)
        || !path.exists()
    {
        return Err("Programma niet gevonden.".into());
    }
    let mut cmd = std::process::Command::new(path);
    if let Some(parent) = path.parent() {
        cmd.current_dir(parent);
    }
    // Losgekoppelde uitvoer: anders schrijft de gestarte tool zijn logregels in
    // de uitvoer van de installer en blijft hij aan diens pijpen hangen.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    // Eigen procesgroep. Zonder dit staat de tool in de procesgroep van de
    // installer en sleept het afsluiten daarvan (een signaal naar de hele
    // groep) elke gestarte tool mee — de tool verdwijnt dan zomaar van het
    // scherm. Een gestarte tool hoort de installer te overleven.
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);

    let mut child = cmd.spawn().map_err(|e| format!("starten: {e}"))?;

    // De gestarte tool opruimen zodra de gebruiker hem sluit. Zonder wait()
    // blijft elk afgesloten programma als zombieproces in de procestabel staan
    // zolang de installer draait.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_key_orders_numerically() {
        // 0.10.0 hoort ná 0.9.0 (numeriek, niet lexicografisch).
        assert!(version_key("0.10.0".into()) > version_key("0.9.0".into()));
        assert!(version_key("2026.7.10".into()) > version_key("2026.7.9".into()));
    }

    /// Eigen tijdelijke datamap per test, zodat parallel draaiende tests
    /// elkaar niet in de weg zitten.
    fn temp_home(naam: &str) -> PathBuf {
        let tmp = std::env::temp_dir().join(format!("openaec-test-{}-{naam}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    fn apps_root(home: &Path) -> PathBuf {
        home.join("openaec-installer").join("apps")
    }

    #[test]
    fn handles_appimage_names_with_spaces() {
        // Een productName met spaties levert een asset met spaties op. Dat mag
        // niet stuklopen op het pad in de menu-snelkoppeling.
        let tmp = temp_home("spaces");

        let download = tmp.join("Open 3D Studio_0.8.0_amd64.AppImage");
        std::fs::write(&download, b"fake").unwrap();
        install_appimage_in(&tmp, "open-3d-studio", "Open 3D Studio", &download).unwrap();

        let found = scan_in(&apps_root(&tmp), &[ToolQuery {
            id: "open-3d-studio".into(),
            display_name: "Open 3D Studio".into(),
            exe_name: None,
        }]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].version.as_deref(), Some("0.8.0"));

        // Het pad in de snelkoppeling staat tussen aanhalingstekens, anders
        // breekt de spatie het Exec-veld op in losse argumenten.
        let entry =
            std::fs::read_to_string(tmp.join("applications/openaec-open-3d-studio.desktop")).unwrap();
        let exec = entry.lines().find(|l| l.starts_with("Exec=")).unwrap();
        assert!(exec.contains("\"") && exec.contains("Open 3D Studio_0.8.0"), "{exec}");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_scan_and_update_roundtrip() {
        // Eigen datamap zodat de test nooit in de echte gebruikersmap schrijft.
        let tmp = temp_home("roundtrip");

        let download = tmp.join("Open.2D.Studio_0.35.0_amd64.AppImage");
        std::fs::write(&download, b"fake").unwrap();
        install_appimage_in(&tmp, "open-2d-studio", "Open 2D Studio", &download).unwrap();

        let queries = vec![ToolQuery {
            id: "open-2d-studio".into(),
            display_name: "Open 2D Studio".into(),
            exe_name: None,
        }];
        let found = scan_in(&apps_root(&tmp), &queries);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].version.as_deref(), Some("0.35.0"));
        let exe = found[0].exe_path.clone().unwrap();
        assert!(Path::new(&exe).exists());

        // Update: nieuwe versie vervangt de oude, ook in de detectie.
        let newer = tmp.join("Open.2D.Studio_0.36.0_amd64.AppImage");
        std::fs::write(&newer, b"fake2").unwrap();
        install_appimage_in(&tmp, "open-2d-studio", "Open 2D Studio", &newer).unwrap();
        let found = scan_in(&apps_root(&tmp), &queries);
        assert_eq!(found[0].version.as_deref(), Some("0.36.0"));
        assert!(!Path::new(&exe).exists(), "oude versie is opgeruimd");

        // Snelkoppeling wijst naar de nieuwe versie.
        let desktop = tmp.join("applications/openaec-open-2d-studio.desktop");
        let entry = std::fs::read_to_string(&desktop).unwrap();
        assert!(entry.contains("0.36.0"));

        // Verwijderen haalt de AppImage en de snelkoppeling weg; een vreemd id wordt geweigerd.
        assert!(uninstall_in(&tmp, "../x").is_err());
        uninstall_in(&tmp, "open-2d-studio").unwrap();
        assert!(scan_in(&apps_root(&tmp), &queries).is_empty());
        assert!(!desktop.exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
