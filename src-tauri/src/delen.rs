//! Deelvenster: een bestand of map delen met een collega (bekijken of bewerken) of met een link (bekijken, of bij een
//! map bestanden opvragen), en zien wie er toegang heeft, met intrekken. Het venster `delen` opent vanuit het
//! rechtermuismenu van de Verkenner (met een lokaal pad, dat het venster eerst laat opzoeken) of vanuit de
//! bestandsweergave van Ruimtes (met het id op de server). Fouten krijgen een vaste code, zodat het venster de
//! juiste uitleg toont: geen verbinding, geen rechten, een gearchiveerde ruimte of een onbekende gebruiker.

use std::path::Path;

use openaec_sync::api::{Collega, Ruimtelid};
use openaec_sync::opslag::ClientOpslag;
use openaec_sync::Fout;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::{bestand_id, client, systeemvak, zoek, AppState};

pub const VENSTER: &str = "delen";

/// Wat er gedeeld wordt: een bestand of map op de server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deeldoel {
    /// `Opslagbestand` of `Opslagmap`.
    pub doctype: String,
    pub id: String,
    pub naam: String,
    /// Het lokale pad, of de plek in de ruimte ("Ruimte / Map / bestand.pdf").
    pub pad: String,
    pub ruimte: String,
    pub ruimte_naam: String,
}

/// Opdracht aan het deelvenster: een lokaal pad (nog op te zoeken) of een doel dat al bekend is.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Deelverzoek {
    #[serde(default)]
    pub pad: Option<String>,
    #[serde(default)]
    pub doel: Option<Deeldoel>,
}

/// Fout voor het deelvenster: een vaste code en een melding voor mensen.
#[derive(Debug, PartialEq, Serialize)]
pub struct DeelFout {
    pub code: &'static str,
    pub melding: String,
}

impl From<String> for DeelFout {
    fn from(melding: String) -> Self {
        DeelFout { code: "fout", melding }
    }
}

impl From<&str> for DeelFout {
    fn from(melding: &str) -> Self {
        melding.to_string().into()
    }
}

impl From<Fout> for DeelFout {
    fn from(f: Fout) -> Self {
        deelfout(&f)
    }
}

/// Een fout van de API vertalen naar een code en een duidelijke melding.
pub fn deelfout(f: &Fout) -> DeelFout {
    let melding = |code, tekst: &str| DeelFout { code, melding: tekst.to_string() };
    if let Fout::Netwerk(_) = f {
        return melding("geen_verbinding", "Geen verbinding met de server. Controleer je internetverbinding en probeer het opnieuw.");
    }
    if f.foutcode().as_deref() == Some("ruimte_gearchiveerd") {
        return melding(
            "ruimte_gearchiveerd",
            "Deze ruimte is gearchiveerd en alleen te lezen: delen en intrekken kan niet. Vraag een beheerder om hem terug te zetten.",
        );
    }
    let server = f.servermelding();
    // Nieuwere servers geven een foutcode; een oudere alleen de melding.
    if f.foutcode().as_deref() == Some("gebruiker_onbekend") || server.as_deref().is_some_and(|m| m.contains("bestaande gebruiker")) {
        return melding("onbekende_gebruiker", "Er is geen gebruiker met dit e-mailadres in deze omgeving.");
    }
    match f.status() {
        Some(401) => melding("niet_ingelogd", "Je sessie is verlopen. Log opnieuw in via het hoofdvenster."),
        Some(403) => melding(
            "geen_rechten",
            "Je hebt hiervoor geen rechten. Delen en intrekken kan als schrijver of beheerder van de ruimte.",
        ),
        Some(404) => melding("niet_gevonden", "Dit bestand of deze map bestaat niet (meer) op de server."),
        Some(502..=504) => melding("geen_verbinding", "De server is tijdelijk niet bereikbaar. Probeer het straks opnieuw."),
        _ => DeelFout { code: "fout", melding: server.unwrap_or_else(|| f.to_string()) },
    }
}

/// Mogen deze rechten bij deze soort deling? Bewerken alleen met een collega, bestanden opvragen alleen met een link
/// naar een map (dezelfde regels als de server, maar met een melding voordat er iets verstuurd wordt).
pub fn controleer_rechten(doctype: &str, soort: &str, rechten: &str) -> Result<(), DeelFout> {
    if doctype != "Opslagbestand" && doctype != "Opslagmap" {
        return Err("alleen mappen en bestanden kunnen gedeeld worden".into());
    }
    match (soort, rechten) {
        ("gebruiker", "lezen" | "schrijven") | ("link", "lezen") => Ok(()),
        ("link", "uploaden") if doctype == "Opslagmap" => Ok(()),
        ("link", "uploaden") => Err("bestanden opvragen kan alleen voor een map".into()),
        _ => Err(format!("deze rechten passen niet bij delen via een {soort}").into()),
    }
}

/// E-mailadres van een collega: zonder spaties, met één '@' en een punt in het domein.
pub fn controleer_email(email: &str) -> Result<String, DeelFout> {
    let email = email.trim().to_lowercase();
    let geldig = email.split_once('@').is_some_and(|(wie, domein)| {
        !wie.is_empty() && !domein.contains('@') && domein.contains('.') && !domein.starts_with('.') && !domein.ends_with('.')
    }) && !email.chars().any(char::is_whitespace);
    if geldig {
        Ok(email)
    } else {
        Err(DeelFout { code: "ongeldig_email", melding: "Vul een geldig e-mailadres in.".into() })
    }
}

/// Lege tekst (uit een leeg invoerveld) wordt None.
fn ingevuld(v: Option<String>) -> Option<String> {
    v.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// Het deelvenster tonen voor een pad of doel; het venster leest het verzoek bij het openen (`delen_verzoek`) en
/// hoort een nieuw verzoek als het al open is.
pub fn toon(app: &AppHandle, verzoek: Deelverzoek) {
    *app.state::<AppState>().deelverzoek.lock().unwrap() = Some(verzoek.clone());
    systeemvak::verberg_status(app);
    if let Some(venster) = app.get_webview_window(VENSTER) {
        if !venster.is_visible().unwrap_or(false) {
            let _ = venster.center();
        }
        let _ = venster.unminimize();
        let _ = venster.show();
        let _ = venster.set_focus();
    }
    let _ = app.emit_to(VENSTER, "delen-verzoek", &verzoek);
}

/// Het deelvenster openen (vanuit het hoofdvenster).
#[tauri::command]
pub fn open_delen(app: AppHandle, verzoek: Deelverzoek) {
    toon(&app, verzoek);
}

/// Het deelvenster sluiten (verbergen, zoals elk venster van de app).
#[tauri::command]
pub fn sluit_delen(app: AppHandle) {
    if let Some(venster) = app.get_webview_window(VENSTER) {
        let _ = venster.hide();
    }
}

/// Het laatste verzoek aan het deelvenster.
#[tauri::command]
pub fn delen_verzoek(state: tauri::State<'_, AppState>) -> Option<Deelverzoek> {
    state.deelverzoek.lock().unwrap().clone()
}

/// Een lokaal pad in een gekoppelde map opzoeken op de server.
#[tauri::command]
pub async fn deeldoel(state: tauri::State<'_, AppState>, pad: String) -> Result<Deeldoel, DeelFout> {
    let (k, rel) = zoek(&state, &pad)?;
    let naam = Path::new(&pad).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| pad.clone());
    let (doctype, id) = if Path::new(&pad).is_dir() {
        if rel.is_empty() {
            return Err(DeelFout {
                code: "hoofdmap",
                melding: "De gekoppelde map is de hele ruimte. Deel een map of bestand daarin, of laat een beheerder je collega lid van de ruimte maken.".into(),
            });
        }
        let id = ClientOpslag::new(client(&state).await?, &k.ruimte)
            .zoek_map(&rel)
            .await?
            .ok_or("deze map staat nog niet op de server; wacht tot hij gesynchroniseerd is")?;
        ("Opslagmap", id)
    } else {
        ("Opslagbestand", bestand_id(&state, &pad)?)
    };
    Ok(Deeldoel { doctype: doctype.into(), id, naam, pad, ruimte: k.ruimte, ruimte_naam: k.ruimte_naam })
}

/// Delen met een bestaande gebruiker van de omgeving: `lezen` (kan bekijken) of `schrijven` (kan bewerken).
#[tauri::command]
pub async fn deel_met_collega(
    state: tauri::State<'_, AppState>,
    doctype: String,
    id: String,
    gebruiker: String,
    rechten: String,
    verloopt: Option<String>,
) -> Result<(), DeelFout> {
    controleer_rechten(&doctype, "gebruiker", &rechten)?;
    let gebruiker = controleer_email(&gebruiker)?;
    let verloopt = ingevuld(verloopt);
    client(&state).await?.deel_met_gebruiker(&doctype, &id, &gebruiker, &rechten, verloopt.as_deref()).await?;
    Ok(())
}

/// Een link maken (`lezen`, of `uploaden` voor een map) en meteen op het klembord zetten; geeft de link terug.
#[tauri::command]
pub async fn maak_link(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    doctype: String,
    id: String,
    rechten: String,
    wachtwoord: Option<String>,
    verloopt: Option<String>,
) -> Result<String, DeelFout> {
    controleer_rechten(&doctype, "link", &rechten)?;
    let client = client(&state).await?;
    let (wachtwoord, verloopt) = (ingevuld(wachtwoord), ingevuld(verloopt));
    let deling = client.deel_link(&doctype, &id, &rechten, wachtwoord.as_deref(), verloopt.as_deref()).await?;
    let link = deling.url.unwrap_or_else(|| format!("{}/delen/{}", client.basis(), deling.token.unwrap_or_default()));
    kopieer(app, link.clone())?;
    Ok(link)
}

/// Een bestaande deling, zoals het venster hem toont.
#[derive(Debug, Serialize)]
pub struct DelingRegel {
    pub deling: String,
    pub soort: String,
    pub gebruiker: Option<String>,
    pub rechten: String,
    pub verloopt: Option<String>,
    pub gemaakt: Option<String>,
    /// Alleen bij een link.
    pub url: Option<String>,
    /// Van de server (nieuwere versies); None: het venster rekent of kijkt zelf.
    pub beveiligd: Option<bool>,
    pub verlopen: Option<bool>,
    pub gemaakt_door: Option<String>,
    pub gemaakt_door_naam: Option<String>,
}

/// Wie heeft toegang via een deling: de actieve delingen van dit bestand of deze map, nieuwste eerst.
#[tauri::command]
pub async fn delingen_van(state: tauri::State<'_, AppState>, doctype: String, id: String) -> Result<Vec<DelingRegel>, DeelFout> {
    let client = client(&state).await?;
    let mut lijst = client.delingen(&doctype, &id).await?;
    lijst.sort_by(|a, b| b.creation.cmp(&a.creation));
    Ok(lijst
        .into_iter()
        .map(|d| DelingRegel {
            url: d.token.filter(|_| d.soort == "link").map(|t| format!("{}/delen/{t}", client.basis())),
            deling: d.name,
            soort: d.soort,
            gebruiker: d.gebruiker,
            rechten: d.rechten,
            verloopt: d.verloopt,
            gemaakt: d.creation,
            beveiligd: d.beveiligd,
            verlopen: d.verlopen,
            gemaakt_door: d.gemaakt_door,
            gemaakt_door_naam: d.gemaakt_door_naam,
        })
        .collect())
}

#[tauri::command]
pub async fn trek_deling_in(state: tauri::State<'_, AppState>, deling: String) -> Result<(), DeelFout> {
    client(&state).await?.trek_in(&deling).await?;
    Ok(())
}

/// De leden van de ruimte (om een collega te kiezen, en om te zien wie al toegang heeft via de ruimte).
#[tauri::command]
pub async fn ruimteleden(state: tauri::State<'_, AppState>, ruimte: String) -> Result<Vec<Ruimtelid>, DeelFout> {
    let mut leden = client(&state).await?.ruimteleden(&ruimte).await?;
    leden.sort_by(|a, b| a.gebruiker.cmp(&b.gebruiker));
    Ok(leden)
}

/// Collega's om mee te delen, gezocht op e-mailadres en naam, met hun rol in de ruimte. Kent de server
/// `collegas` niet (een oudere versie), dan de leden van de ruimte die op de zoektekst passen.
#[tauri::command]
pub async fn collegas(state: tauri::State<'_, AppState>, ruimte: String, tekst: String) -> Result<Vec<Collega>, DeelFout> {
    let client = client(&state).await?;
    match client.collegas(tekst.trim(), Some(&ruimte)).await {
        Ok(lijst) => Ok(lijst),
        Err(Fout::Netwerk(e)) => Err(Fout::Netwerk(e).into()),
        Err(_) => {
            let zoek = tekst.trim().to_lowercase();
            let mut leden = client.ruimteleden(&ruimte).await?;
            leden.sort_by(|a, b| a.gebruiker.cmp(&b.gebruiker));
            Ok(leden
                .into_iter()
                .filter(|l| l.gebruiker.to_lowercase().contains(&zoek))
                .take(20)
                .map(|l| Collega { naam: l.gebruiker.clone(), gebruiker: l.gebruiker, lid_als: Some(l.rol) })
                .collect())
        }
    }
}

/// Tekst op het klembord (een link uit de lijst van delingen).
#[tauri::command]
pub fn kopieer(app: AppHandle, tekst: String) -> Result<(), DeelFout> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard().write_text(tekst).map_err(|e| DeelFout::from(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn server(status: u16, antwoord: serde_json::Value) -> Fout {
        Fout::Server { status, tekst: antwoord.to_string() }
    }

    #[test]
    fn fouten_krijgen_een_vaste_code() {
        let gearchiveerd = server(403, json!({"foutcode": "ruimte_gearchiveerd", "exc_type": "PermissionError"}));
        assert_eq!(deelfout(&gearchiveerd).code, "ruimte_gearchiveerd");
        assert_eq!(deelfout(&server(403, json!({"exc_type": "PermissionError"}))).code, "geen_rechten");
        let onbekend = json!({"_server_messages": json!([json!({"message": "Kies een bestaande gebruiker om mee te delen"}).to_string()]).to_string()});
        assert_eq!(deelfout(&server(417, onbekend)).code, "onbekende_gebruiker");
        // Nieuwere server: de foutcode, ook als de melding anders luidt.
        let met_code = json!({"foutcode": "gebruiker_onbekend", "exception": "frappe.exceptions.ValidationError: Onbekend"});
        assert_eq!(deelfout(&server(417, met_code)).code, "onbekende_gebruiker");
        assert_eq!(deelfout(&server(404, json!({}))).code, "niet_gevonden");
        assert_eq!(deelfout(&server(401, json!({}))).code, "niet_ingelogd");
        assert_eq!(deelfout(&server(503, json!({}))).code, "geen_verbinding");
        // Een andere weigering: de melding van de server.
        let anders = json!({"exception": "frappe.exceptions.ValidationError: Ongeldige datum"});
        assert_eq!(deelfout(&server(417, anders)), DeelFout { code: "fout", melding: "Ongeldige datum".into() });
    }

    #[test]
    fn rechten_volgen_de_regels_van_de_server() {
        assert!(controleer_rechten("Opslagbestand", "gebruiker", "lezen").is_ok());
        assert!(controleer_rechten("Opslagmap", "gebruiker", "schrijven").is_ok());
        assert!(controleer_rechten("Opslagbestand", "link", "lezen").is_ok());
        assert!(controleer_rechten("Opslagmap", "link", "uploaden").is_ok());
        assert!(controleer_rechten("Opslagbestand", "link", "uploaden").is_err());
        assert!(controleer_rechten("Opslagmap", "link", "schrijven").is_err());
        assert!(controleer_rechten("Opslagmap", "gebruiker", "uploaden").is_err());
        assert!(controleer_rechten("Ruimte", "link", "lezen").is_err());
    }

    #[test]
    fn email_wordt_opgeschoond_en_gecontroleerd() {
        assert_eq!(controleer_email("  P.Jansen@Voorbeeld.nl ").unwrap(), "p.jansen@voorbeeld.nl");
        for fout in ["", "jansen", "@voorbeeld.nl", "p@voorbeeld", "p@@voorbeeld.nl", "p jansen@voorbeeld.nl", "p@.nl"] {
            assert_eq!(controleer_email(fout).unwrap_err().code, "ongeldig_email", "{fout}");
        }
    }

    #[test]
    fn verzoek_met_pad_of_doel() {
        let v: Deelverzoek = serde_json::from_value(json!({"pad": "C:\\Werk\\a.pdf"})).unwrap();
        assert_eq!((v.pad.as_deref(), v.doel), (Some("C:\\Werk\\a.pdf"), None));
        let v: Deelverzoek = serde_json::from_value(json!({"doel": {
            "doctype": "Opslagmap", "id": "M1", "naam": "Tekeningen", "pad": "Havenkwartier / Tekeningen",
            "ruimte": "R1", "ruimte_naam": "Havenkwartier"
        }}))
        .unwrap();
        assert_eq!(v.doel.unwrap().id, "M1");
        assert_eq!(ingevuld(Some("  ".into())), None);
        assert_eq!(ingevuld(Some(" 2026-12-31 ".into())).as_deref(), Some("2026-12-31"));
    }
}
