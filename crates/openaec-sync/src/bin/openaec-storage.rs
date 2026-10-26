//! `openaec-storage`: OpenAEC Storage op de opdrachtregel, voor Linux (ook servers zonder scherm), macOS en Windows.
//! Dezelfde sync-motor als de desktop-app.
//!
//! ```text
//! openaec-storage inloggen https://bedrijf.cloud.open-aec.eu      # via de browser (OpenAEC-account)
//! openaec-storage inloggen https://… --sleutel                     # API-sleutel uit OPENAEC_SLEUTEL ("sleutel:geheim")
//! openaec-storage ruimtes
//! openaec-storage koppel "Project A" ~/OpenAEC/Project-A
//! openaec-storage sync                                              # één ronde
//! openaec-storage bewaak                                            # blijft draaien (systemd)
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use notify::{RecursiveMode, Watcher};
use openaec_sync::bewaking::{nieuwe_wijzigingen, telt_mee};
use openaec_sync::motor::{sync_met_selectie, Rapport};
use openaec_sync::oauth::{self, Tokens};
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::sessie::{Bewaring, Sessie};
use openaec_sync::{Client, Fout};
use serde::{Deserialize, Serialize};

const CONTROLE: Duration = Duration::from_secs(30);
const VANGNET: Duration = Duration::from_secs(600);
const RUST: Duration = Duration::from_secs(2);

#[derive(Parser)]
#[command(name = "openaec-storage", version, about = "OpenAEC Storage: mappen synchroniseren met een ruimte")]
struct Opdracht {
    #[command(subcommand)]
    actie: Actie,
}

#[derive(Subcommand)]
enum Actie {
    /// Inloggen bij een omgeving (via de browser, of met --sleutel uit OPENAEC_SLEUTEL)
    Inloggen {
        basis: String,
        #[arg(long)]
        sleutel: bool,
    },
    /// Uitloggen (sleutels wissen; koppelingen en bestanden blijven)
    Uitloggen,
    /// Ruimtes waar je lid van bent
    Ruimtes,
    /// Een lokale map aan een ruimte koppelen (naam of id van de ruimte)
    Koppel { ruimte: String, map: PathBuf },
    /// Een koppeling opheffen (de lokale bestanden blijven staan)
    Ontkoppel { map: PathBuf },
    /// Mapselectie: welke mappen van de ruimte niet op deze computer komen (zonder --zonder: alles weer aan)
    Selectie {
        map: PathBuf,
        /// Map in de ruimte die hier niet komt (mag vaker); paden met '/'
        #[arg(long = "zonder")]
        zonder: Vec<String>,
    },
    /// Gekoppelde mappen tonen
    Status,
    /// Eén sync-ronde voor alle gekoppelde mappen
    Sync,
    /// Blijven synchroniseren: direct bij een lokale wijziging, en bij wijzigingen op de server
    Bewaak,
    /// Een bestaande map in één richting naar een ruimte uploaden (overstappen); hervatbaar, de bron blijft staan
    Importeer {
        bron: PathBuf,
        ruimte: String,
        /// Map in de ruimte waarin de bestanden komen (standaard het hoofdniveau)
        #[arg(long, default_value = "")]
        doel: String,
        /// Aantal uploads tegelijk
        #[arg(long, default_value_t = 8)]
        parallel: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Koppeling {
    ruimte: String,
    ruimte_naam: String,
    lokaal: PathBuf,
    /// Mappen die niet op deze computer komen (mapselectie).
    #[serde(default)]
    uitgesloten: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    basis: Option<String>,
    /// API-sleutel ("token sleutel:geheim") of OAuth-tokens; nooit beide.
    sleutel: Option<String>,
    tokens: Option<Tokens>,
    #[serde(default)]
    koppelingen: Vec<Koppeling>,
}

fn config_pad() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("openaec-storage").join("config.json")
}

impl Config {
    fn laad() -> Self {
        std::fs::read(config_pad()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn bewaar(&mut self) -> std::io::Result<()> {
        // Onderweg vernieuwde tokens staan al in het bestand (zie `Configbewaring`) en zijn nieuwer dan die in het
        // geheugen; die niet overschrijven, anders gaat het geldige refresh-token verloren.
        if let (Some(hier), Some(daar)) = (&self.tokens, Config::laad().tokens) {
            if daar.verloopt > hier.verloopt {
                self.tokens = Some(daar);
            }
        }
        let pad = config_pad();
        std::fs::create_dir_all(pad.parent().unwrap())?;
        std::fs::write(&pad, serde_json::to_vec_pretty(self).unwrap())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&pad, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// Client voor de omgeving: de API-sleutel, of de OAuth-tokens die bij elk verzoek zo nodig vernieuwd worden (en
    /// dan meteen in het configuratiebestand komen; zie [`Configbewaring`]).
    async fn client(&mut self) -> Result<Client, String> {
        let basis = self.basis.clone().ok_or("niet ingelogd: gebruik `openaec-storage inloggen <adres>`")?;
        if let Some(s) = &self.sleutel {
            return Ok(Client::new(&basis, s));
        }
        if self.tokens.is_none() {
            return Err("niet ingelogd: gebruik `openaec-storage inloggen <adres>`".into());
        }
        let sessie = std::sync::Arc::new(Sessie::new(&basis, Configbewaring));
        Ok(Client::met_bron(&basis, sessie, openaec_sync::api::nieuwe_http()))
    }
}

/// De tokens in het configuratiebestand: vernieuwde tokens komen er direct in (het oude refresh-token is dan ongeldig).
struct Configbewaring;

impl Bewaring for Configbewaring {
    fn lees(&self) -> Option<String> {
        Config::laad().tokens.and_then(|t| serde_json::to_string(&t).ok())
    }

    fn bewaar(&self, waarde: &str) -> Result<(), String> {
        let tokens: Tokens = serde_json::from_str(waarde).map_err(|e| e.to_string())?;
        let mut config = Config::laad();
        config.tokens = Some(tokens);
        config.bewaar().map_err(|e| e.to_string())
    }
}

/// Volledig pad zonder het `\\?\`-voorvoegsel dat Windows bij canonicalize toevoegt.
fn volledig(pad: &Path) -> std::io::Result<PathBuf> {
    let p = std::fs::canonicalize(pad)?;
    let tekst = p.to_string_lossy();
    Ok(tekst.strip_prefix(r"\\?\").map(PathBuf::from).unwrap_or(p))
}

fn open_in_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

async fn inloggen(config: &mut Config, basis: &str, sleutel: bool) -> Result<(), String> {
    let basis = basis.trim().trim_end_matches('/').to_string();
    if sleutel {
        let waarde = std::env::var("OPENAEC_SLEUTEL").map_err(|_| "zet OPENAEC_SLEUTEL op \"sleutel:geheim\"")?;
        let autorisatie = format!("token {}", waarde.trim());
        let ruimtes = Client::new(&basis, &autorisatie).ruimtes().await.map_err(|e| e.to_string())?;
        config.sleutel = Some(autorisatie);
        config.tokens = None;
        config.basis = Some(basis);
        config.bewaar().map_err(|e| e.to_string())?;
        println!("Ingelogd met API-sleutel; {} ruimte(s).", ruimtes.len());
        return Ok(());
    }
    let luisteraar = tokio::net::TcpListener::bind(("127.0.0.1", oauth::POORT))
        .await
        .map_err(|e| format!("poort {} is bezet ({e})", oauth::POORT))?;
    let verifier = oauth::verifier();
    let staat = oauth::verifier();
    let url = oauth::autorisatie_url(&basis, &oauth::challenge(&verifier), &staat);
    println!("Open deze link om in te loggen (de browser opent vanzelf als die er is):\n\n  {url}\n");
    println!("Zonder browser op deze machine? Log in met een API-sleutel: OPENAEC_SLEUTEL=… openaec-storage inloggen {basis} --sleutel");
    open_in_browser(&url);
    let code = tokio::time::timeout(Duration::from_secs(300), oauth::wacht_op_code(luisteraar, staat))
        .await
        .map_err(|_| "inloggen duurde te lang".to_string())?
        .map_err(|e| e.to_string())?;
    let tokens = oauth::wissel_code(&basis, &code, &verifier).await.map_err(|e| e.to_string())?;
    config.tokens = Some(tokens);
    config.sleutel = None;
    config.basis = Some(basis);
    config.bewaar().map_err(|e| e.to_string())?;
    println!("Ingelogd.");
    Ok(())
}

fn toon_rapport(k: &Koppeling, r: &Result<Rapport, Fout>) {
    match r {
        Ok(r) => {
            let mut delen = Vec::new();
            for (n, woord) in [(r.geupload, "geüpload"), (r.gedownload, "gedownload"), (r.verwijderd, "verwijderd"), (r.conflicten, "conflict(en)")] {
                if n > 0 {
                    delen.push(format!("{n} {woord}"));
                }
            }
            let tekst = if delen.is_empty() { "bijgewerkt".to_string() } else { delen.join(", ") };
            println!("{} ({}): {tekst}", k.ruimte_naam, k.lokaal.display());
            for f in &r.fouten {
                eprintln!("  fout: {f}");
            }
            for o in &r.overgeslagen {
                eprintln!("  overgeslagen: {}: {}", o.pad, o.reden);
            }
            for m in &r.meldingen {
                println!("  {m}");
            }
            if !r.wachtend.is_empty() {
                eprintln!("  {} bestand(en) lukken steeds niet en wachten op een volgende poging:", r.wachtend.len());
                for w in &r.wachtend {
                    eprintln!("    {}: {}", w.pad, w.reden);
                }
            }
            if r.sessie_verlopen {
                eprintln!("  gestopt: {}", openaec_sync::SESSIE_VERLOPEN);
            }
        }
        Err(f) => eprintln!("{} ({}): {f}", k.ruimte_naam, k.lokaal.display()),
    }
}

async fn sync(config: &mut Config, alleen: Option<&HashSet<PathBuf>>) -> Result<(), String> {
    let client = config.client().await?;
    for k in config.koppelingen.iter().filter(|k| alleen.is_none_or(|a| a.contains(&k.lokaal))) {
        let opslag = ClientOpslag::new(client.clone(), &k.ruimte);
        let r = sync_met_selectie(&opslag, &k.lokaal, &k.uitgesloten).await;
        toon_rapport(k, &r);
    }
    Ok(())
}

/// Blijven synchroniseren (zelfde regels als de desktop-app): bestandswachter met 2 s rust, controle van de
/// wijzigingenlijst elke 30 s, eens per 10 minuten een volledige ronde. Stopt netjes op Ctrl+C / SIGTERM.
async fn bewaak(config: &mut Config) -> Result<(), String> {
    if config.koppelingen.is_empty() {
        return Err("nog geen gekoppelde mappen: gebruik `openaec-storage koppel <ruimte> <map>`".into());
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
    let mut wachters = Vec::new();
    for k in &config.koppelingen {
        let root = k.lokaal.clone();
        let tx = tx.clone();
        let mut w = notify::recommended_watcher(move |g: notify::Result<notify::Event>| {
            if let Ok(g) = g {
                if g.paths.iter().any(|p| telt_mee(&root, p)) {
                    let _ = tx.send(root.clone());
                }
            }
        })
        .map_err(|e| e.to_string())?;
        w.watch(&k.lokaal, RecursiveMode::Recursive).map_err(|e| e.to_string())?;
        wachters.push(w);
    }
    println!("Bewaakt {} map(pen). Stoppen met Ctrl+C.", config.koppelingen.len());

    let mut cursors: std::collections::HashMap<PathBuf, u64> = Default::default();
    let mut laatste_volledig = Instant::now();
    sync(config, None).await?;
    let mut controle = tokio::time::interval(CONTROLE);
    controle.tick().await;
    loop {
        let mut te_doen: HashSet<PathBuf> = HashSet::new();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => { println!("Gestopt."); return Ok(()); }
            Some(map) = rx.recv() => {
                te_doen.insert(map);
                loop {
                    tokio::select! {
                        Some(meer) = rx.recv() => { te_doen.insert(meer); }
                        _ = tokio::time::sleep(RUST) => break,
                    }
                }
            }
            _ = controle.tick() => {
                if laatste_volledig.elapsed() >= VANGNET {
                    te_doen.extend(config.koppelingen.iter().map(|k| k.lokaal.clone()));
                    laatste_volledig = Instant::now();
                } else {
                    let client = config.client().await?;
                    for k in &config.koppelingen {
                        let sinds = cursors.get(&k.lokaal).copied().unwrap_or(0);
                        if let Ok((iets, cursor)) = nieuwe_wijzigingen(&client, &k.ruimte, sinds).await {
                            if iets && sinds > 0 {
                                te_doen.insert(k.lokaal.clone());
                            }
                            cursors.insert(k.lokaal.clone(), cursor);
                        }
                    }
                }
            }
        }
        if !te_doen.is_empty() {
            if let Err(f) = sync(config, Some(&te_doen)).await {
                eprintln!("{f}");
            }
        }
    }
}

async fn voer_uit(actie: Actie) -> Result<(), String> {
    let mut config = Config::laad();
    match actie {
        Actie::Inloggen { basis, sleutel } => inloggen(&mut config, &basis, sleutel).await,
        Actie::Uitloggen => {
            config.sleutel = None;
            config.tokens = None;
            config.bewaar().map_err(|e| e.to_string())?;
            println!("Uitgelogd.");
            Ok(())
        }
        Actie::Ruimtes => {
            for r in config.client().await?.ruimtes().await.map_err(|e| e.to_string())? {
                println!("{:<30} {:<12} {}", r.naam, r.rol, r.name);
            }
            Ok(())
        }
        Actie::Koppel { ruimte, map } => {
            let ruimtes = config.client().await?.ruimtes().await.map_err(|e| e.to_string())?;
            let r = ruimtes
                .iter()
                .find(|r| r.name == ruimte || r.naam == ruimte)
                .or_else(|| ruimtes.iter().find(|r| r.naam.eq_ignore_ascii_case(&ruimte)))
                .ok_or_else(|| format!("ruimte \"{ruimte}\" niet gevonden (zie `openaec-storage ruimtes`)"))?
                .clone();
            std::fs::create_dir_all(&map).map_err(|e| e.to_string())?;
            let lokaal = volledig(&map).map_err(|e| e.to_string())?;
            if config.koppelingen.iter().any(|k| k.lokaal == lokaal) {
                return Err("deze map is al gekoppeld".into());
            }
            config.koppelingen.push(Koppeling { ruimte: r.name, ruimte_naam: r.naam.clone(), lokaal: lokaal.clone(), uitgesloten: Vec::new() });
            config.bewaar().map_err(|e| e.to_string())?;
            println!("{} gekoppeld aan {}", r.naam, lokaal.display());
            Ok(())
        }
        Actie::Ontkoppel { map } => {
            let lokaal = volledig(&map).unwrap_or(map);
            let voor = config.koppelingen.len();
            config.koppelingen.retain(|k| k.lokaal != lokaal);
            if config.koppelingen.len() == voor {
                return Err("deze map is niet gekoppeld".into());
            }
            config.bewaar().map_err(|e| e.to_string())?;
            println!("Ontkoppeld; de bestanden blijven staan.");
            Ok(())
        }
        Actie::Selectie { map, zonder } => {
            let lokaal = volledig(&map).unwrap_or(map);
            let k = config.koppelingen.iter_mut().find(|k| k.lokaal == lokaal).ok_or("deze map is niet gekoppeld")?;
            k.uitgesloten = zonder.iter().map(|z| z.trim_matches('/').to_string()).filter(|z| !z.is_empty()).collect();
            let tekst = if k.uitgesloten.is_empty() { "alle mappen staan weer op deze computer".to_string() } else { format!("niet op deze computer: {}", k.uitgesloten.join(", ")) };
            config.bewaar().map_err(|e| e.to_string())?;
            println!("Mapselectie opgeslagen ({tekst}); de volgende sync past het toe.");
            Ok(())
        }
        Actie::Status => {
            println!("Omgeving: {}", config.basis.as_deref().unwrap_or("(niet ingelogd)"));
            for k in &config.koppelingen {
                println!("  {} -> {}", k.ruimte_naam, Path::new(&k.lokaal).display());
                if !k.uitgesloten.is_empty() {
                    println!("      niet op deze computer: {}", k.uitgesloten.join(", "));
                }
            }
            Ok(())
        }
        Actie::Sync => sync(&mut config, None).await,
        Actie::Importeer { bron, ruimte, doel, parallel } => {
            let client = config.client().await?;
            let ruimtes = client.ruimtes().await.map_err(|e| e.to_string())?;
            let r = ruimtes
                .iter()
                .find(|r| r.name == ruimte || r.naam == ruimte || r.naam.eq_ignore_ascii_case(&ruimte))
                .ok_or_else(|| format!("ruimte \"{ruimte}\" niet gevonden (zie `openaec-storage ruimtes`)"))?;
            if !bron.is_dir() {
                return Err(format!("{} is geen map", bron.display()));
            }
            println!("Importeren van {} naar {}{} ({} tegelijk)…", bron.display(), r.naam, if doel.is_empty() { String::new() } else { format!("/{doel}") }, parallel);
            let opslag = ClientOpslag::new(client.clone(), &r.name);
            let begin = Instant::now();
            let (mut n, mut mb) = (0u64, 0f64);
            let rapport = openaec_sync::importeer::importeer(&opslag, &bron, &doel, parallel, |v| {
                n += 1;
                if !v.overgeslagen && v.fout.is_none() {
                    mb += v.bytes as f64 / 1_048_576.0;
                }
                if let Some(f) = &v.fout {
                    eprintln!("  fout: {}: {f}", v.pad);
                }
                if n % 25 == 0 {
                    let s = begin.elapsed().as_secs_f64().max(0.1);
                    println!("  {n} bestanden, {mb:.0} MB geüpload ({:.1} MB/s)", mb / s);
                }
            })
            .await
            .map_err(|e| e.to_string())?;
            println!(
                "Klaar: {} geüpload ({:.0} MB), {} al aanwezig, {} fout(en) in {:.0} s.",
                rapport.geupload,
                rapport.bytes as f64 / 1_048_576.0,
                rapport.overgeslagen,
                rapport.fouten.len(),
                begin.elapsed().as_secs_f64()
            );
            if rapport.fouten.is_empty() { Ok(()) } else { Err("niet alles is geüpload; start opnieuw om het restant te doen".into()) }
        }
        Actie::Bewaak => bewaak(&mut config).await,
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match voer_uit(Opdracht::parse().actie).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => {
            eprintln!("{f}");
            ExitCode::FAILURE
        }
    }
}
