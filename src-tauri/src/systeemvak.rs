//! Systeemvak: de app blijft op de achtergrond synchroniseren als het venster dicht is. Het menu opent het
//! venster, start een sync-ronde of sluit de app af; staat er een update klaar, dan ook "Update installeren".
//! Met `--verborgen` (autostart) start de app zonder venster.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, Window, WindowEvent, Wry};

use crate::{sync_alles, updates, AppState};

/// Het menu van het systeemvak, om er later het update-item in te zetten.
struct Vakmenu(Menu<Wry>);

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
    app.manage(Vakmenu(menu.clone()));

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
            "update" => updates::installeer(app),
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

/// Zet "Update installeren (vX.Y.Z)" in het menu, boven "Afsluiten" (een eerder item voor een oudere versie gaat eruit).
pub fn toon_update(app: &AppHandle, versie: &str) {
    let Some(menu) = app.try_state::<Vakmenu>() else { return };
    if let Some(oud) = menu.0.get("update") {
        let _ = menu.0.remove(&oud);
    }
    let positie = menu.0.items().map(|items| items.len().saturating_sub(1)).unwrap_or(0);
    match MenuItem::with_id(app, "update", format!("Update installeren (v{versie})"), true, None::<&str>) {
        Ok(item) => {
            if let Err(e) = menu.0.insert(&item, positie) {
                log::warn!("update-item in het systeemvak: {e}");
            }
        }
        Err(e) => log::warn!("update-item in het systeemvak: {e}"),
    }
}

/// Venster sluiten = verbergen; afsluiten gaat via het menu in het systeemvak.
pub fn venster_gebeurtenis(venster: &Window, gebeurtenis: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = gebeurtenis {
        let _ = venster.hide();
        api.prevent_close();
    }
}
