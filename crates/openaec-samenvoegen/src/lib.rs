//! Twee versies van een IFC-bestand vergelijken ([`verschil`]) en samenvoegen ([`voeg_samen`]), per object.
//!
//! Een object is een instantie van IfcRoot (met een GlobalId); al het andere (geometrie, plaatsing,
//! eigenschapswaarden...) hoort als knoop bij de objecten die ernaar verwijzen. Twee bestanden met dezelfde inhoud
//! maar een andere nummering (`#n`) zijn gelijk. Zie de README voor de regels.

mod koppelregel;
mod model;
mod ontleden;
mod samenvoegen;
mod samenwerken;
mod schema;
mod schrijven;
mod tekst;
mod verschil;

use serde::Serialize;

pub use model::VersieStatistiek;
pub use samenvoegen::{
    voeg_samen, BijConflict, Conflict, ConflictSoort, Kant, Opties, SamenvoegStatistiek, Samenvoeging, MAX_RONDES,
};
pub use samenwerken::{sla_op, LusFout, LusOpties, Opslag, Uitkomst, Weigering};
pub use verschil::{verschil, AttribuutWijziging, KnoopInfo, LosseKnopen, ObjectInfo, Statistiek, Verschil, Wijziging};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "soort", rename_all = "snake_case")]
pub enum Fout {
    /// Het bestand is geen geldig STEP-bestand.
    #[error("regel {regel}: {melding}")]
    Ongeldig { regel: usize, melding: String },
    #[error("het bestand is 4 GiB of groter; dat wordt niet ondersteund")]
    TeGroot,
    /// Twee objecten met dezelfde GlobalId: dan weten we niet welk object welk is.
    #[error("regel {regel}: GlobalId {globalid} komt twee keer voor")]
    DubbeleGlobalId { globalid: String, regel: usize },
    /// Een fout in de bibliotheek zelf (bijv. een thread die stopte).
    #[error("interne fout: {0}")]
    Intern(String),
}

impl Fout {
    pub(crate) fn intern(melding: &str) -> Self {
        Fout::Intern(melding.to_string())
    }
}

/// Kunnen we dit bestand samenvoegen? Alleen `.ifc` (STEP); `.ifczip` en `.ifcxml` nog niet.
pub fn kan_samenvoegen(bestandsnaam: &str) -> bool {
    bestandsnaam.to_ascii_lowercase().ends_with(".ifc")
}

#[cfg(test)]
mod tests {
    #[test]
    fn bestandsnamen() {
        assert!(super::kan_samenvoegen("Planning.IFC"));
        assert!(super::kan_samenvoegen("map/model.ifc"));
        assert!(!super::kan_samenvoegen("model.ifczip"));
        assert!(!super::kan_samenvoegen("model.ifcxml"));
        assert!(!super::kan_samenvoegen("model.ifc.bak"));
    }
}
