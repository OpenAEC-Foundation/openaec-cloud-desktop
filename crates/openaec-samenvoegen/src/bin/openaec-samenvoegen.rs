//! Opdrachtregelprogramma: IFC-bestanden vergelijken en samenvoegen.
//!
//! Exitcode: 0 = gelukt, 1 = conflicten (bij `voeg-samen`), 2 = fout.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use openaec_samenvoegen::{verschil, voeg_samen, BijConflict, Opties, Samenvoeging, Verschil, VersieStatistiek};

#[derive(Parser)]
#[command(name = "openaec-samenvoegen", version, about = "IFC-bestanden vergelijken en samenvoegen per object")]
struct Cli {
    #[command(subcommand)]
    opdracht: Opdracht,
}

#[derive(Subcommand)]
enum Opdracht {
    /// Het verschil tussen twee versies van een IFC-bestand.
    Verschil {
        a: PathBuf,
        b: PathBuf,
        /// Het verschil als JSON.
        #[arg(long)]
        json: bool,
    },
    /// Voegt de wijzigingen van ons en hun ten opzichte van de basis samen (three-way).
    VoegSamen {
        basis: PathBuf,
        ons: PathBuf,
        hun: PathBuf,
        /// Het samengevoegde bestand.
        #[arg(short = 'o', long = "uit")]
        uit: PathBuf,
        /// Schrijf het rapport (conflicten en cijfers) als JSON naar dit bestand.
        #[arg(long)]
        rapport: Option<PathBuf>,
        /// Wat er gebeurt bij een conflict: weiger (geen resultaat), ons of hun (die kant wint per conflict).
        #[arg(long, value_enum, default_value = "weiger")]
        bij_conflict: Keuze,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Keuze {
    Weiger,
    Ons,
    Hun,
}

fn lees(pad: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(pad).map_err(|e| format!("{}: {e}", pad.display()))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match voer_uit(cli.opdracht) {
        Ok(code) => code,
        Err(melding) => {
            eprintln!("fout: {melding}");
            ExitCode::from(2)
        }
    }
}

fn voer_uit(opdracht: Opdracht) -> Result<ExitCode, String> {
    match opdracht {
        Opdracht::Verschil { a, b, json } => {
            let inhoud_a = lees(&a)?;
            let inhoud_b = lees(&b)?;
            let v = verschil(&inhoud_a, &inhoud_b).map_err(|e| e.to_string())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?);
            } else {
                print!("{}", als_tekst(&v));
            }
            Ok(ExitCode::SUCCESS)
        }
        Opdracht::VoegSamen { basis, ons, hun, uit, rapport, bij_conflict } => {
            let (b, o, h) = (lees(&basis)?, lees(&ons)?, lees(&hun)?);
            let bij_conflict = match bij_conflict {
                Keuze::Weiger => BijConflict::Weiger,
                Keuze::Ons => BijConflict::Ons,
                Keuze::Hun => BijConflict::Hun,
            };
            let s = voeg_samen(&b, &o, &h, &Opties { bij_conflict }).map_err(|e| e.to_string())?;
            if let Some(pad) = &rapport {
                let json = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
                std::fs::write(pad, json + "\n").map_err(|e| format!("{}: {e}", pad.display()))?;
            }
            if let Some(inhoud) = &s.inhoud {
                std::fs::write(&uit, inhoud).map_err(|e| format!("{}: {e}", uit.display()))?;
            }
            print!("{}", rapport_tekst(&s, &uit));
            Ok(if s.conflicten.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
    }
}

fn rapport_tekst(s: &Samenvoeging, uit: &Path) -> String {
    let mut tekst = String::new();
    if !s.conflicten.is_empty() {
        tekst += &format!("Conflicten ({}):\n", s.conflicten.len());
        for c in &s.conflicten {
            let soort =
                serde_json::to_value(c.soort).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let wie = match (&c.type_naam, &c.naam, &c.globalid) {
                (Some(t), Some(n), Some(g)) => format!(" {t} '{n}' [{g}]"),
                (Some(t), None, Some(g)) => format!(" {t} [{g}]"),
                (Some(t), _, None) => format!(" {t}"),
                _ => String::new(),
            };
            tekst += &format!("  ! {soort}{wie}: {}\n", c.melding);
            if let Some(a) = &c.attribuut {
                let w = |x: &Option<String>| x.clone().unwrap_or_else(|| "-".into());
                tekst += &format!("      {a}: basis {}, ons {}, hun {}\n", w(&c.basis), w(&c.ons), w(&c.hun));
            }
            if let Some(k) = c.gekozen {
                tekst +=
                    &format!("      gekozen: {}\n", if k == openaec_samenvoegen::Kant::Ons { "ons" } else { "hun" });
            }
        }
    }
    let st = &s.statistiek;
    if s.gelukt {
        tekst += &format!(
            "Samengevoegd naar {}: {} objecten van ons, {} van hun, {} per attribuut samengevoegd, {} verwijderd.\n",
            uit.display(),
            st.objecten_van_ons,
            st.objecten_van_hun,
            st.objecten_samengevoegd,
            st.objecten_verwijderd
        );
        tekst += &format!(
            "Regels: {} nieuw, {} herschreven, {} weggehaald.\n",
            st.instanties_nieuw, st.instanties_herschreven, st.instanties_verwijderd
        );
    } else {
        tekst += "Niet samengevoegd; er is geen uitvoerbestand geschreven.\n";
    }
    tekst
}

fn als_tekst(v: &Verschil) -> String {
    let mut uit = String::new();
    let naam = |n: &Option<String>| n.as_ref().map(|n| format!(" '{n}'")).unwrap_or_default();
    if !v.toegevoegd.is_empty() {
        uit += &format!("Toegevoegd ({}):\n", v.toegevoegd.len());
        for o in &v.toegevoegd {
            uit += &format!("  + {}{} [{}]\n", o.type_naam, naam(&o.naam), o.globalid);
        }
    }
    if !v.verwijderd.is_empty() {
        uit += &format!("Verwijderd ({}):\n", v.verwijderd.len());
        for o in &v.verwijderd {
            uit += &format!("  - {}{} [{}]\n", o.type_naam, naam(&o.naam), o.globalid);
        }
    }
    if !v.gewijzigd.is_empty() {
        uit += &format!("Gewijzigd ({}):\n", v.gewijzigd.len());
        for w in &v.gewijzigd {
            let nieuw = w.globalid_nieuw.as_ref().map(|g| format!(" -> [{g}]")).unwrap_or_default();
            uit += &format!("  ~ {}{} [{}]{nieuw}\n", w.type_naam, naam(&w.naam), w.globalid);
            for a in &w.attributen {
                if a.erbij.is_empty() && a.eraf.is_empty() {
                    uit += &format!("      {}: {} -> {}\n", a.attribuut, a.oud, a.nieuw);
                } else {
                    uit += &format!("      {}:", a.attribuut);
                    for e in &a.erbij {
                        uit += &format!(" +{e}");
                    }
                    for e in &a.eraf {
                        uit += &format!(" -{e}");
                    }
                    uit += "\n";
                }
            }
        }
    }
    if !v.header.is_empty() {
        uit += "Header:\n";
        for a in &v.header {
            uit += &format!("      {}: {} -> {}\n", a.attribuut, a.oud, a.nieuw);
        }
    }
    let losse = &v.losse_knopen;
    if !losse.toegevoegd.is_empty() || !losse.verwijderd.is_empty() {
        uit += &format!("Losse knopen ({} erbij, {} weg):\n", losse.toegevoegd.len(), losse.verwijderd.len());
        for k in &losse.toegevoegd {
            uit += &format!("  + {}\n", k.tekst);
        }
        for k in &losse.verwijderd {
            uit += &format!("  - {}\n", k.tekst);
        }
    }
    if v.is_leeg() {
        uit += "Geen verschil.\n";
    }
    uit += &format!("a: {}\nb: {}\n", samenvatting(&v.statistiek.a), samenvatting(&v.statistiek.b));
    if v.statistiek.relaties_gekoppeld > 0 {
        uit += &format!("Relaties met een nieuwe GlobalId herkend: {}\n", v.statistiek.relaties_gekoppeld);
    }
    uit
}

fn samenvatting(s: &VersieStatistiek) -> String {
    let schema = if s.schema_tabel.is_empty() { format!("{} (onbekend schema)", s.schema) } else { s.schema.clone() };
    let mut uit = format!("{schema}, {} objecten, {} knopen", s.objecten, s.knopen);
    if !s.onbekende_typen.is_empty() {
        let aantal: usize = s.onbekende_typen.values().sum();
        uit += &format!(", {aantal} instanties van {} onbekende types", s.onbekende_typen.len());
    }
    if s.objecten_via_heuristiek > 0 {
        uit += &format!(", {} objecten herkend aan hun GlobalId", s.objecten_via_heuristiek);
    }
    uit
}
