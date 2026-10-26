//! Rechtermuismenu in de Verkenner (Windows): een submenu "OpenAEC Desktop App" op bestanden en mappen, alleen binnen
//! de gekoppelde mappen (AppliesTo). Staat in HKCU, dus zonder beheerdersrechten; wordt bij het starten en na elke
//! (ont)koppeling opnieuw geschreven. Een keuze start de app met `--actie <naam> "<pad>"`; draait de app al, dan
//! stuurt de single-instance-plugin de opdracht door. "Delen…" staat bovenaan en opent het deelvenster; "Deellink
//! kopiëren" blijft als snelle optie (leeslink meteen op het klembord). "Versturen met OpenAEC…" opent het venster
//! Versturen; dat staat ook als eigen item in het hoofdmenu, op elk bestand en elke map (zie `rechtermuis`).

pub const BESTANDSACTIES: [(&str, &str); 6] = [
    ("delen", "Delen…"),
    ("deel", "Deellink kopiëren"),
    ("versturen", "Versturen met OpenAEC…"),
    ("vergrendel", "Vergrendelen (ik bewerk dit)"),
    ("ontgrendel", "Ontgrendelen"),
    ("versies", "Versies bekijken"),
];
pub const MAPACTIES: [(&str, &str); 4] = [
    ("delen", "Delen…"),
    ("deel", "Deellink kopiëren"),
    ("opvragen", "Bestanden opvragen (uploadlink)"),
    ("versturen", "Versturen met OpenAEC…"),
];

/// AQS-voorwaarde: het pad begint met een van de gekoppelde mappen.
pub fn applies_to(mappen: &[String]) -> String {
    mappen
        .iter()
        .map(|m| format!("System.ItemPathDisplay:~<\"{}\"", m.trim_end_matches(['\\', '/'])))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// De sleutelnaam `OpenAECStorage` blijft bewust gelijk (ook na de nieuwe productnaam): zo vervangt elke start de
/// registratie van een oudere versie in plaats van er een tweede menu naast te zetten.
#[cfg(windows)]
pub fn registreer(mappen: &[String]) -> std::io::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let klassen = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Software\\Classes", winreg::enums::KEY_ALL_ACCESS)?;
    for soort in ["*", "Directory"] {
        let _ = klassen.delete_subkey_all(format!("{soort}\\shell\\OpenAECStorage"));
    }
    if mappen.is_empty() {
        return Ok(());
    }
    let exe = std::env::current_exe()?.to_string_lossy().to_string();
    let voorwaarde = applies_to(mappen);
    for (soort, acties) in [("*", &BESTANDSACTIES[..]), ("Directory", &MAPACTIES[..])] {
        let (menu, _) = klassen.create_subkey(format!("{soort}\\shell\\OpenAECStorage"))?;
        menu.set_value("MUIVerb", &"OpenAEC Desktop App")?;
        menu.set_value("SubCommands", &"")?;
        menu.set_value("Icon", &format!("\"{exe}\",0"))?;
        menu.set_value("AppliesTo", &voorwaarde)?;
        for (i, (actie, label)) in acties.iter().enumerate() {
            let (item, _) = menu.create_subkey(format!("shell\\{}{actie}", i + 1))?;
            item.set_value("MUIVerb", label)?;
            let (opdracht, _) = item.create_subkey("command")?;
            opdracht.set_value("", &format!("\"{exe}\" --actie {actie} \"%1\""))?;
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn registreer(_mappen: &[String]) -> std::io::Result<()> {
    Ok(())
}

/// Welk venster een actie opent: "delen" het deelvenster, "versturen" (en "verzend" uit het menu van een oudere
/// versie) het venster Versturen, "versies" het hoofdvenster (deelscherm); de rest (deellink, uploadlink,
/// vergrendelen) gebeurt op de achtergrond met een melding.
pub fn venster_voor(actie: &str) -> Option<&'static str> {
    match actie {
        "delen" => Some("delen"),
        "versturen" | "verzend" => Some("versturen"),
        "versies" => Some("main"),
        _ => None,
    }
}

/// `--actie <naam> <pad>` uit de opdrachtregel (het eerste pad).
#[cfg(test)]
pub fn actie_uit(argv: &[String]) -> Option<(String, String)> {
    let (actie, paden) = actie_met_paden(argv)?;
    Some((actie, paden.into_iter().next()?))
}

/// `--actie <naam> <pad> [<pad>…]` uit de opdrachtregel: alles na de naam is een pad (Linux en macOS geven bij een
/// selectie alle paden in één keer; de Verkenner start de app per item).
pub fn actie_met_paden(argv: &[String]) -> Option<(String, Vec<String>)> {
    let i = argv.iter().position(|a| a == "--actie")?;
    let actie = argv.get(i + 1)?.clone();
    let paden: Vec<String> = argv[i + 2..].iter().filter(|p| !p.is_empty()).cloned().collect();
    (!paden.is_empty()).then_some((actie, paden))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voorwaarde_en_opdrachtregel() {
        let v = applies_to(&["C:\\Werk\\Project A\\".into(), "D:\\Archief".into()]);
        assert_eq!(v, "System.ItemPathDisplay:~<\"C:\\Werk\\Project A\" OR System.ItemPathDisplay:~<\"D:\\Archief\"");
        let argv: Vec<String> = ["app.exe", "--actie", "deel", "C:\\Werk\\a.pdf"].iter().map(|s| s.to_string()).collect();
        assert_eq!(actie_uit(&argv), Some(("deel".into(), "C:\\Werk\\a.pdf".into())));
        assert_eq!(actie_uit(&["app.exe".to_string()]), None);
    }

    #[test]
    fn delen_staat_bovenaan_en_deellink_blijft() {
        for acties in [&BESTANDSACTIES[..], &MAPACTIES[..]] {
            assert_eq!(acties[0], ("delen", "Delen…"));
            assert!(acties.contains(&("deel", "Deellink kopiëren")));
            // Elke actie maar één keer: de sleutelnamen in het register (`1delen`, `2deel`, …) blijven uniek.
            let mut namen: Vec<_> = acties.iter().map(|(a, _)| *a).collect();
            namen.sort();
            namen.dedup();
            assert_eq!(namen.len(), acties.len());
        }
        assert!(MAPACTIES.iter().any(|(a, _)| *a == "opvragen"));
        assert!(!BESTANDSACTIES.iter().any(|(a, _)| *a == "opvragen"));
    }

    #[test]
    fn actie_delen_opent_het_deelvenster() {
        let argv: Vec<String> = ["app.exe", "--actie", "delen", "C:\\Werk\\Tekeningen"].iter().map(|s| s.to_string()).collect();
        assert_eq!(actie_uit(&argv), Some(("delen".into(), "C:\\Werk\\Tekeningen".into())));
        assert_eq!(venster_voor("delen"), Some("delen"));
        assert_eq!(venster_voor("versturen"), Some("versturen"));
        assert_eq!(venster_voor("verzend"), Some("versturen"));
        assert_eq!(venster_voor("versies"), Some("main"));
        assert_eq!(venster_voor("deel"), None);
        assert_eq!(venster_voor("opvragen"), None);
    }

    #[test]
    fn versturen_met_meerdere_paden() {
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let r = actie_met_paden(&argv(&["openaec-cloud", "--actie", "versturen", "/home/jan/a.pdf", "/home/jan/Tekeningen", "/tmp/b c.txt"]));
        assert_eq!(
            r,
            Some(("versturen".into(), vec!["/home/jan/a.pdf".into(), "/home/jan/Tekeningen".into(), "/tmp/b c.txt".into()]))
        );
        // Eén pad uit de Verkenner, en het eerste pad voor de andere acties.
        assert_eq!(actie_met_paden(&argv(&["app.exe", "--actie", "versturen", r"C:\a.pdf"])).unwrap().1, [r"C:\a.pdf"]);
        assert_eq!(actie_uit(&argv(&["app", "--actie", "deel", "/a", "/b"])), Some(("deel".into(), "/a".into())));
        // Zonder pad (of met een leeg pad) geen actie.
        assert_eq!(actie_met_paden(&argv(&["app", "--actie", "versturen"])), None);
        assert_eq!(actie_met_paden(&argv(&["app", "--actie", "versturen", ""])), None);
        // Het submenu in de gekoppelde mappen heeft het item ook.
        assert!(BESTANDSACTIES.contains(&("versturen", "Versturen met OpenAEC…")));
        assert!(MAPACTIES.contains(&("versturen", "Versturen met OpenAEC…")));
    }
}
