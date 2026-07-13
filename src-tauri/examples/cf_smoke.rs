// Zelfstandige headless smoke-test voor de Cloud Filter API (Fase 2a + 2b).
// Draai met:  cargo run --example cf_smoke     (opruimen: ... -- clean)
//
// BEWUST self-contained (alleen de `windows`-crate, geen tauri/webview2 via de
// lib) zodat de .exe licht is en puur de CfAPI-keten test:
//   register -> connect (FETCH_DATA-callback) -> placeholder -> OPEN -> hydrate.
#![cfg(windows)]

use anyhow::{anyhow, Result};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::sync::OnceLock;
use windows::core::PCWSTR;
use windows::Win32::Foundation::NTSTATUS;
use windows::Win32::Storage::CloudFilters::*;
use windows::Win32::Storage::FileSystem::FILE_BASIC_INFO;

const CONTENT: &[u8] = b"Hallo vanuit OpenAEC Cloud - Fase 2b hydration!\n";

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

static HYDRATOR: OnceLock<Box<dyn Fn(&str) -> Vec<u8> + Send + Sync>> = OnceLock::new();

fn main() -> Result<()> {
    let dir = std::env::temp_dir().join("openaec-cf-smoke");

    if std::env::args().nth(1).as_deref() == Some("clean") {
        unsafe { let _ = CfUnregisterSyncRoot(PCWSTR(wide(&dir.to_string_lossy()).as_ptr())); }
        let _ = std::fs::remove_dir_all(&dir);
        println!("opgeruimd.");
        return Ok(());
    }

    std::fs::create_dir_all(&dir)?;
    let _ = std::fs::remove_file(dir.join("voorbeeld.txt"));
    let path = dir.to_string_lossy().to_string();
    println!("sync-root: {path}");

    let _ = HYDRATOR.set(Box::new(|id| {
        println!("  [hydrator] FETCH_DATA voor identiteit '{id}'");
        CONTENT.to_vec()
    }));

    let wpath = wide(&path);
    let wprov = wide("OpenAEC Cloud");
    let wver = wide("0.1.0");
    let ident = wide(&path);
    let wname = wide("voorbeeld.txt");
    let fid = wide("/remote/voorbeeld.txt");

    unsafe {
        // --- Fase 2a: registreren ---
        let mut reg: CF_SYNC_REGISTRATION = zeroed();
        reg.StructSize = size_of::<CF_SYNC_REGISTRATION>() as u32;
        reg.ProviderName = PCWSTR(wprov.as_ptr());
        reg.ProviderVersion = PCWSTR(wver.as_ptr());
        reg.SyncRootIdentity = ident.as_ptr() as *const c_void;
        reg.SyncRootIdentityLength = (ident.len() * 2) as u32;
        let mut pol: CF_SYNC_POLICIES = zeroed();
        pol.StructSize = size_of::<CF_SYNC_POLICIES>() as u32;
        CfRegisterSyncRoot(PCWSTR(wpath.as_ptr()), &reg, &pol, CF_REGISTER_FLAG_UPDATE)
            .map_err(|e| anyhow!("CfRegisterSyncRoot: {e}"))?;
        println!("  CfRegisterSyncRoot   OK");

        // --- Fase 2b: verbinden met FETCH_DATA-callback ---
        let table = [
            CF_CALLBACK_REGISTRATION { Type: CF_CALLBACK_TYPE_FETCH_DATA, Callback: Some(fetch_cb) },
            CF_CALLBACK_REGISTRATION { Type: CF_CALLBACK_TYPE_NONE, Callback: None },
        ];
        let key = CfConnectSyncRoot(PCWSTR(wpath.as_ptr()), table.as_ptr(), None, CF_CONNECT_FLAG_NONE)
            .map_err(|e| anyhow!("CfConnectSyncRoot: {e}"))?;
        println!("  CfConnectSyncRoot    OK");

        // --- placeholder aanmaken ---
        let mut basic: FILE_BASIC_INFO = zeroed();
        basic.FileAttributes = 0x80;
        let mut info: CF_PLACEHOLDER_CREATE_INFO = zeroed();
        info.RelativeFileName = PCWSTR(wname.as_ptr());
        info.FsMetadata.BasicInfo = basic;
        info.FsMetadata.FileSize = CONTENT.len() as i64;
        info.FileIdentity = fid.as_ptr() as *const c_void;
        info.FileIdentityLength = (fid.len() * 2) as u32;
        info.Flags = CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC;
        let mut arr = [info];
        let mut processed = 0u32;
        CfCreatePlaceholders(PCWSTR(wpath.as_ptr()), &mut arr, CF_CREATE_FLAG_NONE, Some(&mut processed as *mut u32))
            .map_err(|e| anyhow!("CfCreatePlaceholders: {e}"))?;
        println!("  CfCreatePlaceholders OK (processed={processed})");

        let before = std::fs::metadata(dir.join("voorbeeld.txt"))?.file_attributes();
        println!("  vóór openen: attrs=0x{before:X} (0x400000 = nog online-only)");

        // --- OPEN -> hydration ---
        let got = std::fs::read(dir.join("voorbeeld.txt"))?;
        let after = std::fs::metadata(dir.join("voorbeeld.txt"))?.file_attributes();
        println!("  na openen : attrs=0x{after:X}");
        println!("  gelezen {} bytes: {:?}", got.len(), String::from_utf8_lossy(&got));
        println!("  HYDRATION {}", if got == CONTENT { "OK - inhoud klopt" } else { "MISLUKT" });

        let _ = CfDisconnectSyncRoot(key);
    }
    println!("\nOpen '{}' in Verkenner; het bestand is nu lokaal beschikbaar.", dir.display());
    Ok(())
}

unsafe extern "system" fn fetch_cb(info: *const CF_CALLBACK_INFO, params: *const CF_CALLBACK_PARAMETERS) {
    let info = &*info;
    let params = &*params;

    let id = if !info.FileIdentity.is_null() && info.FileIdentityLength >= 2 {
        let s = std::slice::from_raw_parts(info.FileIdentity as *const u16, (info.FileIdentityLength / 2) as usize);
        String::from_utf16_lossy(s).trim_end_matches('\0').to_string()
    } else {
        String::new()
    };
    let fetch = params.Anonymous.FetchData;
    let data = HYDRATOR.get().map(|h| h(&id)).unwrap_or_default();

    let mut opinfo: CF_OPERATION_INFO = zeroed();
    opinfo.StructSize = size_of::<CF_OPERATION_INFO>() as u32;
    opinfo.Type = CF_OPERATION_TYPE_TRANSFER_DATA;
    opinfo.ConnectionKey = info.ConnectionKey;
    opinfo.TransferKey = info.TransferKey;

    let mut op: CF_OPERATION_PARAMETERS = zeroed();
    op.ParamSize = size_of::<CF_OPERATION_PARAMETERS>() as u32;
    op.Anonymous.TransferData.CompletionStatus = NTSTATUS(0);
    op.Anonymous.TransferData.Buffer = data.as_ptr() as *const c_void;
    op.Anonymous.TransferData.Offset = fetch.RequiredFileOffset;
    op.Anonymous.TransferData.Length = data.len() as i64;

    let _ = CfExecute(&opinfo, &mut op);
}
