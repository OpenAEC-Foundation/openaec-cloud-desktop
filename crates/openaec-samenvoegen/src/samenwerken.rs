//! De opslaan-lus (ontwerp §5.6): schrijf; is de basis verouderd, lees de basis en de huidige versie, voeg samen,
//! en schrijf opnieuw met de nieuwe basis. Na enkele pogingen geven we op.
//!
//! Een opslag (openaec-storage, Nextcloud...) past op de trait [`Opslag`]. Een slot van een ander is een aparte
//! weigering: daar helpt samenvoegen niet, en de lus probeert het dan ook niet.

use crate::samenvoegen::{voeg_samen, Opties, Samenvoeging};
use crate::Fout;

/// Waarom een opslag een verzoek weigert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Weigering<V> {
    /// De basis is niet meer de huidige versie; `huidige` is de versie die er nu staat.
    Verouderd { huidige: V },
    /// Een ander heeft het bestand vergrendeld.
    Vergrendeld { door: Option<String> },
    /// Een andere fout (netwerk, rechten, ...).
    Fout(String),
}

/// Een opslag met versies.
pub trait Opslag {
    /// Een versie, bijv. een ETag of een versienummer.
    type Versie: Clone + PartialEq + std::fmt::Debug;

    /// De huidige versie.
    fn huidige(&mut self) -> Result<Self::Versie, Weigering<Self::Versie>>;

    /// De inhoud van een versie.
    fn lees(&mut self, versie: &Self::Versie) -> Result<Vec<u8>, Weigering<Self::Versie>>;

    /// Schrijft `inhoud` als nieuwe versie, alleen als `basis` nog de huidige versie is. Geeft de nieuwe versie, of
    /// [`Weigering::Verouderd`] met de versie die er nu staat.
    fn schrijf_als(&mut self, basis: &Self::Versie, inhoud: &[u8]) -> Result<Self::Versie, Weigering<Self::Versie>>;
}

#[derive(Debug, Clone)]
pub struct LusOpties {
    /// Zo vaak proberen we hoogstens te schrijven.
    pub max_pogingen: usize,
    pub samenvoegen: Opties,
}

impl Default for LusOpties {
    fn default() -> Self {
        LusOpties { max_pogingen: 5, samenvoegen: Opties::default() }
    }
}

/// De uitkomst van [`sla_op`].
#[derive(Debug, Clone)]
pub enum Uitkomst<V> {
    /// Opgeslagen.
    Opgeslagen {
        /// De nieuwe versie.
        versie: V,
        /// Wat er nu in de opslag staat: de eigen inhoud, of het resultaat van het samenvoegen.
        inhoud: Vec<u8>,
        /// De eigen inhoud, zoals die binnenkwam.
        eigen: Vec<u8>,
        /// Hoe vaak we schreven.
        pogingen: usize,
        /// De samenvoegingen die nodig waren, met hun rapport.
        samenvoegingen: Vec<Samenvoeging>,
    },
    /// Samenvoegen gaf een conflict; er is niets opgeslagen. De client maakt een conflictkopie van `eigen` en zet
    /// het rapport ernaast.
    Conflict {
        /// De versie waartegen het conflict ontstond.
        huidige: V,
        samenvoeging: Box<Samenvoeging>,
        /// De eigen inhoud, zoals die binnenkwam.
        eigen: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LusFout<V: std::fmt::Debug> {
    /// Een ander heeft het bestand vergrendeld; we hebben niet geprobeerd samen te voegen.
    #[error("het bestand is vergrendeld{}", door.as_ref().map(|d| format!(" door {d}")).unwrap_or_default())]
    Vergrendeld { door: Option<String> },
    /// De opslag gaf een fout.
    #[error("de opslag gaf een fout: {0}")]
    Opslag(String),
    /// Na `pogingen` keer schrijven was de basis nog steeds verouderd.
    #[error("na {pogingen} pogingen was er steeds een nieuwere versie")]
    TeVaakVerouderd { pogingen: usize, huidige: V },
    /// Een versie is geen geldig IFC-bestand.
    #[error("samenvoegen mislukte: {0}")]
    Fout(Fout),
}

fn opslagfout<V: std::fmt::Debug>(w: Weigering<V>) -> LusFout<V> {
    match w {
        Weigering::Vergrendeld { door } => LusFout::Vergrendeld { door },
        Weigering::Fout(f) => LusFout::Opslag(f),
        Weigering::Verouderd { huidige } => LusFout::Opslag(format!("onverwacht verouderd (huidige: {huidige:?})")),
    }
}

/// Slaat `ons` op, gebaseerd op versie `basis`. Is er intussen een nieuwere versie, dan voegen we samen en proberen
/// het opnieuw, hoogstens `max_pogingen` keer.
pub fn sla_op<S: Opslag>(
    opslag: &mut S,
    basis: S::Versie,
    ons: &[u8],
    opties: &LusOpties,
) -> Result<Uitkomst<S::Versie>, LusFout<S::Versie>> {
    let mut basis_versie = basis;
    let mut basis_inhoud: Option<Vec<u8>> = None;
    let mut inhoud = ons.to_vec();
    let mut samenvoegingen = Vec::new();
    let max = opties.max_pogingen.max(1);
    for poging in 1..=max {
        let huidige = match opslag.schrijf_als(&basis_versie, &inhoud) {
            Ok(versie) => {
                return Ok(Uitkomst::Opgeslagen {
                    versie,
                    inhoud,
                    eigen: ons.to_vec(),
                    pogingen: poging,
                    samenvoegingen,
                })
            }
            Err(Weigering::Verouderd { huidige }) => huidige,
            Err(w) => return Err(opslagfout(w)),
        };
        if poging == max {
            return Err(LusFout::TeVaakVerouderd { pogingen: poging, huidige });
        }
        let b = match basis_inhoud.take() {
            Some(b) => b,
            None => opslag.lees(&basis_versie).map_err(opslagfout)?,
        };
        let hun = opslag.lees(&huidige).map_err(opslagfout)?;
        let s = voeg_samen(&b, &inhoud, &hun, &opties.samenvoegen).map_err(LusFout::Fout)?;
        match &s.inhoud {
            Some(r) => {
                inhoud = r.clone();
                basis_versie = huidige;
                basis_inhoud = Some(hun);
                samenvoegingen.push(s);
            }
            None => return Ok(Uitkomst::Conflict { huidige, samenvoeging: Box::new(s), eigen: ons.to_vec() }),
        }
    }
    unreachable!("de lus eindigt altijd met return")
}
