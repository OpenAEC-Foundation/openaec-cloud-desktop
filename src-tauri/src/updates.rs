//! Automatische updates: kort na het starten en daarna elke 6 uur stil controleren op een nieuwe versie (`latest.json`
//! bij de laatste GitHub-release, ondertekend met de updater-sleutel). Een nieuwe versie wordt meteen gedownload; dan
//! volgen een melding en in het systeemvakmenu "Update installeren (vX.Y.Z)". Pas na die klik wordt hij geïnstalleerd,
//! en nooit tijdens een sync-ronde. Op Windows sluit de installer de app en start hem daarna opnieuw; op Linux start
//! de app zelf opnieuw. Fouten bij het controleren komen alleen in het logboek.

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{melding, systeemvak, AppState};

const EERSTE_CONTROLE: Duration = Duration::from_secs(60);
const CONTROLE: Duration = Duration::from_secs(6 * 60 * 60);

/// Een gedownloade (en gecontroleerde) update die wacht op de klik in het systeemvak.
struct Klaar {
    update: Update,
    bytes: Vec<u8>,
}

static KLAAR: Mutex<Option<Klaar>> = Mutex::new(None);

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(EERSTE_CONTROLE).await;
        loop {
            if let Err(e) = controleer(&app).await {
                log::warn!("controleren op updates mislukt: {e}");
            }
            tokio::time::sleep(CONTROLE).await;
        }
    });
}

/// Controleer op een nieuwe versie en download hem; geeft het versienummer van de klaargezette update terug.
pub(crate) async fn controleer(app: &AppHandle) -> tauri_plugin_updater::Result<Option<String>> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(None);
    };
    if KLAAR.lock().unwrap().as_ref().is_some_and(|k| k.update.version == update.version) {
        return Ok(Some(update.version));
    }
    let bytes = update.download(|_, _| {}, || {}).await?;
    let versie = update.version.clone();
    *KLAAR.lock().unwrap() = Some(Klaar { update, bytes });
    log::info!("update naar versie {versie} gedownload");
    systeemvak::toon_update(app, &versie);
    melding(app, &format!("Versie {versie} staat klaar. Kies \"Update installeren\" in het systeemvak; de app start daarna opnieuw."));
    Ok(Some(versie))
}

/// Na de klik in het systeemvak: wachten tot een lopende sync-ronde klaar is, dan installeren.
pub fn installeer(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _slot = state.bezig.lock().await;
        let Some(klaar) = KLAAR.lock().unwrap().take() else {
            return;
        };
        // Op Windows sluit de installer de app hier af (en start hem na afloop opnieuw).
        match klaar.update.install(&klaar.bytes) {
            Ok(()) => app.restart(),
            Err(e) => {
                log::error!("update installeren mislukt: {e}");
                melding(&app, &format!("Update installeren mislukt: {e}"));
                *KLAAR.lock().unwrap() = Some(klaar);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use semver::Version;

    /// De updater (tauri-plugin-updater) installeert een release als `latest.json` een hogere semver-versie heeft dan
    /// de app zelf. Na de overstap naar jaar.week (`JJJJ.W.N`) moet elke nieuwe versie dus hoger zijn dan 0.8.1.
    #[test]
    fn jaar_week_is_nieuwer_dan_oude_versies() {
        let v = |s: &str| Version::parse(s).unwrap();
        assert!(v("2026.44.0") > v("0.8.1"));
        assert!(v("2026.44.1") > v("2026.44.0"));
        assert!(v("2026.45.0") > v("2026.44.9"));
        // Weeknummers zonder voorloopnul vergelijken als getal: week 10 komt na week 9.
        assert!(v("2026.10.0") > v("2026.9.3"));
        assert!(v("2027.1.0") > v("2026.53.2"));
        // Geldige semver, geen pre-release (een streepje zou er een pre-release van maken).
        assert!(v("2026.44.0").pre.is_empty());
        assert!(Version::parse("2026.044.0").is_err());
    }
}
