//! Gesprekken met de assistent: lokaal bewaard als JSON, één bestand per gesprek in
//! `%APPDATA%\OpenAEC Storage\assistent\` (op macOS en Linux de configuratiemap van de gebruiker), en te verwijderen.
//! Hier ook de berichten voor de gateway: de geschiedenis in de vorm van de gateway, met de bijlagen en de denkblokken van het
//! model ongewijzigd terug.

use std::path::{Path, PathBuf};

use openaec_sync::ai::{Gebruik, DENKBLOKKEN};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::bijlagen::{Bijlage, Inhoud};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rol {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bericht {
    pub rol: Rol,
    #[serde(default)]
    pub tekst: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bijlagen: Vec<Bijlage>,
    /// De ondoorzichtige denkblokken van het model (zie [`DENKBLOKKEN`]); gaan ongewijzigd terug.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denkblokken: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gebruik: Option<Gebruik>,
    /// Een fout bij dit antwoord (de melding); zo'n antwoord gaat niet mee in de volgende vraag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fout: Option<String>,
    /// Gestopt met de knop Stoppen: wat er al was, blijft staan.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub afgebroken: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gesprek {
    pub id: String,
    #[serde(default)]
    pub titel: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Milliseconden sinds 1970.
    #[serde(default)]
    pub aangemaakt: u64,
    #[serde(default)]
    pub bijgewerkt: u64,
    #[serde(default)]
    pub berichten: Vec<Bericht>,
}

/// Voor de lijst met gesprekken.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Samenvatting {
    pub id: String,
    pub titel: String,
    pub bijgewerkt: u64,
}

pub const SYSTEEM: &str = "Je bent de assistent in de OpenAEC Desktop App, voor mensen in de bouw (architecten, constructeurs, \
installateurs en aannemers). Antwoord in de taal van de gebruiker, standaard in het Nederlands, helder en zakelijk. Gebruik \
Markdown: kopjes, lijsten, tabellen en codeblokken waar dat helpt. Bijlagen staan in de vraag tussen <bijlage naam=\"…\"> en \
</bijlage>. Zeg het eerlijk als je iets niet weet of als een bijlage niet genoeg informatie bevat.";

/// Een veilige bestandsnaam: alleen letters, cijfers, `-` en `_` (geen paden).
fn geldig_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// De berichten voor `/v1/chat/completions`: de instructie, en per beurt de tekst met de tekstbijlagen en de
/// afbeeldingen als data-URL. Een antwoord dat misging (zonder tekst) gaat niet mee.
pub fn berichten_voor(gesprek: &Gesprek) -> Vec<Value> {
    let mut uit = vec![json!({"role": "system", "content": SYSTEEM})];
    for b in &gesprek.berichten {
        match b.rol {
            Rol::User => {
                let mut tekst = b.tekst.clone();
                let mut afbeeldingen = Vec::new();
                for bijlage in &b.bijlagen {
                    match &bijlage.inhoud {
                        Inhoud::Tekst { tekst: inhoud } => {
                            let naam = bijlage.naam.replace('"', "'");
                            tekst.push_str(&format!("\n\n<bijlage naam=\"{naam}\">\n{inhoud}\n</bijlage>"));
                        }
                        Inhoud::Afbeelding { data_url, .. } => {
                            afbeeldingen.push(json!({"type": "image_url", "image_url": {"url": data_url}}));
                        }
                    }
                }
                let tekst = if tekst.trim().is_empty() { "(zie de bijlage)".to_string() } else { tekst };
                if afbeeldingen.is_empty() {
                    uit.push(json!({"role": "user", "content": tekst}));
                } else {
                    let mut delen = vec![json!({"type": "text", "text": tekst})];
                    delen.extend(afbeeldingen);
                    uit.push(json!({"role": "user", "content": delen}));
                }
            }
            Rol::Assistant => {
                if b.tekst.trim().is_empty() && b.denkblokken.is_none() {
                    continue;
                }
                let mut bericht = json!({"role": "assistant", "content": b.tekst});
                // De denkblokken alleen bij een volledig antwoord: na Stoppen klopt de beurt niet meer met de inhoud.
                if let Some(d) = b.denkblokken.as_ref().filter(|_| !b.afgebroken) {
                    bericht[DENKBLOKKEN] = Value::String(d.clone());
                }
                uit.push(bericht);
            }
        }
    }
    uit
}

/// Het aantal afbeeldingen in het gesprek (de gateway telt ze over het hele gesprek).
pub fn afbeeldingen(gesprek: &Gesprek) -> usize {
    gesprek
        .berichten
        .iter()
        .flat_map(|b| &b.bijlagen)
        .filter(|b| matches!(b.inhoud, Inhoud::Afbeelding { .. }))
        .count()
}

/// De map met gesprekken.
pub struct Gesprekken {
    map: PathBuf,
}

impl Gesprekken {
    pub fn in_map(map: impl Into<PathBuf>) -> Self {
        Self { map: map.into() }
    }

    /// `%APPDATA%\OpenAEC Storage\assistent` (naast `instellingen.json`).
    pub fn standaard() -> Self {
        Self::in_map(dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("OpenAEC Storage").join("assistent"))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn map(&self) -> &Path {
        &self.map
    }

    fn pad(&self, id: &str) -> Result<PathBuf, String> {
        if !geldig_id(id) {
            return Err("ongeldig gesprek".into());
        }
        Ok(self.map.join(format!("{id}.json")))
    }

    /// De gesprekken, het laatst bijgewerkte eerst. Een kapot bestand wordt overgeslagen.
    pub fn lijst(&self) -> Vec<Samenvatting> {
        let Ok(items) = std::fs::read_dir(&self.map) else { return Vec::new() };
        let mut lijst: Vec<Samenvatting> = items
            .flatten()
            .filter(|i| i.path().extension().is_some_and(|e| e == "json"))
            .filter_map(|i| std::fs::read(i.path()).ok())
            .filter_map(|b| serde_json::from_slice::<Gesprek>(&b).ok())
            .map(|g| Samenvatting { id: g.id, titel: g.titel, bijgewerkt: g.bijgewerkt })
            .collect();
        lijst.sort_by(|a, b| b.bijgewerkt.cmp(&a.bijgewerkt).then_with(|| a.id.cmp(&b.id)));
        lijst
    }

    pub fn lees(&self, id: &str) -> Result<Gesprek, String> {
        let bytes = std::fs::read(self.pad(id)?).map_err(|_| "Dit gesprek bestaat niet meer.".to_string())?;
        serde_json::from_slice(&bytes).map_err(|_| "Dit gesprek is beschadigd.".to_string())
    }

    /// Bewaar (eerst naar een tijdelijk bestand, dan hernoemen: een half geschreven gesprek bestaat nooit).
    pub fn bewaar(&self, gesprek: &Gesprek) -> Result<(), String> {
        let pad = self.pad(&gesprek.id)?;
        std::fs::create_dir_all(&self.map).map_err(|e| format!("gesprek bewaren mislukt: {e}"))?;
        let tijdelijk = pad.with_extension("json.tmp");
        std::fs::write(&tijdelijk, serde_json::to_vec(gesprek).expect("gesprek als JSON")).map_err(|e| format!("gesprek bewaren mislukt: {e}"))?;
        std::fs::rename(&tijdelijk, &pad).map_err(|e| format!("gesprek bewaren mislukt: {e}"))
    }

    pub fn verwijder(&self, id: &str) -> Result<(), String> {
        match std::fs::remove_file(self.pad(id)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("gesprek verwijderen mislukt: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bericht(rol: Rol, tekst: &str) -> Bericht {
        Bericht { rol, tekst: tekst.into(), bijlagen: vec![], denkblokken: None, model: None, gebruik: None, fout: None, afgebroken: false }
    }

    fn gesprek(id: &str, bijgewerkt: u64) -> Gesprek {
        Gesprek { id: id.into(), titel: format!("Gesprek {id}"), model: None, aangemaakt: 1, bijgewerkt, berichten: vec![bericht(Rol::User, "Hoi")] }
    }

    fn tijdelijke_map(naam: &str) -> PathBuf {
        let map = std::env::temp_dir().join(format!("openaec-assistent-{naam}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&map);
        map
    }

    #[test]
    fn bewaren_lijst_lezen_en_verwijderen() {
        let map = tijdelijke_map("opslag");
        let g = Gesprekken::in_map(&map);
        assert!(g.lijst().is_empty());
        g.bewaar(&gesprek("a1", 10)).unwrap();
        g.bewaar(&gesprek("b2", 30)).unwrap();
        let mut bijgewerkt = gesprek("a1", 50);
        bijgewerkt.berichten.push(bericht(Rol::Assistant, "Hallo!"));
        g.bewaar(&bijgewerkt).unwrap();
        std::fs::write(map.join("kapot.json"), b"{").unwrap();
        let lijst = g.lijst();
        assert_eq!(lijst.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["a1", "b2"]);
        assert_eq!(g.lees("a1").unwrap(), bijgewerkt);
        assert!(!map.join("a1.json.tmp").exists());
        g.verwijder("a1").unwrap();
        g.verwijder("a1").unwrap(); // al weg: geen fout
        assert_eq!(g.lijst().len(), 1);
        assert!(g.lees("a1").unwrap_err().contains("bestaat niet meer"));
        // Geen paden buiten de map.
        assert!(g.lees("../instellingen").is_err());
        assert!(g.bewaar(&gesprek("..\\x", 1)).is_err());
        let _ = std::fs::remove_dir_all(&map);
    }

    #[cfg(windows)]
    #[test]
    fn standaard_in_appdata() {
        let map = Gesprekken::standaard();
        let pad = map.map().to_string_lossy().to_string();
        assert!(pad.ends_with("\\OpenAEC Storage\\assistent"), "{pad}");
        assert!(pad.to_lowercase().contains("appdata\\roaming"), "{pad}");
    }

    #[test]
    fn berichten_met_bijlagen_en_denkblokken() {
        let mut g = gesprek("c3", 1);
        g.berichten[0].bijlagen = vec![
            Bijlage { naam: "notitie \"1\".txt".into(), grootte: 5, inhoud: Inhoud::Tekst { tekst: "hallo".into() } },
            Bijlage {
                naam: "foto.jpg".into(),
                grootte: 9,
                inhoud: Inhoud::Afbeelding { data_url: "data:image/jpeg;base64,AAAA".into(), breedte: 2, hoogte: 2 },
            },
        ];
        let mut antwoord = bericht(Rol::Assistant, "Antwoord");
        antwoord.denkblokken = Some("b1.x".into());
        g.berichten.push(antwoord);
        let mut mislukt = bericht(Rol::User, "Nog een vraag");
        mislukt.fout = None;
        g.berichten.push(mislukt);
        let mut fout = bericht(Rol::Assistant, "");
        fout.fout = Some("Het AI-tegoed is op.".into());
        g.berichten.push(fout);
        g.berichten.push(bericht(Rol::User, "Opnieuw"));

        let b = berichten_voor(&g);
        assert_eq!(b[0]["role"], "system");
        assert_eq!(
            b[1],
            json!({"role": "user", "content": [
                {"type": "text", "text": "Hoi\n\n<bijlage naam=\"notitie '1'.txt\">\nhallo\n</bijlage>"},
                {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,AAAA"}},
            ]})
        );
        assert_eq!(b[2], json!({"role": "assistant", "content": "Antwoord", "openaec_denkblokken": "b1.x"}));
        // Het mislukte antwoord ontbreekt; de twee vragen worden bij de gateway één beurt.
        assert_eq!(b[3], json!({"role": "user", "content": "Nog een vraag"}));
        assert_eq!(b[4], json!({"role": "user", "content": "Opnieuw"}));
        assert_eq!(b.len(), 5);
        assert_eq!(afbeeldingen(&g), 1);

        // Na Stoppen: de tekst wel, de denkblokken niet.
        let mut gestopt = bericht(Rol::Assistant, "Half");
        gestopt.denkblokken = Some("b1.y".into());
        gestopt.afgebroken = true;
        g.berichten = vec![bericht(Rol::User, "Vraag"), gestopt, bericht(Rol::User, "Verder")];
        assert_eq!(berichten_voor(&g)[2], json!({"role": "assistant", "content": "Half"}));
    }

    #[test]
    fn oud_of_onvolledig_gesprek_is_te_lezen() {
        let g: Gesprek = serde_json::from_str(r#"{"id":"x","berichten":[{"rol":"user","tekst":"Hoi"}]}"#).unwrap();
        assert_eq!(g.berichten[0].bijlagen.len(), 0);
        // Een bijlage in de bewaarde vorm (zoals het venster hem ook krijgt).
        let b: Bijlage = serde_json::from_str(r#"{"naam":"a.txt","grootte":3,"soort":"tekst","tekst":"abc"}"#).unwrap();
        assert_eq!(b.inhoud, Inhoud::Tekst { tekst: "abc".into() });
        assert_eq!(serde_json::to_value(&b).unwrap()["soort"], "tekst");
    }
}
