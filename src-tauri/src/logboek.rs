//! Logboek van de app: wat de app met `log::…` meldt, komt in `logboek.txt` naast `instellingen.json`
//! (Windows: `%APPDATA%\OpenAEC Storage\logboek.txt`), en een paniek ook, voordat de app stopt. Zonder dit gingen alle
//! meldingen verloren (een release-build heeft geen console). Het bestand blijft klein: is het bij het starten groter
//! dan 1 MB, dan wordt het `logboek.1.txt` en begint een nieuw. Geen tokens, sleutels of inhoud van bestanden: alleen
//! wat de code zelf meldt. Meer of minder detail met de omgevingsvariabele `OPENAEC_LOG` (bijv. `debug`).

use std::io::Write;
use std::path::{Path, PathBuf};

const GROOTSTE: u64 = 1024 * 1024;

/// Waar het logboek staat.
pub fn pad() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("OpenAEC Storage").join("logboek.txt")
}

/// Een te groot logboek opzij zetten (één oude versie blijft bewaard).
fn draai(pad: &Path, grootste: u64) {
    if std::fs::metadata(pad).is_ok_and(|m| m.len() > grootste) {
        let _ = std::fs::rename(pad, pad.with_file_name("logboek.1.txt"));
    }
}

/// Naar het bestand, en in een debug-build ook naar de console (`npm run tauri dev`).
struct Uit(std::fs::File);

impl Write for Uit {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        #[cfg(debug_assertions)]
        let _ = std::io::stderr().write_all(buf);
        self.0.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// Het logboek starten (één keer, bij het begin van `run`). Lukt het bestand niet, dan draait de app gewoon door.
pub fn start() {
    let pad = pad();
    if let Some(map) = pad.parent() {
        let _ = std::fs::create_dir_all(map);
    }
    draai(&pad, GROOTSTE);
    let Ok(bestand) = std::fs::OpenOptions::new().create(true).append(true).open(&pad) else { return };
    let mut bouwer = env_logger::Builder::new();
    bouwer
        .filter_level(log::LevelFilter::Warn)
        .filter_module("openaec_cloud_lib", log::LevelFilter::Info)
        .filter_module("openaec_sync", log::LevelFilter::Info)
        .format_timestamp_millis()
        .target(env_logger::Target::Pipe(Box::new(Uit(bestand))));
    if let Ok(filter) = std::env::var("OPENAEC_LOG") {
        bouwer.parse_filters(&filter);
    }
    if bouwer.try_init().is_err() {
        return;
    }
    // Een paniek ook in het logboek (de standaardmelding gaat naar een console die er niet is).
    let vorige = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let draad = std::thread::current().name().unwrap_or("naamloos").to_string();
        log::error!("paniek in draad {draad}: {info}");
        vorige(info);
    }));
    log::info!("OpenAEC Desktop App {} gestart", env!("CARGO_PKG_VERSION"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn te_groot_logboek_gaat_opzij() {
        let map = std::env::temp_dir().join(format!("openaec-logboek-{}", std::process::id()));
        std::fs::create_dir_all(&map).unwrap();
        let pad = map.join("logboek.txt");
        std::fs::write(&pad, b"klein").unwrap();
        draai(&pad, 10);
        assert!(pad.exists(), "klein blijft staan");
        std::fs::write(&pad, b"veel te groot logboek").unwrap();
        draai(&pad, 10);
        assert!(!pad.exists());
        assert_eq!(std::fs::read(map.join("logboek.1.txt")).unwrap(), b"veel te groot logboek");
        let _ = std::fs::remove_dir_all(&map);
        assert!(super::pad().ends_with("OpenAEC Storage/logboek.txt") || super::pad().ends_with(r"OpenAEC Storage\logboek.txt"));
    }
}
