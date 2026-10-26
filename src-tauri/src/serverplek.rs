//! Van een lokaal pad naar de plek op de server: de gekoppelde map met het pad erin, en het id van de map op de
//! server. Voor delen, deellinks, uploadlinks en versturen.
//!
//! Een pad uit de Verkenner kan anders geschreven zijn dan de koppeling: andere hoofdletters, een korte 8.3-naam, het
//! voorvoegsel `\\?\`, '/' in plaats van '\' of een '\' aan het eind. Bestaat het pad, dan telt het echte pad op schijf
//! (met de schrijfwijze van de mappen zelf); anders vergelijkt de app op Windows zonder hoofdletters.
//!
//! De map op de server zoekt [`ClientOpslag::zoek_map`] (in één verzoek op een nieuwere server). De sync neemt alleen
//! mappen met bestanden mee: een lege map (of een map met alleen lege mappen of genegeerde bestanden) staat dus niet op
//! de server, ook al ziet hij er in de Verkenner gesynchroniseerd uit. Bij delen maakt de app zo'n map alsnog aan.

use std::path::{Component, Path, PathBuf};

use openaec_sync::opslag::{ClientOpslag, Mapplek};

use crate::delen::DeelFout;
use crate::instellingen::Koppeling;

/// Zonder het voorvoegsel `\\?\` (dat `canonicalize` op Windows geeft).
fn zonder_voorvoegsel(pad: &Path) -> PathBuf {
    let tekst = pad.to_string_lossy();
    if let Some(rest) = tekst.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = tekst.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        pad.to_path_buf()
    }
}

/// Twee namen in een pad gelijk? Op Windows (en macOS) maken hoofdletters niet uit.
fn zelfde_naam(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

fn delen(p: &Path) -> Vec<Component<'_>> {
    p.components().filter(|c| !matches!(c, Component::CurDir)).collect()
}

/// Het pad (met '/') van `pad` binnen `root`, of None als het er niet in ligt. "" is `root` zelf.
pub fn rel_binnen(root: &Path, pad: &Path) -> Option<String> {
    let (root, pad) = (zonder_voorvoegsel(root), zonder_voorvoegsel(pad));
    let (r, p) = (delen(&root), delen(&pad));
    if p.len() < r.len() {
        return None;
    }
    for (a, b) in r.iter().zip(&p) {
        let gelijk = match (a, b) {
            (Component::Prefix(x), Component::Prefix(y)) => zelfde_naam(x.as_os_str(), y.as_os_str()),
            (Component::RootDir, Component::RootDir) => true,
            (Component::Normal(x), Component::Normal(y)) => zelfde_naam(x, y),
            _ => false,
        };
        if !gelijk {
            return None;
        }
    }
    Some(p[r.len()..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/"))
}

/// De koppeling bij een lokaal pad en het pad erin (met '/'). Bij geneste koppelingen wint de diepste.
pub fn zoek_in(koppelingen: &[Koppeling], pad: &str) -> Option<(Koppeling, String)> {
    let gegeven = PathBuf::from(pad);
    let echt = std::fs::canonicalize(&gegeven).ok();
    koppelingen
        .iter()
        .filter_map(|k| {
            let root = Path::new(&k.lokaal);
            // Eerst het echte pad op schijf (tegen het echte pad van de koppeling): dan klopt ook de schrijfwijze.
            let rel = echt
                .as_ref()
                .and_then(|e| std::fs::canonicalize(root).ok().and_then(|r| rel_binnen(&r, e)))
                .or_else(|| rel_binnen(root, &gegeven))?;
            Some((k.clone(), rel))
        })
        .max_by_key(|(k, _)| k.lokaal.len())
}

/// Het id van de map `rel` (in de koppeling met lokale map `root`) op de server, met een duidelijke fout als hij er
/// niet staat (zie [`ClientOpslag::map_van_lokaal`]). Met `maak_leeg` maakt de app een lege map aan, zodat hij te delen
/// is (een uploadlink voor een lege map is heel gewoon).
pub async fn map_op_server(opslag: &ClientOpslag, root: &Path, rel: &str, maak_leeg: bool) -> Result<String, DeelFout> {
    let fout = |code, tekst: &str| DeelFout { code, melding: tekst.to_string() };
    match opslag.map_van_lokaal(root, rel, maak_leeg).await? {
        Mapplek::Gevonden(id) => Ok(id),
        Mapplek::NogNietGesynchroniseerd => Err(fout(
            "nog_niet_gesynchroniseerd",
            "Deze map staat nog niet op de server: de bestanden erin worden nog gesynchroniseerd. Probeer het opnieuw als dat klaar is.",
        )),
        Mapplek::NietGevonden => Err(fout(
            "niet_gevonden",
            "Deze map staat op deze computer gesynchroniseerd, maar is op de server niet (meer) te vinden. Laat de app synchroniseren en probeer het opnieuw.",
        )),
        Mapplek::Leeg if maak_leeg => Err(fout(
            "lege_map",
            "Deze map is leeg en staat daarom niet op de server, en in een map die met je gedeeld is om te bekijken kan de app hem niet aanmaken.",
        )),
        Mapplek::Leeg => Err(fout("lege_map", "Deze map is leeg en staat daarom niet op de server. Zet er eerst een bestand in.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn koppeling(lokaal: &str) -> Koppeling {
        Koppeling::ruimte("R1".into(), "OpenAEC Studio".into(), lokaal.into())
    }

    #[test]
    fn pad_binnen_een_koppeling() {
        let root = std::env::temp_dir().join("openaec-plek").join("OpenAEC Studio");
        let pad = root.join("50_projecten").join("05 Klant Noord");
        assert_eq!(rel_binnen(&root, &pad).as_deref(), Some("50_projecten/05 Klant Noord"));
        assert_eq!(rel_binnen(&root, &root).as_deref(), Some(""));
        assert_eq!(rel_binnen(&pad, &root), None);
        assert_eq!(rel_binnen(&root.join("a"), &root.join("ab")), None, "alleen hele namen");
    }

    #[cfg(windows)]
    #[test]
    fn windows_paden_met_spaties_hoofdletters_en_voorvoegsel() {
        let root = Path::new(r"C:\Users\jan\Bureau Projecten\Jan de Vries\OpenAEC Studio");
        let rel = |p: &str| rel_binnen(root, Path::new(p));
        assert_eq!(rel(r"C:\Users\jan\Bureau Projecten\Jan de Vries\OpenAEC Studio\50_projecten\Fase 2").as_deref(), Some("50_projecten/Fase 2"));
        assert_eq!(rel(r"c:\users\jan\bureau projecten\jan de vries\openaec studio\Fase 2\").as_deref(), Some("Fase 2"));
        assert_eq!(rel(r"\\?\C:\Users\jan\Bureau Projecten\Jan de Vries\OpenAEC Studio\a b\c").as_deref(), Some("a b/c"));
        assert_eq!(rel("C:/Users/jan/Bureau Projecten/Jan de Vries/OpenAEC Studio/x/y.pdf").as_deref(), Some("x/y.pdf"));
        assert_eq!(rel(r"D:\Users\jan\Bureau Projecten\Jan de Vries\OpenAEC Studio\x"), None);
        assert_eq!(rel(r"C:\Users\jan\Bureau Projecten\Jan de Vries\OpenAEC Studio 2\x"), None);
    }

    #[test]
    fn echte_schrijfwijze_op_schijf_en_diepste_koppeling() {
        let werk = tempfile_map("schrijfwijze");
        let root = werk.join("OpenAEC Studio");
        let diep = root.join("50_projecten").join("05 Klant Noord").join("Fase 2 - Ontwerp");
        std::fs::create_dir_all(&diep).unwrap();
        let k = vec![koppeling(&root.to_string_lossy()), koppeling(&werk.to_string_lossy())];
        let (gevonden, rel) = zoek_in(&k, &diep.to_string_lossy()).unwrap();
        assert_eq!((gevonden.lokaal.as_str(), rel.as_str()), (root.to_string_lossy().as_ref(), "50_projecten/05 Klant Noord/Fase 2 - Ontwerp"));
        #[cfg(windows)]
        {
            // Andere hoofdletters in het gegeven pad: het pad op de server volgt de mappen op schijf.
            let anders = diep.to_string_lossy().replace("05 Klant Noord", "05 KLANT noord").replace("Fase 2", "fase 2");
            assert_eq!(zoek_in(&k, &anders).unwrap().1, "50_projecten/05 Klant Noord/Fase 2 - Ontwerp");
        }
        assert!(zoek_in(&k, &std::env::temp_dir().join("ergens-anders").to_string_lossy()).is_none());
        let _ = std::fs::remove_dir_all(&werk);
    }

    fn tempfile_map(naam: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("openaec-plek-{naam}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
