"""Maakt de tabellen voor docs/samenwerken/metingen.md uit de CSV van meet.py.

    python3 -I tabel.py <resultaten.csv> <bronmap>

Per bron: tijd en piek per grootte, en het geheugen per MB invoer. Daarna een fit van het piekgeheugen op
de grootte S (MB) en het aantal instanties N (miljoen): piek = a*S + b*N (+ c), per opdracht, met de kleinste
kwadraten. N komt uit `k * instanties van de bron - (k-1)` (het IfcProject staat er maar één keer in); het aantal
instanties per bron tel je met de meetkopie (`openaec-samenvoegen tel <bron>`) en geef je hieronder op.
"""

import csv
import sys
from collections import defaultdict

# Instanties per bronmodel (meetkopie `tel`, 5 november 2026).
INSTANTIES = {
    "ifc2x3-infra-landscaping.ifc": 27419,
    "ifc4-infra-bridge.ifc": 919,
    "ifc4-infra-landscaping.ifc": 1186,
    "ifc4x3-infra-bridge.ifc": 902,
    "ops-13-supermarkt-albert-heijn.ifc": 292,
}
KOLOMMEN = [("verschil", "weinig"), ("verschil", "veel"), ("voeg-samen", "weinig"), ("voeg-samen", "veel"),
            ("voeg-samen", "groot"), ("voeg-samen", "conflict")]


def los_op(rijen):
    """Kleinste kwadraten voor piek = a*S + b*N + c, zonder numpy (3x3-stelsel)."""
    x = [(s, n, 1.0) for s, n, _ in rijen]
    y = [p for _, _, p in rijen]
    ata = [[sum(r[i] * r[j] for r in x) for j in range(3)] for i in range(3)]
    aty = [sum(r[i] * v for r, v in zip(x, y)) for i in range(3)]
    m = [ata[i] + [aty[i]] for i in range(3)]
    for i in range(3):
        p = max(range(i, 3), key=lambda r: abs(m[r][i]))
        m[i], m[p] = m[p], m[i]
        for r in range(3):
            if r != i:
                f = m[r][i] / m[i][i]
                m[r] = [a - f * b for a, b in zip(m[r], m[i])]
    return [m[i][3] / m[i][i] for i in range(3)]


def main():
    rijen = list(csv.DictReader(open(sys.argv[1])))
    per_bron = defaultdict(lambda: defaultdict(dict))
    schema = {}
    for r in rijen:
        per_bron[r["bron"]][int(r["k"])][(r["opdracht"], r["soort"])] = r
        schema[r["bron"]] = r["schema"]
    fit = defaultdict(list)
    for bron, per_k in per_bron.items():
        print(f"\n### {bron} ({schema[bron]}, {INSTANTIES[bron]} instanties per kopie)\n")
        print("| MB | instanties | " + " | ".join(f"{o} {s}" for o, s in KOLOMMEN) + " |")
        print("|---" * (2 + len(KOLOMMEN)) + "|")
        for k in sorted(per_k):
            mb = next(iter(per_k[k].values()))["mb"]
            n = k * INSTANTIES[bron] - (k - 1)
            cellen = []
            for kol in KOLOMMEN:
                r = per_k[k].get(kol)
                if not r:
                    cellen.append("-")
                elif r["exit"] not in ("0", "1") or not r["piek_mb"]:
                    cellen.append(f"gestopt (exit {r['exit']})")
                else:
                    piek = float(r["piek_mb"])
                    cellen.append(f"{float(r['seconden']):.2f} s, {piek:.0f} MB ({piek / float(mb):.1f}/MB)")
                    fit[kol].append((float(mb), n / 1e6, piek))
            print(f"| {mb} | {n} | " + " | ".join(cellen) + " |")
    print("\n### Fit: piek = a*S + b*N + c\n")
    print("| opdracht | a (MB per MB) | b (MB per miljoen instanties) | c (MB) | grootste afwijking |")
    print("|---|---|---|---|---|")
    for kol in KOLOMMEN:
        if len(fit[kol]) < 3:
            continue
        a, b, c = los_op(fit[kol])
        afw = max(abs(a * s + b * n + c - p) / p for s, n, p in fit[kol] if p > 50)
        print(f"| {kol[0]} {kol[1]} | {a:.2f} | {b:.0f} | {c:.0f} | {afw * 100:.0f} % (punten boven 50 MB) |")


if __name__ == "__main__":
    main()
