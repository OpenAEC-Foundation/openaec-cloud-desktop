//! Opdrachtregelprogramma: twee IFC-bestanden vergelijken.
//!
//! Exitcode: 0 = gelukt, 2 = fout.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use openaec_samenvoegen::{verschil, Verschil, VersieStatistiek};

#[derive(Parser)]
#[command(
    name = "openaec-samenvoegen",
    version,
    about = "IFC-bestanden vergelijken per object"
)]
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
    }
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
            let nieuw = w
                .globalid_nieuw
                .as_ref()
                .map(|g| format!(" -> [{g}]"))
                .unwrap_or_default();
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
    let losse = &v.losse_knopen;
    if !losse.toegevoegd.is_empty() || !losse.verwijderd.is_empty() {
        uit += &format!(
            "Losse knopen ({} erbij, {} weg):\n",
            losse.toegevoegd.len(),
            losse.verwijderd.len()
        );
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
    uit += &format!(
        "a: {}\nb: {}\n",
        samenvatting(&v.statistiek.a),
        samenvatting(&v.statistiek.b)
    );
    if v.statistiek.relaties_gekoppeld > 0 {
        uit += &format!(
            "Relaties met een nieuwe GlobalId herkend: {}\n",
            v.statistiek.relaties_gekoppeld
        );
    }
    uit
}

fn samenvatting(s: &VersieStatistiek) -> String {
    let schema = if s.schema_tabel.is_empty() {
        format!("{} (onbekend schema)", s.schema)
    } else {
        s.schema.clone()
    };
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
