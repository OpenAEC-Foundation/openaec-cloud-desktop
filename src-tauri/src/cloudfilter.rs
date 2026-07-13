// Windows Cloud Filter API (cldapi) — Fase 2a.
//
// Registreert een lokale map als "sync root" zodat Verkenner cloud-status-
// overlays toont (wolk / groen vinkje / sync-pijlen) en maakt placeholders:
// bestanden die als metadata bestaan maar pas bij openen worden gedownload
// (files-on-demand), net als OneDrive. Hydration (het downloaden bij openen)
// is Fase 2b; in-sync-states + pin/unpin Fase 2c.
#![cfg(windows)]

use anyhow::{anyhow, Result};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::Storage::CloudFilters::*;
use windows::Win32::Storage::FileSystem::FILE_BASIC_INFO;

/// UTF-16, null-terminated (voor PCWSTR).
fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Registreer `path` als sync-root. Met CF_REGISTER_FLAG_UPDATE is dit
/// herhaalbaar (bestaat er al één, dan wordt die bijgewerkt).
pub fn register(path: &str, provider: &str, version: &str) -> Result<()> {
    let wpath = wide(path);
    let wprovider = wide(provider);
    let wversion = wide(version);
    // Opaque sync-root-identiteit; het pad volstaat als unieke sleutel.
    let identity = wide(path);

    unsafe {
        let mut reg: CF_SYNC_REGISTRATION = zeroed();
        reg.StructSize = size_of::<CF_SYNC_REGISTRATION>() as u32;
        reg.ProviderName = PCWSTR(wprovider.as_ptr());
        reg.ProviderVersion = PCWSTR(wversion.as_ptr());
        reg.SyncRootIdentity = identity.as_ptr() as *const c_void;
        reg.SyncRootIdentityLength = (identity.len() * 2) as u32;

        // Zeroed policies = PARTIAL hydration + PARTIAL population = precies het
        // files-on-demand-gedrag dat we willen.
        let mut pol: CF_SYNC_POLICIES = zeroed();
        pol.StructSize = size_of::<CF_SYNC_POLICIES>() as u32;

        CfRegisterSyncRoot(PCWSTR(wpath.as_ptr()), &reg, &pol, CF_REGISTER_FLAG_UPDATE)
            .map_err(|e| anyhow!("CfRegisterSyncRoot({path}): {e}"))?;
    }
    Ok(())
}

/// Verwijder de sync-root-registratie weer.
pub fn unregister(path: &str) -> Result<()> {
    let wpath = wide(path);
    unsafe {
        CfUnregisterSyncRoot(PCWSTR(wpath.as_ptr()))
            .map_err(|e| anyhow!("CfUnregisterSyncRoot({path}): {e}"))?;
    }
    Ok(())
}

/// Maak één file-placeholder (online-only, wolk-icoon) onder `base_dir`.
/// `remote_id` is de opaque provider-identiteit (bv. het remote-pad) die we
/// bij hydration (Fase 2b) terugkrijgen om het juiste bestand te downloaden.
pub fn create_file_placeholder(base_dir: &str, name: &str, size: i64, remote_id: &str) -> Result<()> {
    let wbase = wide(base_dir);
    let wname = wide(name);
    let ident = wide(remote_id);

    unsafe {
        let mut basic: FILE_BASIC_INFO = zeroed();
        basic.FileAttributes = 0x80; // FILE_ATTRIBUTE_NORMAL

        let mut info: CF_PLACEHOLDER_CREATE_INFO = zeroed();
        info.RelativeFileName = PCWSTR(wname.as_ptr());
        info.FsMetadata.BasicInfo = basic;
        info.FsMetadata.FileSize = size;
        info.FileIdentity = ident.as_ptr() as *const c_void;
        info.FileIdentityLength = (ident.len() * 2) as u32;
        info.Flags = CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC;

        let mut arr = [info];
        let mut processed = 0u32;
        CfCreatePlaceholders(
            PCWSTR(wbase.as_ptr()),
            &mut arr,
            CF_CREATE_FLAG_NONE,
            Some(&mut processed as *mut u32),
        )
        .map_err(|e| anyhow!("CfCreatePlaceholders({name}): {e}"))?;

        if processed != 1 {
            return Err(anyhow!("placeholder {name} niet aangemaakt (result {:?})", arr[0].Result));
        }
    }
    Ok(())
}
