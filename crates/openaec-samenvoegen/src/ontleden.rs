//! Lezen van een STEP-bestand (ISO 10303-21, "Part 21"), zoals een IFC-bestand.
//!
//! Het bestand blijft als bytes in het geheugen. Per instantie (`#n=TYPE(...);`) bewaren we alleen het nummer, de
//! type-naam, het tekstbereik van de hele instantie en van elk attribuut op het hoogste niveau, en de nummers waar
//! hij naar verwijst. Waarden zelf lezen we later opnieuw uit het tekstbereik ([`Tokens`]); zo maken we geen kopie
//! per token.
//!
//! Ongeldige invoer geeft een [`Fout::Ongeldig`] met het regelnummer. De parser gebruikt geen recursie op de
//! invoer (een diep geneste lijst kan dus geen stack overflow geven) en geen indexering zonder controle.

use std::collections::HashMap;

use crate::Fout;

/// Een tekstbereik `[begin, eind)` in bytes. Bestanden zijn kleiner dan 4 GiB (zie [`lees`]).
pub(crate) type Bereik = (u32, u32);

/// Een gelezen STEP-bestand.
pub(crate) struct Bestand<'a> {
    pub data: &'a [u8],
    /// Eerste naam uit FILE_SCHEMA, in hoofdletters (bijv. `IFC4X3_ADD2`); leeg als die ontbreekt.
    pub schema: String,
    /// Type-namen in hoofdletters; [`Instantie::type_id`] wijst hierin. Een complexe instantie heeft als naam de
    /// delen met `+` ertussen.
    pub typen: Vec<String>,
    pub instanties: Vec<Instantie>,
    /// Tekstbereik per attribuut op het hoogste niveau (voor een complexe instantie: per deel `NAAM(...)`).
    pub attributen: Vec<Bereik>,
    /// Indexen (in `instanties`) van alle verwijzingen, per instantie aaneengesloten.
    pub verwijzingen: Vec<u32>,
    /// Instantienummer -> index in `instanties`.
    pub index: Index,
    /// Begin van de `ENDSEC;` van de laatste DATA-sectie: hier komen nieuwe instanties.
    pub data_einde: usize,
    /// Het hoogste instantienummer.
    pub hoogste: u64,
    /// De regels van de HEADER (FILE_DESCRIPTION, FILE_NAME, FILE_SCHEMA...).
    pub header: Vec<HeaderRegel>,
}

/// Een regel uit de HEADER, bijv. `FILE_NAME('a.ifc','2026-10-26T11:31:17',('Anne'),...);`.
pub(crate) struct HeaderRegel {
    /// In hoofdletters.
    pub naam: String,
    /// Elke parameter op het hoogste niveau.
    pub parameters: Vec<Bereik>,
}

/// Instantienummer -> index. Liggen de nummers dicht bij elkaar (zoals meestal), dan een gewone tabel; anders een
/// HashMap.
pub(crate) enum Index {
    Tabel(Vec<u32>),
    Kaart(HashMap<u64, u32>),
}

impl Default for Index {
    fn default() -> Self {
        Index::Kaart(HashMap::new())
    }
}

const GEEN: u32 = u32::MAX;

/// Zo diep mogen lijsten en getypte waarden in één attribuut in elkaar zitten. Echte IFC komt niet verder dan een
/// paar niveaus. De canonisatie en de weergave werken recursief; dieper is daarom een fout en geen stack overflow.
pub(crate) const MAX_DIEPTE: usize = 256;

impl Index {
    fn nieuw(hoogste: u64, aantal: usize) -> Self {
        if hoogste <= (aantal as u64).saturating_mul(4).saturating_add(1024) {
            Index::Tabel(vec![GEEN; hoogste as usize + 1])
        } else {
            Index::Kaart(HashMap::with_capacity(aantal))
        }
    }

    /// Geeft false als het nummer al bestond.
    fn zet(&mut self, nummer: u64, index: u32) -> bool {
        match self {
            Index::Tabel(t) => {
                let plek = &mut t[nummer as usize];
                let nieuw = *plek == GEEN;
                *plek = index;
                nieuw
            }
            Index::Kaart(k) => k.insert(nummer, index).is_none(),
        }
    }

    pub fn get(&self, nummer: &u64) -> Option<&u32> {
        match self {
            Index::Tabel(t) => usize::try_from(*nummer).ok().and_then(|n| t.get(n)).filter(|&&i| i != GEEN),
            Index::Kaart(k) => k.get(nummer),
        }
    }
}

pub(crate) struct Instantie {
    pub id: u64,
    pub type_id: u32,
    pub complex: bool,
    /// Van `#` tot en met `;`.
    pub bereik: Bereik,
    /// De type-naam zoals hij in de tekst staat (bij een complexe instantie: alles tussen de buitenste haakjes).
    pub type_bereik: Bereik,
    attrs: (u32, u32),
    refs: (u32, u32),
}

impl Bestand<'_> {
    pub fn attributen(&self, i: usize) -> &[Bereik] {
        let (begin, aantal) = self.instanties[i].attrs;
        &self.attributen[begin as usize..(begin + aantal) as usize]
    }

    pub fn verwijzingen(&self, i: usize) -> &[u32] {
        let (begin, aantal) = self.instanties[i].refs;
        &self.verwijzingen[begin as usize..(begin + aantal) as usize]
    }

    pub fn type_naam(&self, i: usize) -> &str {
        &self.typen[self.instanties[i].type_id as usize]
    }

    pub fn tekst(&self, bereik: Bereik) -> &[u8] {
        &self.data[bereik.0 as usize..bereik.1 as usize]
    }

    pub fn regel(&self, positie: usize) -> usize {
        regel(self.data, positie)
    }
}

/// Regelnummer (vanaf 1) van een positie.
pub(crate) fn regel(data: &[u8], positie: usize) -> usize {
    let tot = positie.min(data.len());
    data[..tot].iter().filter(|&&b| b == b'\n').count() + 1
}

fn ongeldig(data: &[u8], positie: usize, melding: impl Into<String>) -> Fout {
    Fout::Ongeldig { regel: regel(data, positie), melding: melding.into() }
}

/// Leest een STEP-bestand.
pub(crate) fn lees(data: &[u8]) -> Result<Bestand<'_>, Fout> {
    if data.len() >= u32::MAX as usize {
        return Err(Fout::TeGroot);
    }
    let mut lezer = Lezer { data, pos: 0 };
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        lezer.pos = 3;
    }
    let mut bestand = Bestand {
        data,
        schema: String::new(),
        typen: Vec::new(),
        instanties: Vec::new(),
        attributen: Vec::new(),
        verwijzingen: Vec::new(),
        index: Index::default(),
        data_einde: 0,
        hoogste: 0,
        header: Vec::new(),
    };
    let mut typen: HashMap<Vec<u8>, u32> = HashMap::new();
    let mut ruwe_refs: Vec<u64> = Vec::new();

    lezer.spatie()?;
    let begin = lezer.pos;
    if lezer.trefwoord_met_streepjes()? != b"ISO-10303-21" {
        return Err(ongeldig(data, begin, "het bestand begint niet met ISO-10303-21; (geen STEP-bestand)"));
    }
    lezer.spatie()?;
    lezer.verwacht(b';')?;
    let mut header_gezien = false;
    let mut data_gezien = false;
    loop {
        lezer.spatie()?;
        if lezer.pos >= data.len() {
            return Err(ongeldig(data, lezer.pos, "het bestand eindigt zonder END-ISO-10303-21;"));
        }
        let begin = lezer.pos;
        let woord = lezer.trefwoord_met_streepjes()?;
        match woord {
            b"HEADER" => {
                if header_gezien {
                    return Err(ongeldig(data, begin, "tweede HEADER-sectie"));
                }
                header_gezien = true;
                lezer.spatie()?;
                lezer.verwacht(b';')?;
                lezer.header(&mut bestand.schema, &mut bestand.header)?;
            }
            b"DATA" => {
                if !header_gezien {
                    return Err(ongeldig(data, begin, "DATA-sectie vóór de HEADER"));
                }
                data_gezien = true;
                lezer.spatie()?;
                if lezer.kijk() == Some(b'(') {
                    let mut genegeerd = Vec::new();
                    lezer.lijst(&mut genegeerd)?;
                    lezer.spatie()?;
                }
                lezer.verwacht(b';')?;
                bestand.data_einde = lezer.data_sectie(&mut bestand, &mut typen, &mut ruwe_refs)?;
            }
            b"END-ISO-10303-21" => {
                lezer.spatie()?;
                lezer.verwacht(b';')?;
                break;
            }
            b"ANCHOR" | b"REFERENCE" | b"SIGNATURE" => {
                return Err(ongeldig(
                    data,
                    begin,
                    format!("de sectie {} wordt niet ondersteund", String::from_utf8_lossy(woord)),
                ));
            }
            _ => {
                return Err(ongeldig(
                    data,
                    begin,
                    format!("onverwacht woord '{}' (verwacht HEADER, DATA of END-ISO-10303-21)", toon(woord)),
                ));
            }
        }
    }
    if !data_gezien {
        return Err(ongeldig(data, lezer.pos, "het bestand heeft geen DATA-sectie"));
    }

    // Instantienummers koppelen aan indexen.
    bestand.hoogste = bestand.instanties.iter().map(|i| i.id).max().unwrap_or(0);
    bestand.index = Index::nieuw(bestand.hoogste, bestand.instanties.len());
    for (i, inst) in bestand.instanties.iter().enumerate() {
        if !bestand.index.zet(inst.id, i as u32) {
            return Err(ongeldig(data, inst.bereik.0 as usize, format!("instantie #{} komt twee keer voor", inst.id)));
        }
    }
    bestand.verwijzingen.reserve_exact(ruwe_refs.len());
    for inst in &bestand.instanties {
        let (begin, aantal) = inst.refs;
        for &doel in &ruwe_refs[begin as usize..(begin + aantal) as usize] {
            match bestand.index.get(&doel) {
                Some(&j) => bestand.verwijzingen.push(j),
                None => {
                    return Err(ongeldig(
                        data,
                        inst.bereik.0 as usize,
                        format!("#{} verwijst naar #{doel}, maar die bestaat niet", inst.id),
                    ))
                }
            }
        }
    }
    let mut namen = vec![String::new(); typen.len()];
    for (naam, id) in typen {
        namen[id as usize] = String::from_utf8_lossy(&naam).into_owned();
    }
    bestand.typen = namen;
    Ok(bestand)
}

fn toon(woord: &[u8]) -> String {
    let tekst = String::from_utf8_lossy(woord);
    tekst.chars().take(40).collect()
}

struct Lezer<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Lezer<'a> {
    fn kijk(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    /// Slaat witruimte en commentaar `/* ... */` over.
    fn spatie(&mut self) -> Result<(), Fout> {
        loop {
            match self.kijk() {
                Some(b) if b <= b' ' => self.pos += 1,
                Some(b'/') if self.data.get(self.pos + 1) == Some(&b'*') => {
                    let begin = self.pos;
                    match zoek(self.data, self.pos + 2, b"*/") {
                        Some(eind) => self.pos = eind + 2,
                        None => return Err(ongeldig(self.data, begin, "commentaar /* zonder */")),
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn verwacht(&mut self, teken: u8) -> Result<(), Fout> {
        if self.kijk() == Some(teken) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.onverwacht(&format!("'{}'", teken as char)))
        }
    }

    fn onverwacht(&self, verwacht: &str) -> Fout {
        match self.kijk() {
            None => ongeldig(self.data, self.pos, format!("het bestand houdt op; verwacht {verwacht}")),
            Some(b) => {
                let teken = if b.is_ascii_graphic() { format!("'{}'", b as char) } else { format!("byte 0x{b:02X}") };
                ongeldig(self.data, self.pos, format!("onverwacht teken {teken}; verwacht {verwacht}"))
            }
        }
    }

    /// Een trefwoord: een letter, `_` of `!`, dan letters, cijfers en `_`.
    fn trefwoord(&mut self) -> Result<&'a [u8], Fout> {
        let begin = self.pos;
        match self.kijk() {
            Some(b) if b.is_ascii_alphabetic() || b == b'_' || b == b'!' => self.pos += 1,
            _ => return Err(self.onverwacht("een type-naam")),
        }
        while matches!(self.kijk(), Some(b) if b.is_ascii_alphanumeric() || b == b'_') {
            self.pos += 1;
        }
        Ok(&self.data[begin..self.pos])
    }

    /// Als [`trefwoord`](Self::trefwoord), maar ook met `-` (voor `ISO-10303-21` en `END-ISO-10303-21`).
    fn trefwoord_met_streepjes(&mut self) -> Result<&'a [u8], Fout> {
        let begin = self.pos;
        while matches!(self.kijk(), Some(b) if b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            self.pos += 1;
        }
        if self.pos == begin {
            return Err(self.onverwacht("een trefwoord"));
        }
        Ok(&self.data[begin..self.pos])
    }

    /// De HEADER-sectie tot en met `ENDSEC;`. Haalt de eerste naam uit FILE_SCHEMA.
    fn header(&mut self, schema: &mut String, regels: &mut Vec<HeaderRegel>) -> Result<(), Fout> {
        loop {
            self.spatie()?;
            let begin = self.pos;
            let naam = self.trefwoord()?;
            self.spatie()?;
            if naam.eq_ignore_ascii_case(b"ENDSEC") {
                return self.verwacht(b';');
            }
            let lijst_begin = self.pos;
            let mut refs = Vec::new();
            let mut parameters = Vec::new();
            self.parameters(&mut parameters, &mut refs)?;
            if !refs.is_empty() {
                return Err(ongeldig(self.data, begin, "verwijzing (#n) in de HEADER"));
            }
            if naam.eq_ignore_ascii_case(b"FILE_SCHEMA") {
                *schema = eerste_tekst(&self.data[lijst_begin..self.pos])
                    .map(|t| crate::tekst::decodeer(t).trim().to_ascii_uppercase())
                    .unwrap_or_default();
            }
            self.spatie()?;
            self.verwacht(b';')?;
            regels.push(HeaderRegel {
                naam: String::from_utf8_lossy(naam).to_ascii_uppercase(),
                parameters: parameters.into_iter().map(|(a, b)| (a as u32, b as u32)).collect(),
            });
        }
    }

    /// Een parameterlijst `( waarde, waarde, ... )`; legt het bereik van elke waarde op het hoogste niveau vast.
    fn parameters(&mut self, bereiken: &mut Vec<(usize, usize)>, refs: &mut Vec<u64>) -> Result<(), Fout> {
        self.verwacht(b'(')?;
        let begin = bereiken.len();
        loop {
            self.spatie()?;
            if self.kijk() == Some(b')') && bereiken.len() == begin {
                self.pos += 1;
                return Ok(());
            }
            let a = self.pos;
            self.waarde(refs)?;
            bereiken.push((a, self.pos));
            self.spatie()?;
            match self.kijk() {
                Some(b',') => self.pos += 1,
                Some(b')') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err(self.onverwacht("',' of ')'")),
            }
        }
    }

    /// De DATA-sectie tot en met `ENDSEC;`. Geeft het begin van `ENDSEC` terug.
    fn data_sectie(
        &mut self,
        bestand: &mut Bestand<'a>,
        typen: &mut HashMap<Vec<u8>, u32>,
        refs: &mut Vec<u64>,
    ) -> Result<usize, Fout> {
        loop {
            self.spatie()?;
            let begin = self.pos;
            match self.kijk() {
                Some(b'#') => {}
                Some(b) if b.is_ascii_alphabetic() => {
                    let woord = self.trefwoord()?;
                    if woord.eq_ignore_ascii_case(b"ENDSEC") {
                        self.spatie()?;
                        self.verwacht(b';')?;
                        return Ok(begin);
                    }
                    return Err(ongeldig(
                        self.data,
                        begin,
                        format!("onverwacht woord '{}'; verwacht #n= of ENDSEC;", toon(woord)),
                    ));
                }
                _ => return Err(self.onverwacht("#n= of ENDSEC;")),
            }
            self.pos += 1;
            let id = self.getal_na_hekje(begin)?;
            self.spatie()?;
            self.verwacht(b'=')?;
            self.spatie()?;
            let attr_begin = bestand.attributen.len() as u32;
            let ref_begin = refs.len() as u32;
            let (complex, type_bereik, naam) = if self.kijk() == Some(b'(') {
                // Complexe instantie: (A(...)B(...)).
                let romp_begin = self.pos;
                self.pos += 1;
                let mut namen: Vec<u8> = Vec::new();
                loop {
                    self.spatie()?;
                    if self.kijk() == Some(b')') {
                        self.pos += 1;
                        break;
                    }
                    let deel_begin = self.pos;
                    let deel = self.trefwoord()?;
                    if !namen.is_empty() {
                        namen.push(b'+');
                    }
                    namen.extend(deel.iter().map(u8::to_ascii_uppercase));
                    self.spatie()?;
                    self.lijst(refs)?;
                    bestand.attributen.push((deel_begin as u32, self.pos as u32));
                }
                if namen.is_empty() {
                    return Err(ongeldig(self.data, romp_begin, format!("#{id}: lege complexe instantie")));
                }
                (true, (romp_begin as u32 + 1, self.pos as u32 - 1), namen)
            } else {
                let naam_begin = self.pos;
                let naam = self.trefwoord()?;
                let naam_eind = self.pos;
                self.spatie()?;
                let mut bereiken = Vec::new();
                self.parameters(&mut bereiken, refs)?;
                bestand.attributen.extend(bereiken.into_iter().map(|(a, b)| (a as u32, b as u32)));
                (false, (naam_begin as u32, naam_eind as u32), naam.to_ascii_uppercase())
            };
            self.spatie()?;
            self.verwacht(b';')?;
            let volgend = typen.len() as u32;
            let type_id = *typen.entry(naam).or_insert(volgend);
            bestand.instanties.push(Instantie {
                id,
                type_id,
                complex,
                bereik: (begin as u32, self.pos as u32),
                type_bereik,
                attrs: (attr_begin, bestand.attributen.len() as u32 - attr_begin),
                refs: (ref_begin, refs.len() as u32 - ref_begin),
            });
        }
    }

    fn getal_na_hekje(&mut self, begin: usize) -> Result<u64, Fout> {
        let start = self.pos;
        let mut waarde: u64 = 0;
        while let Some(b) = self.kijk().filter(u8::is_ascii_digit) {
            waarde = waarde
                .checked_mul(10)
                .and_then(|w| w.checked_add(u64::from(b - b'0')))
                .ok_or_else(|| ongeldig(self.data, begin, "instantienummer is te groot"))?;
            self.pos += 1;
        }
        if self.pos == start {
            return Err(self.onverwacht("cijfers na #"));
        }
        Ok(waarde)
    }

    /// Een lijst `( waarde, waarde, ... )`.
    fn lijst(&mut self, refs: &mut Vec<u64>) -> Result<(), Fout> {
        if self.kijk() != Some(b'(') {
            return Err(self.onverwacht("'('"));
        }
        self.waarde(refs)
    }

    /// Eén waarde, met geneste lijsten. Zonder recursie: de diepte telt in `diepte`, hoogstens [`MAX_DIEPTE`].
    fn waarde(&mut self, refs: &mut Vec<u64>) -> Result<(), Fout> {
        let mut diepte: usize = 0;
        // Na een '(' mag direct een ')' komen (lege lijst).
        loop {
            self.spatie()?;
            let begin = self.pos;
            match self.kijk() {
                Some(b'(') => {
                    self.pos += 1;
                    diepte += 1;
                    if diepte > MAX_DIEPTE {
                        return Err(ongeldig(self.data, begin, format!("waarde dieper dan {MAX_DIEPTE} niveaus")));
                    }
                    self.spatie()?;
                    if self.kijk() == Some(b')') {
                        self.pos += 1;
                        diepte -= 1;
                    } else {
                        continue;
                    }
                }
                Some(b'$') | Some(b'*') => self.pos += 1,
                Some(b'#') => {
                    self.pos += 1;
                    refs.push(self.getal_na_hekje(begin)?);
                }
                Some(b'\'') => self.tekst()?,
                Some(b'"') => {
                    self.pos += 1;
                    while matches!(self.kijk(), Some(b) if b.is_ascii_hexdigit()) {
                        self.pos += 1;
                    }
                    self.verwacht(b'"')?;
                }
                Some(b'.') => {
                    self.pos += 1;
                    let start = self.pos;
                    while matches!(self.kijk(), Some(b) if b.is_ascii_alphanumeric() || b == b'_') {
                        self.pos += 1;
                    }
                    if self.pos == start {
                        return Err(ongeldig(self.data, begin, "lege opsommingswaarde (..)"));
                    }
                    self.verwacht(b'.')?;
                }
                Some(b) if b.is_ascii_digit() || b == b'-' || b == b'+' => self.getal()?,
                Some(b) if b.is_ascii_alphabetic() || b == b'_' || b == b'!' => {
                    // Getypte waarde: NAAM(waarde, ...).
                    self.trefwoord()?;
                    self.spatie()?;
                    if self.kijk() != Some(b'(') {
                        return Err(self.onverwacht("'(' na een type-naam"));
                    }
                    continue;
                }
                _ => return Err(self.onverwacht("een waarde")),
            }
            // Na een waarde: sluit lijsten af of ga door met de volgende.
            loop {
                if diepte == 0 {
                    return Ok(());
                }
                self.spatie()?;
                match self.kijk() {
                    Some(b',') => {
                        self.pos += 1;
                        break;
                    }
                    Some(b')') => {
                        self.pos += 1;
                        diepte -= 1;
                    }
                    _ => return Err(self.onverwacht("',' of ')'")),
                }
            }
        }
    }

    /// Een tekst `'...'`; de codes erin moeten kloppen ([`crate::tekst`]).
    fn tekst(&mut self) -> Result<(), Fout> {
        match crate::tekst::einde(self.data, self.pos + 1) {
            Ok(eind) => {
                self.pos = eind + 1;
                Ok(())
            }
            Err(f) => Err(ongeldig(self.data, f.positie, f.melding)),
        }
    }

    fn getal(&mut self) -> Result<(), Fout> {
        let begin = self.pos;
        if matches!(self.kijk(), Some(b'-') | Some(b'+')) {
            self.pos += 1;
        }
        let mut cijfers = 0;
        while matches!(self.kijk(), Some(b) if b.is_ascii_digit()) {
            self.pos += 1;
            cijfers += 1;
        }
        if self.kijk() == Some(b'.') {
            self.pos += 1;
            while matches!(self.kijk(), Some(b) if b.is_ascii_digit()) {
                self.pos += 1;
                cijfers += 1;
            }
        }
        if cijfers == 0 {
            return Err(ongeldig(self.data, begin, "ongeldig getal"));
        }
        if matches!(self.kijk(), Some(b'E') | Some(b'e')) {
            self.pos += 1;
            if matches!(self.kijk(), Some(b'-') | Some(b'+')) {
                self.pos += 1;
            }
            let start = self.pos;
            let mut exponent: u64 = 0;
            while let Some(b) = self.kijk().filter(u8::is_ascii_digit) {
                exponent = (exponent * 10 + u64::from(b - b'0')).min(u64::from(u32::MAX));
                self.pos += 1;
            }
            if self.pos == start {
                return Err(ongeldig(self.data, begin, "ongeldig getal (exponent zonder cijfers)"));
            }
            // Zo vergelijken we getallen precies, zonder afronden (zie `model::getal`).
            if exponent > i32::MAX as u64 {
                return Err(ongeldig(self.data, begin, "getal met een te grote exponent"));
            }
        }
        Ok(())
    }
}

fn zoek(data: &[u8], vanaf: usize, naald: &[u8]) -> Option<usize> {
    if vanaf >= data.len() {
        return None;
    }
    data[vanaf..].windows(naald.len()).position(|w| w == naald).map(|p| p + vanaf)
}

fn zoek_byte(data: &[u8], vanaf: usize, byte: u8) -> Option<usize> {
    data.get(vanaf..)?.iter().position(|&b| b == byte).map(|p| p + vanaf)
}

/// De ruwe inhoud van de eerste tekst (`'...'`) in een stuk tekst.
fn eerste_tekst(stuk: &[u8]) -> Option<&[u8]> {
    Tokens::nieuw(stuk).find_map(|(_, t)| match t {
        Token::Tekst(t) => Some(t),
        _ => None,
    })
}

/// Een token in een al gecontroleerd stuk tekst.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Token<'a> {
    Open,
    Sluit,
    /// `$`
    Leeg,
    /// `*`
    Afgeleid,
    Verwijzing(u64),
    Getal(&'a [u8]),
    /// De ruwe inhoud tussen de aanhalingstekens, nog met `''` en `\X2\`-codes.
    Tekst(&'a [u8]),
    /// De naam tussen de punten.
    Enum(&'a [u8]),
    /// De hexadecimale inhoud tussen de dubbele aanhalingstekens.
    Binair(&'a [u8]),
    /// Een type-naam vóór een lijst (getypte waarde of deel van een complexe instantie).
    Trefwoord(&'a [u8]),
}

/// Loopt de tokens van een stuk tekst af dat de parser al heeft goedgekeurd. Komma's, witruimte en commentaar
/// slaat hij over. Bij tekst die toch niet klopt, stopt hij (geen panic).
pub(crate) struct Tokens<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Tokens<'a> {
    pub fn nieuw(data: &'a [u8]) -> Self {
        Tokens { data, pos: 0 }
    }

    /// De positie na het laatste token.
    pub fn positie(&self) -> usize {
        self.pos
    }
}

/// De elementen van een lijst op het hoogste niveau, als bereiken in `tekst`; None als de waarde geen lijst is.
pub(crate) fn elementen(tekst: &[u8]) -> Option<Vec<(usize, usize)>> {
    let mut tokens = Tokens::nieuw(tekst);
    match tokens.next() {
        Some((_, Token::Open)) => {}
        _ => return None,
    }
    let mut diepte = 1usize;
    let mut uit = Vec::new();
    let mut begin: Option<usize> = None;
    while let Some((start, token)) = tokens.next() {
        if diepte == 1 && token != Token::Sluit && begin.is_none() {
            begin = Some(start);
        }
        match token {
            Token::Open => diepte += 1,
            Token::Sluit => diepte -= 1,
            _ => {}
        }
        if diepte == 0 {
            return Some(uit);
        }
        if diepte == 1 && !matches!(token, Token::Trefwoord(_)) {
            if let Some(b) = begin.take() {
                uit.push((b, tokens.positie()));
            }
        }
    }
    None
}

impl<'a> Iterator for Tokens<'a> {
    /// Het begin (in het stuk tekst) en het token.
    type Item = (usize, Token<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let data = self.data;
        loop {
            let b = *data.get(self.pos)?;
            if b <= b' ' || b == b',' {
                self.pos += 1;
            } else if b == b'/' && data.get(self.pos + 1) == Some(&b'*') {
                self.pos = zoek(data, self.pos + 2, b"*/").map_or(data.len(), |p| p + 2);
            } else {
                break;
            }
        }
        let begin = self.pos;
        let b = data[begin];
        let token = match b {
            b'(' => {
                self.pos += 1;
                Token::Open
            }
            b')' => {
                self.pos += 1;
                Token::Sluit
            }
            b'$' => {
                self.pos += 1;
                Token::Leeg
            }
            b'*' => {
                self.pos += 1;
                Token::Afgeleid
            }
            b'#' => {
                self.pos += 1;
                let start = self.pos;
                let mut waarde: u64 = 0;
                while let Some(c) = data.get(self.pos).filter(|c| c.is_ascii_digit()) {
                    waarde = waarde.saturating_mul(10).saturating_add(u64::from(c - b'0'));
                    self.pos += 1;
                }
                if self.pos == start {
                    self.pos = data.len();
                    return None;
                }
                Token::Verwijzing(waarde)
            }
            b'\'' => match crate::tekst::einde(data, begin + 1) {
                Ok(q) => {
                    self.pos = q + 1;
                    Token::Tekst(&data[begin + 1..q])
                }
                Err(_) => {
                    self.pos = data.len();
                    return None;
                }
            },
            b'"' => {
                let eind = zoek_byte(data, begin + 1, b'"').unwrap_or(data.len());
                self.pos = (eind + 1).min(data.len());
                Token::Binair(&data[(begin + 1).min(eind)..eind])
            }
            b'.' => {
                let eind = zoek_byte(data, begin + 1, b'.').unwrap_or(data.len());
                self.pos = (eind + 1).min(data.len());
                Token::Enum(&data[(begin + 1).min(eind)..eind])
            }
            c if c.is_ascii_digit() || c == b'-' || c == b'+' => {
                let mut p = begin + 1;
                while let Some(&c) = data.get(p) {
                    let vorige = data[p - 1];
                    let deel = c.is_ascii_digit()
                        || c == b'.'
                        || c == b'E'
                        || c == b'e'
                        || ((c == b'-' || c == b'+') && (vorige == b'E' || vorige == b'e'));
                    if !deel {
                        break;
                    }
                    p += 1;
                }
                self.pos = p;
                Token::Getal(&data[begin..p])
            }
            c if c.is_ascii_alphabetic() || c == b'_' || c == b'!' => {
                let mut p = begin + 1;
                while matches!(data.get(p), Some(c) if c.is_ascii_alphanumeric() || *c == b'_') {
                    p += 1;
                }
                self.pos = p;
                Token::Trefwoord(&data[begin..p])
            }
            _ => {
                self.pos = data.len();
                return None;
            }
        };
        Some((begin, token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ifc(data: &str) -> String {
        format!("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('','',(''),(''),'','','');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n")
    }

    #[test]
    fn leest_instanties_en_attributen() {
        let tekst = ifc(
            "#1=IFCCARTESIANPOINT((0.,1.E-3,-2.5));\n#2 = IFCAXIS2PLACEMENT3D( #1 ,$,$ ) ;\n#3=IFCLABEL('a''b;c)');",
        );
        let b = lees(tekst.as_bytes()).unwrap();
        assert_eq!(b.schema, "IFC4");
        assert_eq!(b.header.len(), 3);
        assert_eq!(b.header[1].naam, "FILE_NAME");
        assert_eq!(b.header[1].parameters.len(), 7);
        assert_eq!(b.instanties.len(), 3);
        assert_eq!(b.type_naam(1), "IFCAXIS2PLACEMENT3D");
        let attrs: Vec<&[u8]> = b.attributen(1).iter().map(|&r| b.tekst(r)).collect();
        assert_eq!(attrs, vec![&b"#1"[..], b"$", b"$"]);
        assert_eq!(b.verwijzingen(1), &[0]);
        assert_eq!(b.tekst(b.attributen(2)[0]), b"'a''b;c)'");
        assert_eq!(b.hoogste, 3);
    }

    #[test]
    fn complexe_instantie_en_commentaar() {
        let tekst = ifc(
            "/* c */#5=( IFCNAMEDUNIT(*,.LENGTHUNIT.) IFCSIUNIT() /* x */ );\n#6=IFCX(#5,'/* geen commentaar */');",
        );
        let b = lees(tekst.as_bytes()).unwrap();
        assert!(b.instanties[0].complex);
        assert_eq!(b.type_naam(0), "IFCNAMEDUNIT+IFCSIUNIT");
        assert_eq!(b.attributen(0).len(), 2);
        assert_eq!(b.verwijzingen(1), &[0]);
    }

    #[test]
    fn fouten_met_regelnummer() {
        let gevallen = [
            ("#1=IFCX(1,2;", 8),
            ("#1=IFCX('open);", 8),
            ("#1=IFCX(#2);", 8),
            ("#1=IFCX(1);\n#1=IFCX(2);", 9),
            ("#1=IFCX(1.E);", 8),
            ("#1=IFCX(/* open );", 8),
            ("#1=IFCX(());\nX", 9),
        ];
        for (data, regel) in gevallen {
            match lees(ifc(data).as_bytes()) {
                Err(Fout::Ongeldig { regel: r, .. }) => assert_eq!(r, regel, "{data}"),
                andere => panic!("{data}: {:?}", andere.map(|_| ())),
            }
        }
        assert!(lees(b"").is_err());
        assert!(lees(b"ISO-10303-21;").is_err());
    }

    #[test]
    fn diepe_lijst_is_een_fout() {
        let diep = |n: usize| format!("#1=IFCX(\n{}1{});", "(".repeat(n), ")".repeat(n));
        assert!(lees(ifc(&diep(MAX_DIEPTE)).as_bytes()).is_ok());
        let regel = ifc("").lines().position(|r| r == "DATA;").unwrap() + 3;
        for n in [MAX_DIEPTE + 1, 100_000] {
            match lees(ifc(&diep(n)).as_bytes()) {
                Err(Fout::Ongeldig { regel: r, melding }) => assert_eq!((r, melding.contains("dieper")), (regel, true)),
                andere => panic!("{n}: {:?}", andere.map(|_| ())),
            }
        }
    }

    #[test]
    fn tokens() {
        let t: Vec<Token> =
            Tokens::nieuw(b"(#12,'a''b',.T.,IFCLABEL('x'),-1.5E-3,$,*,\"0F\")").map(|(_, t)| t).collect();
        assert_eq!(
            t,
            vec![
                Token::Open,
                Token::Verwijzing(12),
                Token::Tekst(b"a''b"),
                Token::Enum(b"T"),
                Token::Trefwoord(b"IFCLABEL"),
                Token::Open,
                Token::Tekst(b"x"),
                Token::Sluit,
                Token::Getal(b"-1.5E-3"),
                Token::Leeg,
                Token::Afgeleid,
                Token::Binair(b"0F"),
                Token::Sluit
            ]
        );
    }
}
