"""Maakt invoer voor de metingen: een IFC-model k keer vermenigvuldigd, met een basis, ons en hun.

Gebruik (alleen de standaardbibliotheek):

    python3 -I maak_invoer.py <bron.ifc> <k> <soort> <uitmap>

Kopie 0 is de bron zelf. Elke volgende kopie krijgt nieuwe nummers (#n) en nieuwe GlobalIds, en hangt onder het
IfcProject van de bron (dat staat er maar één keer in). Zo groeit het bestand ongeveer k keer.

De soort bepaalt wat ons en hun wijzigen (de Name van een object, het derde attribuut van IfcRoot):

- weinig:   ons 0,1 % van de objecten, hun een ander 0,1 % (minstens 1 object per kant). Geen conflict.
- veel:     ons 10 %, hun een ander 10 %. Geen conflict.
- groot:    ons als bij weinig; hun voegt een hele extra kopie van de bron toe (één grote wijziging).
- conflict: ons en hun wijzigen dezelfde 0,1 % elk anders. Elk daarvan is een conflict.

Schrijft <uitmap>/basis.ifc, ons.ifc en hun.ifc, regel voor regel (weinig geheugen), en als laatste het lege bestand
<uitmap>/klaar. <uitmap> moet nieuw zijn, of een lege map die geen link is. Het script maakt alleen nieuwe bestanden
(modus "x"): het overschrijft niets en volgt geen bestaande link. Zo kan het nooit zijn bron of iets anders vervangen.
"""

import hashlib
import re
import sys
from pathlib import Path

TEKENS = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$"
# Eén instantie: alles tot de puntkomma buiten een tekst.
INSTANTIE = re.compile(r"(?:[^';]|'(?:[^']|'')*')*;", re.S)
KOP = re.compile(r"\s*#(\d+)\s*=\s*([A-Za-z0-9_]+)\s*\(")
VERWIJZING = re.compile(r"'(?:[^']|'')*'|#(\d+)")
GLOBALID = re.compile(r"'([0-9A-Za-z_$]{22})'")
DELEN = re.compile(r"'(?:[^']|'')*'|[(),]")


def nieuwe_guid(guid: str, kopie: int) -> str:
    h = hashlib.blake2b(f"{guid}/{kopie}".encode(), digest_size=17).digest()
    n = int.from_bytes(h, "big")
    uit = [TEKENS[n % 4]]
    n //= 4
    for _ in range(21):
        uit.append(TEKENS[n % 64])
        n //= 64
    return "".join(uit)


def zet_naam(instantie: str, naam: str) -> str:
    """Vervangt het derde attribuut (Name) van een instantie `#n=TYPE(a,b,c,...)`."""
    open_haakje = KOP.match(instantie).end()
    diepte, komma = 0, 0
    begin = None
    for m in DELEN.finditer(instantie, open_haakje):
        t = m.group()
        if t == "(":
            diepte += 1
        elif t == ")":
            if diepte == 0:
                if begin is not None:
                    return instantie[:begin] + f"'{naam}'" + instantie[m.start():]
                break
            diepte -= 1
        elif t == "," and diepte == 0:
            komma += 1
            if komma == 2:
                begin = m.end()
            elif komma == 3:
                return instantie[:begin] + f"'{naam}'" + instantie[m.start():]
    raise ValueError(f"geen derde attribuut: {instantie[:80]}")


def lees(bron: Path):
    tekst = bron.read_text(encoding="latin-1")
    begin = tekst.index("DATA;") + len("DATA;")
    einde = tekst.index("ENDSEC;", begin)
    kop = tekst[:begin]
    instanties = [m.group().strip() for m in INSTANTIE.finditer(tekst, begin, einde)]
    instanties = [i for i in instanties if i != ";"]
    return kop, instanties, tekst[einde:]


def kopie(instanties, k: int, verschuiving: int, project: int):
    """Geeft kopie k: nieuwe nummers en GlobalIds; verwijzingen naar het project blijven."""
    for inst in instanties:
        m = KOP.match(inst)
        if m and int(m.group(1)) == project:
            if k > 0:
                continue
        if k == 0:
            yield inst, m
            continue

        def nummer(v):
            if v.group(1) is None:
                return v.group()
            n = int(v.group(1))
            return v.group() if n == project else f"#{n + k * verschuiving}"

        inst = VERWIJZING.sub(nummer, inst)
        if m:
            g = GLOBALID.match(inst, KOP.match(inst).end())
            if g:
                inst = inst[: g.start(1)] + nieuwe_guid(g.group(1), k) + inst[g.end(1):]
        yield inst, KOP.match(inst)


def main():
    bron, k, soort, uitmap = Path(sys.argv[1]), int(sys.argv[2]), sys.argv[3], Path(sys.argv[4])
    assert soort in ("weinig", "veel", "groot", "conflict"), soort
    kop, instanties, staart = lees(bron)
    hoogste = max(int(KOP.match(i).group(1)) for i in instanties if KOP.match(i))
    project = next(int(KOP.match(i).group(1)) for i in instanties if KOP.match(i) and KOP.match(i).group(2).upper() == "IFCPROJECT")
    verschuiving = 10 ** len(str(hoogste))
    objecten = sum(1 for i in instanties if KOP.match(i) and GLOBALID.match(i, KOP.match(i).end())) * k
    per = 10 if soort == "veel" else 1000
    # Kies elke `per`-de object; ons de eerste rest, hun de tweede (bij conflict dezelfde).
    stap = max(1, min(per, objecten // 2))
    uitmap.parent.mkdir(parents=True, exist_ok=True)
    try:
        uitmap.mkdir()
    except FileExistsError:
        if uitmap.is_symlink() or not uitmap.is_dir() or any(uitmap.iterdir()):
            sys.exit(f"{uitmap} bestaat al en is geen lege map; kies een nieuwe map")
    bestanden = {z: open(uitmap / f"{z}.ifc", "x", encoding="latin-1", newline="\n") for z in ("basis", "ons", "hun")}
    for f in bestanden.values():
        f.write(kop + "\n")
    j = 0
    gewijzigd = {"ons": 0, "hun": 0}
    for kk in range(k):
        for inst, m in kopie(instanties, kk, verschuiving, project):
            regel = {"basis": inst, "ons": inst, "hun": inst}
            if m and GLOBALID.match(inst, m.end()):
                r = j % stap
                if r == 0:
                    regel["ons"] = zet_naam(inst, f"ons {j}")
                    gewijzigd["ons"] += 1
                    if soort == "conflict":
                        regel["hun"] = zet_naam(inst, f"hun {j}")
                        gewijzigd["hun"] += 1
                elif r == 1 and soort in ("weinig", "veel"):
                    regel["hun"] = zet_naam(inst, f"hun {j}")
                    gewijzigd["hun"] += 1
                j += 1
            for z, f in bestanden.items():
                f.write(regel[z] + "\n")
    if soort == "groot":
        for inst, _ in kopie(instanties, k, verschuiving, project):
            bestanden["hun"].write(inst + "\n")
    for f in bestanden.values():
        f.write(staart)
        f.close()
    open(uitmap / "klaar", "x").close()
    groot = (uitmap / "basis.ifc").stat().st_size
    print(f"{bron.name} k={k} {soort}: {groot / 1e6:.1f} MB, {j} objecten, ons {gewijzigd['ons']}, hun {gewijzigd['hun']}")


if __name__ == "__main__":
    main()
