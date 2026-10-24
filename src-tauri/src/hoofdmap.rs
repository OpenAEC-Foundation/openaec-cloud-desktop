//! De hoofdmap: één map (standaard `%USERPROFILE%\OpenAEC`) met per ruimte een submap. Op Windows is hij één
//! sync-root met een eigen item "OpenAEC" in het navigatiepaneel van de Verkenner, en met bestanden op aanvraag (zie
//! `cloudfilter`, Fase 3, en `opaanvraag`). Losse gekoppelde mappen blijven werken zoals ze werkten; de app biedt een
//! overstap naar de hoofdmap aan ([`verplaats_naar_hoofdmap`]), maar doet dat nooit vanzelf.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::instellingen::{self, submapnaam, Koppeling, GEDEELD_MET_MIJ};
use crate::{fout, werk_menu_bij, AppState};

/// Naam van het item in het navigatiepaneel.
pub const NAAM: &str = "OpenAEC";

#[derive(Serialize)]
pub struct HoofdmapStatus {
    /// De ingestelde hoofdmap (None: nog niet ingesteld).
    hoofdmap: Option<String>,
    /// Voorstel bij de eerste start.
    voorstel: String,
    /// Staat hij in het navigatiepaneel (geregistreerd en verbonden)?
    actief: bool,
    /// Kent dit systeem een plek in het navigatiepaneel en bestanden op aanvraag (Windows)?
    ondersteund: bool,
    /// Waarom hij niet actief is (registreren mislukt).
    fout: Option<String>,
}

#[cfg(windows)]
mod echt {
    use std::sync::Mutex;

    use tauri::{AppHandle, Manager};

    use crate::{cloudfilter, opaanvraag, AppState};

    /// De hoofdmap waarvoor registreren en verbinden in dit proces gelukt is, met de verbinding; of de fout.
    pub struct Stand {
        pub pad: String,
        pub verbinding: Option<windows::Win32::Storage::CloudFilters::CF_CONNECTION_KEY>,
        pub fout: Option<String>,
    }

    pub static STAND: Mutex<Option<Stand>> = Mutex::new(None);

    pub fn actief(pad: &str) -> (bool, Option<String>) {
        match &*STAND.lock().unwrap() {
            Some(s) if s.pad == pad => (s.verbinding.is_some(), s.fout.clone()),
            _ => (false, None),
        }
    }

    /// Registreren (herhaalbaar), de hoofdmap zelf in sync, verbinden en de ophaler zetten. Blocking.
    pub fn zorg(app: &AppHandle, pad: &str) -> Result<(), String> {
        if STAND.lock().unwrap().as_ref().is_some_and(|s| s.pad == pad && s.verbinding.is_some()) {
            return Ok(());
        }
        let uitkomst = (|| {
            std::fs::create_dir_all(pad).map_err(|e| e.to_string())?;
            let icoon = std::env::current_exe().map(|p| format!("{},0", p.display())).map_err(|e| e.to_string())?;
            cloudfilter::registreer_hoofdmap(pad, super::NAAM, env!("CARGO_PKG_VERSION"), &icoon).map_err(|e| e.to_string())?;
            let _ = cloudfilter::markeer_op_aanvraag(std::path::Path::new(pad), std::path::Path::new(pad), true);
            let (a1, a2) = (app.clone(), app.clone());
            cloudfilter::zet_ophaler(opaanvraag::ophaler(
                move || a1.state::<AppState>().instellingen.lock().unwrap().basis.clone(),
                move || {
                    let state = a2.state::<AppState>();
                    let inst = state.instellingen.lock().unwrap();
                    inst.koppelingen.iter().filter(|k| inst.in_hoofdmap(k)).map(|k| k.lokaal.clone()).collect()
                },
                |basis| tauri::async_runtime::block_on(crate::autorisatie(basis)),
            ));
            cloudfilter::verbind_hoofdmap(pad).map_err(|e| e.to_string())
        })();
        let mut stand = STAND.lock().unwrap();
        if let Some(oud) = stand.take().and_then(|s| s.verbinding) {
            let _ = cloudfilter::disconnect(oud);
        }
        match uitkomst {
            Ok(k) => {
                log::info!("hoofdmap {pad} staat in het navigatiepaneel");
                *stand = Some(Stand { pad: pad.to_string(), verbinding: Some(k), fout: None });
                Ok(())
            }
            Err(e) => {
                log::warn!("hoofdmap {pad}: {e}");
                *stand = Some(Stand { pad: pad.to_string(), verbinding: None, fout: Some(e.clone()) });
                Err(e)
            }
        }
    }
}

/// Zorg dat de ingestelde hoofdmap geregistreerd en verbonden is (bij het starten, vóór de ruimtes erin hun status
/// krijgen). Blocking; doet niets zonder hoofdmap of buiten Windows.
pub fn zorg_voor_registratie(app: &AppHandle) {
    let pad = app.state::<AppState>().instellingen.lock().unwrap().hoofdmap.clone();
    #[cfg(windows)]
    if let Some(pad) = pad {
        let _ = echt::zorg(app, &pad);
    }
    #[cfg(not(windows))]
    let _ = (app, pad);
}

fn status_van(state: &AppState) -> HoofdmapStatus {
    let hoofdmap = state.instellingen.lock().unwrap().hoofdmap.clone();
    #[cfg(windows)]
    let (actief, fout) = hoofdmap.as_deref().map(echt::actief).unwrap_or((false, None));
    #[cfg(not(windows))]
    let (actief, fout) = (false, None);
    HoofdmapStatus {
        hoofdmap,
        voorstel: instellingen::standaard_hoofdmap().to_string_lossy().to_string(),
        actief,
        ondersteund: cfg!(windows),
        fout,
    }
}

#[tauri::command]
pub fn hoofdmap_status(state: tauri::State<'_, AppState>) -> HoofdmapStatus {
    status_van(&state)
}

/// Ligt `a` in `b` of `b` in `a` (of zijn ze gelijk)?
fn overlapt(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// De hoofdmap instellen (bij de eerste start of later). Hij mag niet overlappen met een losse gekoppelde map, en een
/// hoofdmap met ruimtes erin wordt niet zomaar vervangen.
#[tauri::command]
pub async fn zet_hoofdmap(app: AppHandle, state: tauri::State<'_, AppState>, pad: String) -> Result<HoofdmapStatus, String> {
    let pad = pad.trim().trim_end_matches(['\\', '/']).to_string();
    if pad.is_empty() || !Path::new(&pad).is_absolute() {
        return Err("kies een volledige map, bijvoorbeeld C:\\Users\\naam\\OpenAEC".into());
    }
    let zelfde = {
        let inst = state.instellingen.lock().unwrap();
        let zelfde = inst.hoofdmap.as_deref().is_some_and(|oud| Path::new(oud) == Path::new(&pad));
        if !zelfde {
            if inst.hoofdmap.is_some() && inst.koppelingen.iter().any(|k| inst.in_hoofdmap(k)) {
                return Err("de huidige hoofdmap bevat nog ruimtes; ontkoppel die eerst".into());
            }
            if let Some(k) = inst.koppelingen.iter().find(|k| overlapt(Path::new(&k.lokaal), Path::new(&pad))) {
                return Err(format!("deze map overlapt met de gekoppelde map {}", k.lokaal));
            }
        }
        zelfde
    };
    std::fs::create_dir_all(&pad).map_err(fout)?;
    #[cfg(windows)]
    {
        let (a, p) = (app.clone(), pad.clone());
        tauri::async_runtime::spawn_blocking(move || echt::zorg(&a, &p)).await.map_err(fout)??;
    }
    #[cfg(not(windows))]
    let _ = &app;
    if zelfde {
        return Ok(status_van(&state));
    }
    let mut inst = state.instellingen.lock().unwrap();
    inst.hoofdmap = Some(pad);
    inst.bewaar().map_err(fout)?;
    drop(inst);
    Ok(status_van(&state))
}

/// Submappen die al in de hoofdmap staan (of voor een koppeling gebruikt worden).
fn bezette_namen(hoofdmap: &Path, koppelingen: &[Koppeling]) -> Vec<String> {
    let mut namen: Vec<String> = std::fs::read_dir(hoofdmap)
        .map(|r| r.flatten().map(|i| i.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default();
    namen.extend(koppelingen.iter().filter_map(|k| Path::new(&k.lokaal).file_name().map(|n| n.to_string_lossy().to_string())));
    namen.push(GEDEELD_MET_MIJ.to_string());
    namen
}

/// Waar een koppeling in de hoofdmap komt: een ruimte direct eronder, een gedeelde map onder "Gedeeld met mij".
fn plek_in_hoofdmap(hoofdmap: &Path, k: &Koppeling, koppelingen: &[Koppeling]) -> PathBuf {
    if k.deling.is_some() {
        let map = hoofdmap.join(GEDEELD_MET_MIJ);
        let bezet: Vec<String> = std::fs::read_dir(&map)
            .map(|r| r.flatten().map(|i| i.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default();
        map.join(submapnaam(&k.ruimte_naam, &bezet))
    } else {
        hoofdmap.join(submapnaam(&k.ruimte_naam, &bezette_namen(hoofdmap, koppelingen)))
    }
}

/// Is dit dezelfde ruimte of dezelfde gedeelde map?
fn zelfde_bron(a: &Koppeling, b: &Koppeling) -> bool {
    a.ruimte == b.ruimte && a.deling == b.deling
}

/// Een map die met mij gedeeld is, synchroniseren in de hoofdmap, onder "Gedeeld met mij".
#[tauri::command]
pub fn koppel_gedeeld_in_hoofdmap(state: tauri::State<'_, AppState>, gedeeld: openaec_sync::api::Gedeeld) -> Result<Vec<Koppeling>, String> {
    let hoofdmap = PathBuf::from(state.instellingen.lock().unwrap().hoofdmap.clone().ok_or("stel eerst de hoofdmap in")?);
    let mut k = crate::gedeelde_koppeling(gedeeld, String::new())?;
    {
        let inst = state.instellingen.lock().unwrap();
        if inst.koppelingen.iter().any(|o| zelfde_bron(o, &k) && inst.in_hoofdmap(o)) {
            return Err("deze map staat al in de hoofdmap".into());
        }
        k.lokaal = plek_in_hoofdmap(&hoofdmap, &k, &inst.koppelingen).to_string_lossy().to_string();
    }
    crate::voeg_koppeling_toe(&state, k)
}

/// Een ruimte synchroniseren in de hoofdmap: een submap met de naam van de ruimte.
#[tauri::command]
pub fn koppel_in_hoofdmap(state: tauri::State<'_, AppState>, ruimte: String, ruimte_naam: String) -> Result<Vec<Koppeling>, String> {
    let mut inst = state.instellingen.lock().unwrap();
    let hoofdmap = PathBuf::from(inst.hoofdmap.clone().ok_or("stel eerst de hoofdmap in")?);
    if inst.koppelingen.iter().any(|k| k.ruimte == ruimte && k.deling.is_none() && inst.in_hoofdmap(k)) {
        return Err("deze ruimte staat al in de hoofdmap".into());
    }
    let naam = submapnaam(&ruimte_naam, &bezette_namen(&hoofdmap, &inst.koppelingen));
    let lokaal = hoofdmap.join(naam).to_string_lossy().to_string();
    std::fs::create_dir_all(&lokaal).map_err(fout)?;
    inst.koppelingen.push(Koppeling::ruimte(ruimte, ruimte_naam, lokaal));
    inst.bewaar().map_err(fout)?;
    werk_menu_bij(&inst.koppelingen);
    Ok(inst.koppelingen.clone())
}

/// Overstap van een losse gekoppelde map naar de hoofdmap: na een eventueel lopende ronde gaan de statusvinkjes eraf,
/// verhuist de map (met zijn toestand, dus zonder opnieuw te downloaden) naar een submap van de hoofdmap, en krijgt
/// hij daar zijn status terug. Alleen binnen hetzelfde station (verplaatsen, geen kopie).
#[tauri::command]
pub async fn verplaats_naar_hoofdmap(state: tauri::State<'_, AppState>, lokaal: String) -> Result<Vec<Koppeling>, String> {
    let _slot = state.bezig.lock().await;
    let (k, doel) = {
        let inst = state.instellingen.lock().unwrap();
        let hoofdmap = PathBuf::from(inst.hoofdmap.clone().ok_or("stel eerst de hoofdmap in")?);
        let k = inst.koppelingen.iter().find(|k| k.lokaal == lokaal).cloned().ok_or("deze map is niet gekoppeld")?;
        if inst.in_hoofdmap(&k) {
            return Err("deze ruimte staat al in de hoofdmap".into());
        }
        if inst.koppelingen.iter().any(|o| zelfde_bron(o, &k) && inst.in_hoofdmap(o)) {
            return Err("deze ruimte staat al in de hoofdmap".into());
        }
        let doel = plek_in_hoofdmap(&hoofdmap, &k, &inst.koppelingen);
        (k, doel)
    };
    let (van, naar) = (PathBuf::from(&k.lokaal), doel.clone());
    tauri::async_runtime::spawn_blocking(move || {
        if !van.is_dir() {
            return Err("de map bestaat niet (meer) op deze computer".to_string());
        }
        if let Some(ouder) = naar.parent() {
            std::fs::create_dir_all(ouder).map_err(|e| e.to_string())?;
        }
        crate::vinkjes::ontkoppel(&van.to_string_lossy());
        std::fs::rename(&van, &naar).map_err(|e| {
            if e.raw_os_error() == Some(17) {
                "de hoofdmap staat op een ander station; verplaats de map zelf of kies een hoofdmap op hetzelfde station".to_string()
            } else {
                format!("verplaatsen lukte niet ({e}); sluit programma's die bestanden in de map open hebben")
            }
        })
    })
    .await
    .map_err(fout)??;
    let mut inst = state.instellingen.lock().unwrap();
    if let Some(k) = inst.koppelingen.iter_mut().find(|k| k.lokaal == lokaal) {
        k.lokaal = doel.to_string_lossy().to_string();
    }
    inst.bewaar().map_err(fout)?;
    werk_menu_bij(&inst.koppelingen);
    Ok(inst.koppelingen.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_van_mappen() {
        let t = std::env::temp_dir();
        assert!(overlapt(&t.join("OpenAEC"), &t.join("OpenAEC").join("A")));
        assert!(overlapt(&t.join("OpenAEC").join("A"), &t.join("OpenAEC")));
        assert!(overlapt(&t.join("OpenAEC"), &t.join("OpenAEC")));
        assert!(!overlapt(&t.join("OpenAEC"), &t.join("OpenAEC-oud")));
    }
}
