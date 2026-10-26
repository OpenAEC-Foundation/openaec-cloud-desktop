//! Rooktest van de COM-server: laadt de DLL via `CoCreateInstance` met een test-CLSID (in HKCU, daarna opgeruimd),
//! roept `GetTitle`, `GetIcon`, `GetState`, `EnumSubCommands` en `Invoke` aan op echte paden, en controleert dat de
//! DLL daarna weer ontladen mag worden. `Invoke` start deze rooktest zelf als "app" (`menu.json` wijst ernaar), die
//! de ontvangen opdrachtregel opschrijft; zo is ook het doorgeven van paden met spaties getest.
//!
//! ```text
//! cd crates/openaec-shellmenu
//! cargo build && cargo run --example rooktest [-- <pad naar openaec_shellmenu.dll>]
//! ```
//!
//! Raakt de echte installatie niet: een eigen CLSID, een eigen `menu.json` in een tijdelijke map, geen pakket.

#[cfg(not(windows))]
fn main() {
    eprintln!("alleen op Windows");
}

#[cfg(windows)]
fn main() {
    if let Err(e) = windows_rooktest::start() {
        eprintln!("MISLUKT: {e}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod windows_rooktest {
    use openaec_shellmenu::logica::{self, Menu};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};
    use windows::core::{Interface, GUID, HRESULT, HSTRING, PWSTR};
    use windows::Win32::Foundation::{FreeLibrary, CLASS_E_CLASSNOTAVAILABLE, S_OK};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IClassFactory, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        IExplorerCommand, IShellItemArray, SHCreateShellItemArrayFromIDLists, SHParseDisplayName, ECF_HASSUBCOMMANDS, ECS_ENABLED, ECS_HIDDEN,
    };

    /// Test-CLSID: niet die van het echte menu.
    const TEST_CLSID: &str = "33ABEE4D-ADA8-4B8E-A749-5B1C4A409669";
    /// Waar de "app" (deze rooktest, gestart door Invoke) zijn opdrachtregel neerzet.
    const ENV_UIT: &str = "OPENAEC_ROOKTEST_UIT";

    type Fout = Box<dyn std::error::Error>;

    fn controleer(ok: bool, wat: &str) -> Result<(), Fout> {
        if ok {
            println!("ok: {wat}");
            Ok(())
        } else {
            Err(format!("klopt niet: {wat}").into())
        }
    }

    fn tekst(p: PWSTR) -> String {
        let s = unsafe { p.to_string() }.unwrap_or_default();
        unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
        s
    }

    fn selectie(paden: &[&Path]) -> Result<IShellItemArray, Fout> {
        let mut pidls: Vec<*mut ITEMIDLIST> = Vec::new();
        for p in paden {
            let mut pidl = std::ptr::null_mut();
            unsafe { SHParseDisplayName(&HSTRING::from(p.as_os_str()), None, &mut pidl, 0, None) }?;
            pidls.push(pidl);
        }
        let constant: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| *p as *const _).collect();
        let array = unsafe { SHCreateShellItemArrayFromIDLists(&constant) };
        for p in pidls {
            unsafe { CoTaskMemFree(Some(p as *const _)) };
        }
        Ok(array?)
    }

    fn staat(cmd: &IExplorerCommand, sel: &IShellItemArray) -> Result<u32, Fout> {
        Ok(unsafe { cmd.GetState(sel, true) }?)
    }

    /// Wacht tot de door Invoke gestarte "app" zijn opdrachtregel heeft opgeschreven.
    fn ontvangen(uit: &Path) -> Result<Vec<String>, Fout> {
        let begin = Instant::now();
        while begin.elapsed() < Duration::from_secs(15) {
            if let Ok(b) = std::fs::read(uit) {
                if let Ok(v) = serde_json::from_slice::<Vec<String>>(&b) {
                    let _ = std::fs::remove_file(uit);
                    return Ok(v);
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err("de app is niet gestart".into())
    }

    struct Opruimen {
        map: PathBuf,
        sleutel: String,
    }

    impl Drop for Opruimen {
        fn drop(&mut self) {
            use winreg::enums::HKEY_CURRENT_USER;
            let _ = winreg::RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.sleutel);
            let _ = std::fs::remove_dir_all(&self.map);
            println!("opgeruimd: HKCU\\{} en {}", self.sleutel, self.map.display());
        }
    }

    pub fn start() -> Result<(), Fout> {
        let argv: Vec<String> = std::env::args().collect();
        // Gestart door Invoke: de opdrachtregel opschrijven en stoppen.
        if argv.iter().any(|a| a == "--actie") {
            // Gestart door de Verkenner (testpakket) zonder die variabele: naast deze exe.
            let uit = match std::env::var_os(ENV_UIT) {
                Some(u) => u,
                None => std::env::current_exe()?.with_file_name("ontvangen.json").into_os_string(),
            };
            std::fs::write(PathBuf::from(&uit).with_extension("tmp"), serde_json::to_vec(&argv[1..])?)?;
            std::fs::rename(PathBuf::from(&uit).with_extension("tmp"), &uit)?;
            return Ok(());
        }

        let exe = std::env::current_exe()?;
        let dll = match argv.get(1) {
            Some(p) => PathBuf::from(p),
            // target\<profiel>\examples\rooktest.exe → target\<profiel>\openaec_shellmenu.dll
            None => exe.parent().and_then(Path::parent).ok_or("geen map")?.join(logica::DLL),
        };
        let dll = dll.canonicalize().map_err(|e| format!("{}: {e}", dll.display()))?;
        let dll = PathBuf::from(dll.to_string_lossy().trim_start_matches(r"\\?\").to_string());
        println!("DLL: {}", dll.display());

        let map = std::env::temp_dir().join(format!("openaec-shellmenu-rooktest-{}", std::process::id()));
        let sleutel = format!("Software\\Classes\\CLSID\\{{{TEST_CLSID}}}");
        let _opruimen = Opruimen { map: map.clone(), sleutel: sleutel.clone() };
        let gekoppeld = map.join("Gekoppeld project");
        let submap = gekoppeld.join("Map met spatie");
        let bestand = gekoppeld.join("tekening a.pdf");
        let los = map.join("los").join("b c.txt");
        std::fs::create_dir_all(&submap)?;
        std::fs::create_dir_all(los.parent().unwrap())?;
        std::fs::write(&bestand, b"%PDF")?;
        std::fs::write(&los, b"x")?;

        let menu_json = map.join("menu.json");
        let mut menu =
            Menu { exe: Some(exe.to_string_lossy().into()), mappen: vec![gekoppeld.to_string_lossy().into()], versturen: true };
        menu.bewaar(&menu_json)?;
        std::env::set_var(logica::ENV_MENU, &menu_json);
        std::env::set_var(logica::ENV_TEST_CLSID, TEST_CLSID);
        let uit = map.join("ontvangen.json");
        std::env::set_var(ENV_UIT, &uit);

        // De test-CLSID in HKCU (alleen voor deze gebruiker, zonder beheerdersrechten).
        {
            use winreg::enums::HKEY_CURRENT_USER;
            let (k, _) = winreg::RegKey::predef(HKEY_CURRENT_USER).create_subkey(format!("{sleutel}\\InprocServer32"))?;
            k.set_value("", &dll.to_string_lossy().to_string())?;
            k.set_value("ThreadingModel", &"Apartment")?;
        }

        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
        let resultaat = (|| -> Result<(), Fout> {
            let clsid = GUID::try_from(TEST_CLSID).map_err(|_| "GUID")?;
            let hoofd: IExplorerCommand = unsafe { CoCreateInstance(&clsid, None, CLSCTX_INPROC_SERVER) }?;
            let los_sel = selectie(&[&los])?;
            let gekoppeld_sel = selectie(&[&bestand, &submap])?;
            let gemengd_sel = selectie(&[&bestand, &los])?;

            controleer(tekst(unsafe { hoofd.GetTitle(&los_sel) }?) == "OpenAEC", "titel OpenAEC")?;
            let icoon = tekst(unsafe { hoofd.GetIcon(&los_sel) }?);
            controleer(icoon == format!("{},0", exe.display()), &format!("pictogram van de app ({icoon})"))?;
            controleer(unsafe { hoofd.GetFlags() }? & ECF_HASSUBCOMMANDS.0 as u32 != 0, "hoofditem heeft subitems")?;
            controleer(staat(&hoofd, &los_sel)? == ECS_ENABLED.0 as u32, "OpenAEC zichtbaar op een los bestand")?;

            let lijst = unsafe { hoofd.EnumSubCommands() }?;
            let mut items: Vec<IExplorerCommand> = Vec::new();
            loop {
                let mut item = [None];
                let mut n = 0u32;
                let hr: HRESULT = unsafe { lijst.Next(&mut item, Some(&mut n)) };
                if n == 0 {
                    break;
                }
                items.extend(item.into_iter().flatten());
                if hr != S_OK {
                    break;
                }
            }
            let titels: Vec<String> = items.iter().map(|i| tekst(unsafe { i.GetTitle(&los_sel) }.unwrap())).collect();
            controleer(titels == ["Delen…", "Versturen met OpenAEC"], &format!("subitems {titels:?}"))?;
            let (delen, versturen) = (&items[0], &items[1]);

            let (aan, verborgen) = (ECS_ENABLED.0 as u32, ECS_HIDDEN.0 as u32);
            controleer(staat(delen, &los_sel)? == verborgen, "Delen… verborgen buiten een gekoppelde map")?;
            controleer(staat(delen, &gekoppeld_sel)? == aan, "Delen… zichtbaar binnen een gekoppelde map")?;
            controleer(staat(delen, &gemengd_sel)? == verborgen, "Delen… verborgen als niet alles in een gekoppelde map ligt")?;
            controleer(staat(versturen, &los_sel)? == aan && staat(versturen, &gekoppeld_sel)? == aan, "Versturen op alles")?;

            unsafe { versturen.Invoke(&gekoppeld_sel, None) }?;
            let v = ontvangen(&uit)?;
            let verwacht: Vec<String> =
                ["--actie".into(), "versturen".into(), bestand.to_string_lossy().into(), submap.to_string_lossy().into()].into();
            controleer(v == verwacht, &format!("Invoke Versturen start de app met {v:?}"))?;
            unsafe { delen.Invoke(&gekoppeld_sel, None) }?;
            let v = ontvangen(&uit)?;
            controleer(v[..3] == ["--actie", "delen", &bestand.to_string_lossy()], &format!("Invoke Delen start de app met {v:?}"))?;

            // Versturen uitgezet in de app: op een los bestand dan geen OpenAEC meer.
            menu.versturen = false;
            menu.bewaar(&menu_json)?;
            controleer(staat(versturen, &los_sel)? == verborgen, "Versturen verborgen als het uit staat")?;
            controleer(staat(&hoofd, &los_sel)? == verborgen, "OpenAEC verborgen zonder zichtbare subitems")?;
            controleer(staat(&hoofd, &gekoppeld_sel)? == aan, "OpenAEC zichtbaar met alleen Delen…")?;
            // Zonder menu.json: alleen Versturen.
            std::fs::remove_file(&menu_json)?;
            controleer(staat(delen, &gekoppeld_sel)? == verborgen && staat(versturen, &los_sel)? == aan, "zonder menu.json alleen Versturen")?;
            // Een tweede CoCreateInstance werkt ook.
            let _tweede: IExplorerCommand = unsafe { CoCreateInstance(&clsid, None, CLSCTX_INPROC_SERVER) }?;
            let _ = hoofd.cast::<windows::core::IUnknown>()?;
            Ok(())
        })();

        // Alles losgelaten: dan mag de DLL ontladen worden.
        let module = unsafe { LoadLibraryW(&HSTRING::from(dll.as_os_str())) }?;
        let kan = unsafe { GetProcAddress(module, windows::core::s!("DllCanUnloadNow")) }.ok_or("DllCanUnloadNow ontbreekt")?;
        let kan: extern "system" fn() -> HRESULT = unsafe { std::mem::transmute(kan) };
        let ontladen = kan();
        // Een CLSID die niet van deze DLL is, geeft geen fabriek.
        let haal = unsafe { GetProcAddress(module, windows::core::s!("DllGetClassObject")) }.ok_or("DllGetClassObject ontbreekt")?;
        let haal: unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut core::ffi::c_void) -> HRESULT =
            unsafe { std::mem::transmute(haal) };
        let vreemd = GUID::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
        let mut fabriek = std::ptr::null_mut();
        let geweigerd = unsafe { haal(&vreemd, &IClassFactory::IID, &mut fabriek) };
        unsafe { FreeLibrary(module) }?;
        unsafe { CoUninitialize() };
        resultaat?;
        controleer(ontladen == S_OK, "DllCanUnloadNow na het loslaten van alle objecten")?;
        controleer(geweigerd == CLASS_E_CLASSNOTAVAILABLE && fabriek.is_null(), "een vreemde CLSID wordt geweigerd")?;
        println!("rooktest geslaagd");
        Ok(())
    }
}
