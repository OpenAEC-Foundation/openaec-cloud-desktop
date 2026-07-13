// Zelfstandige headless smoke-test voor de Cloud Filter API (Fase 2a).
// Draai met:  cargo run --example cf_smoke
//
// BEWUST self-contained: gebruikt alleen de `windows`-crate (geen tauri/webview2
// via de lib), zodat de .exe licht is en puur de CfAPI-calls test.
#![cfg(windows)]

use anyhow::{anyhow, Result};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use windows::core::PCWSTR;
use windows::Win32::Storage::CloudFilters::*;
use windows::Win32::Storage::FileSystem::FILE_BASIC_INFO;

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

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

    let wpath = wide(&path);
    let wprov = wide("OpenAEC Cloud");
    let wver = wide("0.1.0");
    let ident = wide(&path);
    let wname = wide("voorbeeld.txt");
    let fid = wide("/remote/voorbeeld.txt");

    unsafe {
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

        let mut basic: FILE_BASIC_INFO = zeroed();
        basic.FileAttributes = 0x80; // FILE_ATTRIBUTE_NORMAL
        let mut info: CF_PLACEHOLDER_CREATE_INFO = zeroed();
        info.RelativeFileName = PCWSTR(wname.as_ptr());
        info.FsMetadata.BasicInfo = basic;
        info.FsMetadata.FileSize = 1234;
        info.FileIdentity = fid.as_ptr() as *const c_void;
        info.FileIdentityLength = (fid.len() * 2) as u32;
        info.Flags = CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC;

        let mut arr = [info];
        let mut processed = 0u32;
        CfCreatePlaceholders(
            PCWSTR(wpath.as_ptr()),
            &mut arr,
            CF_CREATE_FLAG_NONE,
            Some(&mut processed as *mut u32),
        )
        .map_err(|e| anyhow!("CfCreatePlaceholders: {e}"))?;
        println!("  CfCreatePlaceholders OK (processed={processed}, result={:?})", arr[0].Result);
    }

    let md = std::fs::metadata(dir.join("voorbeeld.txt"))?;
    // 0x1000=OFFLINE, 0x40000=RECALL_ON_OPEN, 0x400000=RECALL_ON_DATA_ACCESS, 0x400=REPARSE
    println!("placeholder: attrs=0x{:X}  size={}", md.file_attributes(), md.len());
    println!("\nOK — open '{}' in Verkenner voor de wolk-overlay.", dir.display());
    Ok(())
}
