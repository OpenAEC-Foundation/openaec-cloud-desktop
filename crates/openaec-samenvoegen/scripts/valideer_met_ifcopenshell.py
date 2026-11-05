"""Controleert samengevoegde IFC-bestanden met IfcOpenShell (schema, verplichte attributen, grenzen, INVERSE's).

Los van de Rust-tests: die hangen niet van IfcOpenShell af. Gebruik:

    SAMENVOEGEN_UITVOER=/tmp/resultaten cargo test --test samenvoegen
    uv run --python 3.12 --with ifcopenshell python -I scripts/valideer_met_ifcopenshell.py /tmp/resultaten

Exitcode 0 als alle bestanden geldig zijn, anders 1.
"""

import logging
import pathlib
import sys

import ifcopenshell
import ifcopenshell.validate


class Verzamel(logging.Handler):
    def __init__(self):
        super().__init__()
        self.meldingen = []

    def emit(self, record):
        self.meldingen.append(record.getMessage())


def valideer(pad):
    log = logging.getLogger(f"valideer.{pad.name}")
    log.propagate = False
    verzamel = Verzamel()
    log.addHandler(verzamel)
    log.setLevel(logging.INFO)
    bestand = ifcopenshell.open(str(pad))
    ifcopenshell.validate.validate(bestand, log, express_rules=False)
    return bestand.schema, verzamel.meldingen


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 2
    paden = sorted(pathlib.Path(argv[1]).glob("*.ifc"))
    if not paden:
        print(f"geen .ifc-bestanden in {argv[1]}")
        return 2
    fout = 0
    for pad in paden:
        schema, meldingen = valideer(pad)
        if meldingen:
            fout += 1
            print(f"FOUT {pad.name} ({schema}):")
            for m in meldingen[:20]:
                print(f"    {m}")
        else:
            print(f"goed {pad.name} ({schema})")
    print(f"{len(paden) - fout} van {len(paden)} bestanden geldig volgens IfcOpenShell {ifcopenshell.version}")
    return 1 if fout else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
