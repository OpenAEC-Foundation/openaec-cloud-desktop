//! Systeemvak: de app blijft op de achtergrond synchroniseren als het venster dicht is. Het menu opent het
//! venster, start een sync-ronde of sluit de app af. Met `--verborgen` (autostart) start de app zonder venster.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, Window, WindowEvent};

use crate::{sync_alles, AppState};

pub const VERBORGEN: &str = "--verborgen";

pub fn toon(app: &AppHandle) {
    if let Some(venster) = app.get_webview_window("main") {
        let _ = venster.unminimize();
        let _ = venster.show();
        let _ = venster.set_focus();
    }
}

pub fn maak(app: &mut App) -> tauri::Result<()> {
    let openen = MenuItem::with_id(app, "openen", "OpenAEC Storage openen", true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", "Nu synchroniseren", true, None::<&str>)?;
    let afsluiten = MenuItem::with_id(app, "afsluiten", "Afsluiten", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&openen, &sync, &afsluiten])?;

    let mut vak = TrayIconBuilder::with_id("openaec-storage")
        .tooltip("OpenAEC Storage")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, gebeurtenis| match gebeurtenis.id.as_ref() {
            "openen" => toon(app),
            "sync" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    if let Ok(r) = sync_alles(&state).await {
                        let _ = app.emit("sync", &r);
                    }
                });
            }
            "afsluiten" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|vak, gebeurtenis| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = gebeurtenis {
                toon(vak.app_handle());
            }
        });
    if let Some(icoon) = app.default_window_icon() {
        vak = vak.icon(icoon.clone());
    }
    vak.build(app)?;

    if std::env::args().any(|a| a == VERBORGEN) {
        if let Some(venster) = app.get_webview_window("main") {
            let _ = venster.hide();
        }
    }
    Ok(())
}

/// Venster sluiten = verbergen; afsluiten gaat via het menu in het systeemvak.
pub fn venster_gebeurtenis(venster: &Window, gebeurtenis: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = gebeurtenis {
        let _ = venster.hide();
        api.prevent_close();
    }
}
