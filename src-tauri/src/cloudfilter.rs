// Windows Cloud Filter API (cldapi) — Fase 2a.
//
// Registreert een lokale map als "sync root" zodat Verkenner cloud-status-
// overlays toont (wolk / groen vinkje / sync-pijlen) en maakt placeholders:
// bestanden die als metadata bestaan maar pas bij openen worden gedownload
// (files-on-demand). Hydration (het downloaden bij openen)
// is Fase 2b; in-sync-states + pin/unpin Fase 2c (statusvinkjes voor
// gekoppelde mappen met volledige bestanden, onderaan dit bestand).
#![cfg(windows)]

use anyhow::{anyhow, Result};
use std::ffi::c_void;
use std::fs::File;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::OnceLock;
use openaec_sync::bewaking::telt_mee;
use openaec_sync::motor::TOESTANDSMAP;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Storage::Provider::{
    StorageProviderHardlinkPolicy, StorageProviderHydrationPolicy, StorageProviderHydrationPolicyModifier,
    StorageProviderInSyncPolicy, StorageProviderPopulationPolicy, StorageProviderProtectionMode,
    StorageProviderSyncRootInfo, StorageProviderSyncRootManager,
};
use windows::Storage::StorageFolder;
use windows::Win32::Foundation::{CloseHandle, LocalFree, ERROR_MORE_DATA, ERROR_NOT_A_CLOUD_FILE, HANDLE, HLOCAL, NTSTATUS};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::CloudFilters::*;
use windows::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_DATA,
    WRITE_DAC,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

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

// ---- Fase 2c: statusvinkjes voor gekoppelde mappen (volledige bestanden) ----
//
// Een gekoppelde map bevat gewone, volledige bestanden die de sync-motor
// bijhoudt. Als sync-root toont de Verkenner per bestand en map de status:
// groen vinkje = in sync, sync-pijlen = (nog) niet in sync. Wat de motor
// gesynchroniseerd heeft, wordt een placeholder met de inhoud er gewoon in.
// Een verbinding (callbacks) is niet nodig: er wordt nooit iets ontwaterd,
// dus er valt ook niets op te halen. Registreren gaat via de shell, zodat de
// Verkenner de status ook echt toont.

/// Provider-deel van de shell-registratie (id `OpenAECStorage!<SID>!<map>`);
/// CfAPI geeft het terug als providernaam van de sync-root.
pub const PROVIDER_ID: &str = "OpenAECStorage";

/// Onder wiens sync-root valt een map?
enum SyncRoot {
    Geen,
    /// De map is zelf een sync-root van ons voor volledige bestanden.
    Eigen,
    /// De map valt onder de sync-root van een andere provider (of onder een
    /// andere sync-root van ons, bijvoorbeeld een cloud-map).
    Vreemd(String),
}

/// UTF-16 tot de eerste nul.
fn tekst(w: &[u16]) -> String {
    let eind = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..eind])
}

/// Bestandsnummer (NTFS) van een bestand of map.
fn bestand_id(pad: &Path) -> Option<i64> {
    let f = open(pad).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle(&f), &mut info) }.ok()?;
    Some((((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64) as i64)
}

fn sync_root(path: &str) -> SyncRoot {
    // CF_SYNC_ROOT_STANDARD_INFO plus ruimte voor de identiteit; u64's voor de uitlijning.
    let mut buf = vec![0u64; (size_of::<CF_SYNC_ROOT_STANDARD_INFO>() + 4096) / 8];
    let wpath = wide(path);
    let r = unsafe {
        CfGetSyncRootInfoByPath(
            PCWSTR(wpath.as_ptr()),
            CF_SYNC_ROOT_INFO_STANDARD,
            buf.as_mut_ptr() as *mut c_void,
            (buf.len() * 8) as u32,
            None,
        )
    };
    match r {
        Ok(()) => {}
        // Onze sync-roots hebben geen identiteit; een van meer dan 4 KB is niet van ons.
        Err(e) if e.code() == ERROR_MORE_DATA.to_hresult() => return SyncRoot::Vreemd("onbekend".into()),
        Err(_) => return SyncRoot::Geen,
    }
    let info = unsafe { &*(buf.as_ptr() as *const CF_SYNC_ROOT_STANDARD_INFO) };
    let naam = tekst(&info.ProviderName);
    if naam == PROVIDER_ID && bestand_id(Path::new(path)) == Some(info.SyncRootFileId) {
        SyncRoot::Eigen
    } else {
        SyncRoot::Vreemd(naam)
    }
}

/// SID van de huidige gebruiker als tekst (S-1-5-21-…).
fn sid() -> Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let mut lengte = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut lengte);
        let mut buf = vec![0u64; (lengte as usize).div_ceil(8)];
        let r = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), lengte, &mut lengte);
        let _ = CloseHandle(token);
        r?;
        let gebruiker = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut tekst = PWSTR::null();
        ConvertSidToStringSidW(gebruiker.User.Sid, &mut tekst)?;
        let uit = tekst.to_string();
        let _ = LocalFree(Some(HLOCAL(tekst.0 as *mut c_void)));
        Ok(uit?)
    }
}

/// Id van de shell-registratie: provider, SID en een vaste sleutel per map
/// (FNV-1a over het pad, zonder hoofdletters en afsluitend scheidingsteken).
fn registratie_id(path: &str) -> Result<HSTRING> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.trim_end_matches(['\\', '/']).to_lowercase().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    Ok(HSTRING::from(format!("{PROVIDER_ID}!{}!{h:016x}", sid()?)))
}

/// Registreer `path` als sync-root voor volledige bestanden (een gekoppelde
/// map). Via de shell (StorageProviderSyncRootManager), niet alleen via
/// CfRegisterSyncRoot: anders ziet de Verkenner een gewone lokale map en toont
/// hij geen status. Met in-sync-tracking: wijzigt de wijzigingstijd, de grootte
/// of een attribuut, dan zet Windows het bestand zelf op "niet in sync"
/// (schrijven midden in een bestand niet; dat doet de bestandswachter van de
/// app). Herhaalbaar; een map die al onder een andere sync-provider valt, wordt
/// geweigerd.
pub fn registreer_volledig(path: &str, naam: &str, version: &str, icoon: &str) -> Result<()> {
    // Zonder naam of pictogram crasht de registratie in Windows zelf.
    if naam.is_empty() || icoon.is_empty() {
        return Err(anyhow!("registreren van {path}: naam en pictogram zijn verplicht"));
    }
    match sync_root(path) {
        SyncRoot::Eigen => return Ok(()),
        SyncRoot::Vreemd(provider) => return Err(anyhow!("{path} valt al onder een andere sync-provider ({provider})")),
        SyncRoot::Geen => {}
    }
    let map = StorageFolder::GetFolderFromPathAsync(&HSTRING::from(path))?.get()?;
    let info = StorageProviderSyncRootInfo::new()?;
    info.SetId(&registratie_id(path)?)?;
    info.SetPath(&map)?;
    info.SetDisplayNameResource(&HSTRING::from(naam))?;
    info.SetIconResource(&HSTRING::from(icoon))?;
    info.SetVersion(&HSTRING::from(version))?;
    // ALWAYS_FULL in plaats van FULL: dan weigert Windows ook het ontwateren
    // ("ruimte vrijmaken"). Voor deze mappen draait geen hydratie-callback, dus
    // een ontwaterd bestand zou niet meer te openen zijn.
    info.SetHydrationPolicy(StorageProviderHydrationPolicy::AlwaysFull)?;
    info.SetHydrationPolicyModifier(StorageProviderHydrationPolicyModifier::None)?;
    info.SetPopulationPolicy(StorageProviderPopulationPolicy::AlwaysFull)?;
    info.SetInSyncPolicy(StorageProviderInSyncPolicy(CF_INSYNC_POLICY_TRACK_ALL.0))?;
    info.SetHardlinkPolicy(StorageProviderHardlinkPolicy::None)?;
    info.SetAllowPinning(true)?;
    info.SetShowSiblingsAsGroup(false)?;
    info.SetProtectionMode(StorageProviderProtectionMode::Unknown)?;
    StorageProviderSyncRootManager::Register(&info).map_err(|e| anyhow!("registreren van {path}: {e}"))
}

/// Meld een gekoppelde map af (shell en filter); eerst [`zet_terug`].
pub fn afmelden_volledig(path: &str) -> Result<()> {
    StorageProviderSyncRootManager::Unregister(&registratie_id(path)?).map_err(|e| anyhow!("afmelden van {path}: {e}"))
}

/// Is `path` zelf een sync-root van ons voor volledige bestanden? Alleen dan
/// mogen we bij ontkoppelen terugzetten en afmelden.
pub fn eigen_sync_root(path: &str) -> bool {
    matches!(sync_root(path), SyncRoot::Eigen)
}

/// Placeholderstatus van een bestand of map in een sync-root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placeholder {
    pub in_sync: bool,
    /// Vastgezet: altijd op dit apparaat houden.
    pub vastgezet: bool,
    /// Uitgesloten van synchronisatie (de toestandsmap van de motor).
    pub uitgesloten: bool,
}

/// Open een bestand of map voor de CfAPI-functies hieronder. WRITE_DAC
/// volstaat voor allemaal en telt niet mee voor de deelmodus, dus het lukt ook
/// als een programma het bestand open heeft; anders lezen + schrijven.
fn open(pad: &Path) -> Result<File> {
    let delen = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    let mut fout = None;
    for toegang in [WRITE_DAC | FILE_READ_ATTRIBUTES, FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES] {
        let poging = std::fs::OpenOptions::new()
            .access_mode(toegang.0)
            .share_mode(delen.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0) // ook mappen
            .open(pad);
        match poging {
            Ok(f) => return Ok(f),
            Err(e) => fout = Some(e),
        }
    }
    Err(anyhow!("{} openen: {}", pad.display(), fout.unwrap()))
}

fn handle(f: &File) -> HANDLE {
    HANDLE(f.as_raw_handle())
}

fn lees_placeholder(h: HANDLE) -> Result<Option<Placeholder>> {
    // CF_PLACEHOLDER_BASIC_INFO plus ruimte voor een bestandsidentiteit (max. 4 KB).
    let mut buf = [0u64; (size_of::<CF_PLACEHOLDER_BASIC_INFO>() + 4096) / 8 + 1];
    let r = unsafe {
        CfGetPlaceholderInfo(
            h,
            CF_PLACEHOLDER_INFO_BASIC,
            buf.as_mut_ptr() as *mut c_void,
            std::mem::size_of_val(&buf) as u32,
            None,
        )
    };
    match r {
        Ok(()) => {}
        Err(e) if e.code() == ERROR_MORE_DATA.to_hresult() => {}
        Err(e) if e.code() == ERROR_NOT_A_CLOUD_FILE.to_hresult() => return Ok(None),
        Err(e) => return Err(anyhow!("CfGetPlaceholderInfo: {e}")),
    }
    let info = unsafe { &*(buf.as_ptr() as *const CF_PLACEHOLDER_BASIC_INFO) };
    Ok(Some(Placeholder {
        in_sync: info.InSyncState == CF_IN_SYNC_STATE_IN_SYNC,
        vastgezet: info.PinState == CF_PIN_STATE_PINNED,
        uitgesloten: info.PinState == CF_PIN_STATE_EXCLUDED,
    }))
}

/// Placeholderstatus van een bestand of map; None als het (nog) geen placeholder is.
pub fn placeholder_status(pad: &Path) -> Result<Option<Placeholder>> {
    let f = open(pad)?;
    lees_placeholder(handle(&f))
}

/// Zet een bestand of map in de gekoppelde map `root` op "in sync" (groen
/// vinkje). Is het nog geen placeholder, dan wordt het er een
/// (CfConvertToPlaceholder; de inhoud blijft gewoon lokaal), anders volstaat
/// CfSetInSyncState. Daarbij vastgezet, zodat het altijd lokaal blijft. De
/// toestandsmap van de motor en tijdelijke downloads worden overgeslagen.
/// Geeft terug of er iets veranderd is.
pub fn markeer_in_sync(root: &Path, pad: &Path) -> Result<bool> {
    markeer(root, pad, CF_IN_SYNC_STATE_IN_SYNC)
}

/// Zet een bestand of map op "niet in sync" (sync-pijlen), bijvoorbeeld na een
/// fout; verder als [`markeer_in_sync`].
pub fn markeer_niet_in_sync(root: &Path, pad: &Path) -> Result<bool> {
    markeer(root, pad, CF_IN_SYNC_STATE_NOT_IN_SYNC)
}

fn markeer(root: &Path, pad: &Path, staat: CF_IN_SYNC_STATE) -> Result<bool> {
    if !telt_mee(root, pad) {
        return Ok(false);
    }
    let f = open(pad)?;
    let h = handle(&f);
    let gewenst = Placeholder { in_sync: staat == CF_IN_SYNC_STATE_IN_SYNC, vastgezet: true, uitgesloten: false };
    let voor = lees_placeholder(h)?;
    if voor == Some(gewenst) {
        return Ok(false);
    }
    if voor.is_none() {
        let vlag = if gewenst.in_sync { CF_CONVERT_FLAG_MARK_IN_SYNC } else { CF_CONVERT_FLAG_NONE };
        unsafe { CfConvertToPlaceholder(h, None, 0, vlag, None, None) }
            .map_err(|e| anyhow!("CfConvertToPlaceholder({}): {e}", pad.display()))?;
    }
    if !voor.is_some_and(|p| p.vastgezet) {
        zet_vast_handle(h, pad)?;
    }
    if lees_placeholder(h)?.map(|p| p.in_sync) != Some(gewenst.in_sync) {
        unsafe { CfSetInSyncState(h, staat, CF_SET_IN_SYNC_FLAG_NONE, None) }
            .map_err(|e| anyhow!("CfSetInSyncState({}): {e}", pad.display()))?;
    }
    Ok(true)
}

/// Markeer de toestandsmap van de motor (`.openaec`) als uitgesloten van
/// synchronisatie. Anders toont de Verkenner hem (als gewone map in de
/// sync-root) altijd als "wacht op upload"; wat erin komt, erft de uitsluiting.
/// Geeft terug of er iets veranderd is.
pub fn sluit_toestandsmap_uit(root: &Path) -> Result<bool> {
    let pad = root.join(TOESTANDSMAP);
    if !pad.is_dir() {
        return Ok(false);
    }
    let f = open(&pad)?;
    let h = handle(&f);
    let voor = lees_placeholder(h)?;
    if voor.is_some_and(|p| p.uitgesloten) {
        return Ok(false);
    }
    if voor.is_none() {
        unsafe { CfConvertToPlaceholder(h, None, 0, CF_CONVERT_FLAG_MARK_IN_SYNC, None, None) }
            .map_err(|e| anyhow!("CfConvertToPlaceholder({}): {e}", pad.display()))?;
    }
    unsafe { CfSetPinState(h, CF_PIN_STATE_EXCLUDED, CF_SET_PIN_FLAG_NONE, None) }
        .map_err(|e| anyhow!("CfSetPinState({}): {e}", pad.display()))?;
    Ok(true)
}

/// Zet een placeholder vast (CF_PIN_STATE_PINNED): altijd lokaal, "ruimte
/// vrijmaken" kan niet.
pub fn zet_vast(pad: &Path) -> Result<()> {
    let f = open(pad)?;
    zet_vast_handle(handle(&f), pad)
}

fn zet_vast_handle(h: HANDLE, pad: &Path) -> Result<()> {
    unsafe { CfSetPinState(h, CF_PIN_STATE_PINNED, CF_SET_PIN_FLAG_NONE, None) }
        .map_err(|e| anyhow!("CfSetPinState({}): {e}", pad.display()))
}

/// Zet een placeholder terug naar een gewoon bestand of gewone map; geeft
/// terug of het een placeholder was. Eerst losmaken: CfRevertPlaceholder laat
/// het vastzet-attribuut anders staan. De sync-root zelf wordt alleen
/// losgemaakt; die ruimt het afmelden op.
fn zet_terug_een(pad: &Path, is_root: bool) -> Result<bool> {
    let f = open(pad)?;
    let h = handle(&f);
    if lees_placeholder(h)?.is_none() {
        return Ok(false);
    }
    unsafe { CfSetPinState(h, CF_PIN_STATE_UNSPECIFIED, CF_SET_PIN_FLAG_NONE, None) }
        .map_err(|e| anyhow!("CfSetPinState({}): {e}", pad.display()))?;
    if !is_root {
        unsafe { CfRevertPlaceholder(h, CF_REVERT_FLAG_NONE, None) }
            .map_err(|e| anyhow!("CfRevertPlaceholder({}): {e}", pad.display()))?;
    }
    Ok(true)
}

/// Zet alle placeholders onder `root` terug naar gewone bestanden en mappen
/// (vóór het afmelden bij ontkoppelen); de inhoud blijft staan. Geeft het
/// aantal teruggezette bestanden en mappen.
pub fn zet_terug(root: &Path) -> Result<u32> {
    let mut aantal = 0u32;
    let mut fouten: Vec<String> = Vec::new();
    let mut te_doen = vec![root.to_path_buf()];
    while let Some(map) = te_doen.pop() {
        let mut paden = Vec::new();
        if let Ok(items) = std::fs::read_dir(&map) {
            for item in items.flatten() {
                match item.file_type() {
                    Ok(t) if t.is_dir() => te_doen.push(item.path()),
                    Ok(t) if t.is_file() => paden.push(item.path()),
                    _ => {}
                }
            }
        }
        paden.push(map);
        for pad in paden {
            match zet_terug_een(&pad, pad == root) {
                Ok(true) => aantal += 1,
                Ok(false) => {}
                Err(e) => fouten.push(e.to_string()),
            }
        }
    }
    match fouten.first() {
        None => Ok(aantal),
        Some(eerste) => Err(anyhow!("{} niet teruggezet ({aantal} wel), o.a. {eerste}", fouten.len())),
    }
}
