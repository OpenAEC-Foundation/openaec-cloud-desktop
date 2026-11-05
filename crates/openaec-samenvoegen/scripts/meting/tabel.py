"""Maakt de tabellen voor docs/samenwerken/metingen.md uit de CSV van meet.py.

    python3 -I tabel.py <resultaten.csv>

Per bron: tijd en piek per grootte, en het geheugen per MB invoer. Het piekgeheugen staat in MiB (de kB van
`/usr/bin/time -v` gedeeld door 1024), de invoer in MB (10^6 bytes). Daarna een fit van het piekgeheugen (MiB) op de
grootte S (MB), het aantal knopen K en het aantal objecten O (miljoen): piek = a*S + b*K + c*O + d, per opdracht, met de
kleinste kwadraten. K = k * knopen van de bron, O = k * objecten van de bron - (k-1) (het IfcProject staat er maar
één keer in). Tel ze per bron met de meetkopie (`openaec-samenvoegen tel <bron>`) en geef ze hieronder op.
"""

import csv
import sys
from collections import defaultdict

# (objecten, knopen) per bronmodel (meetkopie `tel`, 5 november 2026).
TELLING = {
    "ifc2x3-infra-landscaping.ifc": (185, 27234),
    "ifc4-infra-bridge.ifc": (192, 727),
    "ifc4-infra-landscaping.ifc": (202, 984),
    "ifc4x3-infra-bridge.ifc": (175, 727),
    "ops-13-supermarkt-albert-heijn.ifc": (165, 127),
}
KOLOMMEN = [("verschil", "weinig"), ("verschil", "veel"), ("voeg-samen", "weinig"), ("voeg-samen", "veel"),
            ("voeg-samen", "groot"), ("voeg-samen", "conflict")]


def los_op(x, y):
    """Kleinste kwadraten voor y = x * coëfficiënten, zonder numpy (normaalvergelijkingen, Gauss)."""
    n = len(x[0])
    m = [[sum(r[i] * r[j] for r in x) for j in range(n)] + [sum(r[i] * v for r, v in zip(x, y))] for i in range(n)]
    for i in range(n):
        p = max(range(i, n), key=lambda r: abs(m[r][i]))
        m[i], m[p] = m[p], m[i]
        for r in range(n):
            if r != i:
                f = m[r][i] / m[i][i]
                m[r] = [a - f * b for a, b in zip(m[r], m[i])]
    return [m[i][n] / m[i][i] for i in range(n)]


def main():
    rijen = list(csv.DictReader(open(sys.argv[1])))
    per_bron = defaultdict(lambda: defaultdict(dict))
    schema = {}
    for r in rijen:
        per_bron[r["bron"]][int(r["k"])][(r["opdracht"], r["soort"])] = r
        schema[r["bron"]] = r["schema"]
    fit = defaultdict(list)
    for bron, per_k in per_bron.items():
        obj, kn = TELLING[bron]
        print(f"\n### {bron} ({schema[bron]}, {obj} objecten en {kn} knopen per kopie)\n")
        print("| invoer (MB) | instanties | " + " | ".join(f"{o} {s}" for o, s in KOLOMMEN) + " |")
        print("|---" * (2 + len(KOLOMMEN)) + "|")
        for k in sorted(per_k):
            mb = next(iter(per_k[k].values()))["mb"]
            o, kk = k * obj - (k - 1), k * kn
            n = o + kk
            cellen = []
            for kol in KOLOMMEN:
                r = per_k[k].get(kol)
                if not r:
                    cellen.append("-")
                elif r["exit"] not in ("0", "1") or not r["piek_mb"]:
                    cellen.append(f"gestopt (exit {r['exit']})")
                else:
                    piek = float(r["piek_mb"])
                    cellen.append(f"{float(r['seconden']):.2f} s, {piek:.0f} MiB ({piek / float(mb):.1f} MiB/MB)")
                    fit[kol].append(((float(mb), kk / 1e6, o / 1e6, 1.0), piek, float(r["seconden"])))
            print(f"| {mb} | {n} | " + " | ".join(cellen) + " |")
    print("\n### Fit: piek (MiB) = a*S + b*K + c*O + d\n")
    print("| opdracht | punten | a (MiB per MB) | b (bytes per knoop) | c (bytes per object) | d (MiB) "
          "| grootste afwijking, punten boven 50 MiB | grootste afwijking, alle punten |")
    print("|---|---|---|---|---|---|---|---|")
    for kol in KOLOMMEN:
        if len(fit[kol]) < 4:
            continue
        x, y = [r for r, _, _ in fit[kol]], [p for _, p, _ in fit[kol]]
        co = los_op(x, y)
        fout = [(abs(sum(c * v for c, v in zip(co, r)) - p) / p, p) for r, p, _ in fit[kol]]
        boven = max(f for f, p in fout if p > 50)
        alle = max(f for f, _ in fout)
        a, b, c, d = co
        print(f"| {kol[0]} {kol[1]} | {len(fout)} | {a:.2f} | {b * 1.048576:.0f} | {c * 1.048576:.0f} | {d:.0f} "
              f"| {boven * 100:.0f} % | {alle * 100:.1f} % |")

    print("\n### Fit van de tijd: seconden = a*S + b*K + c*O (zonder vaste term; ruw: drukke machine en wisselende bestandscache)\n")
    print("| opdracht | a (s per 100 MB) | b (µs per knoop) | c (µs per object) |")
    print("|---|---|---|---|")
    for kol in KOLOMMEN:
        if len(fit[kol]) < 4:
            continue
        a, b, c = los_op([r[:3] for r, _, _ in fit[kol]], [t for _, _, t in fit[kol]])
        print(f"| {kol[0]} {kol[1]} | {a * 100:.2f} | {b:.2f} | {c:.1f} |")


if __name__ == "__main__":
    main()
