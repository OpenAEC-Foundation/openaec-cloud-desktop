mod sync;
mod webdav;

#[cfg(windows)]
pub mod cloudfilter;

use std::sync::Mutex;
use webdav::{Conn, RemoteEntry};

#[derive(Default)]
struct AppState {
    conn: Mutex<Option<Conn>>,
}

fn current(state: &tauri::State<'_, AppState>) -> Result<Conn, String> {
    state.conn.lock().unwrap().clone().ok_or_else(|| "niet verbonden".into())
}

/// Verbinden + de root-map teruggeven (dient meteen als connectie-test).
#[tauri::command]
async fn connect(
    state: tauri::State<'_, AppState>,
    base: String,
    user: String,
    pass: String,
) -> Result<Vec<RemoteEntry>, String> {
    let c = Conn::new(&base, &user, &pass).map_err(|e| e.to_string())?;
    let root = c.list("/").await.map_err(|e| e.to_string())?;
    *state.conn.lock().unwrap() = Some(c);
    Ok(root)
}

#[tauri::command]
async fn list_remote(state: tauri::State<'_, AppState>, path: String) -> Result<Vec<RemoteEntry>, String> {
    current(&state)?.list(&path).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_link(state: tauri::State<'_, AppState>, path: String) -> Result<String, String> {
    current(&state)?.create_public_link(&path).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn sync_folder(
    state: tauri::State<'_, AppState>,
    local: String,
    remote: String,
) -> Result<sync::SyncReport, String> {
    let c = current(&state)?;
    sync::sync_once(&c, &local, &remote).await.map_err(|e| e.to_string())
}

/// Fase 2a: maak van een lokale map een cloud-map (Verkenner-overlays) en vul
/// 'm met placeholders vanuit de remote-map. Geeft het aantal placeholders terug.
#[tauri::command]
async fn enable_cloud_folder(
    state: tauri::State<'_, AppState>,
    local: String,
    remote: String,
) -> Result<u32, String> {
    #[cfg(windows)]
    {
        let c = current(&state)?;
        std::fs::create_dir_all(&local).map_err(|e| e.to_string())?;
        cloudfilter::register(&local, "OpenAEC Cloud", "0.1.0").map_err(|e| e.to_string())?;
        let entries = c.list(&remote).await.map_err(|e| e.to_string())?;
        let mut n = 0u32;
        for e in entries.iter().filter(|e| !e.is_dir) {
            if cloudfilter::create_file_placeholder(&local, &e.name, e.size as i64, &e.path).is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }
    #[cfg(not(windows))]
    {
        let _ = (&state, &local, &remote);
        Err("Cloud Filter is alleen op Windows beschikbaar".into())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            connect,
            list_remote,
            create_link,
            sync_folder,
            enable_cloud_folder
        ])
        .run(tauri::generate_context!())
        .expect("fout bij het starten van OpenAEC Cloud");
}
