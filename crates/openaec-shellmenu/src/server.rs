//! De COM-server die de Verkenner laadt: een `IExplorerCommand` "OpenAEC" met de subitems "Delen…" en "Versturen met
//! OpenAEC" (via `IEnumExplorerCommand`), een `IClassFactory`, en de exports `DllGetClassObject` en `DllCanUnloadNow`.
//!
//! Alles draait in het proces van de Verkenner. Daarom: geen netwerk (alleen `menu.json` lezen), geen paniek over de
//! grens (elke methode vangt die op en geeft E_FAIL) en een opgeruimde teller, zodat de Verkenner de DLL kan
//! ontladen.

use crate::logica::{self, Menu, Opdracht};
use std::cell::Cell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_NOTIMPL, E_POINTER, HINSTANCE, HMODULE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::Win32::UI::Shell::{
    IEnumExplorerCommand, IEnumExplorerCommand_Impl, IExplorerCommand, IExplorerCommand_Impl, IShellItemArray, SHStrDupW, ECF_DEFAULT,
    ECF_HASSUBCOMMANDS, ECS_ENABLED, ECS_HIDDEN, SIGDN_FILESYSPATH,
};
use windows_core::{implement, IUnknown, Interface, Ref, Result, BOOL, GUID, HRESULT, HSTRING, PWSTR};

/// Levende objecten en `LockServer`-vergrendelingen: zolang een van beide niet nul is, blijft de DLL geladen.
static OBJECTEN: AtomicUsize = AtomicUsize::new(0);
static VERGRENDELD: AtomicUsize = AtomicUsize::new(0);
/// De module van deze DLL (voor het pad van de app als `menu.json` er nog niet is).
static MODULE: AtomicUsize = AtomicUsize::new(0);

/// Telt een levend object (zie `DllCanUnloadNow`).
struct Teller;

impl Teller {
    fn nieuw() -> Teller {
        OBJECTEN.fetch_add(1, Ordering::SeqCst);
        Teller
    }
}

impl Drop for Teller {
    fn drop(&mut self) {
        OBJECTEN.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Een paniek mag niet in de Verkenner terechtkomen.
fn veilig<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| Err(E_FAIL.into()))
}

/// Tekst voor de Verkenner (die geeft hem zelf weer vrij met CoTaskMemFree).
fn tekst(s: &str) -> Result<PWSTR> {
    unsafe { SHStrDupW(&HSTRING::from(s)) }
}

/// De paden van de selectie. Zit er iets tussen zonder pad op schijf (een printer, een zip-onderdeel), dan geen
/// paden: dan is het menu er niet.
fn paden(selectie: &Ref<'_, IShellItemArray>) -> Vec<String> {
    let Some(selectie) = selectie.as_ref() else { return Vec::new() };
    let mut uit = Vec::new();
    let aantal = unsafe { selectie.GetCount() }.unwrap_or(0);
    for i in 0..aantal {
        let pad = unsafe { selectie.GetItemAt(i).and_then(|item| item.GetDisplayName(SIGDN_FILESYSPATH)) };
        match pad {
            Ok(p) => {
                let s = unsafe { p.to_string() }.unwrap_or_default();
                unsafe { CoTaskMemFree(Some(p.0 as *const c_void)) };
                if s.is_empty() {
                    return Vec::new();
                }
                uit.push(s);
            }
            Err(_) => return Vec::new(),
        }
    }
    uit
}

fn menu() -> Menu {
    logica::menu_pad().map(|p| Menu::laad(&p)).unwrap_or_default()
}

/// Het pad van deze DLL.
fn dll_pad() -> Option<PathBuf> {
    let module = MODULE.load(Ordering::SeqCst);
    if module == 0 {
        return None;
    }
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(Some(HMODULE(module as *mut c_void)), &mut buf) } as usize;
    (n > 0 && n < buf.len()).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..n])))
}

/// De app: uit `menu.json`, of anders `openaec-cloud.exe` boven de map van de DLL (`<installatiemap>\shellmenu\<arch>\`).
fn programma(menu: &Menu) -> Option<PathBuf> {
    if let Some(exe) = menu.exe.as_deref().filter(|e| !e.is_empty()) {
        return Some(PathBuf::from(exe));
    }
    let dll = dll_pad()?;
    dll.ancestors().skip(1).take(3).map(|m| m.join("openaec-cloud.exe")).find(|p| p.is_file())
}

fn start(exe: &Path, opdracht: Opdracht, paden: &[String]) -> Result<()> {
    use std::process::{Command, Stdio};
    Command::new(exe)
        .args(logica::argumenten(opdracht, paden))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| E_FAIL.into())
}

/// Het hoofditem "OpenAEC" met de subitems.
#[implement(IExplorerCommand)]
struct Hoofditem {
    _teller: Teller,
}

impl IExplorerCommand_Impl for Hoofditem_Impl {
    fn GetTitle(&self, _selectie: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        tekst(logica::LABEL_HOOFDITEM)
    }

    fn GetIcon(&self, _selectie: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        veilig(|| {
            let exe = programma(&menu()).ok_or(windows_core::Error::from(E_NOTIMPL))?;
            tekst(&format!("{},0", exe.display()))
        })
    }

    fn GetToolTip(&self, _selectie: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> Result<GUID> {
        GUID::try_from(logica::NAAM_HOOFDITEM).map_err(|_| E_FAIL.into())
    }

    fn GetState(&self, selectie: Ref<'_, IShellItemArray>, _traag_mag: BOOL) -> Result<u32> {
        veilig(|| {
            let zichtbaar = !menu().items(&paden(&selectie)).is_empty();
            Ok(if zichtbaar { ECS_ENABLED.0 as u32 } else { ECS_HIDDEN.0 as u32 })
        })
    }

    fn Invoke(&self, _selectie: Ref<'_, IShellItemArray>, _bc: Ref<'_, IBindCtx>) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn GetFlags(&self) -> Result<u32> {
        Ok(ECF_HASSUBCOMMANDS.0 as u32)
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        veilig(|| {
            let items: Vec<IExplorerCommand> =
                Opdracht::ALLE.into_iter().map(|opdracht| Subitem { opdracht, _teller: Teller::nieuw() }.into()).collect();
            Ok(Lijst { items, plek: Cell::new(0), _teller: Teller::nieuw() }.into())
        })
    }
}

/// "Delen…" of "Versturen met OpenAEC".
#[implement(IExplorerCommand)]
struct Subitem {
    opdracht: Opdracht,
    _teller: Teller,
}

impl IExplorerCommand_Impl for Subitem_Impl {
    fn GetTitle(&self, _selectie: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        tekst(self.opdracht.label())
    }

    fn GetIcon(&self, _selectie: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetToolTip(&self, _selectie: Ref<'_, IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> Result<GUID> {
        GUID::try_from(self.opdracht.canonieke_naam()).map_err(|_| E_FAIL.into())
    }

    fn GetState(&self, selectie: Ref<'_, IShellItemArray>, _traag_mag: BOOL) -> Result<u32> {
        veilig(|| {
            let zichtbaar = menu().zichtbaar(self.opdracht, &paden(&selectie));
            Ok(if zichtbaar { ECS_ENABLED.0 as u32 } else { ECS_HIDDEN.0 as u32 })
        })
    }

    fn Invoke(&self, selectie: Ref<'_, IShellItemArray>, _bc: Ref<'_, IBindCtx>) -> Result<()> {
        veilig(|| {
            let paden = paden(&selectie);
            if paden.is_empty() {
                return Err(E_INVALIDARG.into());
            }
            let menu = menu();
            let exe = programma(&menu).ok_or(windows_core::Error::from(E_FAIL))?;
            start(&exe, self.opdracht, &paden)
        })
    }

    fn GetFlags(&self) -> Result<u32> {
        Ok(ECF_DEFAULT.0 as u32)
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        Err(E_NOTIMPL.into())
    }
}

/// De subitems voor de Verkenner.
#[implement(IEnumExplorerCommand)]
struct Lijst {
    items: Vec<IExplorerCommand>,
    plek: Cell<usize>,
    _teller: Teller,
}

impl IEnumExplorerCommand_Impl for Lijst_Impl {
    fn Next(&self, aantal: u32, uit: *mut Option<IExplorerCommand>, gehaald: *mut u32) -> HRESULT {
        if uit.is_null() {
            return E_POINTER;
        }
        let mut n = 0usize;
        while n < aantal as usize && self.plek.get() < self.items.len() {
            unsafe { uit.add(n).write(Some(self.items[self.plek.get()].clone())) };
            self.plek.set(self.plek.get() + 1);
            n += 1;
        }
        if !gehaald.is_null() {
            unsafe { *gehaald = n as u32 };
        }
        if n == aantal as usize {
            S_OK
        } else {
            S_FALSE
        }
    }

    fn Skip(&self, aantal: u32) -> Result<()> {
        self.plek.set((self.plek.get() + aantal as usize).min(self.items.len()));
        Ok(())
    }

    fn Reset(&self) -> Result<()> {
        self.plek.set(0);
        Ok(())
    }

    fn Clone(&self) -> Result<IEnumExplorerCommand> {
        Ok(Lijst { items: self.items.clone(), plek: Cell::new(self.plek.get()), _teller: Teller::nieuw() }.into())
    }
}

#[implement(IClassFactory)]
struct Fabriek {
    _teller: Teller,
}

impl IClassFactory_Impl for Fabriek_Impl {
    fn CreateInstance(&self, buiten: Ref<'_, IUnknown>, riid: *const GUID, ppv: *mut *mut c_void) -> Result<()> {
        if ppv.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppv = std::ptr::null_mut() };
        if !buiten.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let item: IExplorerCommand = Hoofditem { _teller: Teller::nieuw() }.into();
        unsafe { item.query(riid, ppv) }.ok()
    }

    fn LockServer(&self, vast: BOOL) -> Result<()> {
        if vast.as_bool() {
            VERGRENDELD.fetch_add(1, Ordering::SeqCst);
        } else {
            let _ = VERGRENDELD.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| v.checked_sub(1));
        }
        Ok(())
    }
}

/// Hoort deze CLSID bij deze DLL: de vaste, of een test-CLSID (`OPENAEC_SHELLMENU_TEST_CLSID`, tijdens het draaien
/// voor de rooktest of al bij het bouwen voor een testpakket).
fn onze_clsid(clsid: &GUID) -> bool {
    if GUID::try_from(logica::CLSID).is_ok_and(|g| g == *clsid) {
        return true;
    }
    let test = |t: &str| GUID::try_from(t.trim_matches(['{', '}'])).is_ok_and(|g| g == *clsid);
    option_env!("OPENAEC_SHELLMENU_TEST_CLSID").is_some_and(test) || std::env::var(logica::ENV_TEST_CLSID).is_ok_and(|t| test(&t))
}

/// # Safety
/// Aangeroepen door COM met geldige pointers (of null, dan een foutcode).
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    unsafe { *ppv = std::ptr::null_mut() };
    if rclsid.is_null() || riid.is_null() {
        return E_INVALIDARG;
    }
    let resultaat = std::panic::catch_unwind(|| {
        if !onze_clsid(unsafe { &*rclsid }) {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let fabriek: IClassFactory = Fabriek { _teller: Teller::nieuw() }.into();
        unsafe { fabriek.query(riid, ppv) }
    });
    resultaat.unwrap_or(E_FAIL)
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECTEN.load(Ordering::SeqCst) == 0 && VERGRENDELD.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[no_mangle]
pub extern "system" fn DllMain(module: HINSTANCE, reden: u32, _gereserveerd: *mut c_void) -> BOOL {
    if reden == DLL_PROCESS_ATTACH {
        MODULE.store(module.0 as usize, Ordering::SeqCst);
    }
    true.into()
}
