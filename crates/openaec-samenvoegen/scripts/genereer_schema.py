#!/usr/bin/env python3
"""Maakt de schematabellen in src/schema/*.txt uit de EXPRESS-schema's van buildingSMART.

Gebruik:
    python3 scripts/genereer_schema.py IFC2X3_TC1.exp IFC4.exp IFC4X3_ADD2.exp

Bron van de schema's (officiële publicaties van buildingSMART International):
    https://github.com/buildingSMART/Standards-builds/tree/main/IFC/RELEASE
      IFC2x3/TC1/EXPRESS/IFC2X3_TC1.exp
      IFC4/ADD2_TC1/EXPRESS/IFC4.exp
      IFC4_3/HTML/IFC4X3_ADD2.exp

Uitvoer: eerst "!SCHEMA", dan "~TYPE,TYPE,..." met de TYPE's die een SET of BAG zijn (bijv.
IfcPropertySetDefinitionSet; in een bestand als getypte waarde), dan één regel per entiteit:
    NAAM|SUPERTYPE|VLAGGEN|ATTRIBUTEN|INVERSES
- NAAM en SUPERTYPE zoals in het schema (bijv. IfcTask); SUPERTYPE is "-" als er geen is. In een IFC-bestand
  staan ze in hoofdletters (IFCTASK).
- VLAGGEN: R = IfcRoot of een subtype ervan, Q = subtype van IfcRelationship,
  N = subtype van IfcProperty of IfcPhysicalQuantity (een knoop met een naam als eerste attribuut).
- ATTRIBUTEN: de expliciete attributen in de volgorde van het bestand, ook de geërfde, gescheiden door komma's.
  Elk attribuut is "Naam", met "?" als het OPTIONAL is, en bij een aggregaat de soort en de grenzen:
  ":S1-?" (SET [1:?]), ":L2-3" (LIST [2:3]), ":B1-?" (BAG) of ":A" (ARRAY, vaste lengte). De grenzen gelden voor
  het buitenste aggregaat. Een TYPE dat een aggregaat is, telt ook als aggregaat.
- INVERSES: de eigen INVERSE-attributen (de geërfde staan bij het supertype), gescheiden door komma's, elk als
  "Naam=Type.ForAttribuut[min:max]" (max "?" = onbegrensd). Een inverse zonder SET/BAG is [1:1]. Een
  herdeclaratie (SELF\\X.Naam) vervangt de geërfde inverse met dezelfde naam.
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
    """(soort, minimum, maximum als tekst of "?") of None."""
    type_tekst = type_tekst.strip()
    if type_tekst.upper().startswith("UNIQUE "):
        type_tekst = type_tekst[7:]
    m = AGGREGAAT.match(type_tekst)
    if m:
        laag, hoog = m.group(2), m.group(3)
        return SOORT[m.group(1)], (0 if laag == "?" else int(laag)), hoog
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
        inverses = lees_inverses(naam, romp)
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
        entiteiten[naam] = {
            "supertype": supertype,
            "eigen": eigen,
            "herdeclaraties": herdeclaraties,
            "inverses": inverses,
        }
    return entiteiten


INVERSE = re.compile(
    r"^(SELF\\\w+\.)?(\w+)\s*:\s*(?:(SET|BAG)\s*\[\s*([0-9]+)\s*:\s*([0-9?]+)\s*\]\s*OF\s+)?(\w+)\s+FOR\s+(?:\w+\.)?(\w+)$"
)


def lees_inverses(naam, romp):
    """[(naam, type, for-attribuut, min, max, herdeclaratie)]; max None = onbegrensd."""
    m = re.search(r"^\s*INVERSE\b(.*?)(?=^\s*(?:UNIQUE|WHERE|DERIVE)\b|\Z)", romp, flags=re.S | re.M)
    if not m:
        return []
    uit = []
    for decl in m.group(1).split(";"):
        decl = " ".join(decl.split())
        if not decl:
            continue
        d = INVERSE.match(decl)
        if not d:
            raise SystemExit(f"{naam}: onbekende INVERSE '{decl}'")
        herdeclaratie, attr, soort, laag, hoog, doel, voor = d.groups()
        if soort is None:
            laag, hoog = "1", "1"
        uit.append((attr, doel, voor, int(laag), None if hoog == "?" else int(hoog), herdeclaratie is not None))
    return uit


def tekst_inverse(inv):
    attr, doel, voor, laag, hoog, _herdeclaratie = inv
    return f"{attr}={doel}.{voor}[{laag}:{'?' if hoog is None else hoog}]"


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
        soort, minimum, maximum = agg
        uit += ":" + soort + ("" if soort == "A" else f"{minimum}-{maximum}")
    return uit


def genereer(pad):
    tekst = zonder_commentaar(Path(pad).read_text(encoding="latin-1"))
    schema = re.search(r"^SCHEMA\s+(\w+)\s*;", tekst, flags=re.M).group(1)
    typen = lees_typen(tekst)
    entiteiten = lees_entiteiten(tekst, typen)
    regels = [f"!{schema}"]
    ongeordend = [t for t in typen if (aggregaat(typen[t], typen) or ("",))[0] in ("S", "B")]
    namen = {m.group(1).upper(): m.group(1) for m in re.finditer(r"^TYPE\s+(\w+)", tekst, flags=re.M)}
    regels.append("~" + ",".join(sorted(namen[t] for t in ongeordend)))
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
        invs = ",".join(tekst_inverse(i) for i in entiteiten[naam]["inverses"])
        regels.append(f"{naam}|{supertype if supertype else '-'}|{vlaggen}|{attrs}|{invs}")
    return schema, "\n".join(regels) + "\n"


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    doel = Path(__file__).resolve().parent.parent / "src" / "schema"
    for pad in sys.argv[1:]:
        schema, inhoud = genereer(pad)
        uit = doel / f"{schema.lower()}.txt"
        uit.write_text(inhoud, encoding="utf-8")
        print(f"{uit}: {inhoud.count(chr(10)) - 2} entiteiten, {len(inhoud)} bytes")


if __name__ == "__main__":
    main()
