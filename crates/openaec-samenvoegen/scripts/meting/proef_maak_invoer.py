"""Proef: maak_invoer.py overschrijft nooit iets, ook niet zijn bron of het doel van een link.

    python3 -I proef_maak_invoer.py [pad naar maak_invoer.py]

Zonder argument neemt de proef maak_invoer.py uit deze map. Exitcode 0 = alles goed.
"""

import hashlib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HIER = Path(__file__).resolve().parent
SCRIPT = Path(sys.argv[1]) if len(sys.argv) > 1 else HIER / "maak_invoer.py"
VOORBEELD = HIER.parents[1] / "tests/bestanden/13-supermarkt-albert-heijn.ifc"


def hash_van(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def maak(bron: Path, uit: Path) -> int:
    return subprocess.run([sys.executable, "-I", str(SCRIPT), str(bron), "2", "weinig", str(uit)],
                          capture_output=True).returncode


def main():
    fouten = []
    with tempfile.TemporaryDirectory() as t:
        t = Path(t)
        bron_map = t / "model"
        bron_map.mkdir()
        bron = bron_map / "basis.ifc"
        shutil.copy(VOORBEELD, bron)
        voor = hash_van(bron)

        # 1. De uitvoermap is de map van de bron (en de bron heet basis.ifc).
        if maak(bron, bron_map) == 0:
            fouten.append("1: een map met de bron erin werd geaccepteerd")
        if hash_van(bron) != voor:
            fouten.append("1: de bron is veranderd")

        # 2. De uitvoermap is een link naar de map van de bron.
        link = t / "link"
        link.symlink_to(bron_map)
        if maak(bron, link) == 0:
            fouten.append("2: een link naar een map werd geaccepteerd")
        if hash_van(bron) != voor:
            fouten.append("2: de bron is veranderd via een link naar de map")

        # 3. In de uitvoermap staat een link hun.ifc naar de bron.
        met_link = t / "met-link"
        met_link.mkdir()
        (met_link / "hun.ifc").symlink_to(bron)
        if maak(bron, met_link) == 0:
            fouten.append("3: een map met een link erin werd geaccepteerd")
        if hash_van(bron) != voor:
            fouten.append("3: de bron is veranderd via een link in de map")

        # 4. Een nieuwe map werkt, en een tweede keer in dezelfde map niet.
        nieuw = t / "nieuw"
        if maak(bron, nieuw) != 0:
            fouten.append("4: een nieuwe map werd geweigerd")
        elif sorted(p.name for p in nieuw.iterdir()) != ["basis.ifc", "hun.ifc", "klaar", "ons.ifc"]:
            fouten.append(f"4: onverwachte bestanden: {sorted(p.name for p in nieuw.iterdir())}")
        else:
            eerst = {p.name: hash_van(p) for p in nieuw.glob("*.ifc")}
            if maak(bron, nieuw) == 0:
                fouten.append("4: een tweede run in dezelfde map werd geaccepteerd")
            if {p.name: hash_van(p) for p in nieuw.glob("*.ifc")} != eerst:
                fouten.append("4: de tweede run veranderde de bestanden")

        # 5. Een bestaande lege map werkt.
        leeg = t / "leeg"
        leeg.mkdir()
        if maak(bron, leeg) != 0:
            fouten.append("5: een lege map werd geweigerd")
        if hash_van(bron) != voor:
            fouten.append("5: de bron is veranderd")

    for f in fouten:
        print("FOUT", f)
    print("goed" if not fouten else f"{len(fouten)} fouten")
    sys.exit(1 if fouten else 0)


if __name__ == "__main__":
    main()
