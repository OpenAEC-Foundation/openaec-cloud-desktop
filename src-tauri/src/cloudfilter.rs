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
    StorageProviderInSyncPolicy, StorageProviderItemProperties, StorageProviderItemProperty,
    StorageProviderItemPropertyDefinition, StorageProviderPopulationPolicy, StorageProviderProtectionMode,
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
    markeer(root, pad, CF_IN_SYNC_STATE_IN_SYNC, true)
}

/// Zet een bestand of map op "niet in sync" (sync-pijlen), bijvoorbeeld na een fout; verder als [`markeer_in_sync`].
pub fn markeer_niet_in_sync(root: &Path, pad: &Path) -> Result<bool> {
    markeer(root, pad, CF_IN_SYNC_STATE_NOT_IN_SYNC, true)
}

/// Als [`markeer_in_sync`] en [`markeer_niet_in_sync`], voor de hoofdmap (bestanden op aanvraag): de pin-status is
/// daar van de gebruiker en blijft staan. Een nieuw bestand in een vastgezette map wordt zelf ook vastgezet.
pub fn markeer_op_aanvraag(root: &Path, pad: &Path, in_sync: bool) -> Result<bool> {
    let staat = if in_sync { CF_IN_SYNC_STATE_IN_SYNC } else { CF_IN_SYNC_STATE_NOT_IN_SYNC };
    markeer(root, pad, staat, false)
}

fn markeer(root: &Path, pad: &Path, staat: CF_IN_SYNC_STATE, vastzetten: bool) -> Result<bool> {
    if !telt_mee(root, pad) {
        return Ok(false);
    }
    let f = open(pad)?;
    let h = handle(&f);
    let in_sync = staat == CF_IN_SYNC_STATE_IN_SYNC;
    let voor = lees_placeholder(h)?;
    let klaar = match voor {
        Some(p) if vastzetten => p == Placeholder { in_sync, vastgezet: true, uitgesloten: false },
        Some(p) => p.in_sync == in_sync && !p.uitgesloten,
        None => false,
    };
    if klaar {
        return Ok(false);
    }
    if voor.is_none() {
        let vlag = if in_sync { CF_CONVERT_FLAG_MARK_IN_SYNC } else { CF_CONVERT_FLAG_NONE };
        unsafe { CfConvertToPlaceholder(h, None, 0, vlag, None, None) }
            .map_err(|e| anyhow!("CfConvertToPlaceholder({}): {e}", pad.display()))?;
        // Op aanvraag: een nieuw bestand erft "altijd op dit apparaat" van de map waarin het staat.
        let erft = !vastzetten && pad.parent().and_then(attributen).is_some_and(|a| a & ATTR_VASTGEZET != 0);
        if erft {
            zet_vast_handle(h, pad)?;
        }
    }
    if vastzetten && !voor.is_some_and(|p| p.vastgezet) {
        zet_vast_handle(h, pad)?;
    }
    if lees_placeholder(h)?.map(|p| p.in_sync) != Some(in_sync) {
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

// ---- Fase 3: de hoofdmap (één plek in het navigatiepaneel, bestanden op aanvraag) ----
//
// De hoofdmap (bijv. `%USERPROFILE%\OpenAEC`) is één sync-root met per ruimte een submap. Anders dan een losse
// gekoppelde map mogen bestanden hier "alleen online" staan: een placeholder met de metadata, die pas bij openen
// wordt opgehaald (FETCH_DATA-callback, zie [`verbind_hoofdmap`]). De Verkenner toont dan zelf "Altijd op dit
// apparaat behouden" en "Ruimte vrijmaken"; die zetten alleen de pin-status (attribuut), het ophalen en vrijmaken
// zelf doet de app ([`volg_pinstatus`]).
//
// Wat Windows nodig heeft voor een eigen item in het navigatiepaneel: een registratie via de shell
// (StorageProviderSyncRootManager::Register, niet alleen CfRegisterSyncRoot) met een id van de vorm
// `<provider>!<SID>!<account>`, een DisplayNameResource en een IconResource. Windows maakt dan zelf een
// naamruimte-item aan (HKCU\Software\Classes\CLSID\{…} met `System.IsPinnedToNamespaceTree` = 1 en een
// `TargetFolderPath`, en de sleutel onder Explorer\Desktop\NameSpace) en zet de CLSID als `NamespaceCLSID` in de
// registratie onder HKLM\…\Explorer\SyncRootManager. Bij afmelden ruimt Windows dat item weer op.

/// Attributen die de Cloud Files API op bestanden zet.
pub const ATTR_OFFLINE: u32 = 0x0000_1000;
pub const ATTR_VASTGEZET: u32 = 0x0008_0000;
pub const ATTR_LOSGEMAAKT: u32 = 0x0010_0000;
pub const ATTR_OPHALEN_BIJ_OPENEN: u32 = 0x0004_0000;
pub const ATTR_OPHALEN_BIJ_LEZEN: u32 = 0x0040_0000;

/// Eigen kolommen in de Verkenner (StorageProviderItemProperty): de status bij een fout of overgeslagen bestand, en
/// wie het bestand vergrendeld heeft.
pub const EIGENSCHAP_STATUS: i32 = 1;
pub const EIGENSCHAP_SLOT: i32 = 2;

/// Pictogrammen voor de eigen kolommen (uit Windows zelf, zodat ze overal hetzelfde ogen).
pub const ICOON_FOUT: &str = "%SystemRoot%\\System32\\imageres.dll,-98";
pub const ICOON_OVERGESLAGEN: &str = "%SystemRoot%\\System32\\imageres.dll,-81";
pub const ICOON_SLOT: &str = "%SystemRoot%\\System32\\imageres.dll,-59";

/// Is het bestand ontwaterd (alleen online)? Uit de attributen, zonder het bestand te openen of op te halen.
pub fn is_ontwaterd(attributen: u32) -> bool {
    attributen & (ATTR_OFFLINE | ATTR_OPHALEN_BIJ_LEZEN | ATTR_OPHALEN_BIJ_OPENEN) != 0
}

/// Attributen van een pad (zonder op te halen); None als het niet bestaat.
pub fn attributen(pad: &Path) -> Option<u32> {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(pad).ok().map(|m| m.file_attributes())
}

/// Registreer de hoofdmap als sync-root met bestanden op aanvraag. Herhaalbaar; een map die al onder een andere
/// sync-provider valt, wordt geweigerd. `naam` is wat het navigatiepaneel toont ("OpenAEC").
pub fn registreer_hoofdmap(path: &str, naam: &str, version: &str, icoon: &str) -> Result<()> {
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
    // FULL: openen haalt het hele bestand op voordat het programma verder gaat; ontwateren ("ruimte vrijmaken") mag,
    // ook door Windows zelf bij weinig schijfruimte (opslagbeheer) voor bestanden die niet vastgezet zijn.
    info.SetHydrationPolicy(StorageProviderHydrationPolicy::Full)?;
    info.SetHydrationPolicyModifier(StorageProviderHydrationPolicyModifier::AutoDehydrationAllowed)?;
    // ALWAYS_FULL: de app zet zelf alle placeholders neer, Windows hoeft geen mapinhoud op te vragen.
    info.SetPopulationPolicy(StorageProviderPopulationPolicy::AlwaysFull)?;
    info.SetInSyncPolicy(StorageProviderInSyncPolicy(CF_INSYNC_POLICY_TRACK_ALL.0))?;
    info.SetHardlinkPolicy(StorageProviderHardlinkPolicy::None)?;
    info.SetAllowPinning(true)?;
    info.SetShowSiblingsAsGroup(false)?;
    info.SetProtectionMode(StorageProviderProtectionMode::Unknown)?;
    let definities = info.StorageProviderItemPropertyDefinitions()?;
    for (id, kop) in [(EIGENSCHAP_STATUS, "Status van OpenAEC"), (EIGENSCHAP_SLOT, "Vergrendeld door")] {
        let d = StorageProviderItemPropertyDefinition::new()?;
        d.SetId(id)?;
        d.SetDisplayNameResource(&HSTRING::from(kop))?;
        definities.Append(&d)?;
    }
    StorageProviderSyncRootManager::Register(&info).map_err(|e| anyhow!("registreren van {path}: {e}"))
}

/// Meld de hoofdmap af (eerst [`zet_terug`]); het item in het navigatiepaneel verdwijnt dan ook.
pub fn afmelden_hoofdmap(path: &str) -> Result<()> {
    afmelden_volledig(path)
}

/// Ophaler voor de hoofdmap: (volledig pad, offset, lengte, schrijf) -> klaar of een fout. `schrijf` krijgt de bytes
/// op volgorde vanaf `offset` en geeft `false` als Windows het verzoek intussen heeft afgebroken.
pub type Ophaler = Box<dyn Fn(&Path, i64, i64, &mut dyn FnMut(&[u8]) -> bool) -> std::result::Result<(), String> + Send + Sync>;
static OPHALER: OnceLock<Ophaler> = OnceLock::new();

/// Zet de ophaler voor bestanden op aanvraag (één keer per proces).
pub fn zet_ophaler(f: Ophaler) {
    let _ = OPHALER.set(f);
}

/// Verbind de callbacks met de hoofdmap, zodat een bestand dat alleen online staat bij openen wordt opgehaald. De
/// sleutel moet bij afsluiten naar [`disconnect`].
pub fn verbind_hoofdmap(path: &str) -> Result<CF_CONNECTION_KEY> {
    let wpath = wide(path);
    let table = [
        CF_CALLBACK_REGISTRATION { Type: CF_CALLBACK_TYPE_FETCH_DATA, Callback: Some(ophaal_cb) },
        CF_CALLBACK_REGISTRATION { Type: CF_CALLBACK_TYPE_NONE, Callback: None },
    ];
    unsafe {
        CfConnectSyncRoot(PCWSTR(wpath.as_ptr()), table.as_ptr(), None, CF_CONNECT_FLAG_REQUIRE_FULL_FILE_PATH)
            .map_err(|e| anyhow!("CfConnectSyncRoot({path}): {e}"))
    }
}

/// Zoveel bytes per TRANSFER_DATA (een veelvoud van 4 KB; alleen het laatste stuk mag korter).
const STUK: usize = 1024 * 1024;

fn breng_over(info: &CF_CALLBACK_INFO, data: &[u8], offset: i64, lengte: i64, status: NTSTATUS) -> bool {
    unsafe {
        let mut opinfo: CF_OPERATION_INFO = zeroed();
        opinfo.StructSize = size_of::<CF_OPERATION_INFO>() as u32;
        opinfo.Type = CF_OPERATION_TYPE_TRANSFER_DATA;
        opinfo.ConnectionKey = info.ConnectionKey;
        opinfo.TransferKey = info.TransferKey;
        opinfo.RequestKey = info.RequestKey;
        let mut op: CF_OPERATION_PARAMETERS = zeroed();
        op.ParamSize = size_of::<CF_OPERATION_PARAMETERS>() as u32;
        op.Anonymous.TransferData.CompletionStatus = status;
        op.Anonymous.TransferData.Buffer = if data.is_empty() { std::ptr::null() } else { data.as_ptr() as *const c_void };
        op.Anonymous.TransferData.Offset = offset;
        op.Anonymous.TransferData.Length = lengte;
        CfExecute(&opinfo, &mut op).is_ok()
    }
}

/// FETCH_DATA voor de hoofdmap: haal het gevraagde bereik op via de ophaler en geef het in stukken van 1 MB door
/// (Windows toont daarmee de voortgang). Lukt het niet, dan krijgt het programma dat het bestand opende een fout.
unsafe extern "system" fn ophaal_cb(info: *const CF_CALLBACK_INFO, params: *const CF_CALLBACK_PARAMETERS) {
    let info = &*info;
    let fetch = (*params).Anonymous.FetchData;
    let (begin, lengte) = (fetch.RequiredFileOffset, fetch.RequiredLength);
    // Met CF_CONNECT_FLAG_REQUIRE_FULL_FILE_PATH is NormalizedPath het volledige pad met stationsletter.
    let pad = std::path::PathBuf::from(info.NormalizedPath.to_string().unwrap_or_default());
    let mut volgende = begin;
    let mut buffer: Vec<u8> = Vec::with_capacity(STUK);
    let mut gelukt = true;
    let uitkomst = match OPHALER.get() {
        None => Err("de app is niet klaar om bestanden op te halen".to_string()),
        Some(ophaler) => ophaler(&pad, begin, lengte, &mut |stuk: &[u8]| {
            buffer.extend_from_slice(stuk);
            while buffer.len() >= STUK && gelukt {
                gelukt = breng_over(info, &buffer[..STUK], volgende, STUK as i64, NTSTATUS(0));
                volgende += STUK as i64;
                buffer.drain(..STUK);
            }
            gelukt
        }),
    };
    let rest = begin + lengte - volgende;
    match uitkomst {
        Ok(()) if gelukt && buffer.len() as i64 == rest => {
            if !buffer.is_empty() {
                breng_over(info, &buffer, volgende, buffer.len() as i64, NTSTATUS(0));
            }
        }
        _ => {
            match &uitkomst {
                Err(e) => log::warn!("ophalen van {} mislukt: {e}", pad.display()),
                Ok(()) if gelukt => log::warn!("ophalen van {}: {} bytes ontvangen, {rest} verwacht", pad.display(), buffer.len()),
                Ok(()) => log::info!("ophalen van {} afgebroken", pad.display()),
            }
            // STATUS_UNSUCCESSFUL voor de rest van het bereik.
            breng_over(info, &[], volgende, rest.max(0), NTSTATUS(0xC000_0001u32 as i32));
        }
    }
}

/// FILETIME (100 ns sinds 1601) voor nu.
fn nu_filetime() -> i64 {
    let sinds_1970 = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (sinds_1970.as_nanos() / 100) as i64 + 116_444_736_000_000_000
}

/// Maak een bestand dat alleen online staat (placeholder, in sync, wolkje) op `pad`, met de grootte van de versie op
/// de server en als bestandsidentiteit het bestand-id op de server. De map erboven moet bestaan.
pub fn maak_alleen_online(pad: &Path, grootte: u64, bestand_id: &str) -> Result<()> {
    let map = pad.parent().ok_or_else(|| anyhow!("{} heeft geen map", pad.display()))?;
    let naam = pad.file_name().ok_or_else(|| anyhow!("{} heeft geen naam", pad.display()))?.to_string_lossy().to_string();
    let wmap = wide(&map.to_string_lossy());
    let wnaam = wide(&naam);
    let ident = wide(bestand_id);
    let nu = nu_filetime();
    unsafe {
        let mut info: CF_PLACEHOLDER_CREATE_INFO = zeroed();
        info.RelativeFileName = PCWSTR(wnaam.as_ptr());
        info.FsMetadata.BasicInfo.FileAttributes = 0x80; // FILE_ATTRIBUTE_NORMAL
        info.FsMetadata.BasicInfo.CreationTime = nu;
        info.FsMetadata.BasicInfo.LastWriteTime = nu;
        info.FsMetadata.BasicInfo.ChangeTime = nu;
        info.FsMetadata.BasicInfo.LastAccessTime = nu;
        info.FsMetadata.FileSize = grootte as i64;
        info.FileIdentity = ident.as_ptr() as *const c_void;
        info.FileIdentityLength = (ident.len() * 2) as u32;
        info.Flags = CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC | CF_PLACEHOLDER_CREATE_FLAG_SUPERSEDE;
        let mut arr = [info];
        let mut verwerkt = 0u32;
        CfCreatePlaceholders(PCWSTR(wmap.as_ptr()), &mut arr, CF_CREATE_FLAG_NONE, Some(&mut verwerkt))
            .map_err(|e| anyhow!("CfCreatePlaceholders({}): {e}", pad.display()))?;
        if verwerkt != 1 || arr[0].Result.is_err() {
            return Err(anyhow!("placeholder {} niet aangemaakt ({:?})", pad.display(), arr[0].Result));
        }
    }
    Ok(())
}

/// Werk een bestand dat alleen online staat bij naar een nieuwe versie op de server: nieuwe grootte en
/// wijzigingstijd, blijft alleen online en in sync. Er wordt niets gedownload.
pub fn werk_alleen_online_bij(pad: &Path, grootte: u64, bestand_id: &str) -> Result<()> {
    let f = open(pad)?;
    let ident = wide(bestand_id);
    let nu = nu_filetime();
    let mut meta: CF_FS_METADATA = unsafe { zeroed() };
    meta.BasicInfo.LastWriteTime = nu;
    meta.BasicInfo.ChangeTime = nu;
    meta.FileSize = grootte as i64;
    unsafe {
        CfUpdatePlaceholder(
            handle(&f),
            Some(&meta),
            Some(ident.as_ptr() as *const c_void),
            (ident.len() * 2) as u32,
            None,
            CF_UPDATE_FLAG_DEHYDRATE | CF_UPDATE_FLAG_MARK_IN_SYNC,
            None,
            None,
        )
    }
    .map_err(|e| anyhow!("CfUpdatePlaceholder({}): {e}", pad.display()))
}

/// Bestandsidentiteit van een placeholder (het bestand-id op de server, zie [`maak_alleen_online`]).
pub fn bestandsidentiteit(pad: &Path) -> Option<String> {
    let f = open(pad).ok()?;
    let mut buf = [0u64; (size_of::<CF_PLACEHOLDER_BASIC_INFO>() + 4096) / 8 + 1];
    unsafe {
        let r = CfGetPlaceholderInfo(handle(&f), CF_PLACEHOLDER_INFO_BASIC, buf.as_mut_ptr() as *mut c_void, std::mem::size_of_val(&buf) as u32, None);
        if r.is_err() {
            return None;
        }
        let info = &*(buf.as_ptr() as *const CF_PLACEHOLDER_BASIC_INFO);
        let lengte = (info.FileIdentityLength / 2) as usize;
        if lengte == 0 {
            return None;
        }
        let begin = std::ptr::addr_of!(info.FileIdentity) as *const u16;
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(begin, lengte));
        Some(s.trim_end_matches('\0').to_string()).filter(|s| !s.is_empty())
    }
}

/// Haal een bestand dat alleen online staat helemaal op (bijv. na "Altijd op dit apparaat behouden").
pub fn hydrateer(pad: &Path) -> Result<()> {
    let f = open(pad)?;
    unsafe { CfHydratePlaceholder(handle(&f), 0, -1, CF_HYDRATE_FLAG_NONE, None) }
        .map_err(|e| anyhow!("CfHydratePlaceholder({}): {e}", pad.display()))
}

/// Maak ruimte vrij: de inhoud gaat weg, het bestand blijft als placeholder (alleen online). Weigert Windows als het
/// bestand niet in sync is: dan staat er werk dat nog niet op de server staat.
pub fn ontwater(pad: &Path) -> Result<()> {
    let f = open(pad)?;
    unsafe { CfDehydratePlaceholder(handle(&f), 0, -1, CF_DEHYDRATE_FLAG_NONE, None) }
        .map_err(|e| anyhow!("CfDehydratePlaceholder({}): {e}", pad.display()))
}

/// Wat [`volg_pinstatus`] deed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pinactie {
    Niets,
    Opgehaald,
    Vrijgemaakt,
}

/// Volg de pin-status die de Verkenner zet: vastgezet ("Altijd op dit apparaat behouden") en alleen online -> ophalen;
/// losgemaakt ("Ruimte vrijmaken") en lokaal -> vrijmaken. Alleen voor bestanden.
pub fn volg_pinstatus(pad: &Path) -> Result<Pinactie> {
    let Some(a) = attributen(pad) else { return Ok(Pinactie::Niets) };
    if a & 0x10 != 0 {
        return Ok(Pinactie::Niets); // een map: de bestanden erin krijgen elk hun eigen gebeurtenis
    }
    if a & ATTR_VASTGEZET != 0 && is_ontwaterd(a) {
        hydrateer(pad)?;
        return Ok(Pinactie::Opgehaald);
    }
    if a & ATTR_LOSGEMAAKT != 0 && !is_ontwaterd(a) && placeholder_status(pad)?.is_some_and(|p| p.in_sync) {
        ontwater(pad)?;
        return Ok(Pinactie::Vrijgemaakt);
    }
    Ok(Pinactie::Niets)
}

/// Pin-status van een map of bestand zetten (bij een map ook voor alles eronder).
pub fn zet_pin(pad: &Path, vast: Option<bool>) -> Result<()> {
    let f = open(pad)?;
    let staat = match vast {
        Some(true) => CF_PIN_STATE_PINNED,
        Some(false) => CF_PIN_STATE_UNPINNED,
        None => CF_PIN_STATE_UNSPECIFIED,
    };
    let vlag = if pad.is_dir() { CF_SET_PIN_FLAG_RECURSE } else { CF_SET_PIN_FLAG_NONE };
    unsafe { CfSetPinState(handle(&f), staat, vlag, None) }.map_err(|e| anyhow!("CfSetPinState({}): {e}", pad.display()))
}

/// Eigen kolommen op een bestand of map zetten (leeg: weghalen). Per eigenschap (id, waarde, pictogram).
pub fn zet_eigenschappen(pad: &Path, eigenschappen: &[(i32, String, &str)]) -> Result<()> {
    use windows::core::Interface;
    let pad_h = HSTRING::from(pad.as_os_str());
    let item: windows::Storage::IStorageItem = if pad.is_dir() {
        StorageFolder::GetFolderFromPathAsync(&pad_h)?.get()?.cast()?
    } else {
        windows::Storage::StorageFile::GetFileFromPathAsync(&pad_h)?.get()?.cast()?
    };
    let mut lijst = Vec::new();
    for (id, waarde, icoon) in eigenschappen {
        let e = StorageProviderItemProperty::new()?;
        e.SetId(*id)?;
        e.SetValue(&HSTRING::from(waarde.as_str()))?;
        e.SetIconResource(&HSTRING::from(*icoon))?;
        lijst.push(Some(e));
    }
    let iterabel: windows_collections::IIterable<StorageProviderItemProperty> = lijst.into();
    StorageProviderItemProperties::SetAsync(&item, &iterabel)?.get().map_err(|e| anyhow!("eigenschappen van {}: {e}", pad.display()))
}
