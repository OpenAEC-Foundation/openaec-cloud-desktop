//! Vergrendelingen (Opslagvergrendeling) in de Verkenner: een vergrendeld bestand krijgt in de kolom "Vergrendeld door"
//! een slotje met wie het vergrendelde. De app leest de wijzigingenlijst van de server toch al (zie `bewaking`); daarin
//! staan ook "vergrendeld" en "ontgrendeld". Bij het eerste lezen (cursor 0) is dat de hele geschiedenis, zo komen ook
//! vergrendelingen van vóór het starten binnen. Een vergrendeling verloopt ook zonder melding; daarom kijkt de app af
//! en toe na of hij nog geldt ([`herbekijk`]).

use std::collections::BTreeMap;
use std::path::Path;

use openaec_sync::api::{Vergrendeling, Wijziging};
use openaec_sync::{Client, Fout};

use crate::instellingen::Koppeling;
use crate::vinkjes;

/// Laatste slotgebeurtenis per bestand-id: true = vergrendeld, false = ontgrendeld (op volgorde van de lijst).
pub fn slotgebeurtenissen(wijzigingen: &[Wijziging]) -> BTreeMap<String, bool> {
    let mut uit = BTreeMap::new();
    for w in wijzigingen.iter().filter(|w| w.doel_doctype == "Opslagbestand") {
        match w.soort.as_str() {
            "vergrendeld" => {
                uit.insert(w.doel.clone(), true);
            }
            "ontgrendeld" => {
                uit.insert(w.doel.clone(), false);
            }
            _ => {}
        }
    }
    uit
}

/// Iets anders dan een vergrendeling: dan is er een sync-ronde nodig.
pub fn vraagt_ronde(w: &Wijziging) -> bool {
    !matches!(w.soort.as_str(), "vergrendeld" | "ontgrendeld")
}

/// Tekst voor de kolom: "Vergrendeld door jan@voorbeeld.nl (tot 2026-10-23 14:00)".
pub fn slottekst(v: &Vergrendeling) -> String {
    let tot = v.verloopt.as_deref().map(|t| format!(" (tot {})", t.get(..16).unwrap_or(t))).unwrap_or_default();
    format!("Vergrendeld door {}{tot}", v.gebruiker)
}

/// Wijzigingen na `sinds` (alle pagina's): of er een ronde nodig is, de nieuwe cursor en de slotgebeurtenissen.
pub async fn wijzigingen(client: &Client, ruimte: &str, sinds: u64) -> Result<(bool, u64, BTreeMap<String, bool>), Fout> {
    let (mut cursor, mut ronde, mut alle) = (sinds, false, Vec::new());
    loop {
        let pagina = client.wijzigingen(ruimte, cursor).await?;
        ronde |= pagina.wijzigingen.iter().any(vraagt_ronde);
        cursor = pagina.cursor.max(cursor);
        alle.extend(pagina.wijzigingen);
        if !pagina.meer {
            return Ok((ronde, cursor, slotgebeurtenissen(&alle)));
        }
    }
}

/// Bestand-id -> pad hier (met '/'), uit de toestand van de laatste ronde.
fn paden(lokaal: &str) -> BTreeMap<String, String> {
    openaec_sync::motor::bekende_bestanden(Path::new(lokaal)).into_iter().map(|b| (b.bestand, b.pad)).collect()
}

/// Zet de slotgebeurtenissen in de Verkenner: bij "vergrendeld" eerst nakijken wie (en of hij nog geldt).
pub async fn werk_bij(client: &Client, k: &Koppeling, gebeurtenissen: BTreeMap<String, bool>) {
    if gebeurtenissen.is_empty() {
        return;
    }
    let lokaal = k.lokaal.clone();
    let paden = tauri::async_runtime::spawn_blocking(move || paden(&lokaal)).await.unwrap_or_default();
    let mut zetten = Vec::new();
    for (id, vergrendeld) in gebeurtenissen {
        let Some(pad) = paden.get(&id) else { continue };
        let wie = if vergrendeld { client.info(&id).await.ok().and_then(|i| i.vergrendeling).map(|v| slottekst(&v)) } else { None };
        zetten.push((pad.clone(), wie));
    }
    let lokaal = k.lokaal.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        for (pad, wie) in zetten {
            vinkjes::zet_slot(&lokaal, &pad, wie);
        }
    })
    .await;
}

/// Kijk de vergrendelingen die de Verkenner toont opnieuw na (verlopen, of opgeheven zonder dat we het zagen).
pub async fn herbekijk(client: &Client, k: &Koppeling) {
    let lokaal = k.lokaal.clone();
    let (met_slot, paden) = tauri::async_runtime::spawn_blocking(move || (vinkjes::sloten(&lokaal), paden(&lokaal))).await.unwrap_or_default();
    if met_slot.is_empty() {
        return;
    }
    let id_van: BTreeMap<&String, &String> = paden.iter().map(|(id, p)| (p, id)).collect();
    let mut zetten = Vec::new();
    for pad in met_slot {
        let wie = match id_van.get(&pad) {
            Some(id) => match client.info(id).await {
                Ok(i) => i.vergrendeling.map(|v| slottekst(&v)),
                Err(_) => continue, // later opnieuw
            },
            None => None,
        };
        zetten.push((pad, wie));
    }
    let lokaal = k.lokaal.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        for (pad, wie) in zetten {
            vinkjes::zet_slot(&lokaal, &pad, wie);
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(n: u64, soort: &str, doctype: &str, doel: &str) -> Wijziging {
        serde_json::from_value(serde_json::json!({"volgnummer": n, "soort": soort, "doel_doctype": doctype, "doel": doel, "naam": null})).unwrap()
    }

    #[test]
    fn laatste_slotgebeurtenis_telt() {
        let lijst = vec![
            w(1, "vergrendeld", "Opslagbestand", "B1"),
            w(2, "vergrendeld", "Opslagbestand", "B2"),
            w(3, "ontgrendeld", "Opslagbestand", "B1"),
            w(4, "gewijzigd", "Opslagbestand", "B3"),
            w(5, "vergrendeld", "Opslagmap", "M1"),
        ];
        let s = slotgebeurtenissen(&lijst);
        assert_eq!(s.get("B1"), Some(&false));
        assert_eq!(s.get("B2"), Some(&true));
        assert_eq!(s.len(), 2);
        assert!(!vraagt_ronde(&lijst[0]) && vraagt_ronde(&lijst[3]));
    }

    #[test]
    fn tekst_voor_de_kolom() {
        let v = Vergrendeling { gebruiker: "jan@voorbeeld.nl".into(), apparaat: None, verloopt: Some("2026-10-23 14:00:00.123456".into()) };
        assert_eq!(slottekst(&v), "Vergrendeld door jan@voorbeeld.nl (tot 2026-10-23 14:00)");
        let v = Vergrendeling { verloopt: None, ..v };
        assert_eq!(slottekst(&v), "Vergrendeld door jan@voorbeeld.nl");
    }
}
