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
    sync_map(&map)?;
    #[cfg(test)]
    proeven::NA_STAP.with(|n| n.borrow_mut().as_mut().map_or(Ok(()), |f| f(v)))?;
    Ok(())
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

#[cfg(test)]
pub(crate) mod proeven {
    //! De stappen van één vervanging, met onderbrekingen en andere programma's ertussen. De proeven op het niveau van
    //! een hele ronde staan in `tests/vervang.rs`.

    use std::cell::RefCell;

    use super::*;

    type NaStap = Box<dyn FnMut(&Vervanging) -> io::Result<()>>;

    thread_local! {
        /// Wordt aangeroepen nadat een stap in het journaal staat: een fout is een onderbreking op die plek.
        pub(crate) static NA_STAP: RefCell<Option<NaStap>> = const { RefCell::new(None) };
    }

    fn na_stap(f: impl FnMut(&Vervanging) -> io::Result<()> + 'static) {
        NA_STAP.with(|n| *n.borrow_mut() = Some(Box::new(f)));
    }

    fn onderbreek_na(stap: fn(&Stap) -> bool) {
        na_stap(move |v| if stap(&v.stap) { Err(io::Error::other("onderbroken (proef)")) } else { Ok(()) });
    }

    struct Proef {
        _dir: tempfile::TempDir,
        root: PathBuf,
        oud: (u64, u128),
    }

    impl Proef {
        fn new() -> Self {
            NA_STAP.with(|n| *n.borrow_mut() = None);
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_path_buf();
            fs::write(root.join("a.txt"), "oud").unwrap();
            let oud = kenmerk(&root.join("a.txt")).unwrap();
            Self { _dir: dir, root, oud }
        }

        fn a(&self) -> PathBuf {
            self.root.join("a.txt")
        }

        fn lees(&self) -> Option<String> {
            fs::read_to_string(self.a()).ok()
        }

        /// Vervang a.txt (versie 1) door `nieuw` (versie 2), of verwijder het (None).
        fn opdracht(&self, nieuw: Option<&str>) -> Opdracht {
            let nieuw = nieuw.map(|inhoud| {
                let p = nieuw_pad(&self.root).unwrap();
                fs::write(&p, inhoud).unwrap();
                (p, Versie { bestand: "b1".into(), versie: 2 })
            });
            Opdracht {
                root: self.root.clone(),
                pad: "a.txt".into(),
                sleutel: "a.txt".into(),
                lokaal: self.a(),
                bekend: Some((Versie { bestand: "b1".into(), versie: 1 }, self.oud)),
                nieuw,
            }
        }

        /// Herstel zoals bij het begin van een ronde; `kent`: staat versie 2 al in de bewaarde toestand?
        fn herstel(&self, kent: bool) -> Herstel {
            let root = self.root.clone();
            herstel(&self.root, move |rel| root.join(rel), move |_, n| kent && n.versie == 2)
        }

        fn aantal(&self, naam: &str) -> usize {
            fs::read_dir(map(&self.root, naam)).map(|d| d.count()).unwrap_or(0)
        }
    }

    #[test]
    fn onderbreking_na_het_journaal_laat_alles_staan() {
        let p = Proef::new();
        onderbreek_na(|s| *s == Stap::Journaal);
        assert!(vervang_binnen(&p.opdracht(Some("nieuw"))).is_err());
        let h = p.herstel(false);
        assert!(h.bijwerken.is_empty() && h.geblokkeerd.is_empty());
        assert_eq!(p.lees().as_deref(), Some("oud"));
        assert_eq!(kenmerk(&p.a()).unwrap(), p.oud, "ongewijzigd: de volgende ronde downloadt gewoon opnieuw");
        assert_eq!((p.aantal(JOURNAAL), p.aantal(BRON), p.aantal(NIEUW)), (0, 0, 0));
    }

    #[test]
    fn onderbreking_na_het_verplaatsen_zet_de_bron_terug() {
        let p = Proef::new();
        onderbreek_na(|s| *s == Stap::Verplaatst);
        assert!(vervang_binnen(&p.opdracht(Some("nieuw"))).is_err());
        assert_eq!(p.lees(), None, "halverwege: het pad is leeg");
        p.herstel(false);
        // Terug met dezelfde wijzigingstijd: de vergelijking ziet geen verwijderd bestand en ook geen wijziging.
        assert_eq!(p.lees().as_deref(), Some("oud"));
        assert_eq!(kenmerk(&p.a()).unwrap(), p.oud);
        assert_eq!((p.aantal(JOURNAAL), p.aantal(BRON), p.aantal(NIEUW)), (0, 0, 0));
    }

    #[test]
    fn onderbreking_tussen_verplaatsen_en_journaal_zet_de_bron_terug() {
        // De rename is gebeurd, het journaal zegt nog "journaal".
        let p = Proef::new();
        let root = p.root.clone();
        na_stap(move |v| {
            if v.stap == Stap::Journaal {
                fs::create_dir_all(map(&root, BRON))?;
                fs::rename(root.join("a.txt"), bron(&root, &v.id))?;
                return Err(io::Error::other("onderbroken (proef)"));
            }
            Ok(())
        });
        assert!(vervang_binnen(&p.opdracht(Some("nieuw"))).is_err());
        p.herstel(false);
        assert_eq!(p.lees().as_deref(), Some("oud"));
        assert_eq!((p.aantal(JOURNAAL), p.aantal(BRON)), (0, 0));
    }

    #[test]
    fn onderbreking_na_het_plaatsen_werkt_de_toestand_bij() {
        let p = Proef::new();
        onderbreek_na(|s| matches!(s, Stap::Geplaatst { .. }));
        assert!(vervang_binnen(&p.opdracht(Some("nieuw"))).is_err());
        assert_eq!(p.lees().as_deref(), Some("nieuw"));
        let nieuw = kenmerk(&p.a()).unwrap();
        // De toestand was nog niet bewaard: herstel geeft versie 2 met het kenmerk van het geplaatste bestand.
        let h = p.herstel(false);
        assert_eq!(h.bijwerken, vec![("a.txt".to_string(), Versie { bestand: "b1".into(), versie: 2 }, nieuw)]);
        assert_eq!(p.aantal(BRON), 1, "de bron wacht op afronden");
        assert!(rond_af(&p.root).is_empty());
        assert_eq!((p.aantal(JOURNAAL), p.aantal(BRON)), (0, 0));
        assert_eq!(p.lees().as_deref(), Some("nieuw"));
    }

    #[test]
    fn onderbreking_voor_het_afronden_rondt_bij_de_volgende_ronde_af() {
        let p = Proef::new();
        assert!(matches!(vervang(p.opdracht(Some("nieuw"))).unwrap(), Uitkomst::Geplaatst(_)));
        assert_eq!((p.aantal(JOURNAAL), p.aantal(BRON), p.aantal(NIEUW)), (1, 1, 0));
        // De toestand met versie 2 is bewaard; dan is er niets bij te werken.
        let h = p.herstel(true);
        assert!(h.bijwerken.is_empty() && h.geblokkeerd.is_empty());
        assert!(rond_af(&p.root).is_empty());
        assert_eq!((p.aantal(JOURNAAL), p.aantal(BRON)), (0, 0));
    }

    #[test]
    fn geplaatst_en_daarna_weg_zonder_bewaarde_toestand_zet_de_bron_terug() {
        let p = Proef::new();
        onderbreek_na(|s| matches!(s, Stap::Geplaatst { .. }));
        assert!(vervang_binnen(&p.opdracht(Some("nieuw"))).is_err());
        fs::remove_file(p.a()).unwrap();
        // De toestand kent versie 1 nog: terug naar versie 1, dan downloadt de volgende ronde opnieuw.
        let h = p.herstel(false);
        assert!(h.bijwerken.is_empty());
        assert_eq!(p.lees().as_deref(), Some("oud"));
        assert_eq!((p.aantal(JOURNAAL), p.aantal(BRON)), (0, 0));
    }

    #[test]
    fn verwijdering_onderbroken_na_het_verplaatsen_komt_niet_terug() {
        let p = Proef::new();
        onderbreek_na(|s| *s == Stap::Verplaatst);
        assert!(vervang_binnen(&p.opdracht(None)).is_err());
        let h = p.herstel(false);
        assert!(h.bijwerken.is_empty() && h.geblokkeerd.is_empty());
        assert_eq!(p.lees(), None, "een verwijdering is af zodra de bron er is");
        assert_eq!(p.aantal(BRON), 1);
        assert!(rond_af(&p.root).is_empty());
        assert_eq!((p.aantal(JOURNAAL), p.aantal(BRON)), (0, 0));
    }

    #[test]
    fn bestand_dat_vlak_voor_de_plaatsing_verschijnt_blijft_staan() {
        let p = Proef::new();
        let a = p.a();
        na_stap(move |v| {
            if v.stap == Stap::Verplaatst {
                fs::write(&a, "van de gebruiker")?;
            }
            Ok(())
        });
        assert_eq!(vervang(p.opdracht(Some("nieuw"))).unwrap(), Uitkomst::PadBezet);
        assert_eq!(p.lees().as_deref(), Some("van de gebruiker"));
        assert_eq!(p.aantal(NIEUW), 0);
        // De bron is de verwerkte inhoud (versie 1 staat op de server): mag weg.
        assert!(rond_af(&p.root).is_empty());
        assert_eq!((p.aantal(JOURNAAL), p.aantal(BRON)), (0, 0));
        assert_eq!(p.lees().as_deref(), Some("van de gebruiker"));
    }

    #[test]
    fn opslaan_tussen_controle_en_verplaatsen_geeft_een_afwijkende_bron() {
        for nieuw in [None, Some("nieuw")] {
            let p = Proef::new();
            let a = p.a();
            na_stap(move |v| {
                if v.stap == Stap::Journaal {
                    fs::write(&a, "oud + werk")?;
                }
                Ok(())
            });
            assert!(vervang(p.opdracht(nieuw)).is_ok());
            let afwijkend = rond_af(&p.root);
            assert_eq!(afwijkend.len(), 1, "het werk staat in de bron en gaat naar de server");
            assert_eq!(fs::read_to_string(bron(&p.root, &afwijkend[0].id)).unwrap(), "oud + werk");
        }
    }

    #[test]
    fn lokaal_gewijzigd_voor_de_controle_laat_alles_staan() {
        let p = Proef::new();
        let mut o = p.opdracht(None);
        o.bekend.as_mut().unwrap().1 .0 += 1;
        assert_eq!(vervang(o).unwrap(), Uitkomst::LokaalGewijzigd);
        assert_eq!(p.lees().as_deref(), Some("oud"));
        assert_eq!((p.aantal(JOURNAAL), p.aantal(BRON)), (0, 0));
    }

    #[test]
    fn plaatsen_vervangt_nooit_een_bestaand_bestand() {
        let p = Proef::new();
        let b = p.root.join("b.txt");
        fs::write(&b, "b").unwrap();
        assert_eq!(plaats(&p.a(), &b).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!((p.lees().as_deref(), fs::read_to_string(&b).unwrap().as_str()), (Some("oud"), "b"));
        plaats(&p.a(), &p.root.join("c.txt")).unwrap();
        assert_eq!(fs::read_to_string(p.root.join("c.txt")).unwrap(), "oud");
        assert_eq!(p.lees(), None);
    }

    #[test]
    fn herstel_ruimt_tijdelijke_bestanden_op_maar_nooit_een_bron() {
        let p = Proef::new();
        for (naam, bestand) in [(NIEUW, "x"), (JOURNAAL, "y.tmp"), (BRON, "z")] {
            fs::create_dir_all(map(&p.root, naam)).unwrap();
            fs::write(map(&p.root, naam).join(bestand), "w").unwrap();
        }
        p.herstel(false);
        assert_eq!((p.aantal(NIEUW), p.aantal(JOURNAAL), p.aantal(BRON)), (0, 0, 1));
    }

    #[test]
    fn in_gebruik_ziet_een_open_bestand() {
        let p = Proef::new();
        let open = File::options().read(true).write(true).open(p.a()).unwrap();
        assert_eq!(in_gebruik(&[p.a()]), vec![Gebruik::Bezet]);
        drop(open);
        assert_eq!(in_gebruik(&[p.a()]), vec![Gebruik::Vrij]);
    }

    #[cfg(unix)]
    #[test]
    fn in_gebruik_ziet_een_geheugenkoppeling_ook_na_het_sluiten() {
        use std::os::fd::AsRawFd;
        let p = Proef::new();
        let f = File::options().read(true).write(true).open(p.a()).unwrap();
        let lengte = 3;
        let adres = unsafe {
            libc::mmap(std::ptr::null_mut(), lengte, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, f.as_raw_fd(), 0)
        };
        assert_ne!(adres, libc::MAP_FAILED);
        drop(f);
        assert_eq!(in_gebruik(&[p.a()]), vec![Gebruik::Bezet]);
        unsafe { libc::munmap(adres, lengte) };
        assert_eq!(in_gebruik(&[p.a()]), vec![Gebruik::Vrij]);
    }

    #[cfg(unix)]
    #[test]
    fn in_gebruik_ziet_een_ander_proces() {
        let p = Proef::new();
        let mut kind = std::process::Command::new("sh")
            .args(["-c", "exec 3>>\"$1\"; echo klaar; read x", "sh"])
            .arg(p.a())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        // Wachten tot het bestand echt open is.
        let mut regel = String::new();
        io::BufRead::read_line(&mut io::BufReader::new(kind.stdout.as_mut().unwrap()), &mut regel).unwrap();
        assert_eq!(in_gebruik(&[p.a()]), vec![Gebruik::Bezet]);
        drop(kind.stdin.take());
        kind.wait().unwrap();
        assert_eq!(in_gebruik(&[p.a()]), vec![Gebruik::Vrij]);
    }
}
