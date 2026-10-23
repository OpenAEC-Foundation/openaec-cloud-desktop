//! Overgeslagen bestanden (een naam die niet kan, zoals `nul`) tonen en na bevestiging veilig opruimen. Het
//! verwijderen zelf gaat via de crate (`motor::ruim_op`, op Windows met het voorvoegsel `\\?\`); hier zit de prullenbak
//! van Windows en de commands voor het statusvenster en het overzicht.

use std::path::Path;

use openaec_sync::motor::{self, Overgeslagen};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::{fout, AppState};

/// Naar de prullenbak van Windows (geeft `true` als het lukte). De motor geeft een pad met een veilige naam in de
/// toestandsmap: de shell kan niet met `\\?\`-paden of apparaatnamen werken.
#[cfg(windows)]
pub fn naar_prullenbak(pad: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW,
    };
    // pFrom is een lijst namen die eindigt met twee nullen.
    let mut van: Vec<u16> = pad.as_os_str().encode_wide().collect();
    van.extend([0, 0]);
    let mut opdracht = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(van.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT).0 as u16,
        ..Default::default()
    };
    // SAFETY: `van` leeft tot na de aanroep en is volgens de eisen van SHFileOperationW afgesloten.
    let uitkomst = unsafe { SHFileOperationW(&mut opdracht) };
    uitkomst == 0 && !opdracht.fAnyOperationsAborted.as_bool() && !pad.exists()
}

/// De prullenbak voor de motor: op Windows die van de shell, elders geen (dan wordt opruimen verwijderen).
pub fn prullenbak() -> Option<fn(&Path) -> bool> {
    #[cfg(windows)]
    {
        Some(naar_prullenbak)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[derive(Serialize)]
pub struct KoppelOvergeslagen {
    lokaal: String,
    ruimte_naam: String,
    bestanden: Vec<Overgeslagen>,
}

/// De overgeslagen bestanden van de laatste ronde, per gekoppelde map (alleen mappen met overgeslagen bestanden).
#[tauri::command]
pub async fn overgeslagen(state: tauri::State<'_, AppState>) -> Result<Vec<KoppelOvergeslagen>, String> {
    let koppelingen = state.instellingen.lock().unwrap().koppelingen.clone();
    tauri::async_runtime::spawn_blocking(move || {
        koppelingen
            .into_iter()
            .map(|k| KoppelOvergeslagen { bestanden: motor::overgeslagen(Path::new(&k.lokaal)), lokaal: k.lokaal, ruimte_naam: k.ruimte_naam })
            .filter(|k| !k.bestanden.is_empty())
            .collect()
    })
    .await
    .map_err(fout)
}

#[derive(Serialize)]
pub struct Opgeruimd {
    verwijderd: u32,
    fouten: Vec<String>,
}

/// De opruimbare overgeslagen bestanden veilig verwijderen (eerst naar de prullenbak), voor één gekoppelde map of
/// alle. Wacht op een lopende ronde; daarna volgt een nieuwe lijst in het event "overgeslagen".
#[tauri::command]
pub async fn ruim_overgeslagen_op(app: AppHandle, lokaal: Option<String>) -> Result<Opgeruimd, String> {
    let state = app.state::<AppState>();
    let _slot = state.bezig.lock().await;
    let mappen: Vec<String> = state
        .instellingen
        .lock()
        .unwrap()
        .koppelingen
        .iter()
        .map(|k| k.lokaal.clone())
        .filter(|l| lokaal.as_ref().is_none_or(|x| x == l))
        .collect();
    let uit = tauri::async_runtime::spawn_blocking(move || {
        let mut uit = Opgeruimd { verwijderd: 0, fouten: Vec::new() };
        for map in mappen {
            let root = Path::new(&map);
            let paden: Vec<String> =
                motor::overgeslagen(root).into_iter().filter(|o| o.opruimbaar).map(|o| o.pad).collect();
            for (pad, r) in motor::ruim_op(root, &paden, prullenbak()) {
                match r {
                    Ok(()) => uit.verwijderd += 1,
                    Err(f) => uit.fouten.push(format!("{pad}: {f}")),
                }
            }
        }
        uit
    })
    .await
    .map_err(fout)?;
    drop(_slot);
    if let Ok(lijst) = overgeslagen(app.state::<AppState>()).await {
        let _ = app.emit("overgeslagen", &lijst);
    }
    Ok(uit)
}
