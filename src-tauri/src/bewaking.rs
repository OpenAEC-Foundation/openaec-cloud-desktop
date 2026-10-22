//! Wanneer de app synchroniseert: direct na een lokale wijziging (bestandswachter, 2 s rust) en wanneer de
//! wijzigingenlijst van de server iets nieuws heeft (controle elke 30 s). Elke 10 minuten volgt hoe dan ook een
//! volledige ronde, als vangnet voor gemiste gebeurtenissen. Een nieuwe koppeling (ook bij het starten) krijgt eerst
//! zijn statusvinkjes in de Verkenner.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use openaec_sync::bewaking::{nieuwe_wijzigingen, telt_mee};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use crate::instellingen::Koppeling;
use crate::{client, sync_mappen, vinkjes, AppState};

const RUST: Duration = Duration::from_secs(2);
const CONTROLE: Duration = Duration::from_secs(30);
const VANGNET: Duration = Duration::from_secs(600);

fn wachter(lokaal: &str, tx: UnboundedSender<String>) -> notify::Result<RecommendedWatcher> {
    let root = PathBuf::from(lokaal);
    let sleutel = lokaal.to_string();
    let mut w = notify::recommended_watcher(move |gebeurtenis: notify::Result<notify::Event>| {
        if let Ok(g) = gebeurtenis {
            // Alle paden langs (ook de statusvinkjes bijwerken); het zetten van een vinkje zelf telt niet mee.
            let mut mee = false;
            for p in &g.paths {
                if telt_mee(&root, p) && vinkjes::gebeurtenis(&root, p) {
                    mee = true;
                }
            }
            if mee {
                let _ = tx.send(sleutel.clone());
            }
        }
    })?;
    w.watch(&PathBuf::from(lokaal), RecursiveMode::Recursive)?;
    Ok(w)
}

/// Statusvinkjes voor een (nieuwe) koppeling, niet tegelijk met een sync-ronde.
async fn markeer(state: &AppState, k: Koppeling) {
    let _slot = state.bezig.lock().await;
    let _ = tauri::async_runtime::spawn_blocking(move || vinkjes::koppel(&k)).await;
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = unbounded_channel::<String>();
        let mut wachters: HashMap<String, RecommendedWatcher> = HashMap::new();
        let mut cursors: HashMap<String, u64> = HashMap::new();
        let mut te_doen: HashSet<String> = HashSet::new();
        let mut laatste_volledig: Option<Instant> = None;
        let mut controle = tokio::time::interval(CONTROLE);

        loop {
            let state = app.state::<AppState>();
            let koppelingen = state.instellingen.lock().unwrap().koppelingen.clone();
            let actief: HashSet<String> = koppelingen.iter().map(|k| k.lokaal.clone()).collect();

            for k in &koppelingen {
                if vinkjes::nog_te_koppelen(&k.lokaal) {
                    markeer(&state, k.clone()).await;
                }
            }

            // Wachters en cursors gelijk houden met de koppelingen; een nieuwe koppeling direct syncen.
            wachters.retain(|lokaal, _| actief.contains(lokaal));
            cursors.retain(|lokaal, _| actief.contains(lokaal));
            for lokaal in &actief {
                if !wachters.contains_key(lokaal) {
                    if let Ok(w) = wachter(lokaal, tx.clone()) {
                        wachters.insert(lokaal.clone(), w);
                    }
                    te_doen.insert(lokaal.clone());
                }
            }

            te_doen.retain(|lokaal| actief.contains(lokaal));
            if !te_doen.is_empty() {
                if let Ok(r) = sync_mappen(&app, Some(&te_doen)).await {
                    let _ = app.emit("sync", &r);
                }
                te_doen.clear();
            }

            tokio::select! {
                Some(lokaal) = rx.recv() => {
                    te_doen.insert(lokaal);
                    // Wachten tot het 2 s stil is: opslaan in een programma geeft vaak een reeks gebeurtenissen.
                    loop {
                        tokio::select! {
                            Some(meer) = rx.recv() => { te_doen.insert(meer); }
                            _ = tokio::time::sleep(RUST) => break,
                        }
                    }
                }
                _ = controle.tick() => {
                    if laatste_volledig.is_none_or(|t| t.elapsed() >= VANGNET) {
                        te_doen.extend(actief.iter().cloned());
                        laatste_volledig = Some(Instant::now());
                    } else if let Ok(client) = client(&state).await {
                        for k in &koppelingen {
                            let sinds = cursors.get(&k.lokaal).copied().unwrap_or(0);
                            if let Ok((iets, cursor)) = nieuwe_wijzigingen(&client, &k.ruimte, sinds).await {
                                if iets {
                                    te_doen.insert(k.lokaal.clone());
                                }
                                cursors.insert(k.lokaal.clone(), cursor);
                            }
                        }
                    }
                }
            }
        }
    });
}
