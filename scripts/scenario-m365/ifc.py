"""Kleine bewerkingen op een IFC-bestand voor de scenarioproeven, en een controle op een verschil.

Gebruik (altijd met `python3 -I`):
  ifc.py hernummer IN UIT K           elke #n wordt #(n+K), zoals een programma dat bij opslaan opnieuw nummert
  ifc.py schuif IN UIT GLOBALID DAGEN alle datums in de IfcTaskTime van die taak schuiven DAGEN dagen op
  ifc.py duur IN UIT GLOBALID DAGEN   ScheduleDuration van die taak wordt DAGEN dagen; het einde schuift mee
  ifc.py vervang IN UIT OUD NIEUW     vervang een tekst die precies één keer voorkomt
  ifc.py plaats IN UIT GLOBALID X Y Z het object krijgt een nieuw punt in zijn IfcAxis2Placement3D
  ifc.py controleer VERSCHIL.json [--gewijzigd G,G] [--toegevoegd G] [--verwijderd G] [--waarden BESTAND]
                                      het verschil noemt precies deze objecten, en verder niets; de HEADER en de
                                      losse knopen zijn gelijk; en elke gewijzigde waarde staat letterlijk in
                                      BESTAND (een regel per waarde, zie `waarde_regel`). Zonder BESTAND mag er
                                      geen enkele waarde gewijzigd zijn
  ifc.py status VERSCHIL.json GLOBALID
                                      bestaat, gewijzigd of verwijderd volgens het verschil
  ifc.py blokken NIEUW OUD            hoeveel blokken van 8 MiB op dezelfde plek gelijk zijn

Elke bewerking stopt met een fout als ze niet precies één plek raakt. Zo kan een proef nooit slagen omdat er
niets gewijzigd is.
"""

import datetime
import json
import re
import sys


def lees(pad):
    with open(pad, encoding="utf-8", newline="") as f:
        return f.read()


def schrijf(pad, tekst):
    with open(pad, "w", encoding="utf-8", newline="") as f:
        f.write(tekst)


def fout(melding):
    sys.exit(f"ifc.py: {melding}")


def regel(tekst, nummer):
    """De regel van instantie #nummer."""
    gevonden = re.findall(rf"^#{nummer}=.*$", tekst, re.M)
    if len(gevonden) != 1:
        fout(f"#{nummer} komt {len(gevonden)} keer voor")
    return gevonden[0]


def object_regel(tekst, globalid):
    gevonden = re.findall(rf"^#\d+=IFC[A-Z0-9]+\('{re.escape(globalid)}'.*$", tekst, re.M)
    if len(gevonden) != 1:
        fout(f"object {globalid} komt {len(gevonden)} keer voor")
    return gevonden[0]


def vervang_een(tekst, oud, nieuw):
    if tekst.count(oud) != 1:
        fout(f"{oud[:80]!r} komt {tekst.count(oud)} keer voor")
    return tekst.replace(oud, nieuw)


def taaktijd(tekst, globalid):
    """De regel van de IfcTaskTime van een taak (het attribuut TaskTime, het twaalfde)."""
    taak = object_regel(tekst, globalid)
    if not taak.split("=", 1)[1].startswith("IFCTASK("):
        fout(f"{globalid} is geen IfcTask")
    verwijzing = re.search(r",#(\d+),\.[A-Z_]+\.\);$", taak)
    if not verwijzing:
        fout(f"taak {globalid} heeft geen TaskTime")
    return regel(tekst, verwijzing.group(1))


DATUM = re.compile(r"'(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)'")


def tijden(tekst, globalid):
    """De IfcTaskTime van een taak in stukken: (regel, begin tot en met de duur, duur in dagen, de rest per komma).
    In de rest staan eerst ScheduleStart, ScheduleFinish, EarlyStart, EarlyFinish, LateStart en LateFinish."""
    oud = taaktijd(tekst, globalid)
    m = re.search(r"^(#\d+=IFCTASKTIME\('(?:[^']|'')*',[^,]*,[^,]*,[^,]*,)'P0Y0M(\d+)D'", oud)
    if not m:
        fout(f"taak {globalid} heeft geen duur in dagen")
    return oud, m.group(1), int(m.group(2)), oud[m.end():].split(",")


def verschuif(delen, posities, stap):
    for i in posities:
        d = DATUM.fullmatch(delen[i])
        if not d:
            fout(f"geen datum op plek {i}: {delen[i]}")
        delen[i] = "'" + (datetime.datetime.fromisoformat(d.group(1)) + stap).isoformat() + "'"


def schuif(tekst, globalid, dagen):
    """Schuif de geplande, vroegste en laatste begin- en einddatum DAGEN dagen op."""
    oud, begin, duur_nu, delen = tijden(tekst, globalid)
    verschuif(delen, range(1, 7), datetime.timedelta(days=dagen))
    return vervang_een(tekst, oud, begin + f"'P0Y0M{duur_nu}D'" + ",".join(delen))


def duur(tekst, globalid, dagen):
    """Zet ScheduleDuration op DAGEN dagen en schuif de einddatums mee."""
    oud, begin, duur_nu, delen = tijden(tekst, globalid)
    if dagen == duur_nu:
        fout(f"taak {globalid} duurt al {dagen} dagen")
    verschuif(delen, (2, 4, 6), datetime.timedelta(days=dagen - duur_nu))
    return vervang_een(tekst, oud, begin + f"'P0Y0M{dagen}D'" + ",".join(delen))


def hernummer(tekst, k):
    # Alleen buiten teksten: een # binnen '...' blijft staan.
    delen = re.split(r"('(?:[^']|'')*')", tekst)
    for i in range(0, len(delen), 2):
        delen[i] = re.sub(r"#(\d+)", lambda m: f"#{int(m.group(1)) + k}", delen[i])
    return "".join(delen)


def plaats(tekst, globalid, x, y, z):
    """Zet het punt van de IfcAxis2Placement3D van het object op (x, y, z). Weigert als iets anders dat punt ook
    gebruikt: dan zou de wijziging ook een ander object raken."""
    obj = object_regel(tekst, globalid)
    plaatsing = re.search(r"^#\d+=IFC[A-Z0-9]+\('[^']*',#\d+,(?:'(?:[^']|'')*'|\$),(?:'(?:[^']|'')*'|\$),"
                          r"(?:'(?:[^']|'')*'|\$),#(\d+),", obj)
    if not plaatsing:
        fout(f"{globalid} heeft geen ObjectPlacement")
    lokaal = regel(tekst, plaatsing.group(1))
    as3d = re.search(r"IFCLOCALPLACEMENT\((?:#\d+|\$),#(\d+)\)", lokaal)
    if not as3d:
        fout(f"plaatsing van {globalid} is geen IfcLocalPlacement")
    punt = re.match(r"#\d+=IFCAXIS2PLACEMENT3D\(#(\d+)", regel(tekst, as3d.group(1)))
    if not punt:
        fout(f"plaatsing van {globalid} heeft geen punt")
    gebruik = len(re.findall(rf"#{punt.group(1)}(?![0-9])", tekst)) - 1
    if gebruik != 1:
        fout(f"het punt #{punt.group(1)} wordt {gebruik} keer gebruikt")
    oud = regel(tekst, punt.group(1))
    return vervang_een(tekst, oud, f"#{punt.group(1)}=IFCCARTESIANPOINT(({x},{y},{z}));")


def waarde_regel(waar, a):
    """Eén gewijzigde waarde als regel: `WAAR | ATTRIBUUT | OUD | NIEUW`. WAAR is een GlobalId of `HEADER`. Bij een
    lijst van verwijzingen (bijv. HasProperties) staan er wat eraf ging en wat erbij kwam, want oud en nieuw zijn
    daar ingekort."""
    if a.get("erbij") or a.get("eraf"):
        return f"{waar} | {a['attribuut']} | eraf: {'; '.join(a.get('eraf', []))} | erbij: {'; '.join(a.get('erbij', []))}"
    return f"{waar} | {a['attribuut']} | {a['oud']} | {a['nieuw']}"


def waarden_van(v):
    regels = [waarde_regel("HEADER", h) for h in v.get("header", [])]
    regels += [waarde_regel(o["globalid"], a) for o in v["gewijzigd"] for a in o["attributen"]]
    return sorted(regels)


def controleer(pad, gewijzigd, toegevoegd, verwijderd, waarden_pad=None):
    v = json.loads(lees(pad))
    echt = {
        "gewijzigd": sorted(o["globalid"] for o in v["gewijzigd"]),
        "toegevoegd": sorted(o["globalid"] for o in v["toegevoegd"]),
        "verwijderd": sorted(o["globalid"] for o in v["verwijderd"]),
    }
    verwacht = {"gewijzigd": sorted(gewijzigd), "toegevoegd": sorted(toegevoegd), "verwijderd": sorted(verwijderd)}
    if echt != verwacht:
        fout(f"verschil klopt niet:\n  verwacht {verwacht}\n  gevonden {echt}")
    los = v["losse_knopen"]
    if los["toegevoegd"] or los["verwijderd"]:
        fout(f"onverwachte losse knopen: {los}")
    verwachte_waarden = []
    if waarden_pad:
        verwachte_waarden = sorted(
            r for r in lees(waarden_pad).splitlines() if r.strip() and not r.startswith("#")
        )
    echte_waarden = waarden_van(v)
    if echte_waarden != verwachte_waarden:
        teveel = [r for r in echte_waarden if r not in verwachte_waarden]
        mist = [r for r in verwachte_waarden if r not in echte_waarden]
        fout("waarden kloppen niet:\n  niet verwacht:\n    " + "\n    ".join(teveel) + "\n  ontbreken:\n    " + "\n    ".join(mist))
    print(f"controle: het verschil noemt precies {verwacht}")
    print(f"controle: de HEADER is gelijk, en de {len(echte_waarden)} gewijzigde waarden zijn precies die uit "
          f"{waarden_pad.rsplit('/', 1)[-1] if waarden_pad else '(geen)'}")


BLOK = 8 * 1024 * 1024


def blokken(nieuw, oud):
    """Zoveel blokken van 8 MiB van NIEUW zijn byte voor byte gelijk aan het blok op dezelfde plek in OUD. Zo
    vergelijkt openaec-storage (`_herbruikbare_blokken`: dezelfde BLAKE3-hash op dezelfde positie); alleen de
    andere blokken gaan over het netwerk."""
    gelijk = totaal = 0
    with open(nieuw, "rb") as n, open(oud, "rb") as o:
        while True:
            a = n.read(BLOK)
            if not a:
                break
            totaal += 1
            gelijk += a == o.read(BLOK)
    return gelijk, totaal


def status(pad, globalid):
    """Wat een opmerking aan dit object over de nieuwe versie zou zeggen: bestaat, gewijzigd of verwijderd."""
    v = json.loads(lees(pad))
    if any(o["globalid"] == globalid for o in v["verwijderd"]):
        return "verwijderd"
    if any(o["globalid"] == globalid for o in v["gewijzigd"]):
        return "gewijzigd"
    return "bestaat"


def main(a):
    if not a:
        fout(__doc__)
    soort = a[0]
    if soort == "status":
        print(f"{a[2]}: {status(a[1], a[2])}")
        return
    if soort == "blokken":
        gelijk, totaal = blokken(a[1], a[2])
        print(f"{gelijk} van {totaal} blokken van 8 MiB gelijk op dezelfde plek; {totaal - gelijk} gaan over het netwerk")
        return
    if soort == "controleer":
        lijst = {"--gewijzigd": [], "--toegevoegd": [], "--verwijderd": []}
        waarden_pad = None
        rest = a[2:]
        for i in range(0, len(rest), 2):
            if rest[i] == "--waarden":
                waarden_pad = rest[i + 1]
            elif rest[i] in lijst:
                lijst[rest[i]] = [g for g in rest[i + 1].split(",") if g]
            else:
                fout(f"onbekende optie {rest[i]}")
        controleer(a[1], lijst["--gewijzigd"], lijst["--toegevoegd"], lijst["--verwijderd"], waarden_pad)
        return
    tekst = lees(a[1])
    if soort == "hernummer":
        tekst = hernummer(tekst, int(a[3]))
    elif soort == "schuif":
        tekst = schuif(tekst, a[3], int(a[4]))
    elif soort == "duur":
        tekst = duur(tekst, a[3], int(a[4]))
    elif soort == "vervang":
        tekst = vervang_een(tekst, a[3], a[4])
    elif soort == "plaats":
        tekst = plaats(tekst, a[3], a[4], a[5], a[6])
    else:
        fout(f"onbekende opdracht {soort}")
    schrijf(a[2], tekst)


if __name__ == "__main__":
    main(sys.argv[1:])
