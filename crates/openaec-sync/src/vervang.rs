//! Veilig een lokaal bestand vervangen of verwijderen (ontwerp samenwerken §5.6, "Terugzetten in de lokale map").
//!
//! De motor vervangt een lokaal bestand (een download) of verwijdert het (op de server verwijderd, of in een map die
//! niet meer op deze computer hoort) nooit rechtstreeks. Een programma kan het bestand open hebben, en de gebruiker kan
//! net opslaan. Daarom gaat het zo:
//!
//! 1. **Journaal eerst.** In `.openaec/journaal/` komt per vervanging het pad, de bron, de hash van de verwerkte inhoud,
//!    de nieuwe inhoud en de stap. Geschreven met fsync; bij elke stap bijgewerkt.
//! 2. **Verplaatsen.** Het lokale bestand gaat met één rename naar `.openaec/bron/`. Lukt dat niet, dan blijft alles
//!    staan en probeert een volgende ronde het opnieuw.
//! 3. **Plaatsen.** De nieuwe inhoud komt op het pad met een bewerking die nooit een bestaand bestand vervangt
//!    ([`plaats`]). Staat er intussen een bestand, dan blijft dat staan als lokale wijziging.
//! 4. **Afronden** ([`rond_af`]). Pas als geen proces de bron nog open of in het geheugen gekoppeld heeft
//!    ([`in_gebruik`]), volgt een laatste controle van de inhoud. Gelijk aan de verwerkte inhoud: de bron gaat weg.
//!    Anders verwerkt de motor haar als lokale wijziging. Kan de sync het niet vaststellen, dan blijft de bron staan.
//!
//! Bij het begin van elke ronde maakt [`herstel`] onafgemaakte vervangingen af of terug, vóór de vergelijking. Een bron
//! valt nooit onder opruimen. De motor bekijkt de bronnen elke ronde ([`rond_af`]); een wijziging die alleen in een bron
//! gebeurt, komt zo ook mee.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::motor::{kenmerk, TOESTANDSMAP};

const JOURNAAL: &str = "journaal";
const BRON: &str = "bron";
const NIEUW: &str = "nieuw";

/// Een bestand op de server in een bepaalde versie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Versie {
    pub bestand: String,
    pub versie: u64,
}

/// Hoe ver een vervanging is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stap {
    /// Vastgelegd; het lokale bestand staat nog op zijn pad.
    Journaal,
    /// Het lokale bestand staat in de bron.
    Verplaatst,
    /// De nieuwe inhoud staat op het pad, met deze grootte en wijzigingstijd.
    Geplaatst { grootte: u64, mtime_ns: u128 },
}

/// Eén vervanging of verwijdering in het journaal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Vervanging {
    /// Naam van de bron in `.openaec/bron/` en van het journaal.
    pub id: String,
    /// Pad hier (met '/').
    pub pad: String,
    /// Pad op de server (de sleutel in de toestand).
    pub sleutel: String,
    /// BLAKE3 van de verwerkte inhoud: wat er op het pad stond toen de sync besliste.
    pub verwacht: String,
    /// De versie op de server waarvan die inhoud een kopie is.
    pub basis: Versie,
    /// De nieuwe inhoud; None bij een verwijdering.
    pub nieuw: Option<Versie>,
    pub stap: Stap,
}

/// Wat de motor wil: `lokaal` (het pad hier) vervangen door `nieuw`, of verwijderen (None).
pub(crate) struct Opdracht {
    pub root: PathBuf,
    pub pad: String,
    pub sleutel: String,
    pub lokaal: PathBuf,
    /// De versie en het kenmerk (grootte, wijzigingstijd) uit de toestand; None als het bestand daar niet in staat.
    pub bekend: Option<(Versie, (u64, u128))>,
    /// Het tijdelijke bestand met de nieuwe inhoud (in `.openaec/nieuw/`) en zijn versie op de server.
    pub nieuw: Option<(PathBuf, Versie)>,
}

/// Uitkomst van [`vervang`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Uitkomst {
    /// De nieuwe inhoud staat op het pad, met dit kenmerk.
    Geplaatst((u64, u128)),
    /// Het lokale bestand is weg (of was er al niet meer).
    Verwijderd,
    /// Het lokale bestand is anders dan de toestand zegt: niets gedaan. De volgende ronde ziet de wijziging.
    LokaalGewijzigd,
    /// Er stond intussen een ander bestand op het pad: dat blijft staan.
    PadBezet,
}

fn map(root: &Path, naam: &str) -> PathBuf {
    root.join(TOESTANDSMAP).join(naam)
}

pub(crate) fn bron(root: &Path, id: &str) -> PathBuf {
    map(root, BRON).join(id)
}

fn journaal(root: &Path, id: &str) -> PathBuf {
    map(root, JOURNAAL).join(format!("{id}.json"))
}

/// Een nieuw tijdelijk bestand voor een download, in `.openaec/nieuw/` (de map bestaat daarna).
pub(crate) fn nieuw_pad(root: &Path) -> io::Result<PathBuf> {
    let map = map(root, NIEUW);
    fs::create_dir_all(&map)?;
    Ok(map.join(nieuw_id()))
}

fn nieuw_id() -> String {
    let nu = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}{:08x}", nu, rand::random::<u32>())
}

pub(crate) fn hash(pad: &Path) -> io::Result<String> {
    let mut h = blake3::Hasher::new();
    h.update_reader(File::open(pad)?)?;
    Ok(h.finalize().to_hex().to_string())
}

/// Maak een map duurzaam (de namen erin), waar het systeem dat kent.
fn sync_map(map: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(map)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = map;
    Ok(())
}

/// Schrijf het journaal van `v` duurzaam weg (tijdelijk bestand, fsync, rename, fsync van de map).
fn schrijf(root: &Path, v: &Vervanging) -> io::Result<()> {
    let map = map(root, JOURNAAL);
    fs::create_dir_all(&map)?;
    let tijdelijk = map.join(format!("{}.tmp", v.id));
    let mut f = File::create(&tijdelijk)?;
    f.write_all(&serde_json::to_vec(v)?)?;
    f.sync_all()?;
    fs::rename(&tijdelijk, journaal(root, &v.id))?;
    sync_map(&map)
}

fn verwijder_journaal(root: &Path, id: &str) -> io::Result<()> {
    match fs::remove_file(journaal(root, id)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Alle vervangingen in het journaal. Een journaal dat niet te lezen is, blijft staan (met een logregel).
pub(crate) fn lees(root: &Path) -> Vec<Vervanging> {
    let Ok(items) = fs::read_dir(map(root, JOURNAAL)) else { return Vec::new() };
    let mut uit = Vec::new();
    for item in items.flatten() {
        let pad = item.path();
        if pad.extension().is_some_and(|e| e == "json") {
            match fs::read(&pad).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
                Ok(v) => uit.push(v),
                Err(e) => log::warn!("journaal {} niet te lezen: {e}", pad.display()),
            }
        }
    }
    uit.sort_by(|a: &Vervanging, b| a.id.cmp(&b.id));
    uit
}

/// Vervang `o.lokaal` door de nieuwe inhoud, of verwijder het (zie de uitleg bovenaan). Een fout laat het lokale
/// bestand op zijn pad, of in de bron met het journaal erbij; [`herstel`] maakt het dan bij de volgende ronde af.
pub(crate) fn vervang(o: Opdracht) -> io::Result<Uitkomst> {
    let gevolg = vervang_binnen(&o);
    // Wat niet geplaatst is, hoeft niet te blijven.
    if let Some((nieuw, _)) = &o.nieuw {
        let _ = fs::remove_file(nieuw);
    }
    gevolg
}

fn vervang_binnen(o: &Opdracht) -> io::Result<Uitkomst> {
    if fs::symlink_metadata(&o.lokaal).is_err() {
        // Er staat hier niets (meer): niets te beschermen.
        return match &o.nieuw {
            Some((nieuw, _)) => plaats_nieuw(nieuw, &o.lokaal),
            None => Ok(Uitkomst::Verwijderd),
        };
    }
    let Some((basis, bekend)) = &o.bekend else {
        // Een bestand dat de toestand niet kent, is van de gebruiker: blijft staan.
        return Ok(if o.nieuw.is_some() { Uitkomst::PadBezet } else { Uitkomst::LokaalGewijzigd });
    };
    // De verwerkte inhoud: alleen als het bestand vóór en na het lezen nog is wat de toestand zegt.
    if kenmerk(&o.lokaal)? != *bekend {
        return Ok(Uitkomst::LokaalGewijzigd);
    }
    let verwacht = hash(&o.lokaal)?;
    if kenmerk(&o.lokaal)? != *bekend {
        return Ok(Uitkomst::LokaalGewijzigd);
    }
    if let Some((nieuw, _)) = &o.nieuw {
        // De nieuwe inhoud eerst duurzaam, zodat het pad na een stroomstoring niet leeg of half is.
        File::options().write(true).open(nieuw)?.sync_all()?;
    }
    let mut v = Vervanging {
        id: nieuw_id(),
        pad: o.pad.clone(),
        sleutel: o.sleutel.clone(),
        verwacht,
        basis: basis.clone(),
        nieuw: o.nieuw.as_ref().map(|(_, versie)| versie.clone()),
        stap: Stap::Journaal,
    };
    schrijf(&o.root, &v)?;
    let bron = bron(&o.root, &v.id);
    let verplaatst = fs::create_dir_all(bron.parent().unwrap()).and_then(|()| fs::rename(&o.lokaal, &bron));
    if let Err(e) = verplaatst {
        // Bijvoorbeeld Windows bij een programma dat het bestand zonder FILE_SHARE_DELETE open heeft: alles blijft.
        verwijder_journaal(&o.root, &v.id)?;
        return Err(e);
    }
    v.stap = Stap::Verplaatst;
    schrijf(&o.root, &v)?;
    let Some((nieuw, _)) = &o.nieuw else { return Ok(Uitkomst::Verwijderd) };
    let k = kenmerk(nieuw)?;
    match plaats(nieuw, &o.lokaal) {
        Ok(()) => {
            sync_map(o.lokaal.parent().unwrap_or(&o.root))?;
            v.stap = Stap::Geplaatst { grootte: k.0, mtime_ns: k.1 };
            schrijf(&o.root, &v)?;
            Ok(Uitkomst::Geplaatst(k))
        }
        // Het bestand dat er nu staat, is van de gebruiker; de bron wacht op afronden.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(Uitkomst::PadBezet),
        Err(e) => {
            // Terug zoals het was; lukt ook dat niet, dan doet herstel het bij de volgende ronde.
            if plaats(&bron, &o.lokaal).is_ok() {
                verwijder_journaal(&o.root, &v.id)?;
            }
            Err(e)
        }
    }
}

/// Plaats `nieuw` op `lokaal`, waar nog niets staat (de map erboven komt er zo nodig bij).
fn plaats_nieuw(nieuw: &Path, lokaal: &Path) -> io::Result<Uitkomst> {
    if let Some(map) = lokaal.parent() {
        fs::create_dir_all(map)?;
    }
    let k = kenmerk(nieuw)?;
    match plaats(nieuw, lokaal) {
        Ok(()) => Ok(Uitkomst::Geplaatst(k)),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(Uitkomst::PadBezet),
        Err(e) => Err(e),
    }
}

/// Wat [`herstel`] opleverde.
#[derive(Debug, Default)]
pub(crate) struct Herstel {
    /// Sleutels die de toestand als (versie, kenmerk) moet kennen: de nieuwe inhoud stond er al, de toestand nog niet.
    pub bijwerken: Vec<(String, Versie, (u64, u128))>,
    /// Sleutels waarvan de bron niet terug kon: deze ronde niet aanraken.
    pub geblokkeerd: BTreeSet<String>,
}

/// Maak onafgemaakte vervangingen af of terug, vóór de vergelijking van een ronde. `kent(sleutel, versie)`: staat
/// die versie al in de bewaarde toestand? Zo ziet de vergelijking nooit een "lokaal verwijderd" bestand dat alleen
/// halverwege een vervanging was. Wat daarna nog een bron heeft, wacht op [`rond_af`].
pub(crate) fn herstel(root: &Path, lokaal: impl Fn(&str) -> PathBuf, kent: impl Fn(&str, &Versie) -> bool) -> Herstel {
    let mut uit = Herstel::default();
    // Tijdelijke bestanden van een onderbroken ronde: alleen downloads en kopieën, nooit het werk zelf.
    for naam in [NIEUW, JOURNAAL] {
        for item in fs::read_dir(map(root, naam)).into_iter().flatten().flatten() {
            if naam == NIEUW || item.path().extension().is_some_and(|e| e == "tmp") {
                let _ = fs::remove_file(item.path());
            }
        }
    }
    for v in lees(root) {
        let bron = bron(root, &v.id);
        let hier = lokaal(&v.pad);
        if fs::symlink_metadata(&bron).is_err() {
            // Niet verplaatst (of al afgerond): niets te doen.
            let _ = verwijder_journaal(root, &v.id);
            continue;
        }
        let pad_er = fs::symlink_metadata(&hier).is_ok();
        let Some(nieuw) = &v.nieuw else {
            // Een verwijdering is af zodra de bron er is: nooit terugzetten (dan zou een elders verwijderd bestand
            // als nieuw terugkomen). De bron wacht op afronden.
            continue;
        };
        let bewaard = kent(&v.sleutel, nieuw);
        match v.stap {
            Stap::Geplaatst { grootte, mtime_ns } if !bewaard && pad_er => {
                uit.bijwerken.push((v.sleutel.clone(), nieuw.clone(), (grootte, mtime_ns)));
            }
            _ if bewaard || pad_er => {}
            // Halverwege, en het pad is leeg: de bron terug, alsof er niets gebeurde.
            _ => {
                let terug = hier.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| plaats(&bron, &hier));
                match terug.and_then(|()| verwijder_journaal(root, &v.id)) {
                    Ok(()) => log::info!("{}: onderbroken vervanging teruggezet", v.pad),
                    Err(e) => {
                        log::warn!("{}: bron niet terug te zetten: {e}", v.pad);
                        uit.geblokkeerd.insert(v.sleutel.clone());
                    }
                }
            }
        }
    }
    uit
}

/// Rond de open vervangingen af waarvan geen proces de bron nog open of gekoppeld heeft: is de inhoud gelijk aan de
/// verwerkte inhoud, dan gaan bron en journaal weg. Geeft de vervangingen waarvan de bron afwijkt; de motor verwerkt
/// die als lokale wijziging.
pub(crate) fn rond_af(root: &Path) -> Vec<Vervanging> {
    let open: Vec<Vervanging> = lees(root).into_iter().filter(|v| fs::symlink_metadata(bron(root, &v.id)).is_ok()).collect();
    if open.is_empty() {
        return Vec::new();
    }
    let paden: Vec<PathBuf> = open.iter().map(|v| bron(root, &v.id)).collect();
    let mut afwijkend = Vec::new();
    for ((v, pad), gebruik) in open.into_iter().zip(&paden).zip(in_gebruik(&paden)) {
        match gebruik {
            // Eerst de controle op processen, dan pas de inhoud: wie daartussen schrijft en sluit, staat in de inhoud.
            Gebruik::Vrij => match hash(pad) {
                Ok(h) if h == v.verwacht => {
                    if let Err(e) = klaar(root, &v) {
                        log::warn!("{}: bron niet op te ruimen: {e}", v.pad);
                    }
                }
                Ok(_) => afwijkend.push(v),
                Err(e) => log::warn!("{}: bron niet te lezen: {e}", v.pad),
            },
            Gebruik::Bezet => {}
            Gebruik::Onbekend => log::info!("{}: niet vast te stellen of de bron nog open is; ze blijft bewaakt", v.pad),
        }
    }
    afwijkend
}

/// Bron (als die er nog is) en journaal van een afgehandelde vervanging weg.
pub(crate) fn klaar(root: &Path, v: &Vervanging) -> io::Result<()> {
    match fs::remove_file(bron(root, &v.id)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    verwijder_journaal(root, &v.id)
}

// ---- plaatsen zonder te vervangen ----

/// Zet `van` op `naar` met één bewerking die weigert (`AlreadyExists`) als er op `naar` al iets staat. Nooit
/// `std::fs::rename`: die vervangt een bestaand doel.
#[cfg(windows)]
pub(crate) fn plaats(van: &Path, naar: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    // Zonder MOVEFILE_REPLACE_EXISTING: een bestaand doel geeft ERROR_ALREADY_EXISTS.
    if unsafe { MoveFileExW(breed(van).as_ptr(), breed(naar).as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn breed(pad: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    pad.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(unix)]
pub(crate) fn plaats(van: &Path, naar: &Path) -> io::Result<()> {
    if let Some(r) = hernoem_zonder_vervangen(van, naar) {
        return r;
    }
    // Een bestandssysteem zonder zo'n rename: een harde link weigert ook een bestaand doel. Daarna `van` weg, maar alleen
    // als het nog hetzelfde bestand is.
    use std::os::unix::fs::MetadataExt;
    fs::hard_link(van, naar)?;
    let (a, b) = (fs::symlink_metadata(van)?, fs::symlink_metadata(naar)?);
    if (a.dev(), a.ino()) == (b.dev(), b.ino()) {
        fs::remove_file(van)?;
    }
    Ok(())
}

#[cfg(unix)]
fn c_pad(pad: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(pad.as_os_str().as_bytes()).map_err(io::Error::other)
}

/// `renameat2` met `RENAME_NOREPLACE`; None als het bestandssysteem dat niet kent.
#[cfg(target_os = "linux")]
fn hernoem_zonder_vervangen(van: &Path, naar: &Path) -> Option<io::Result<()>> {
    let (a, b) = match (c_pad(van), c_pad(naar)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return Some(Err(e)),
    };
    let r = unsafe { libc::renameat2(libc::AT_FDCWD, a.as_ptr(), libc::AT_FDCWD, b.as_ptr(), libc::RENAME_NOREPLACE) };
    if r == 0 {
        return Some(Ok(()));
    }
    let e = io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::EINVAL | libc::ENOSYS | libc::EOPNOTSUPP) => None,
        _ => Some(Err(e)),
    }
}

/// `renamex_np` met `RENAME_EXCL`; None als het bestandssysteem dat niet kent.
#[cfg(target_os = "macos")]
fn hernoem_zonder_vervangen(van: &Path, naar: &Path) -> Option<io::Result<()>> {
    let (a, b) = match (c_pad(van), c_pad(naar)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return Some(Err(e)),
    };
    if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) } == 0 {
        return Some(Ok(()));
    }
    let e = io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ENOTSUP | libc::EINVAL) => None,
        _ => Some(Err(e)),
    }
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn hernoem_zonder_vervangen(_van: &Path, _naar: &Path) -> Option<io::Result<()>> {
    None
}

// ---- heeft een proces de bron nog open? ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gebruik {
    /// Geen proces heeft het bestand open of gekoppeld.
    Vrij,
    Bezet,
    /// Niet vast te stellen: de bron blijft bewaakt.
    Onbekend,
}

/// Per pad: heeft een proces het bestand open of in het geheugen gekoppeld? Linux: `fd` en `maps` in `/proc/<pid>/`
/// van elk proces dat te bekijken is, en een schrijf-lease voor de processen die dat niet zijn (andere gebruikers).
#[cfg(target_os = "linux")]
pub(crate) fn in_gebruik(paden: &[PathBuf]) -> Vec<Gebruik> {
    use std::os::unix::fs::MetadataExt;
    let doelen: Vec<Option<(u64, u64)>> = paden.iter().map(|p| fs::metadata(p).ok().map(|m| (m.dev(), m.ino()))).collect();
    let mut bezet = vec![false; paden.len()];
    let Ok(processen) = fs::read_dir("/proc") else { return vec![Gebruik::Onbekend; paden.len()] };
    for proces in processen.flatten() {
        if !proces.file_name().to_string_lossy().bytes().all(|c| c.is_ascii_digit()) {
            continue;
        }
        for fd in fs::read_dir(proces.path().join("fd")).into_iter().flatten().flatten() {
            let Ok(m) = fs::metadata(fd.path()) else { continue };
            for (i, d) in doelen.iter().enumerate() {
                bezet[i] |= *d == Some((m.dev(), m.ino()));
            }
        }
        // adres rechten offset apparaat inode [pad]. Alleen de inode vergelijken (btrfs geeft hier een ander
        // apparaat dan stat): een vondst te veel houdt een bron alleen langer bewaakt.
        let maps = fs::read_to_string(proces.path().join("maps")).unwrap_or_default();
        for ino in maps.lines().filter_map(|r| r.split_whitespace().nth(4)?.parse::<u64>().ok()).filter(|i| *i != 0) {
            for (i, d) in doelen.iter().enumerate() {
                bezet[i] |= d.is_some_and(|d| d.1 == ino);
            }
        }
    }
    paden.iter().zip(bezet).map(|(p, b)| if b { Gebruik::Bezet } else { lease(p) }).collect()
}

/// Een schrijf-lease (`F_SETLEASE`) krijgt alleen wie het bestand als enige open heeft, ook tegenover processen van
/// andere gebruikers en koppelingen in het geheugen; daarna meteen weer los. Lukt het om een andere reden niet (een
/// bestandssysteem zonder leases, een bestand van een ander), dan is het niet vast te stellen.
#[cfg(target_os = "linux")]
fn lease(pad: &Path) -> Gebruik {
    use std::os::fd::AsRawFd;
    // Niet in libc voor glibc; gelijk op alle gangbare architecturen.
    const F_SETSIG: libc::c_int = 10;
    let Ok(f) = File::open(pad) else { return Gebruik::Onbekend };
    let fd = f.as_raw_fd();
    let r = unsafe {
        // Wil een ander het bestand openen terwijl we de lease hebben, dan krijgen we een signaal. Standaard is dat
        // SIGIO, en dat stopt het proces; SIGURG wordt standaard genegeerd.
        libc::fcntl(fd, F_SETSIG, libc::SIGURG);
        libc::fcntl(fd, libc::F_SETLEASE, libc::F_WRLCK as libc::c_int)
    };
    if r == 0 {
        unsafe { libc::fcntl(fd, libc::F_SETLEASE, libc::F_UNLCK as libc::c_int) };
        return Gebruik::Vrij;
    }
    match io::Error::last_os_error().raw_os_error() {
        Some(libc::EAGAIN) => Gebruik::Bezet,
        _ => Gebruik::Onbekend,
    }
}

/// macOS: `proc_listpidspath` uit libproc loopt met `proc_pidinfo` de open bestanden en de gekoppelde stukken
/// geheugen van elk proces af. Processen van andere gebruikers kan het niet bekijken.
#[cfg(target_os = "macos")]
pub(crate) fn in_gebruik(paden: &[PathBuf]) -> Vec<Gebruik> {
    use std::os::raw::{c_char, c_int, c_void};
    extern "C" {
        fn proc_listpidspath(soort: u32, info: u32, pad: *const c_char, vlaggen: u32, buffer: *mut c_void, grootte: c_int) -> c_int;
    }
    const PROC_ALL_PIDS: u32 = 1;
    // Alleen bekijken (zoals de Finder met O_EVTONLY) telt niet: zo iemand kan niet schrijven.
    const PROC_LISTPIDSPATH_EXCLUDE_EVTONLY: u32 = 2;
    paden
        .iter()
        .map(|p| {
            let Ok(c) = c_pad(p) else { return Gebruik::Onbekend };
            let mut pids = vec![0 as c_int; 1024];
            let grootte = (pids.len() * std::mem::size_of::<c_int>()) as c_int;
            let n = unsafe {
                proc_listpidspath(PROC_ALL_PIDS, 0, c.as_ptr(), PROC_LISTPIDSPATH_EXCLUDE_EVTONLY, pids.as_mut_ptr().cast(), grootte)
            };
            match n {
                0 => Gebruik::Vrij,
                n if n > 0 => Gebruik::Bezet,
                _ => Gebruik::Onbekend,
            }
        })
        .collect()
}

/// Windows: de Restart Manager (`RmGetList`) noemt elk proces dat het bestand gebruikt.
#[cfg(windows)]
pub(crate) fn in_gebruik(paden: &[PathBuf]) -> Vec<Gebruik> {
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::RestartManager::{
        RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY,
    };
    paden
        .iter()
        .map(|p| unsafe {
            let mut sessie = 0u32;
            let mut sleutel = [0u16; CCH_RM_SESSION_KEY as usize + 1];
            if RmStartSession(&mut sessie, 0, sleutel.as_mut_ptr()) != ERROR_SUCCESS {
                return Gebruik::Onbekend;
            }
            let naam = breed(p);
            let namen = [naam.as_ptr()];
            let gebruik = if RmRegisterResources(sessie, 1, namen.as_ptr(), 0, std::ptr::null(), 0, std::ptr::null()) != ERROR_SUCCESS {
                Gebruik::Onbekend
            } else {
                let (mut nodig, mut aantal, mut redenen) = (0u32, 0u32, 0u32);
                match RmGetList(sessie, &mut nodig, &mut aantal, std::ptr::null_mut(), &mut redenen) {
                    ERROR_SUCCESS if nodig == 0 => Gebruik::Vrij,
                    ERROR_SUCCESS | ERROR_MORE_DATA => Gebruik::Bezet,
                    _ => Gebruik::Onbekend,
                }
            };
            RmEndSession(sessie);
            gebruik
        })
        .collect()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) fn in_gebruik(paden: &[PathBuf]) -> Vec<Gebruik> {
    vec![Gebruik::Onbekend; paden.len()]
}
