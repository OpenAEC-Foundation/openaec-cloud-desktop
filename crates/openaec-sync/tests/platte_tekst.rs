//! Meldingen van de server zonder HTML en entiteiten: Frappe zet `<strong>` en `&#39;` in zijn meldingen.

use openaec_sync::{platte_tekst, serverfout_tekst};

#[test]
fn haalt_tags_en_entiteiten_weg() {
    let melding = "Opslagbestand b59u: <strong>Naam</strong> &#39;(NEN 8707&nbsp;Beoordeling – deel)&#39; zal afgekapt worden";
    assert_eq!(platte_tekst(melding), "Opslagbestand b59u: Naam '(NEN 8707 Beoordeling – deel)' zal afgekapt worden");
}

#[test]
fn laat_een_losse_ampersand_en_kleiner_dan_staan() {
    assert_eq!(platte_tekst("A & B &amp; C"), "A & B & C");
    assert_eq!(platte_tekst("&#x27;x&#x27; &onbekend; &lt;b&gt;"), "'x' &onbekend; <b>");
}

#[test]
fn servermelding_van_frappe_is_platte_tekst() {
    let bericht = serde_json::json!({ "message": "<strong>Naam</strong> &#39;x&#39; is te lang" }).to_string();
    let antwoord = serde_json::json!({ "_server_messages": serde_json::to_string(&vec![bericht]).unwrap() }).to_string();
    assert_eq!(serverfout_tekst(417, &antwoord), "Naam 'x' is te lang");
}
