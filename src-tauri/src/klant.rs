//! "Delen met klant" in het venster Versturen: de bestanden gaan als overdracht naar de server (zoals Versturen met
//! OpenAEC) en worden daarna een levering (datadrop) met het 3D-model uit Speckle, achter één link. Eerst een concept:
//! de app opent de pagina van de ontvanger als voorbeeld in een eigen venster; daarna delen met e-mailadressen.

use openaec_sync::datadrop::{Datadrop, Gedeeld, Gemaakt, Levering, Projecten};
use openaec_sync::Fout;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::delen::DeelFout;
use crate::versturen::{verbinding, versturenfout, VersturenFout};
use crate::AppState;

/// Het venster met het voorbeeld: de echte pagina van de ontvanger, zonder toegang tot de app (geen rechten in
/// `capabilities`).
const VOORBEELD: &str = "klant-voorbeeld";

/// Een fout van de datadrop-API voor het venster; een server zonder datadrops zegt dat eerlijk.
fn klantfout(f: Fout) -> VersturenFout {
    let tekst = f.to_string();
    if tekst.contains("openaec_werkplek.datadrop") && (tekst.contains("No module") || tekst.contains("has no attribute")) {
        return VersturenFout(DeelFout {
            code: "niet_op_server",
            melding: "Delen met klant is op deze omgeving nog niet beschikbaar: de server moet eerst bijgewerkt worden.".into(),
        });
    }
    if f.status() == Some(503) {
        return VersturenFout(DeelFout { code: "fout", melding: "De werkplek staat op deze omgeving nog niet aan; daar hoort Delen met klant bij.".into() });
    }
    VersturenFout(versturenfout(&f))
}

#[tauri::command]
pub async fn klant_projecten(state: tauri::State<'_, AppState>, zoek: Option<String>) -> Result<Projecten, VersturenFout> {
    verbinding(&state).await?.datadrop_projecten(zoek.as_deref().unwrap_or("")).await.map_err(klantfout)
}

#[tauri::command]
pub async fn klant_mijn(state: tauri::State<'_, AppState>) -> Result<Vec<Datadrop>, VersturenFout> {
    verbinding(&state).await?.datadrop_mijn().await.map_err(klantfout)
}

/// Van de zojuist verstuurde overdracht een levering maken (of een nieuwe versie van een bestaande).
#[tauri::command]
pub async fn klant_van_overdracht(state: tauri::State<'_, AppState>, overdracht: String, levering: Levering) -> Result<Gemaakt, VersturenFout> {
    verbinding(&state).await?.datadrop_van_overdracht(&overdracht, &levering).await.map_err(klantfout)
}

#[tauri::command]
pub async fn klant_deel(
    state: tauri::State<'_, AppState>,
    datadrop: String,
    ontvangers: Vec<String>,
    bericht: Option<String>,
) -> Result<Gedeeld, VersturenFout> {
    verbinding(&state).await?.datadrop_deel(&datadrop, &ontvangers, bericht.as_deref()).await.map_err(klantfout)
}

/// Is dit de link van een levering op de verbonden omgeving? Alleen die opent het voorbeeldvenster.
pub fn is_leveringslink(url: &str, basis: &str) -> bool {
    let basis = basis.trim_end_matches('/');
    url.strip_prefix(basis)
        .and_then(|rest| rest.strip_prefix("/datadrop/"))
        .is_some_and(|token| !token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// Het voorbeeld openen (of, als het al open is, naar deze link en naar voren).
#[tauri::command]
pub async fn klant_voorbeeld(app: AppHandle, state: tauri::State<'_, AppState>, url: String) -> Result<(), VersturenFout> {
    let client = verbinding(&state).await?;
    if !is_leveringslink(&url, client.basis()) {
        return Err("dit is geen link van een levering op deze omgeving".into());
    }
    let adres: tauri::Url = url.parse().map_err(|_| VersturenFout::from("ongeldige link"))?;
    if let Some(venster) = app.get_webview_window(VOORBEELD) {
        venster.navigate(adres).map_err(|e| VersturenFout::from(e.to_string()))?;
        let _ = venster.show();
        let _ = venster.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, VOORBEELD, WebviewUrl::External(adres))
        .title("Voorbeeld: zo ziet de klant het")
        .inner_size(1280.0, 820.0)
        .min_inner_size(720.0, 480.0)
        .build()
        .map(|_| ())
        .map_err(|e| VersturenFout::from(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::is_leveringslink;

    #[test]
    fn alleen_een_leveringslink_van_de_eigen_omgeving() {
        let basis = "https://klant.cloud.open-aec.eu";
        assert!(is_leveringslink("https://klant.cloud.open-aec.eu/datadrop/2e466edf8e04", basis));
        assert!(is_leveringslink("https://klant.cloud.open-aec.eu/datadrop/abc", "https://klant.cloud.open-aec.eu/"));
        for fout in [
            "https://elders.nl/datadrop/abc",
            "https://klant.cloud.open-aec.eu.elders.nl/datadrop/abc",
            "https://klant.cloud.open-aec.eu/app/user",
            "https://klant.cloud.open-aec.eu/datadrop/",
            "https://klant.cloud.open-aec.eu/datadrop/abc?x=1",
            "https://klant.cloud.open-aec.eu/datadrop/../app",
        ] {
            assert!(!is_leveringslink(fout, basis), "{fout}");
        }
    }
}
