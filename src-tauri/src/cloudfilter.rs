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
use std::sync::OnceLock;
use windows::core::PCWSTR;
use windows::Win32::Foundation::NTSTATUS;
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

// ---- Fase 2b: hydration (files-on-demand downloaden bij openen) ----

/// Databron voor hydration: (remote-identiteit, offset, lengte) -> bytes.
type Hydrator = Box<dyn Fn(&str, i64, i64) -> Vec<u8> + Send + Sync>;
static HYDRATOR: OnceLock<Hydrator> = OnceLock::new();

/// Zet de globale hydrator. De CfAPI-callback is een kale `extern "system" fn`
/// die geen closure kan capturen, dus de databron leeft in een static.
pub fn set_hydrator<F>(f: F)
where
    F: Fn(&str, i64, i64) -> Vec<u8> + Send + Sync + 'static,
{
    let _ = HYDRATOR.set(Box::new(f));
}

/// Verbind een callback-tabel aan de sync-root zodat FETCH_DATA-verzoeken
/// (het openen van een placeholder) worden bediend. De teruggegeven key moet
/// bij afsluiten aan `disconnect` worden gegeven.
pub fn connect(path: &str) -> Result<CF_CONNECTION_KEY> {
    let wpath = wide(path);
    let table = [
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_FETCH_DATA,
            Callback: Some(fetch_cb),
        },
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_NONE,
            Callback: None,
        },
    ];
    unsafe {
        let key = CfConnectSyncRoot(
            PCWSTR(wpath.as_ptr()),
            table.as_ptr(),
            None,
            CF_CONNECT_FLAG_NONE,
        )
        .map_err(|e| anyhow!("CfConnectSyncRoot({path}): {e}"))?;
        Ok(key)
    }
}

pub fn disconnect(key: CF_CONNECTION_KEY) -> Result<()> {
    unsafe {
        CfDisconnectSyncRoot(key).map_err(|e| anyhow!("CfDisconnectSyncRoot: {e}"))?;
    }
    Ok(())
}

/// FETCH_DATA-callback: haal de data via de hydrator en schrijf 'm met
/// CfExecute(TRANSFER_DATA) in de placeholder. (Grote bestanden: nog per hele
/// gevraagde range in één keer — sector-chunking is een latere verfijning.)
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
    let offset = fetch.RequiredFileOffset;
    let length = fetch.RequiredLength;
    let data = HYDRATOR.get().map(|h| h(&id, offset, length)).unwrap_or_default();

    let mut opinfo: CF_OPERATION_INFO = zeroed();
    opinfo.StructSize = size_of::<CF_OPERATION_INFO>() as u32;
    opinfo.Type = CF_OPERATION_TYPE_TRANSFER_DATA;
    opinfo.ConnectionKey = info.ConnectionKey;
    opinfo.TransferKey = info.TransferKey;

    let mut op: CF_OPERATION_PARAMETERS = zeroed();
    op.ParamSize = size_of::<CF_OPERATION_PARAMETERS>() as u32;
    op.Anonymous.TransferData.CompletionStatus = NTSTATUS(0); // STATUS_SUCCESS
    op.Anonymous.TransferData.Buffer = data.as_ptr() as *const c_void;
    op.Anonymous.TransferData.Offset = offset;
    op.Anonymous.TransferData.Length = data.len() as i64;

    let _ = CfExecute(&opinfo, &mut op);
}
