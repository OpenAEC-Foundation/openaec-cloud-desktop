#!/usr/bin/env python3
"""Mutatieproef voor tests/eigenschappen.rs: zet een bekende fout in de bibliotheek, draai de eigenschap die hem
moet vinden, en zet het bestand daarna terug uit een kopie.

Gebruik (vanuit de map van de crate):
    python3 -I scripts/mutatieproef.py <naam> <filters,met,komma> <aantal-gevallen>
Bijvoorbeeld: python3 -I scripts/mutatieproef.py behoud behoud_zonder_stil_verlies 128
Verwacht: de test faalt (exit 101), en `git diff src` is daarna leeg.
De mutaties staan hieronder: per naam een lijst (bestand, oud, nieuw).
"""

import os
import shutil
import subprocess
import sys
import tempfile

CRATE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KOPIE = tempfile.mkdtemp(prefix="mutatieproef-")

MUTATIES = {
    # 1. Identiteit: "ons wijzigde niets" geeft ons in plaats van hun.
    "identiteit": [
        (
            "src/samenvoegen.rs",
            "} else if ons == basis {\n        Some(hun)",
            "} else if ons == basis {\n        Some(ons)",
        )
    ],
    # 2. Idempotent: "ons en hun gelijk" geeft de basis.
    "idempotent": [
        (
            "src/samenvoegen.rs",
            "} else if zelfde(B, H) || zelfde(O, H) {\n        Some(ons)",
            "} else if zelfde(B, H) {\n        Some(ons)\n    } else if zelfde(O, H) {\n        Some(basis)",
        )
    ],
    # 3. Symmetrie: ons wijzigde, hun verwijderde: geen conflict meer (de andere kant blijft een conflict).
    "symmetrie": [
        (
            "src/samenvoegen.rs",
            "(Some(b), Some(o), None) => {\n            if o != b {",
            "(Some(b), Some(o), None) => {\n            if o != b && false {",
        )
    ],
    # 4. Behoud: alleen hun wijzigde een object, maar we nemen ons.
    "behoud": [
        (
            "src/samenvoegen.rs",
            "} else if o == b {\n                Besluit::Heel(H)",
            "} else if o == b {\n                Besluit::Heel(O)",
        )
    ],
    # 4b. Geen stille keuze: hetzelfde attribuut anders, geen conflict (ons wint).
    "stille_keuze": [
        (
            "src/samenvoegen.rs",
            "        c.ons = Some(toon(O));\n        c.hun = Some(toon(H));\n        conflicten.push(c);\n        delen.push(Deel::Van(O));",
            "        c.ons = Some(toon(O));\n        c.hun = Some(toon(H));\n        let _ = c;\n        delen.push(Deel::Van(O));",
        )
    ],
    # 4c. Vluchtig: altijd ons.
    "vluchtig": [
        (
            "src/samenvoegen.rs",
            "if b == Some(o) && h != o {\n        H",
            "if b == Some(o) && h != o && false {\n        H",
        )
    ],
    # 5. Stabiel: dezelfde wijziging aan twee kanten is een conflict.
    "stabiel": [
        (
            "src/samenvoegen.rs",
            "if o == h || h == b {\n                Besluit::Heel(O)",
            "if h == b {\n                Besluit::Heel(O)",
        ),
        (
            "src/samenvoegen.rs",
            "if h[O] == h[H] || h[H] == h[B] {\n            delen.push(Deel::Van(O));",
            "if h[H] == h[B] {\n            delen.push(Deel::Van(O));",
        ),
    ],
    # 6. Volgorde: de lus schrijft na het samenvoegen de versie van hun in plaats van het resultaat.
    "volgorde": [
        (
            "src/samenwerken.rs",
            "Some(r) => {\n                inhoud = r.clone();",
            "Some(r) => {\n                let _ = r;\n                inhoud = hun.clone();",
        )
    ],
    # 7. Nummering: het nummer van een knoop telt mee in de inhoud.
    "nummering": [
        (
            "src/model.rs",
            "                uit.push(b'K');\n                uit.extend_from_slice(self.hashes.get(k).unwrap_or(&NUL));",
            "                uit.push(b'K');\n                uit.extend_from_slice(self.hashes.get(k).unwrap_or(&NUL));\n                uit.extend_from_slice(&m.bestand.instanties[k].id.to_le_bytes());",
        )
    ],
    # 8. Schrijven: nieuwe nummers beginnen op het hoogste nummer van ons (botsing), en de zelfcontrole kijkt niet.
    "schrijven": [
        ("src/schrijven.rs", "volgende: m[O].bestand.hoogste + 1,", "volgende: m[O].bestand.hoogste,"),
        (
            "src/samenvoegen.rs",
            "    if !gevonden.is_empty() {\n        return mislukt(gevonden, statistiek);",
            "    if false && !gevonden.is_empty() {\n        return mislukt(gevonden, statistiek);",
        ),
    ],
    # 9. Paniek: een dubbel instantienummer geeft een paniek in plaats van een fout.
    "paniek": [
        (
            "src/ontleden.rs",
            'return Err(ongeldig(data, inst.bereik.0 as usize, format!("instantie #{} komt twee keer voor", inst.id)));',
            'panic!("instantie #{} komt twee keer voor", inst.id);',
        )
    ],
    # 10. Rapport: bij een conflict staat gelukt op true.
    "rapport": [
        (
            "src/samenvoegen.rs",
            "Ok(Samenvoeging { inhoud: None, gelukt: false, conflicten, statistiek })",
            "Ok(Samenvoeging { inhoud: None, gelukt: true, conflicten, statistiek })",
        )
    ],
}


def main():
    naam, filt, gevallen = sys.argv[1], sys.argv[2], sys.argv[3]
    os.makedirs(KOPIE, exist_ok=True)
    bestanden = sorted({b for b, _, _ in MUTATIES[naam]})
    for b in bestanden:
        shutil.copy(os.path.join(CRATE, b), os.path.join(KOPIE, os.path.basename(b)))
    try:
        for b, oud, nieuw in MUTATIES[naam]:
            pad = os.path.join(CRATE, b)
            with open(pad) as f:
                tekst = f.read()
            assert tekst.count(oud) == 1, f"{naam}: '{oud[:60]}' komt {tekst.count(oud)} keer voor in {b}"
            with open(pad, "w") as f:
                f.write(tekst.replace(oud, nieuw))
        env = dict(os.environ, PROPTEST_CASES=gevallen, PROPTEST_MAX_SHRINK_TIME="1")
        p = subprocess.run(
            ["cargo", "test", "--test", "eigenschappen", "--", *filt.split(","), "--test-threads=2"],
            cwd=CRATE,
            env=env,
            capture_output=True,
            text=True,
            timeout=3600,
            check=False,
        )
        uit = p.stdout + p.stderr
        regels = [r for r in uit.splitlines() if r.startswith(("test ", "error")) or "Test failed" in r]
        print(f"== mutatie {naam}, filter {filt}, exit {p.returncode}")
        for r in regels:
            print("  " + r[:400])
    finally:
        for b in bestanden:
            shutil.copy(os.path.join(KOPIE, os.path.basename(b)), os.path.join(CRATE, b))
        d = subprocess.run(
            ["git", "diff", "--stat", "--", "src"], cwd=CRATE, capture_output=True, text=True, check=False
        )
        print("  git diff src na terugzetten: " + (d.stdout.strip() or "leeg"))


if __name__ == "__main__":
    main()
