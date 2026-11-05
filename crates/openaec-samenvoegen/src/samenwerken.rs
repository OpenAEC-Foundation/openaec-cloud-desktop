//! De opslaan-lus (ontwerp §5.5 en §5.6): schrijf; is de basis verouderd, lees de basis en de huidige versie, voeg
//! samen, en schrijf opnieuw met de nieuwe basis. Na enkele pogingen geven we op.
//!
//! De basis is het bestand plus een id van de versie dat nooit terugkomt en altijd dezelfde inhoud aanwijst: bij
//! openaec-storage `basis_bestand` en `basis_versie_id`. Het is geen versienummer; dat is alleen voor mensen. Een
//! bestand dat op dezelfde plek opnieuw is gemaakt, is een ander bestand.
//!
//! Een opslag past op de trait [`Opslag`]; openaec-storage past er nu op. Een opslag kan een schrijfverzoek op drie manieren weigeren:
//! - **verouderd**: de basis hoort bij dit bestand, maar is niet meer de huidige versie. De lus leest de basis en de
//!   huidige versie, voegt samen en schrijft opnieuw.
//! - **onbekende basis**: de basis hoort niet (meer) bij dit bestand (bijv. een ander bestand op dezelfde plek), of
//!   de opslag weet het niet zeker (openaec-storage: `onbekende_basis`). De lus leest niets en voegt niet samen: dat
//!   is een conflict, en de client maakt een conflictkopie.
//! - **vergrendeld**: een slot van een ander. Daar helpt samenvoegen niet; de lus probeert het niet.
//!
//! Kan de lus de basis niet lezen (bijv. de versie bestaat niet meer), dan voegt hij ook niet samen: conflict.

use std::fmt::Debug;

use crate::samenvoegen::{voeg_samen, Samenvoeging};
use crate::Fout;

/// Een basis: het bestand en een id van de versie dat nooit terugkomt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Basis<B, V> {
    /// De identiteit van het bestand (bij openaec-storage `basis_bestand`); een opnieuw gemaakt bestand heeft een
    /// andere.
    pub bestand: B,
    /// Het id van de versie (bij openaec-storage `basis_versie_id`); wijst altijd naar dezelfde inhoud of naar niets.
    pub versie: V,
}

/// Waarom een opslag een verzoek weigert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Weigering<B, V> {
    /// De basis hoort bij dit bestand, maar is niet meer de huidige versie; `huidige` is wat er nu staat.
    Verouderd { huidige: Basis<B, V> },
    /// De basis hoort niet (meer) bij dit bestand, of de opslag weet het niet zeker.
    OnbekendeBasis,
    /// Een ander heeft het bestand vergrendeld.
    Vergrendeld { door: Option<String> },
    /// Een andere fout (netwerk, rechten, ...).
    Fout(String),
}

/// De basis bij opslag `S`.
pub type BasisVan<S> = Basis<<S as Opslag>::Bestand, <S as Opslag>::Versie>;

/// Een antwoord van opslag `S`: een waarde, of een [`Weigering`].
pub type Antwoord<S, T> = Result<T, Weigering<<S as Opslag>::Bestand, <S as Opslag>::Versie>>;

/// Een opslag met versies van één bestand. openaec-storage past hierop.
pub trait Opslag {
    /// De identiteit van een bestand.
    type Bestand: Clone + PartialEq + Debug;
    /// Een id van een versie dat nooit terugkomt.
    type Versie: Clone + PartialEq + Debug;

    /// Het bestand en zijn huidige versie.
    fn huidige(&mut self) -> Antwoord<Self, BasisVan<Self>>;

    /// De inhoud van precies deze versie, of een fout als die er niet (meer) is.
    fn lees(&mut self, versie: &BasisVan<Self>) -> Antwoord<Self, Vec<u8>>;

    /// Schrijft `inhoud` als nieuwe versie, alleen als `basis` nog de huidige versie van dit bestand is. Geeft de
    /// nieuwe basis, of een [`Weigering`].
    fn schrijf_als(&mut self, basis: &BasisVan<Self>, inhoud: &[u8]) -> Antwoord<Self, BasisVan<Self>>;
}

#[derive(Debug, Clone)]
pub struct LusOpties {
    /// Zo vaak proberen we hoogstens te schrijven.
    pub max_pogingen: usize,
}

impl Default for LusOpties {
    fn default() -> Self {
        LusOpties { max_pogingen: 5 }
    }
}

/// Waarom de lus niet opsloeg.
#[derive(Debug, Clone)]
pub enum Reden {
    /// Samenvoegen gaf conflicten; het rapport staat erbij.
    Samenvoegen(Box<Samenvoeging>),
    /// De opslag kent de basis niet (meer) bij dit bestand, of weet het niet zeker.
    OnbekendeBasis,
    /// De basis was niet te lezen (bijv. de versie bestaat niet meer).
    BasisNietTeLezen(String),
}

/// De uitkomst van [`sla_op`].
#[derive(Debug, Clone)]
pub enum Uitkomst<B, V> {
    /// Opgeslagen.
    Opgeslagen {
        /// De nieuwe basis: het bestand en de nieuwe versie.
        nieuw: Basis<B, V>,
        /// Wat er nu in de opslag staat: de eigen inhoud, of het resultaat van het samenvoegen.
        inhoud: Vec<u8>,
        /// De eigen inhoud, zoals die binnenkwam.
        eigen: Vec<u8>,
        /// Hoe vaak we schreven.
        pogingen: usize,
        /// De samenvoegingen die nodig waren, met hun rapport.
        samenvoegingen: Vec<Samenvoeging>,
    },
    /// Niet opgeslagen en niet samengevoegd; er is niets geschreven. De client maakt een conflictkopie van `eigen`
    /// en zet het rapport (als dat er is) ernaast.
    Conflict {
        /// De versie waartegen het conflict ontstond, als we die weten.
        huidige: Option<Basis<B, V>>,
        reden: Reden,
        /// De eigen inhoud, zoals die binnenkwam.
        eigen: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LusFout<B: Debug, V: Debug> {
    /// Een ander heeft het bestand vergrendeld; we hebben niet geprobeerd samen te voegen.
    #[error("het bestand is vergrendeld{}", door.as_ref().map(|d| format!(" door {d}")).unwrap_or_default())]
    Vergrendeld { door: Option<String> },
    /// De opslag gaf een fout.
    #[error("de opslag gaf een fout: {0}")]
    Opslag(String),
    /// Na `pogingen` keer schrijven was de basis nog steeds verouderd.
    #[error("na {pogingen} pogingen was er steeds een nieuwere versie")]
    TeVaakVerouderd { pogingen: usize, huidige: Basis<B, V> },
    /// Een versie is geen geldig IFC-bestand.
    #[error("samenvoegen mislukte: {0}")]
    Fout(Fout),
}

fn opslagfout<B: Debug, V: Debug>(w: Weigering<B, V>) -> LusFout<B, V> {
    match w {
        Weigering::Vergrendeld { door } => LusFout::Vergrendeld { door },
        Weigering::Fout(f) => LusFout::Opslag(f),
        Weigering::OnbekendeBasis => LusFout::Opslag("onbekende basis".into()),
        Weigering::Verouderd { huidige } => LusFout::Opslag(format!("onverwacht verouderd (huidige: {huidige:?})")),
    }
}

/// Wat [`sla_op`] geeft bij opslag `S`.
pub type LusUitkomst<S> = Result<
    Uitkomst<<S as Opslag>::Bestand, <S as Opslag>::Versie>,
    LusFout<<S as Opslag>::Bestand, <S as Opslag>::Versie>,
>;

/// Slaat `ons` op, gebaseerd op `basis`. Is er intussen een nieuwere versie van hetzelfde bestand, dan voegen we
/// samen en proberen het opnieuw, hoogstens `max_pogingen` keer.
pub fn sla_op<S: Opslag>(opslag: &mut S, basis: BasisVan<S>, ons: &[u8], opties: &LusOpties) -> LusUitkomst<S> {
    let mut basis = basis;
    let mut basis_inhoud: Option<Vec<u8>> = None;
    let mut inhoud = ons.to_vec();
    let mut samenvoegingen = Vec::new();
    let conflict = |huidige, reden| Ok(Uitkomst::Conflict { huidige, reden, eigen: ons.to_vec() });
    let max = opties.max_pogingen.max(1);
    for poging in 1..=max {
        let huidige = match opslag.schrijf_als(&basis, &inhoud) {
            Ok(nieuw) => {
                return Ok(Uitkomst::Opgeslagen {
                    nieuw,
                    inhoud,
                    eigen: ons.to_vec(),
                    pogingen: poging,
                    samenvoegingen,
                })
            }
            // Een andere identiteit van het bestand is geen verouderde basis: niet samenvoegen.
            Err(Weigering::Verouderd { huidige }) if huidige.bestand != basis.bestand => {
                return conflict(Some(huidige), Reden::OnbekendeBasis)
            }
            Err(Weigering::Verouderd { huidige }) => huidige,
            Err(Weigering::OnbekendeBasis) => return conflict(None, Reden::OnbekendeBasis),
            Err(w) => return Err(opslagfout(w)),
        };
        if poging == max {
            return Err(LusFout::TeVaakVerouderd { pogingen: poging, huidige });
        }
        let b = match basis_inhoud.take() {
            Some(b) => b,
            None => match opslag.lees(&basis) {
                Ok(b) => b,
                Err(Weigering::Vergrendeld { door }) => return Err(LusFout::Vergrendeld { door }),
                Err(w) => {
                    let melding = match w {
                        Weigering::Fout(f) => f,
                        andere => format!("{andere:?}"),
                    };
                    return conflict(Some(huidige), Reden::BasisNietTeLezen(melding));
                }
            },
        };
        let hun = opslag.lees(&huidige).map_err(opslagfout)?;
        let s = voeg_samen(&b, &inhoud, &hun).map_err(LusFout::Fout)?;
        match &s.inhoud {
            Some(r) => {
                inhoud = r.clone();
                basis = huidige;
                basis_inhoud = Some(hun);
                samenvoegingen.push(s);
            }
            None => return conflict(Some(huidige), Reden::Samenvoegen(Box::new(s))),
        }
    }
    unreachable!("de lus eindigt altijd met return")
}
