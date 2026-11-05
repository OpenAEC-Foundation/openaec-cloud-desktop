//! Teksten in een STEP-bestand (ISO 10303-21 §6.4.3): `''`, `\\` en de codes `\S\`, `\P?\`, `\X\hh`,
//! `\X2\…\X0\` en `\X4\…\X0\`.
//!
//! Eén functie loopt een tekst van voor naar achter af. De parser gebruikt hem om het einde van een tekst te vinden
//! en de codes te controleren ([`einde`]); [`decodeer`] maakt er de tekst van zoals een mens hem leest. Een kapotte
//! code is een fout: `\X2\` zonder `\X0\` of met een verkeerd cijfer, een losse `\X0\`, een onbekende codetabel, of
//! een teken dat niet in de codetabel staat. Een `\` die geen code begint (bijv. in `C:\Users`), blijft staan.
//!
//! `\S\c` is het teken met code `c + 0x80` in de codetabel die `\P?\` koos: `\PA\` is ISO 8859-1 (de standaard) tot
//! en met `\PI\` (ISO 8859-9). De keuze geldt tot het einde van de tekst. `\X\hh` is altijd ISO 8859-1. Bytes buiten
//! ASCII (sommige programma's schrijven UTF-8 rechtstreeks) lezen we als UTF-8, en anders als ISO 8859-1.

/// Een kapotte tekst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TekstFout {
    /// De positie in de data waar het misgaat.
    pub positie: usize,
    pub melding: &'static str,
}

/// Zoekt het einde van een tekst en controleert de codes. `begin` wijst net na het openende `'`; het resultaat is
/// de positie van het afsluitende `'`.
pub(crate) fn einde(data: &[u8], begin: usize) -> Result<usize, TekstFout> {
    lees(data, begin, false, &mut Niets)
}

/// De tekst zoals een mens hem leest. `ruw` is de inhoud tussen de aanhalingstekens (die de parser al
/// controleerde). Is hij toch kapot, dan geven we de ruwe bytes.
pub(crate) fn decodeer(ruw: &[u8]) -> String {
    let mut uit = String::with_capacity(ruw.len());
    match lees(ruw, 0, true, &mut uit) {
        Ok(_) => uit,
        Err(_) => String::from_utf8_lossy(ruw).into_owned(),
    }
}

/// Waar de gelezen tekens heen gaan: nergens (alleen controleren) of in een `String`.
trait Uit {
    const SCHRIJFT: bool;
    fn teken(&mut self, c: char);
    fn stuk(&mut self, bytes: &[u8]);
}

struct Niets;

impl Uit for Niets {
    const SCHRIJFT: bool = false;
    fn teken(&mut self, _: char) {}
    fn stuk(&mut self, _: &[u8]) {}
}

impl Uit for String {
    const SCHRIJFT: bool = true;

    fn teken(&mut self, c: char) {
        self.push(c);
    }

    /// Bytes zonder `'` en `\`: UTF-8 waar dat klopt, anders ISO 8859-1.
    fn stuk(&mut self, bytes: &[u8]) {
        if let Ok(s) = std::str::from_utf8(bytes) {
            self.push_str(s);
            return;
        }
        let mut i = 0;
        while i < bytes.len() {
            let lengte = match bytes[i] {
                0x00..=0x7F => 1,
                0xC2..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF4 => 4,
                _ => 0,
            };
            match bytes.get(i..i + lengte.max(1)).and_then(|b| std::str::from_utf8(b).ok()) {
                Some(s) if lengte > 0 => {
                    self.push_str(s);
                    i += lengte;
                }
                _ => {
                    self.push(char::from(bytes[i]));
                    i += 1;
                }
            }
        }
    }
}

/// Loopt een tekst af vanaf `begin`. Zonder `tot_einde` stopt hij bij het afsluitende `'` en geeft hij de positie
/// daarvan; met `tot_einde` loopt hij tot het einde van de data (voor de inhoud van een al gecontroleerde tekst).
fn lees<U: Uit>(data: &[u8], begin: usize, tot_einde: bool, uit: &mut U) -> Result<usize, TekstFout> {
    let mut i = begin;
    let mut tabel = 0usize;
    loop {
        let rest = data.get(i..).unwrap_or_default();
        let stuk = rest.iter().position(|&b| b == b'\'' || b == b'\\').unwrap_or(rest.len());
        if U::SCHRIJFT {
            uit.stuk(&rest[..stuk]);
        }
        i += stuk;
        match data.get(i) {
            None if tot_einde => return Ok(i),
            None => return Err(TekstFout { positie: begin.saturating_sub(1), melding: "tekst zonder afsluitende '" }),
            Some(b'\'') if data.get(i + 1) == Some(&b'\'') => {
                uit.teken('\'');
                i += 2;
            }
            Some(b'\'') if tot_einde => {
                uit.teken('\'');
                i += 1;
            }
            Some(b'\'') => return Ok(i),
            Some(_) => i = code(data, i, &mut tabel, uit)?,
        }
    }
}

/// Een code vanaf `data[i] == '\\'`; geeft de positie erna.
fn code<U: Uit>(data: &[u8], i: usize, tabel: &mut usize, uit: &mut U) -> Result<usize, TekstFout> {
    let rest = &data[i..];
    let fout = |melding| Err(TekstFout { positie: i, melding });
    let na = |n: usize| rest.get(n).copied();
    match (na(1), na(2), na(3)) {
        (Some(b'\\'), _, _) => {
            uit.teken('\\');
            Ok(i + 2)
        }
        (Some(b'S'), Some(b'\\'), c) => {
            // Payload: elk zichtbaar ASCII-teken, ook ' en \.
            let Some(c) = c.filter(|c| (0x20..=0x7E).contains(c)) else {
                return fout("\\S\\ zonder geldig teken erna");
            };
            match char::from_u32(u32::from(HOGE_HELFT[*tabel][usize::from(c - 0x20)])).filter(|&t| t != '\0') {
                Some(t) => {
                    uit.teken(t);
                    Ok(i + 4)
                }
                None => fout("\\S\\ geeft een teken dat niet in de gekozen codetabel staat"),
            }
        }
        (Some(b'P'), Some(letter), Some(b'\\')) if letter.is_ascii_uppercase() => {
            if letter > b'I' {
                return fout("onbekende codetabel (\\P?\\ kent A tot en met I)");
            }
            *tabel = usize::from(letter - b'A');
            Ok(i + 4)
        }
        (Some(b'X'), Some(b'\\'), _) => match rest.get(3..5).and_then(hex) {
            Some(w) => {
                uit.teken(char::from(w as u8));
                Ok(i + 5)
            }
            None => fout("\\X\\ zonder twee hexadecimale cijfers"),
        },
        (Some(b'X'), Some(b'2'), Some(b'\\')) => uitgebreid(data, i, false, uit),
        (Some(b'X'), Some(b'4'), Some(b'\\')) => uitgebreid(data, i, true, uit),
        (Some(b'X'), Some(b'0'), Some(b'\\')) => fout("\\X0\\ zonder \\X2\\ of \\X4\\ ervoor"),
        _ => {
            uit.teken('\\');
            Ok(i + 1)
        }
    }
}

/// `\X2\` (UTF-16, vier cijfers per eenheid) of `\X4\` (acht cijfers per teken), tot en met `\X0\`.
fn uitgebreid<U: Uit>(data: &[u8], i: usize, vier: bool, uit: &mut U) -> Result<usize, TekstFout> {
    let breedte = if vier { 8 } else { 4 };
    let mut p = i + 4;
    let mut hoog: Option<u32> = None;
    let fout = |melding| Err(TekstFout { positie: i, melding });
    loop {
        if data.get(p..).is_some_and(|r| r.starts_with(b"\\X0\\")) {
            if hoog.is_some() {
                return fout("\\X2\\ met een halve UTF-16-tekenreeks (surrogaat)");
            }
            return Ok(p + 4);
        }
        let Some(w) = data.get(p..p + breedte).and_then(hex) else {
            return fout("\\X2\\ of \\X4\\ zonder \\X0\\, of met een verkeerd cijfer");
        };
        p += breedte;
        let teken = match (vier, hoog.take(), w) {
            (true, _, w) => char::from_u32(w),
            (false, None, 0xD800..=0xDBFF) => {
                hoog = Some(w);
                continue;
            }
            (false, Some(h), 0xDC00..=0xDFFF) => char::from_u32(0x10000 + ((h - 0xD800) << 10) + (w - 0xDC00)),
            (false, Some(_), _) => None,
            (false, None, w) => char::from_u32(w),
        };
        match teken {
            Some(t) => uit.teken(t),
            None => return fout("\\X2\\ of \\X4\\ met een ongeldig teken"),
        }
    }
}

fn hex(cijfers: &[u8]) -> Option<u32> {
    let mut waarde: u32 = 0;
    for &c in cijfers {
        waarde = waarde.checked_mul(16)? + char::from(c).to_digit(16)?;
    }
    Some(waarde)
}

/// Een korte tekst: hoogstens `max` tekens, met `…` als hij langer was.
pub(crate) fn kort(tekst: &str, max: usize) -> String {
    if tekst.chars().count() <= max {
        return tekst.to_string();
    }
    let mut uit: String = tekst.chars().take(max.saturating_sub(1)).collect();
    uit.push('…');
    uit
}

/// De bovenste helft (0xA0 tot en met 0xFF) van ISO 8859-1 tot en met 8859-9, als Unicode; 0 = geen teken.
/// Gemaakt met de codecs `iso8859_1` tot en met `iso8859_9` van Python.
#[rustfmt::skip]
const HOGE_HELFT: [[u16; 96]; 9] = [
    // ISO 8859-1
    [
        0x00A0, 0x00A1, 0x00A2, 0x00A3, 0x00A4, 0x00A5, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x00AA, 0x00AB,
        0x00AC, 0x00AD, 0x00AE, 0x00AF, 0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x00B4, 0x00B5, 0x00B6, 0x00B7,
        0x00B8, 0x00B9, 0x00BA, 0x00BB, 0x00BC, 0x00BD, 0x00BE, 0x00BF, 0x00C0, 0x00C1, 0x00C2, 0x00C3,
        0x00C4, 0x00C5, 0x00C6, 0x00C7, 0x00C8, 0x00C9, 0x00CA, 0x00CB, 0x00CC, 0x00CD, 0x00CE, 0x00CF,
        0x00D0, 0x00D1, 0x00D2, 0x00D3, 0x00D4, 0x00D5, 0x00D6, 0x00D7, 0x00D8, 0x00D9, 0x00DA, 0x00DB,
        0x00DC, 0x00DD, 0x00DE, 0x00DF, 0x00E0, 0x00E1, 0x00E2, 0x00E3, 0x00E4, 0x00E5, 0x00E6, 0x00E7,
        0x00E8, 0x00E9, 0x00EA, 0x00EB, 0x00EC, 0x00ED, 0x00EE, 0x00EF, 0x00F0, 0x00F1, 0x00F2, 0x00F3,
        0x00F4, 0x00F5, 0x00F6, 0x00F7, 0x00F8, 0x00F9, 0x00FA, 0x00FB, 0x00FC, 0x00FD, 0x00FE, 0x00FF,
    ],
    // ISO 8859-2
    [
        0x00A0, 0x0104, 0x02D8, 0x0141, 0x00A4, 0x013D, 0x015A, 0x00A7, 0x00A8, 0x0160, 0x015E, 0x0164,
        0x0179, 0x00AD, 0x017D, 0x017B, 0x00B0, 0x0105, 0x02DB, 0x0142, 0x00B4, 0x013E, 0x015B, 0x02C7,
        0x00B8, 0x0161, 0x015F, 0x0165, 0x017A, 0x02DD, 0x017E, 0x017C, 0x0154, 0x00C1, 0x00C2, 0x0102,
        0x00C4, 0x0139, 0x0106, 0x00C7, 0x010C, 0x00C9, 0x0118, 0x00CB, 0x011A, 0x00CD, 0x00CE, 0x010E,
        0x0110, 0x0143, 0x0147, 0x00D3, 0x00D4, 0x0150, 0x00D6, 0x00D7, 0x0158, 0x016E, 0x00DA, 0x0170,
        0x00DC, 0x00DD, 0x0162, 0x00DF, 0x0155, 0x00E1, 0x00E2, 0x0103, 0x00E4, 0x013A, 0x0107, 0x00E7,
        0x010D, 0x00E9, 0x0119, 0x00EB, 0x011B, 0x00ED, 0x00EE, 0x010F, 0x0111, 0x0144, 0x0148, 0x00F3,
        0x00F4, 0x0151, 0x00F6, 0x00F7, 0x0159, 0x016F, 0x00FA, 0x0171, 0x00FC, 0x00FD, 0x0163, 0x02D9,
    ],
    // ISO 8859-3
    [
        0x00A0, 0x0126, 0x02D8, 0x00A3, 0x00A4, 0, 0x0124, 0x00A7, 0x00A8, 0x0130, 0x015E, 0x011E,
        0x0134, 0x00AD, 0, 0x017B, 0x00B0, 0x0127, 0x00B2, 0x00B3, 0x00B4, 0x00B5, 0x0125, 0x00B7,
        0x00B8, 0x0131, 0x015F, 0x011F, 0x0135, 0x00BD, 0, 0x017C, 0x00C0, 0x00C1, 0x00C2, 0,
        0x00C4, 0x010A, 0x0108, 0x00C7, 0x00C8, 0x00C9, 0x00CA, 0x00CB, 0x00CC, 0x00CD, 0x00CE, 0x00CF,
        0, 0x00D1, 0x00D2, 0x00D3, 0x00D4, 0x0120, 0x00D6, 0x00D7, 0x011C, 0x00D9, 0x00DA, 0x00DB,
        0x00DC, 0x016C, 0x015C, 0x00DF, 0x00E0, 0x00E1, 0x00E2, 0, 0x00E4, 0x010B, 0x0109, 0x00E7,
        0x00E8, 0x00E9, 0x00EA, 0x00EB, 0x00EC, 0x00ED, 0x00EE, 0x00EF, 0, 0x00F1, 0x00F2, 0x00F3,
        0x00F4, 0x0121, 0x00F6, 0x00F7, 0x011D, 0x00F9, 0x00FA, 0x00FB, 0x00FC, 0x016D, 0x015D, 0x02D9,
    ],
    // ISO 8859-4
    [
        0x00A0, 0x0104, 0x0138, 0x0156, 0x00A4, 0x0128, 0x013B, 0x00A7, 0x00A8, 0x0160, 0x0112, 0x0122,
        0x0166, 0x00AD, 0x017D, 0x00AF, 0x00B0, 0x0105, 0x02DB, 0x0157, 0x00B4, 0x0129, 0x013C, 0x02C7,
        0x00B8, 0x0161, 0x0113, 0x0123, 0x0167, 0x014A, 0x017E, 0x014B, 0x0100, 0x00C1, 0x00C2, 0x00C3,
        0x00C4, 0x00C5, 0x00C6, 0x012E, 0x010C, 0x00C9, 0x0118, 0x00CB, 0x0116, 0x00CD, 0x00CE, 0x012A,
        0x0110, 0x0145, 0x014C, 0x0136, 0x00D4, 0x00D5, 0x00D6, 0x00D7, 0x00D8, 0x0172, 0x00DA, 0x00DB,
        0x00DC, 0x0168, 0x016A, 0x00DF, 0x0101, 0x00E1, 0x00E2, 0x00E3, 0x00E4, 0x00E5, 0x00E6, 0x012F,
        0x010D, 0x00E9, 0x0119, 0x00EB, 0x0117, 0x00ED, 0x00EE, 0x012B, 0x0111, 0x0146, 0x014D, 0x0137,
        0x00F4, 0x00F5, 0x00F6, 0x00F7, 0x00F8, 0x0173, 0x00FA, 0x00FB, 0x00FC, 0x0169, 0x016B, 0x02D9,
    ],
    // ISO 8859-5
    [
        0x00A0, 0x0401, 0x0402, 0x0403, 0x0404, 0x0405, 0x0406, 0x0407, 0x0408, 0x0409, 0x040A, 0x040B,
        0x040C, 0x00AD, 0x040E, 0x040F, 0x0410, 0x0411, 0x0412, 0x0413, 0x0414, 0x0415, 0x0416, 0x0417,
        0x0418, 0x0419, 0x041A, 0x041B, 0x041C, 0x041D, 0x041E, 0x041F, 0x0420, 0x0421, 0x0422, 0x0423,
        0x0424, 0x0425, 0x0426, 0x0427, 0x0428, 0x0429, 0x042A, 0x042B, 0x042C, 0x042D, 0x042E, 0x042F,
        0x0430, 0x0431, 0x0432, 0x0433, 0x0434, 0x0435, 0x0436, 0x0437, 0x0438, 0x0439, 0x043A, 0x043B,
        0x043C, 0x043D, 0x043E, 0x043F, 0x0440, 0x0441, 0x0442, 0x0443, 0x0444, 0x0445, 0x0446, 0x0447,
        0x0448, 0x0449, 0x044A, 0x044B, 0x044C, 0x044D, 0x044E, 0x044F, 0x2116, 0x0451, 0x0452, 0x0453,
        0x0454, 0x0455, 0x0456, 0x0457, 0x0458, 0x0459, 0x045A, 0x045B, 0x045C, 0x00A7, 0x045E, 0x045F,
    ],
    // ISO 8859-6
    [
        0x00A0, 0, 0, 0, 0x00A4, 0, 0, 0, 0, 0, 0, 0,
        0x060C, 0x00AD, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0x061B, 0, 0, 0, 0x061F, 0, 0x0621, 0x0622, 0x0623,
        0x0624, 0x0625, 0x0626, 0x0627, 0x0628, 0x0629, 0x062A, 0x062B, 0x062C, 0x062D, 0x062E, 0x062F,
        0x0630, 0x0631, 0x0632, 0x0633, 0x0634, 0x0635, 0x0636, 0x0637, 0x0638, 0x0639, 0x063A, 0,
        0, 0, 0, 0, 0x0640, 0x0641, 0x0642, 0x0643, 0x0644, 0x0645, 0x0646, 0x0647,
        0x0648, 0x0649, 0x064A, 0x064B, 0x064C, 0x064D, 0x064E, 0x064F, 0x0650, 0x0651, 0x0652, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    // ISO 8859-7
    [
        0x00A0, 0x2018, 0x2019, 0x00A3, 0x20AC, 0x20AF, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x037A, 0x00AB,
        0x00AC, 0x00AD, 0, 0x2015, 0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x0384, 0x0385, 0x0386, 0x00B7,
        0x0388, 0x0389, 0x038A, 0x00BB, 0x038C, 0x00BD, 0x038E, 0x038F, 0x0390, 0x0391, 0x0392, 0x0393,
        0x0394, 0x0395, 0x0396, 0x0397, 0x0398, 0x0399, 0x039A, 0x039B, 0x039C, 0x039D, 0x039E, 0x039F,
        0x03A0, 0x03A1, 0, 0x03A3, 0x03A4, 0x03A5, 0x03A6, 0x03A7, 0x03A8, 0x03A9, 0x03AA, 0x03AB,
        0x03AC, 0x03AD, 0x03AE, 0x03AF, 0x03B0, 0x03B1, 0x03B2, 0x03B3, 0x03B4, 0x03B5, 0x03B6, 0x03B7,
        0x03B8, 0x03B9, 0x03BA, 0x03BB, 0x03BC, 0x03BD, 0x03BE, 0x03BF, 0x03C0, 0x03C1, 0x03C2, 0x03C3,
        0x03C4, 0x03C5, 0x03C6, 0x03C7, 0x03C8, 0x03C9, 0x03CA, 0x03CB, 0x03CC, 0x03CD, 0x03CE, 0,
    ],
    // ISO 8859-8
    [
        0x00A0, 0, 0x00A2, 0x00A3, 0x00A4, 0x00A5, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x00D7, 0x00AB,
        0x00AC, 0x00AD, 0x00AE, 0x00AF, 0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x00B4, 0x00B5, 0x00B6, 0x00B7,
        0x00B8, 0x00B9, 0x00F7, 0x00BB, 0x00BC, 0x00BD, 0x00BE, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0x2017, 0x05D0, 0x05D1, 0x05D2, 0x05D3, 0x05D4, 0x05D5, 0x05D6, 0x05D7,
        0x05D8, 0x05D9, 0x05DA, 0x05DB, 0x05DC, 0x05DD, 0x05DE, 0x05DF, 0x05E0, 0x05E1, 0x05E2, 0x05E3,
        0x05E4, 0x05E5, 0x05E6, 0x05E7, 0x05E8, 0x05E9, 0x05EA, 0, 0, 0x200E, 0x200F, 0,
    ],
    // ISO 8859-9
    [
        0x00A0, 0x00A1, 0x00A2, 0x00A3, 0x00A4, 0x00A5, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x00AA, 0x00AB,
        0x00AC, 0x00AD, 0x00AE, 0x00AF, 0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x00B4, 0x00B5, 0x00B6, 0x00B7,
        0x00B8, 0x00B9, 0x00BA, 0x00BB, 0x00BC, 0x00BD, 0x00BE, 0x00BF, 0x00C0, 0x00C1, 0x00C2, 0x00C3,
        0x00C4, 0x00C5, 0x00C6, 0x00C7, 0x00C8, 0x00C9, 0x00CA, 0x00CB, 0x00CC, 0x00CD, 0x00CE, 0x00CF,
        0x011E, 0x00D1, 0x00D2, 0x00D3, 0x00D4, 0x00D5, 0x00D6, 0x00D7, 0x00D8, 0x00D9, 0x00DA, 0x00DB,
        0x00DC, 0x0130, 0x015E, 0x00DF, 0x00E0, 0x00E1, 0x00E2, 0x00E3, 0x00E4, 0x00E5, 0x00E6, 0x00E7,
        0x00E8, 0x00E9, 0x00EA, 0x00EB, 0x00EC, 0x00ED, 0x00EE, 0x00EF, 0x011F, 0x00F1, 0x00F2, 0x00F3,
        0x00F4, 0x00F5, 0x00F6, 0x00F7, 0x00F8, 0x00F9, 0x00FA, 0x00FB, 0x00FC, 0x0131, 0x015F, 0x00FF,
    ],
];

#[cfg(test)]
mod tests {
    use super::{decodeer, einde};

    fn fout(ruw: &[u8]) -> bool {
        let mut data = ruw.to_vec();
        data.push(b'\'');
        einde(&data, 0).is_err()
    }

    #[test]
    fn codes() {
        assert_eq!(decodeer(b"King''s Day"), "King's Day");
        assert_eq!(decodeer(b"a\\\\b"), "a\\b");
        assert_eq!(decodeer(b"\\X2\\00E9\\X0\\t\\X2\\20140041\\X0\\"), "ét—A");
        assert_eq!(decodeer(b"caf\\X\\E9"), "café");
        assert_eq!(decodeer(b"\\S\\i"), "é");
        assert_eq!(decodeer(b"\\X4\\0001F600\\X0\\"), "😀");
        assert_eq!(decodeer(b"\\X2\\D83DDE00\\X0\\"), "😀");
        assert_eq!(decodeer("Zürich".as_bytes()), "Zürich");
        assert_eq!(decodeer(b"\xE9"), "é");
        // Een \ die geen code begint, blijft staan.
        assert_eq!(decodeer(b"C:\\Users\\Q"), "C:\\Users\\Q");
        assert_eq!(decodeer(b"\\"), "\\");
    }

    /// Sols voorbeelden: de codetabel van `\P?\` telt mee.
    #[test]
    fn codetabellen() {
        assert_eq!(decodeer(b"\\PA\\\\S\\!"), "¡");
        assert_eq!(decodeer(b"\\PB\\\\S\\!"), "Ą");
        assert_ne!(decodeer(b"\\PA\\\\S\\!"), decodeer(b"\\PB\\\\S\\!"));
        assert_eq!(decodeer(b"\\PE\\\\S\\*\\S\\U\\S\\b"), "Њет");
        // Na de tekst geldt weer tabel A.
        assert_eq!(decodeer(b"\\S\\!"), "¡");
        // \S\ met ' of \ als teken.
        assert_eq!(decodeer(b"\\S\\'"), "§");
        assert_eq!(decodeer(b"\\S\\\\"), "Ü");
    }

    #[test]
    fn einde_van_een_tekst() {
        assert_eq!(einde(b"'abc'", 1), Ok(4));
        assert_eq!(einde(b"'a''b'", 1), Ok(5));
        // Het ' na \S\ is het teken, niet het einde.
        assert_eq!(einde(b"'\\S\\''", 1), Ok(5));
        assert_eq!(einde(b"'a\\\\'", 1), Ok(4));
        assert!(einde(b"'open", 1).is_err());
    }

    #[test]
    fn kapotte_codes() {
        assert!(fout(b"\\X2\\00E\\X0\\"));
        assert!(fout(b"\\X2\\00E9"));
        assert!(fout(b"\\X2\\D83D\\X0\\"));
        assert!(fout(b"\\X4\\00110000\\X0\\"));
        assert!(fout(b"\\X\\G1"));
        assert!(fout(b"\\X0\\"));
        assert!(fout(b"\\PJ\\"));
        assert!(fout(b"\\S\\"));
        // ISO 8859-3 heeft geen teken op 0xA5.
        assert!(fout(b"\\PC\\\\S\\%"));
        assert!(!fout(b"\\PC\\\\S\\$"));
    }

    /// Veel kapotte codes achter elkaar: één keer van voor naar achter, niet kwadratisch (Sols voorbeeld 7).
    #[test]
    fn veel_kapotte_codes_zijn_snel() {
        let ruw = b"\\X2\\a".repeat(200_000);
        let begin = std::time::Instant::now();
        assert!(fout(&ruw));
        let lang = b"x\\Q".repeat(200_000);
        assert!(!fout(&lang));
        assert_eq!(decodeer(&lang).len(), lang.len());
        assert!(begin.elapsed() < std::time::Duration::from_secs(2), "{:?}", begin.elapsed());
    }
}
