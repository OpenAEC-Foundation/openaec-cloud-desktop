//! Een oudere server zonder een methode herkennen, zodat de client kan terugvallen op de oude weg.

use openaec_sync::api::methode_ontbreekt;
use openaec_sync::Fout;
use serde_json::json;

fn server(status: u16, antwoord: serde_json::Value) -> Fout {
    Fout::Server { status, tekst: antwoord.to_string() }
}

#[test]
fn ontbrekende_methode_wordt_herkend() {
    let oud = server(
        404,
        json!({"exception": "frappe.exceptions.DoesNotExistError: Failed to get method for command openaec_storage.api.maak_ruimte with module 'openaec_storage.api' has no attribute 'maak_ruimte'"}),
    );
    assert!(methode_ontbreekt(&oud, "maak_ruimte"));
    assert!(!methode_ontbreekt(&oud, "ruimte_iets_anders"));
}

#[test]
fn een_gewone_fout_is_geen_ontbrekende_methode() {
    let geen_recht = server(403, json!({"exception": "frappe.exceptions.PermissionError: Alleen een beheerder van de omgeving kan een ruimte aanmaken"}));
    assert!(!methode_ontbreekt(&geen_recht, "maak_ruimte"));
}
