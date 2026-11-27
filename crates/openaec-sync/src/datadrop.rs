//! "Delen met klant": een levering (datadrop, Open BIM Platform versie 2) voor iemand buiten de omgeving, met het
//! 3D-model uit Speckle en de bestanden achter één link. De app uploadt de bestanden als overdracht (zie
//! [`crate::overdracht`]); de server maakt er een datadrop van (`openaec_werkplek.datadrop.van_overdracht`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::Client;
use crate::Fout;

const DATADROP: &str = "/api/method/openaec_werkplek.datadrop.";

/// Een project op de Speckle-server van de omgeving, met zijn modellen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub project: String,
    pub naam: String,
    #[serde(default)]
    pub modellen: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Projecten {
    /// Staat Speckle aan op deze omgeving (anders kan het alleen met bestanden).
    pub speckle: bool,
    #[serde(default)]
    pub melding: Option<String>,
    #[serde(default)]
    pub projecten: Vec<Project>,
}

/// Een datadrop in de lijst van de gebruiker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Datadrop {
    pub datadrop: String,
    pub titel: String,
    /// `Concept`, `Actief`, `Verlopen` of `Ingetrokken`.
    pub status: String,
    #[serde(default = "een")]
    pub versie: u32,
    #[serde(default)]
    pub ontvangers: Vec<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub project_naam: Option<String>,
    #[serde(default)]
    pub verloopt_op: Option<String>,
    #[serde(default)]
    pub opmerkingen: u32,
}

fn een() -> u32 {
    1
}

/// Wat `van_overdracht` teruggeeft: de datadrop met zijn (vaste) link en de versie.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gemaakt {
    pub datadrop: String,
    pub titel: String,
    pub status: String,
    pub versie: u32,
    pub url: String,
    #[serde(default)]
    pub verloopt_op: Option<String>,
}

/// Wat `deel` teruggeeft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gedeeld {
    pub status: String,
    pub url: String,
    #[serde(default)]
    pub ontvangers: Vec<String>,
    /// De adressen waar de mail heen ging.
    #[serde(default)]
    pub mail: Vec<String>,
}

/// Een nieuwe datadrop, of (met `datadrop`) een nieuwe versie daarvan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Levering {
    pub titel: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub bericht: Option<String>,
    #[serde(default)]
    pub datadrop: Option<String>,
    #[serde(default)]
    pub notitie: Option<String>,
}

fn tekst(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|t| !t.is_empty())
}

impl Client {
    async fn datadrop<T: serde::de::DeserializeOwned>(&self, methode: &str, gegevens: Value) -> Result<T, Fout> {
        self.roep_op(DATADROP, methode, gegevens).await
    }

    /// De projecten in Speckle om er één voor de levering te kiezen.
    pub async fn datadrop_projecten(&self, zoek: &str) -> Result<Projecten, Fout> {
        self.datadrop("projecten", json!({"zoek": zoek.trim()})).await
    }

    /// De eigen datadrops, nieuwste eerst.
    pub async fn datadrop_mijn(&self) -> Result<Vec<Datadrop>, Fout> {
        self.datadrop("mijn", json!({})).await
    }

    /// Van een voltooide overdracht (door de app geüpload) een datadrop maken: een concept, of een nieuwe versie van
    /// `levering.datadrop`.
    pub async fn datadrop_van_overdracht(&self, overdracht: &str, levering: &Levering) -> Result<Gemaakt, Fout> {
        self.datadrop(
            "van_overdracht",
            json!({"overdracht": overdracht, "titel": levering.titel.trim(), "project": tekst(&levering.project),
                   "bericht": tekst(&levering.bericht), "datadrop": tekst(&levering.datadrop), "notitie": tekst(&levering.notitie),
                   "concept": 1}),
        )
        .await
    }

    /// Delen: een concept wordt actief en de ontvangers krijgen een mail met de link.
    pub async fn datadrop_deel(&self, datadrop: &str, ontvangers: &[String], bericht: Option<&str>) -> Result<Gedeeld, Fout> {
        let ontvangers: Vec<&str> = ontvangers.iter().map(|o| o.trim()).filter(|o| !o.is_empty()).collect();
        self.datadrop("deel", json!({"datadrop": datadrop, "ontvangers": ontvangers, "bericht": bericht.map(str::trim).filter(|b| !b.is_empty())}))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn een_datadrop_van_een_oudere_server_zonder_versie_is_versie_1() {
        let d: Datadrop = serde_json::from_value(json!({"datadrop": "d1", "titel": "DO", "status": "Actief"})).unwrap();
        assert_eq!((d.versie, d.ontvangers.len(), d.url), (1, 0, None));
    }

    #[test]
    fn lege_velden_gaan_als_null() {
        let l = Levering { titel: " DO ".into(), project: Some("  ".into()), ..Default::default() };
        assert_eq!((tekst(&l.project), tekst(&l.bericht)), (None, None));
    }
}
