#!/usr/bin/env python3
"""Maakt de schematabellen in src/schema/*.txt uit de EXPRESS-schema's van buildingSMART.

Gebruik:
    python3 scripts/genereer_schema.py IFC2X3_TC1.exp IFC4.exp IFC4X3_ADD2.exp

Bron van de schema's (officiële publicaties van buildingSMART International):
    https://github.com/buildingSMART/Standards-builds/tree/main/IFC/RELEASE
      IFC2x3/TC1/EXPRESS/IFC2X3_TC1.exp
      IFC4/ADD2_TC1/EXPRESS/IFC4.exp
      IFC4_3/HTML/IFC4X3_ADD2.exp

Uitvoer, één regel per entiteit:
    NAAM|SUPERTYPE|VLAGGEN|ATTRIBUTEN
- NAAM en SUPERTYPE zoals in het schema (bijv. IfcTask); SUPERTYPE is "-" als er geen is. In een IFC-bestand
  staan ze in hoofdletters (IFCTASK).
- VLAGGEN: R = IfcRoot of een subtype ervan, Q = subtype van IfcRelationship,
  N = subtype van IfcProperty of IfcPhysicalQuantity (een knoop met een naam als eerste attribuut).
- ATTRIBUTEN: de expliciete attributen in de volgorde van het bestand, ook de geërfde, gescheiden door komma's.
  Elk attribuut is "Naam", met "?" als het OPTIONAL is, en ":S1" (SET, minstens 1), ":L0" (LIST), ":B1" (BAG)
  of ":A" (ARRAY) als het een aggregaat is. Een TYPE dat een aggregaat is, telt ook als aggregaat.
"""

import re
import sys
from pathlib import Path

AGGREGAAT = re.compile(r"^(SET|LIST|BAG|ARRAY)\s*\[\s*([0-9?]+)\s*:\s*([0-9?]+)\s*\]\s*OF\s+(.*)$", re.S)
SOORT = {"SET": "S", "LIST": "L", "BAG": "B", "ARRAY": "A"}


def zonder_commentaar(tekst):
    tekst = re.sub(r"\(\*.*?\*\)", " ", tekst, flags=re.S)
    return re.sub(r"--[^\n]*", " ", tekst)


def lees_typen(tekst):
    """TYPE-naam -> onderliggend type (tekst), voor aggregaten via een TYPE."""
    typen = {}
    for m in re.finditer(r"^TYPE\s+(\w+)\s*=\s*(.*?);", tekst, flags=re.S | re.M):
        typen[m.group(1).upper()] = " ".join(m.group(2).split())
    return typen


def aggregaat(type_tekst, typen, diepte=0):
    """(soort, minimum) of None."""
    type_tekst = type_tekst.strip()
    if type_tekst.upper().startswith("UNIQUE "):
        type_tekst = type_tekst[7:]
    m = AGGREGAAT.match(type_tekst)
    if m:
        laag = m.group(2)
        return SOORT[m.group(1)], (0 if laag == "?" else int(laag))
    naam = type_tekst.split()[0].upper() if type_tekst else ""
    if diepte < 10 and naam in typen:
        return aggregaat(typen[naam], typen, diepte + 1)
    return None


def lees_entiteiten(tekst, typen):
    entiteiten = {}
    for m in re.finditer(r"^ENTITY\s+(\w+)(.*?)^END_ENTITY\s*;", tekst, flags=re.S | re.M):
        naam = m.group(1)
        blok = m.group(2)
        kop, _, romp = blok.partition(";")
        supertype = None
        s = re.search(r"SUBTYPE\s+OF\s*\(([^)]*)\)", kop)
        if s:
            ouders = [o.strip() for o in s.group(1).split(",") if o.strip()]
            if len(ouders) != 1:
                raise SystemExit(f"{naam}: meer dan één supertype ({ouders}) wordt niet ondersteund")
            supertype = ouders[0]
        eigen = []
        herdeclaraties = []
        expliciet = re.split(r"^\s*(?:DERIVE|INVERSE|UNIQUE|WHERE)\b", romp, maxsplit=1, flags=re.M)[0]
        for decl in expliciet.split(";"):
            decl = " ".join(decl.split())
            if not decl:
                continue
            links, sep, rechts = decl.partition(":")
            if not sep:
                raise SystemExit(f"{naam}: onbekende regel '{decl}'")
            rechts = rechts.strip()
            optioneel = rechts.upper().startswith("OPTIONAL ")
            if optioneel:
                rechts = rechts[9:].strip()
            agg = aggregaat(rechts, typen)
            for attr in [a.strip() for a in links.split(",")]:
                if attr.upper().startswith("SELF\\"):
                    # Herdeclaratie van een geërfd attribuut: geen nieuwe positie, wel een strenger type.
                    herdeclaraties.append((attr.split(".")[-1], optioneel, agg))
                else:
                    eigen.append((attr, optioneel, agg))
        entiteiten[naam] = {"supertype": supertype, "eigen": eigen, "herdeclaraties": herdeclaraties}
    return entiteiten


def keten(entiteiten, naam):
    uit = []
    while naam is not None:
        uit.append(naam)
        naam = entiteiten[naam]["supertype"]
    return uit


def attributen(entiteiten, naam):
    lijst = []
    for voorouder in reversed(keten(entiteiten, naam)):
        e = entiteiten[voorouder]
        lijst.extend([list(a) for a in e["eigen"]])
        for attr, optioneel, agg in e["herdeclaraties"]:
            for a in lijst:
                if a[0] == attr:
                    a[1] = optioneel
                    if agg is not None:
                        a[2] = agg
    return lijst


def tekst_attribuut(attr):
    naam, optioneel, agg = attr
    uit = naam + ("?" if optioneel else "")
    if agg is not None:
        soort, minimum = agg
        uit += ":" + soort + ("" if soort == "A" else str(minimum))
    return uit


def genereer(pad):
    tekst = zonder_commentaar(Path(pad).read_text(encoding="latin-1"))
    schema = re.search(r"^SCHEMA\s+(\w+)\s*;", tekst, flags=re.M).group(1)
    typen = lees_typen(tekst)
    entiteiten = lees_entiteiten(tekst, typen)
    regels = [f"!{schema}"]
    for naam in sorted(entiteiten, key=str.upper):
        voorouders = keten(entiteiten, naam)
        vlaggen = ""
        if "IfcRoot" in voorouders:
            vlaggen += "R"
        if "IfcRelationship" in voorouders[1:]:
            vlaggen += "Q"
        if "IfcProperty" in voorouders[1:] or "IfcPhysicalQuantity" in voorouders[1:]:
            vlaggen += "N"
        supertype = entiteiten[naam]["supertype"]
        attrs = ",".join(tekst_attribuut(a) for a in attributen(entiteiten, naam))
        regels.append(f"{naam}|{supertype if supertype else '-'}|{vlaggen}|{attrs}")
    return schema, "\n".join(regels) + "\n"


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    doel = Path(__file__).resolve().parent.parent / "src" / "schema"
    for pad in sys.argv[1:]:
        schema, inhoud = genereer(pad)
        uit = doel / f"{schema.lower()}.txt"
        uit.write_text(inhoud, encoding="utf-8")
        print(f"{uit}: {inhoud.count(chr(10)) - 1} entiteiten, {len(inhoud)} bytes")


if __name__ == "__main__":
    main()
