//! Teksten in een STEP-bestand decoderen: `''`, `\\`, `\S\`, `\X\hh`, `\X2\...\X0\` en `\X4\...\X0\`.
//!
//! Een onjuiste code laten we letterlijk staan; decoderen faalt nooit. Bytes buiten ASCII (sommige programma's
//! schrijven UTF-8 rechtstreeks, tegen de norm in) lezen we als UTF-8, en anders als ISO 8859-1.

/// De tekst zoals een mens hem leest. `ruw` is de inhoud tussen de aanhalingstekens.
pub(crate) fn decodeer(ruw: &[u8]) -> String {
    let mut uit = String::with_capacity(ruw.len());
    let mut i = 0;
    while i < ruw.len() {
        let b = ruw[i];
        if b == b'\'' {
            uit.push('\'');
            i += if ruw.get(i + 1) == Some(&b'\'') { 2 } else { 1 };
        } else if b == b'\\' {
            i += escape(ruw, i, &mut uit);
        } else if b < 0x80 {
            uit.push(b as char);
            i += 1;
        } else {
            i += utf8_of_latin1(&ruw[i..], &mut uit);
        }
    }
    uit
}

/// Verwerkt een code vanaf `ruw[i] == '\\'`; geeft het aantal gelezen bytes.
fn escape(ruw: &[u8], i: usize, uit: &mut String) -> usize {
    let rest = &ruw[i..];
    if rest.starts_with(b"\\\\") {
        uit.push('\\');
        return 2;
    }
    if rest.len() >= 4 && rest.starts_with(b"\\S\\") {
        // Teken uit de bovenste helft van de codetabel (ISO 8859-1 voor de standaardtabel A).
        uit.push(char::from(rest[3].wrapping_add(0x80)));
        return 4;
    }
    if rest.len() >= 4 && rest[1] == b'P' && rest[3] == b'\\' && rest[2].is_ascii_uppercase() {
        // Andere codetabel; alleen tabel A (ISO 8859-1) kennen we precies.
        return 4;
    }
    if rest.len() >= 5 && rest.starts_with(b"\\X\\") {
        if let Some(waarde) = hex(&rest[3..5]) {
            uit.push(char::from(waarde as u8));
            return 5;
        }
    }
    if rest.starts_with(b"\\X2\\") || rest.starts_with(b"\\X4\\") {
        let breedte = if rest[2] == b'2' { 4 } else { 8 };
        if let Some(eind) = rest[4..].windows(4).position(|w| w == b"\\X0\\") {
            let hexdeel = &rest[4..4 + eind];
            if hexdeel.len().is_multiple_of(breedte) {
                let eenheden: Option<Vec<u32>> = hexdeel.chunks(breedte).map(hex).collect();
                if let Some(eenheden) = eenheden {
                    let tekst = if breedte == 4 {
                        let utf16: Vec<u16> = eenheden.iter().map(|&e| e as u16).collect();
                        String::from_utf16_lossy(&utf16)
                    } else {
                        eenheden
                            .iter()
                            .map(|&e| char::from_u32(e).unwrap_or(char::REPLACEMENT_CHARACTER))
                            .collect()
                    };
                    uit.push_str(&tekst);
                    return 4 + eind + 4;
                }
            }
        }
    }
    uit.push('\\');
    1
}

fn hex(cijfers: &[u8]) -> Option<u32> {
    let mut waarde: u32 = 0;
    for &c in cijfers {
        waarde = waarde.checked_mul(16)? + char::from(c).to_digit(16)?;
    }
    Some(waarde)
}

fn utf8_of_latin1(rest: &[u8], uit: &mut String) -> usize {
    let lengte = match rest[0] {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 0,
    };
    if lengte > 0 && rest.len() >= lengte {
        if let Ok(s) = std::str::from_utf8(&rest[..lengte]) {
            uit.push_str(s);
            return lengte;
        }
    }
    uit.push(char::from(rest[0]));
    1
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

#[cfg(test)]
mod tests {
    use super::decodeer;

    #[test]
    fn codes() {
        assert_eq!(decodeer(b"King''s Day"), "King's Day");
        assert_eq!(decodeer(b"a\\\\b"), "a\\b");
        assert_eq!(decodeer(b"\\X2\\00E9\\X0\\t\\X2\\20140041\\X0\\"), "ét—A");
        assert_eq!(decodeer(b"caf\\X\\E9"), "café");
        assert_eq!(decodeer(b"\\S\\i"), "é");
        assert_eq!(decodeer(b"\\X4\\0001F600\\X0\\"), "😀");
        assert_eq!(decodeer("Zürich".as_bytes()), "Zürich");
        assert_eq!(decodeer(b"\xE9"), "é");
        // Onjuiste codes blijven staan.
        assert_eq!(decodeer(b"\\X2\\00E\\X0\\"), "\\X2\\00E\\X0\\");
        assert_eq!(decodeer(b"\\Q"), "\\Q");
        assert_eq!(decodeer(b"\\"), "\\");
    }
}
