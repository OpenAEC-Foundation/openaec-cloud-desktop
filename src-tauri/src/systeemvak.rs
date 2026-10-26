//! Systeemvak: de app blijft op de achtergrond synchroniseren als het venster dicht is. Een linkerklik op het
//! pictogram opent het statusvenster rechtsonder (wat er nu gebeurt en wat er net gebeurd is); het menu onder de
//! rechtermuisknop opent het hoofdvenster, start een sync-ronde of sluit de app af, en toont "Update installeren"
//! als er een update klaarstaat. Het pictogram zelf laat de toestand zien met een gekleurde stip (bijgewerkt,
//! bezig, gepauzeerd of fout); de tooltip vat hem samen. Met `--verborgen` (autostart) start de app zonder venster.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, PhysicalPosition, Window, WindowEvent, Wry};

use crate::activiteit::Toestand;
use crate::{sync_alles, updates};

/// Het menu van het systeemvak, om er later het update-item in te zetten.
struct Vakmenu(Menu<Wry>);

pub const VERBORGEN: &str = "--verborgen";
const VAK: &str = "openaec-storage";
const STATUS: &str = "status";

/// Wanneer het statusvenster voor het laatst verborgen werd (door focusverlies). Een klik op het pictogram verbergt
/// het venster eerst via focusverlies; zonder deze tijd zou dezelfde klik het meteen weer openen.
static VERBORGEN_OP: Mutex<Option<Instant>> = Mutex::new(None);

pub fn toon(app: &AppHandle) {
    if let Some(venster) = app.get_webview_window("main") {
        let _ = venster.unminimize();
        let _ = venster.show();
        let _ = venster.set_focus();
    }
    verberg_status(app);
    // Het venster ververst dan zijn gegevens (ruimtes, overzicht).
    let _ = app.emit("venster-open", ());
}

/// Statusvenster openen (of sluiten) rechtsonder, bij de plek van de klik en binnen het werkgebied van het scherm.
fn wissel_status(app: &AppHandle, klik: PhysicalPosition<f64>) {
    let Some(venster) = app.get_webview_window(STATUS) else { return };
    if venster.is_visible().unwrap_or(false) {
        let _ = venster.hide();
        return;
    }
    if VERBORGEN_OP.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(300)) {
        return;
    }
    if let (Ok(grootte), Ok(Some(scherm))) = (venster.outer_size(), venster.monitor_from_point(klik.x, klik.y)) {
        let werk = scherm.work_area();
        let (links, boven) = (werk.position.x, werk.position.y);
        let (rechts, onder) = (links + werk.size.width as i32, boven + werk.size.height as i32);
        let marge = 12;
        let (breed, hoog) = (grootte.width as i32, grootte.height as i32);
        let x = (klik.x as i32 - breed / 2).clamp(links + marge, (rechts - breed - marge).max(links + marge));
        // Taakbalk onder (meestal): boven het pictogram; taakbalk boven: eronder.
        let y = if (klik.y as i32) > boven + werk.size.height as i32 / 2 { onder - hoog - marge } else { boven + marge };
        let _ = venster.set_position(PhysicalPosition::new(x, y));
    }
    let _ = venster.show();
    let _ = venster.set_focus();
    let _ = app.emit("venster-open", ());
}

pub fn maak(app: &mut App) -> tauri::Result<()> {
    let openen = MenuItem::with_id(app, "openen", "OpenAEC Desktop App openen", true, None::<&str>)?;
    let assistent = MenuItem::with_id(app, "assistent", "Assistent", true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", "Nu synchroniseren", true, None::<&str>)?;
    let afsluiten = MenuItem::with_id(app, "afsluiten", "Afsluiten", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&openen, &assistent, &sync, &afsluiten])?;
    app.manage(Vakmenu(menu.clone()));

    let mut vak = TrayIconBuilder::with_id(VAK)
        .tooltip("OpenAEC Desktop App")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, gebeurtenis| match gebeurtenis.id.as_ref() {
            "openen" => toon(app),
            // Het hoofdvenster op het scherm Assistent.
            "assistent" => {
                toon(app);
                let _ = app.emit("scherm", "assistent");
            }
            "sync" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(r) = sync_alles(&app).await {
                        let _ = app.emit("sync", &r);
                    }
                });
            }
            "update" => updates::installeer(app),
            "afsluiten" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|vak, gebeurtenis| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, position, .. } = gebeurtenis {
                wissel_status(vak.app_handle(), position);
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

/// Pictogram met een gekleurde stip rechtsonder (wit omrand), afgeleid van het app-pictogram.
fn pictogram(basis: &Image<'_>, kleur: [u8; 3]) -> Image<'static> {
    let (b, h) = (basis.width(), basis.height());
    let mut rgba = basis.rgba().to_vec();
    let straal = b.min(h) as f64 * 0.24;
    let (cx, cy) = (b as f64 - straal - 0.5, h as f64 - straal - 0.5);
    let rand = (straal * 0.22).max(1.0);
    for y in 0..h {
        for x in 0..b {
            let afstand = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
            let pixel = if afstand <= straal - rand {
                [kleur[0], kleur[1], kleur[2], 255]
            } else if afstand <= straal {
                [255, 255, 255, 255]
            } else {
                continue;
            };
            let i = ((y * b + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&pixel);
        }
    }
    Image::new_owned(rgba, b, h)
}

/// Pictogram en tooltip van het systeemvak aanpassen aan de toestand van de sync.
pub fn zet_toestand(app: &AppHandle, toestand: Toestand, tooltip: &str) {
    let Some(vak) = app.tray_by_id(VAK) else { return };
    let _ = vak.set_tooltip(Some(tooltip));
    let Some(basis) = app.default_window_icon() else { return };
    let kleur = match toestand {
        Toestand::Bijgewerkt => [22, 163, 74],
        Toestand::Bezig => [217, 119, 6],
        Toestand::Gepauzeerd => [113, 113, 122],
        Toestand::Fout => [220, 38, 38],
    };
    let _ = vak.set_icon(Some(pictogram(basis, kleur)));
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

/// Venster sluiten = verbergen; afsluiten gaat via het menu in het systeemvak. Het statusvenster verdwijnt ook
/// zodra je ergens anders klikt.
pub fn venster_gebeurtenis(venster: &Window, gebeurtenis: &WindowEvent) {
    match gebeurtenis {
        WindowEvent::CloseRequested { api, .. } => {
            let _ = venster.hide();
            api.prevent_close();
        }
        WindowEvent::Focused(false) if venster.label() == STATUS => {
            let _ = venster.hide();
            *VERBORGEN_OP.lock().unwrap() = Some(Instant::now());
        }
        _ => {}
    }
}

/// Het statusvenster verbergen (na een knop die een ander venster opent).
pub fn verberg_status(app: &AppHandle) {
    if let Some(status) = app.get_webview_window(STATUS) {
        let _ = status.hide();
    }
}
