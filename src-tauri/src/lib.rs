mod sync;
mod webdav;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![connect, list_remote, create_link, sync_folder])
        .run(tauri::generate_context!())
        .expect("fout bij het starten van OpenAEC Cloud");
}
