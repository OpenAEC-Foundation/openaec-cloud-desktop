//! Bijlagen voor de assistent voorbereiden: een afbeelding (png, jpg, webp) gaat verkleind mee als data-URL, tekst en
//! code als tekst, en van een PDF, DOCX of XLSX alleen de tekst. Er zijn grenzen per bestand en voor alle tekst in een
//! gesprek samen; is iets te groot of niet te lezen, dan volgt een Nederlandse melding (geen bijlage).

use std::io::Read;
use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};

/// Zoveel tekens tekst uit één bestand (ruwweg 50.000 tokens).
pub const MAX_TEKST_PER_BESTAND: usize = 200_000;
/// Zoveel tekens tekst uit bijlagen in één gesprek samen.
pub const MAX_TEKST_TOTAAL: usize = 400_000;
/// Groter bestand lezen we niet in (ook niet om er tekst uit te halen).
pub const MAX_BESTAND: u64 = 40 * 1024 * 1024;
/// De langste zijde van een afbeelding na verkleinen (meer ziet het model niet scherper).
pub const MAX_ZIJDE: u32 = 1568;
/// Hoogstens zo groot na verkleinen (de gateway accepteert 3,75 MB per afbeelding; de hele vraag blijft onder 8 MB).
pub const MAX_AFBEELDING: usize = 1_500_000;
/// Zoveel afbeeldingen in één gesprek (de grens van de gateway).
pub const MAX_AFBEELDINGEN: usize = 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bijlage {
    pub naam: String,
    /// De grootte van het oorspronkelijke bestand in bytes.
    pub grootte: u64,
    #[serde(flatten)]
    pub inhoud: Inhoud,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "soort", rename_all = "snake_case")]
pub enum Inhoud {
    Tekst { tekst: String },
    Afbeelding { data_url: String, breedte: u32, hoogte: u32 },
}

impl Bijlage {
    /// Het aantal tekens tekst (0 voor een afbeelding), voor de grens per gesprek.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn tekens(&self) -> usize {
        match &self.inhoud {
            Inhoud::Tekst { tekst } => tekst.chars().count(),
            Inhoud::Afbeelding { .. } => 0,
        }
    }
}

const AFBEELDINGEN: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
/// Bestanden die we zonder meer als tekst lezen. Een onbekende extensie mag ook, als de inhoud tekst blijkt te zijn.
const TEKST: &[&str] = &[
    "txt", "md", "markdown", "csv", "tsv", "json", "jsonl", "xml", "ifc", "ifcxml", "py", "ts", "tsx", "js", "jsx", "mjs",
    "cjs", "rs", "toml", "yaml", "yml", "ini", "cfg", "conf", "log", "html", "htm", "css", "scss", "sql", "sh", "ps1",
    "bat", "cmd", "c", "h", "cpp", "hpp", "cs", "java", "kt", "go", "rb", "php", "r", "swift", "lua", "svg", "tex",
    "rtf", "vue", "svelte", "dart", "gml",
];

fn extensie(naam: &str) -> String {
    Path::new(naam).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Een MB-getal voor in een melding ("1,5 MB").
fn mb(bytes: u64) -> String {
    let waarde = bytes as f64 / 1_048_576.0;
    let tekst = if waarde >= 10.0 { format!("{waarde:.0}") } else { format!("{waarde:.1}") };
    format!("{} MB", tekst.replace('.', ","))
}

fn duizendtal(n: usize) -> String {
    let s = n.to_string();
    let mut uit = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            uit.push('.');
        }
        uit.push(c);
    }
    uit
}

/// Lees een bestand en maak er een bijlage van. `al_tekst`: het aantal tekens tekst dat al in het gesprek zit.
pub fn van_pad(pad: &Path, al_tekst: usize) -> Result<Bijlage, String> {
    let naam = pad.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "bijlage".into());
    let meta = std::fs::metadata(pad).map_err(|e| format!("{naam}: niet te lezen ({e})"))?;
    if meta.is_dir() {
        return Err(format!("{naam} is een map; sleep losse bestanden naar het venster."));
    }
    if meta.len() > MAX_BESTAND {
        return Err(format!("{naam} is te groot ({}); hoogstens {} per bestand.", mb(meta.len()), mb(MAX_BESTAND)));
    }
    let bytes = std::fs::read(pad).map_err(|e| format!("{naam}: niet te lezen ({e})"))?;
    van_bytes(&naam, &bytes, al_tekst)
}

/// Als [`van_pad`], voor wat al in het geheugen staat (een geplakte afbeelding).
pub fn van_bytes(naam: &str, bytes: &[u8], al_tekst: usize) -> Result<Bijlage, String> {
    let grootte = bytes.len() as u64;
    if grootte > MAX_BESTAND {
        return Err(format!("{naam} is te groot ({}); hoogstens {} per bestand.", mb(grootte), mb(MAX_BESTAND)));
    }
    let ext = extensie(naam);
    let inhoud = if AFBEELDINGEN.contains(&ext.as_str()) {
        afbeelding(naam, bytes)?
    } else {
        let tekst = match ext.as_str() {
            "pdf" => pdf_tekst(naam, bytes)?,
            "docx" => docx_tekst(naam, bytes)?,
            "xlsx" => xlsx_tekst(naam, bytes)?,
            "doc" | "xls" | "ppt" | "pptx" | "odt" | "ods" | "zip" | "7z" | "rar" | "exe" | "dll" | "dwg" | "gif"
            | "bmp" | "tif" | "tiff" | "heic" | "mp3" | "mp4" | "mov" => {
                return Err(format!("{naam}: dit soort bestand kan (nog) niet mee. Wel: afbeeldingen (png, jpg, webp), tekst en code, PDF, DOCX en XLSX."))
            }
            e if TEKST.contains(&e) => als_tekst(bytes),
            _ if lijkt_tekst(bytes) => als_tekst(bytes),
            _ => return Err(format!("{naam}: dit lijkt geen tekstbestand; het kan niet mee.")),
        };
        let tekens = tekst.chars().count();
        if tekens > MAX_TEKST_PER_BESTAND {
            return Err(format!(
                "{naam} bevat te veel tekst ({} tekens); hoogstens {} per bestand.",
                duizendtal(tekens),
                duizendtal(MAX_TEKST_PER_BESTAND)
            ));
        }
        if al_tekst + tekens > MAX_TEKST_TOTAAL {
            return Err(format!(
                "{naam} past niet meer in dit gesprek: alle bijlagen samen hoogstens {} tekens. Begin een nieuw gesprek of haal een bijlage weg.",
                duizendtal(MAX_TEKST_TOTAAL)
            ));
        }
        Inhoud::Tekst { tekst }
    };
    Ok(Bijlage { naam: naam.to_string(), grootte, inhoud })
}

/// Tekst zonder NUL-tekens in het begin (zo herken je een binair bestand).
fn lijkt_tekst(bytes: &[u8]) -> bool {
    let begin = &bytes[..bytes.len().min(8192)];
    if begin.contains(&0) {
        return false;
    }
    match std::str::from_utf8(begin) {
        Ok(_) => true,
        // Afgekapt midden in een teken: wel UTF-8.
        Err(e) if e.error_len().is_none() => true,
        Err(_) => begin.iter().all(|b| b.is_ascii() || *b >= 0xa0),
    }
}

/// UTF-8 (zonder BOM); anders Latin-1 / Windows-1252 zoals oudere bestanden vaak zijn.
fn als_tekst(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(t) => t.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

fn afbeelding(naam: &str, bytes: &[u8]) -> Result<Inhoud, String> {
    let beeld = image::load_from_memory(bytes).map_err(|_| format!("{naam}: deze afbeelding is niet te lezen."))?;
    let (b, h) = (beeld.width(), beeld.height());
    let beeld = if b.max(h) > MAX_ZIJDE { beeld.resize(MAX_ZIJDE, MAX_ZIJDE, image::imageops::FilterType::Triangle) } else { beeld };
    let doorzichtig = beeld.color().has_alpha() && beeld.to_rgba8().pixels().any(|p| p.0[3] < 255);
    let mut zijde = MAX_ZIJDE;
    let mut beeld = beeld;
    loop {
        let (mime, data) = if doorzichtig {
            let mut uit = std::io::Cursor::new(Vec::new());
            beeld.write_to(&mut uit, image::ImageFormat::Png).map_err(|e| format!("{naam}: verkleinen mislukt ({e})"))?;
            ("image/png", uit.into_inner())
        } else {
            let mut uit = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut uit, 85)
                .encode_image(&beeld.to_rgb8())
                .map_err(|e| format!("{naam}: verkleinen mislukt ({e})"))?;
            ("image/jpeg", uit)
        };
        if data.len() <= MAX_AFBEELDING || zijde <= 256 {
            if data.len() > MAX_AFBEELDING {
                return Err(format!("{naam}: de afbeelding blijft na verkleinen te groot."));
            }
            return Ok(Inhoud::Afbeelding {
                data_url: format!("data:{mime};base64,{}", STANDARD.encode(&data)),
                breedte: beeld.width(),
                hoogte: beeld.height(),
            });
        }
        zijde = zijde * 3 / 4;
        beeld = beeld.resize(zijde, zijde, image::imageops::FilterType::Triangle);
    }
}

fn pdf_tekst(naam: &str, bytes: &[u8]) -> Result<String, String> {
    // pdf-extract kan in een vreemd bestand in paniek raken: dat wordt gewoon een melding.
    let uitkomst = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes));
    let tekst = match uitkomst {
        Ok(Ok(t)) => t,
        _ => return Err(format!("{naam}: de tekst uit deze PDF is niet te lezen (beveiligd of beschadigd).")),
    };
    let tekst = netjes(&tekst);
    if tekst.trim().is_empty() {
        return Err(format!("{naam}: deze PDF bevat geen tekst die te lezen is (bijvoorbeeld een scan). Stuur een pagina als afbeelding mee."));
    }
    Ok(tekst)
}

/// Witruimte opruimen: geen spaties aan het eind van een regel en hoogstens één lege regel achter elkaar.
fn netjes(tekst: &str) -> String {
    let mut uit = String::with_capacity(tekst.len());
    let mut leeg = 0;
    for regel in tekst.lines().map(str::trim_end) {
        if regel.is_empty() {
            leeg += 1;
            if leeg > 1 {
                continue;
            }
        } else {
            leeg = 0;
        }
        uit.push_str(regel);
        uit.push('\n');
    }
    uit.trim().to_string()
}

fn zipdeel(archief: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, pad: &str) -> Option<String> {
    let mut deel = archief.by_name(pad).ok()?;
    let mut tekst = String::new();
    deel.read_to_string(&mut tekst).ok()?;
    Some(tekst)
}

fn open_zip<'a>(naam: &str, bytes: &'a [u8]) -> Result<zip::ZipArchive<std::io::Cursor<&'a [u8]>>, String> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| format!("{naam}: dit bestand is niet te lezen (beschadigd?)."))
}

fn docx_tekst(naam: &str, bytes: &[u8]) -> Result<String, String> {
    let mut archief = open_zip(naam, bytes)?;
    let xml = zipdeel(&mut archief, "word/document.xml").ok_or_else(|| format!("{naam}: geen Word-document."))?;
    let mut uit = String::new();
    let mut in_tekst = false;
    for deel in xml::delen(&xml) {
        match deel {
            xml::Deel::Open { naam: "w:t", .. } => in_tekst = true,
            xml::Deel::Dicht("w:t") => in_tekst = false,
            xml::Deel::Tekst(t) if in_tekst => uit.push_str(&xml::ontsnap(t)),
            xml::Deel::Open { naam: "w:tab", .. } => uit.push('\t'),
            xml::Deel::Open { naam: "w:br" | "w:cr", .. } => uit.push('\n'),
            xml::Deel::Dicht("w:p") => uit.push('\n'),
            _ => {}
        }
    }
    let tekst = netjes(&uit);
    if tekst.is_empty() {
        return Err(format!("{naam}: dit document bevat geen tekst."));
    }
    Ok(tekst)
}

/// Kolomnummer (0 = A) uit een celverwijzing als `AB12`.
fn kolom(r: &str) -> usize {
    r.chars().take_while(|c| c.is_ascii_alphabetic()).fold(0usize, |n, c| n * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1))
        .saturating_sub(1)
}

fn xlsx_tekst(naam: &str, bytes: &[u8]) -> Result<String, String> {
    let mut archief = open_zip(naam, bytes)?;
    // Gedeelde teksten: per <si> alle <t> samen.
    let mut gedeeld = Vec::new();
    if let Some(xml) = zipdeel(&mut archief, "xl/sharedStrings.xml") {
        let (mut huidig, mut in_t) = (String::new(), false);
        for deel in xml::delen(&xml) {
            match deel {
                xml::Deel::Open { naam: "si", .. } => huidig.clear(),
                xml::Deel::Dicht("si") => gedeeld.push(std::mem::take(&mut huidig)),
                xml::Deel::Open { naam: "t", leeg: false, .. } => in_t = true,
                xml::Deel::Dicht("t") => in_t = false,
                xml::Deel::Tekst(t) if in_t => huidig.push_str(&xml::ontsnap(t)),
                _ => {}
            }
        }
    }
    // De bladen in de volgorde van de werkmap, met hun naam.
    let namen: Vec<String> = zipdeel(&mut archief, "xl/workbook.xml")
        .map(|xml| {
            xml::delen(&xml)
                .filter_map(|d| match d {
                    xml::Deel::Open { naam: "sheet", attributen, .. } => xml::attribuut(attributen, "name").map(|n| xml::ontsnap(n)),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let mut uit = String::new();
    for i in 1.. {
        let Some(xml) = zipdeel(&mut archief, &format!("xl/worksheets/sheet{i}.xml")) else { break };
        let bladnaam = namen.get(i - 1).cloned().unwrap_or_else(|| format!("Blad {i}"));
        uit.push_str(&format!("## {bladnaam}\n"));
        let mut rij: Vec<String> = Vec::new();
        let (mut cel_kolom, mut cel_soort, mut waarde, mut in_waarde) = (0usize, String::new(), String::new(), false);
        for deel in xml::delen(&xml) {
            match deel {
                xml::Deel::Open { naam: "row", leeg: false, .. } => rij.clear(),
                xml::Deel::Dicht("row") => {
                    if rij.iter().any(|c| !c.is_empty()) {
                        uit.push_str(rij.join("\t").trim_end());
                        uit.push('\n');
                    }
                }
                xml::Deel::Open { naam: "c", attributen, leeg } => {
                    cel_kolom = xml::attribuut(attributen, "r").map(kolom).unwrap_or(rij.len());
                    cel_soort = xml::attribuut(attributen, "t").unwrap_or("").to_string();
                    waarde.clear();
                    if leeg {
                        continue;
                    }
                }
                xml::Deel::Open { naam: "v" | "t", leeg: false, .. } => in_waarde = true,
                xml::Deel::Dicht("v" | "t") => in_waarde = false,
                xml::Deel::Tekst(t) if in_waarde => waarde.push_str(&xml::ontsnap(t)),
                xml::Deel::Dicht("c") => {
                    let tekst = match cel_soort.as_str() {
                        "s" => waarde.trim().parse::<usize>().ok().and_then(|n| gedeeld.get(n).cloned()).unwrap_or_default(),
                        "b" => (if waarde.trim() == "1" { "WAAR" } else { "ONWAAR" }).to_string(),
                        _ => waarde.clone(),
                    };
                    let tekst = tekst.replace(['\t', '\n', '\r'], " ");
                    if rij.len() <= cel_kolom {
                        rij.resize(cel_kolom + 1, String::new());
                    }
                    rij[cel_kolom] = tekst;
                }
                _ => {}
            }
        }
        uit.push('\n');
    }
    let tekst = uit.trim().to_string();
    if tekst.lines().all(|r| r.starts_with("## ") || r.trim().is_empty()) {
        return Err(format!("{naam}: deze werkmap bevat geen gegevens."));
    }
    Ok(tekst)
}

/// Een heel kleine XML-lezer: genoeg voor de tekst uit DOCX en XLSX (geen DTD's, geen validatie).
mod xml {
    pub enum Deel<'a> {
        Open { naam: &'a str, attributen: &'a str, leeg: bool },
        Dicht(&'a str),
        Tekst(&'a str),
    }

    pub fn delen(xml: &str) -> impl Iterator<Item = Deel<'_>> {
        let mut rest = xml;
        std::iter::from_fn(move || loop {
            if rest.is_empty() {
                return None;
            }
            if let Some(na) = rest.strip_prefix('<') {
                let einde = na.find('>')?;
                let tag = &na[..einde];
                rest = &na[einde + 1..];
                if tag.starts_with('?') || tag.starts_with('!') {
                    continue;
                }
                if let Some(naam) = tag.strip_prefix('/') {
                    return Some(Deel::Dicht(naam.trim()));
                }
                let leeg = tag.ends_with('/');
                let tag = tag.trim_end_matches('/');
                let (naam, attributen) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
                return Some(Deel::Open { naam, attributen, leeg });
            }
            let einde = rest.find('<').unwrap_or(rest.len());
            let tekst = &rest[..einde];
            rest = &rest[einde..];
            return Some(Deel::Tekst(tekst));
        })
    }

    pub fn attribuut<'a>(attributen: &'a str, naam: &str) -> Option<&'a str> {
        let mut rest = attributen;
        while let Some(i) = rest.find('=') {
            let sleutel = rest[..i].trim();
            let na = rest[i + 1..].trim_start();
            let aanhaling = na.chars().next()?;
            let na = &na[1..];
            let eind = na.find(aanhaling)?;
            if sleutel == naam {
                return Some(&na[..eind]);
            }
            rest = &na[eind + 1..];
        }
        None
    }

    pub fn ontsnap(tekst: &str) -> String {
        if !tekst.contains('&') {
            return tekst.to_string();
        }
        let mut uit = String::with_capacity(tekst.len());
        let mut rest = tekst;
        while let Some(i) = rest.find('&') {
            uit.push_str(&rest[..i]);
            let na = &rest[i + 1..];
            let Some(eind) = na.find(';').filter(|e| *e <= 10) else {
                uit.push('&');
                rest = na;
                continue;
            };
            let entiteit = &na[..eind];
            let teken = match entiteit {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
                e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            };
            match teken {
                Some(c) => {
                    uit.push(c);
                    rest = &na[eind + 1..];
                }
                None => {
                    uit.push('&');
                    rest = na;
                }
            }
        }
        uit.push_str(rest);
        uit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_met(delen: &[(&str, &str)]) -> Vec<u8> {
        let mut uit = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opties = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (pad, inhoud) in delen {
            uit.start_file(*pad, opties).unwrap();
            uit.write_all(inhoud.as_bytes()).unwrap();
        }
        uit.finish().unwrap().into_inner()
    }

    #[test]
    fn tekst_en_code_gaan_als_tekst() {
        let b = van_bytes("notities.md", "# Kop\nÉén regel".as_bytes(), 0).unwrap();
        assert_eq!(b.inhoud, Inhoud::Tekst { tekst: "# Kop\nÉén regel".into() });
        assert_eq!((b.grootte, b.tekens()), (17, 15));
        // BOM weg; Latin-1 wordt gewoon gelezen; een IFC is tekst.
        assert_eq!(van_bytes("a.csv", b"\xef\xbb\xbfa;b", 0).unwrap().inhoud, Inhoud::Tekst { tekst: "a;b".into() });
        assert_eq!(van_bytes("oud.txt", b"caf\xe9", 0).unwrap().inhoud, Inhoud::Tekst { tekst: "café".into() });
        assert!(matches!(van_bytes("m.ifc", b"ISO-10303-21;\nHEADER;", 0).unwrap().inhoud, Inhoud::Tekst { .. }));
        // Een onbekende extensie met tekst mag; binaire inhoud niet.
        assert!(van_bytes("Makefile", b"all:\n\tcargo build", 0).is_ok());
        assert!(van_bytes("data.bin", &[0u8, 1, 2, 3], 0).unwrap_err().contains("geen tekstbestand"));
        assert!(van_bytes("oud.doc", b"x", 0).unwrap_err().contains("kan (nog) niet mee"));
    }

    #[test]
    fn grenzen_per_bestand_en_per_gesprek() {
        let groot = "a".repeat(MAX_TEKST_PER_BESTAND + 1);
        let f = van_bytes("groot.txt", groot.as_bytes(), 0).unwrap_err();
        assert!(f.contains("te veel tekst") && f.contains("200.000"), "{f}");
        let precies = "a".repeat(1000);
        assert!(van_bytes("a.txt", precies.as_bytes(), MAX_TEKST_TOTAAL - 1000).is_ok());
        let f = van_bytes("a.txt", precies.as_bytes(), MAX_TEKST_TOTAAL - 999).unwrap_err();
        assert!(f.contains("past niet meer in dit gesprek"), "{f}");
        assert_eq!(mb(1_572_864), "1,5 MB");
        assert_eq!(mb(MAX_BESTAND), "40 MB");
    }

    #[test]
    fn afbeelding_wordt_verkleind_tot_een_data_url() {
        // Een grote foto (zonder doorzichtigheid) wordt JPEG van hoogstens MAX_ZIJDE.
        let beeld = image::RgbImage::from_fn(3000, 1500, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 120]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(beeld).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let b = van_bytes("foto.png", png.get_ref(), 0).unwrap();
        let Inhoud::Afbeelding { data_url, breedte, hoogte } = &b.inhoud else { panic!("geen afbeelding") };
        assert_eq!((*breedte, *hoogte), (MAX_ZIJDE, MAX_ZIJDE / 2));
        assert!(data_url.starts_with("data:image/jpeg;base64,"));
        let data = STANDARD.decode(&data_url["data:image/jpeg;base64,".len()..]).unwrap();
        assert!(data.len() <= MAX_AFBEELDING && data.starts_with(&[0xff, 0xd8]));
        assert_eq!(b.tekens(), 0);

        // Een kleine afbeelding met doorzichtigheid blijft PNG en even groot.
        let klein = image::RgbaImage::from_fn(40, 30, |x, _| image::Rgba([0, 0, 0, if x < 20 { 0 } else { 255 }]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(klein).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let b = van_bytes("logo.png", png.get_ref(), 0).unwrap();
        assert!(matches!(&b.inhoud, Inhoud::Afbeelding { data_url, breedte: 40, hoogte: 30 } if data_url.starts_with("data:image/png;base64,")));

        assert!(van_bytes("kapot.jpg", b"geen jpeg", 0).unwrap_err().contains("niet te lezen"));
    }

    /// Een minimale PDF met één regel tekst, met kloppende xref-tabel.
    fn pdf(tekst: &str) -> Vec<u8> {
        let stroom = format!("BT /F1 12 Tf 72 712 Td ({tekst}) Tj ET");
        let objecten = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{stroom}\nendstream", stroom.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        ];
        let mut uit = b"%PDF-1.4\n".to_vec();
        let mut posities = Vec::new();
        for (i, o) in objecten.iter().enumerate() {
            posities.push(uit.len());
            uit.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = uit.len();
        uit.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objecten.len() + 1).as_bytes());
        for p in posities {
            uit.extend(format!("{p:010} 00000 n \n").as_bytes());
        }
        uit.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objecten.len() + 1).as_bytes());
        uit
    }

    #[test]
    fn tekst_uit_een_pdf() {
        let b = van_bytes("rapport.pdf", &pdf("Fundering op staal"), 0).unwrap();
        let Inhoud::Tekst { tekst } = &b.inhoud else { panic!() };
        assert!(tekst.contains("Fundering op staal"), "{tekst:?}");
        assert!(van_bytes("kapot.pdf", b"%PDF-1.4 rommel", 0).unwrap_err().contains("PDF"));
    }

    #[test]
    fn tekst_uit_docx_en_xlsx() {
        let docx = zip_met(&[(
            "word/document.xml",
            r#"<?xml version="1.0"?><w:document><w:body><w:p><w:r><w:t>Kop &amp; tekst</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve">Regel </w:t><w:tab/><w:t>twee</w:t></w:r></w:p></w:body></w:document>"#,
        )]);
        let b = van_bytes("brief.docx", &docx, 0).unwrap();
        assert_eq!(b.inhoud, Inhoud::Tekst { tekst: "Kop & tekst\nRegel \ttwee".into() });

        let xlsx = zip_met(&[
            ("xl/workbook.xml", r#"<workbook><sheets><sheet name="Begroting" sheetId="1" r:id="rId1"/></sheets></workbook>"#),
            ("xl/sharedStrings.xml", r#"<sst><si><t>Post</t></si><si><t>Bedrag</t></si><si><r><t>Fun</t></r><r><t>dering</t></r></si></sst>"#),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="C2"><v>1250.5</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Totaal</t></is></c></row></sheetData></worksheet>"#,
            ),
        ]);
        let b = van_bytes("begroting.xlsx", &xlsx, 0).unwrap();
        assert_eq!(b.inhoud, Inhoud::Tekst { tekst: "## Begroting\nPost\tBedrag\nFundering\t\t1250.5\nTotaal".into() });
        assert!(van_bytes("kapot.docx", b"PK rommel", 0).unwrap_err().contains("niet te lezen"));
    }

    #[test]
    fn xml_entiteiten_en_attributen() {
        assert_eq!(xml::ontsnap("a &lt;b&gt; &#233; &#x20AC; & los"), "a <b> é € & los");
        assert_eq!(xml::attribuut(r#"r="B2" t='s'"#, "t"), Some("s"));
        assert_eq!(kolom("A1"), 0);
        assert_eq!(kolom("AB12"), 27);
    }

    #[test]
    fn van_pad_meldt_mappen_en_ontbrekende_bestanden() {
        let map = std::env::temp_dir();
        assert!(van_pad(&map, 0).unwrap_err().contains("is een map"));
        assert!(van_pad(&map.join("bestaat-niet-assistent.txt"), 0).unwrap_err().contains("niet te lezen"));
        let pad = map.join(format!("assistent-test-{}.txt", std::process::id()));
        std::fs::write(&pad, "hallo").unwrap();
        let b = van_pad(&pad, 0).unwrap();
        std::fs::remove_file(&pad).unwrap();
        assert_eq!((b.naam.ends_with(".txt"), b.tekens()), (true, 5));
    }
}
