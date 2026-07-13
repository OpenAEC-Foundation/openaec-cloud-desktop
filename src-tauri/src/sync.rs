// Fase-1 sync-engine: eenvoudige tweerichtings-sync tussen een lokale map en een
// Nextcloud-map. Lokaal-only → upload, remote-only → download. (Conflict-resolutie
// op mtime/etag is Fase 1.5; live filewatcher via `notify` is Fase 2.)
use crate::webdav::{Conn, RemoteEntry};
use anyhow::Result;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Serialize, Default)]
pub struct SyncReport {
    pub uploaded: u32,
    pub downloaded: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
}

pub async fn sync_once(conn: &Conn, local: &str, remote: &str) -> Result<SyncReport> {
    let mut rep = SyncReport::default();
    let remote = remote.trim_matches('/');
    let _ = conn.mkcol(remote).await; // zorg dat de doelmap bestaat

    let remote_files = collect_remote(conn, remote).await;
    let remote_set: HashSet<String> = remote_files
        .iter()
        .filter(|e| !e.is_dir)
        .map(|e| relpath(&e.path, remote))
        .collect();

    // Lokaal → remote (nieuwe bestanden uploaden).
    let mut local_set = HashSet::new();
    for entry in WalkDir::new(local).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(local)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        local_set.insert(rel.clone());
        if remote_set.contains(&rel) {
            rep.skipped += 1;
            continue;
        }
        match std::fs::read(entry.path()) {
            Ok(data) => {
                ensure_remote_dirs(conn, remote, &rel).await;
                match conn.upload(&format!("{}/{}", remote, rel), data).await {
                    Ok(_) => rep.uploaded += 1,
                    Err(e) => rep.errors.push(format!("upload {}: {}", rel, e)),
                }
            }
            Err(e) => rep.errors.push(format!("lezen {}: {}", rel, e)),
        }
    }

    // Remote → lokaal (bestanden die lokaal ontbreken downloaden).
    for e in remote_files.iter().filter(|e| !e.is_dir) {
        let rel = relpath(&e.path, remote);
        if local_set.contains(&rel) {
            continue;
        }
        match conn.download(&e.path).await {
            Ok(data) => {
                let dest = Path::new(local).join(&rel);
                if let Some(p) = dest.parent() {
                    let _ = std::fs::create_dir_all(p);
                }
                match std::fs::write(&dest, data) {
                    Ok(_) => rep.downloaded += 1,
                    Err(err) => rep.errors.push(format!("schrijven {}: {}", rel, err)),
                }
            }
            Err(err) => rep.errors.push(format!("download {}: {}", rel, err)),
        }
    }

    Ok(rep)
}

fn relpath(path: &str, remote: &str) -> String {
    path.trim_start_matches(remote).trim_start_matches('/').to_string()
}

async fn ensure_remote_dirs(conn: &Conn, remote: &str, rel: &str) {
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.len() > 1 {
        let mut cur = remote.to_string();
        for p in &parts[..parts.len() - 1] {
            cur = format!("{}/{}", cur, p);
            let _ = conn.mkcol(&cur).await;
        }
    }
}

/// Recursief alle remote-bestanden verzamelen (PROPFIND depth 1 per map).
async fn collect_remote(conn: &Conn, root: &str) -> Vec<RemoteEntry> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_string()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = conn.list(&dir).await {
            for e in entries {
                if e.path == dir.trim_matches('/') {
                    continue;
                }
                if e.is_dir {
                    stack.push(e.path.clone());
                }
                out.push(e);
            }
        }
    }
    out
}
