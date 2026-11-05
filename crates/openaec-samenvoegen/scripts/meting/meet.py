"""Meet tijd en piekgeheugen van `verschil` en `voeg-samen` als functie van de grootte. Eén run tegelijk.

Gebruik:

    python3 -I meet.py <programma> <werkmap> <resultaten.csv> <bron.ifc>...

<programma> is `target/release/openaec-samenvoegen` (gebouwd met `--features cli`). Per bron maakt het script met
maak_invoer.py invoer van ongeveer 1, 2, 5, 10, 20, 40, 80 en 150 MB in <werkmap> (ook via zwaar.sh), en meet elke
opdracht één keer:

    zwaar.sh [--alleen] systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 /usr/bin/time -v <programma> ...

De harde grens van 2 GB beschermt de andere processen op de machine. Het script schaalt een opdracht niet verder op
als de volgende stap naar verwachting (de vorige stap lineair doorgetrokken) boven 1,8 GB uitkomt. Verwacht het
900 MB of meer, dan meet het met `zwaar.sh --alleen`: dan draait er niets anders zwaars tegelijk. De invoer blijft
staan in <werkmap>, zodat latere versies van de bibliotheek op dezelfde bestanden te meten zijn. Een meting die al in
de CSV staat, slaat het over: zo kun je een reeks hervatten.

Met MEET_IN_EEN_PLEK=1 draai je de hele reeks onder één `zwaar.sh` (`zwaar.sh env MEET_IN_EEN_PLEK=1 python3 ...`): dan
wacht niet elke meting apart in de rij. Metingen die `--alleen` vragen, slaat het dan over; draai die daarna met een
gewone run zonder MEET_IN_EEN_PLEK. Elke regel in de CSV: bron, schema, k, MB, opdracht,
soort, seconden, piek in MB, exitcode.
"""

import csv
import os
import re
import subprocess
import sys
from pathlib import Path

HIER = Path(__file__).resolve().parent
ZWAAR = "/home/aia09/worktrees/zwaar.sh"
GRENS = ["systemd-run", "--user", "--scope", "-q", "-p", "MemoryMax=2G", "-p", "MemorySwapMax=0"]
DOEL_MB = [1, 2, 5, 10, 20, 40, 80, 150]
STOP_MB = 1800
ALLEEN_MB = 900
IN_EEN_PLEK = os.environ.get("MEET_IN_EEN_PLEK") == "1"
# (opdracht, soort van de invoer)
METINGEN = [
    ("verschil", "weinig"),
    ("verschil", "veel"),
    ("voeg-samen", "weinig"),
    ("voeg-samen", "veel"),
    ("voeg-samen", "groot"),
    ("voeg-samen", "conflict"),
]


def meet(programma: str, args: list[str], tijdbestand: Path, alleen: bool):
    tijdbestand.unlink(missing_ok=True)
    zwaar = [] if IN_EEN_PLEK else [ZWAAR, *(["--alleen"] if alleen else [])]
    cmd = [*zwaar, *GRENS, "/usr/bin/time", "-v", "-o", str(tijdbestand), programma, *args]
    p = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    t = tijdbestand.read_text() if tijdbestand.exists() else ""
    sec = re.search(r"Elapsed \(wall clock\) time .*: (?:(\d+):)?(\d+):([\d.]+)", t)
    rss = re.search(r"Maximum resident set size \(kbytes\): (\d+)", t)
    seconden = (int(sec.group(1) or 0) * 3600 + int(sec.group(2)) * 60 + float(sec.group(3))) if sec else None
    piek = int(rss.group(1)) / 1024 if rss else None
    return seconden, piek, p.returncode


def main():
    programma, werk, uit = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
    bronnen = [Path(b) for b in sys.argv[4:]]
    nieuw = not uit.exists()
    gedaan = {}
    if not nieuw:
        with open(uit, newline="") as f:
            gedaan = {(r["bron"], int(r["k"]), r["opdracht"], r["soort"]): r for r in csv.DictReader(f)}
    with open(uit, "a", newline="") as f:
        w = csv.writer(f)
        if nieuw:
            w.writerow(["bron", "schema", "k", "mb", "opdracht", "soort", "seconden", "piek_mb", "exit"])
        for bron in bronnen:
            schema = re.search(r"FILE_SCHEMA\s*\(\s*\(\s*'([^']+)'", bron.read_text(encoding="latin-1")[:4000]).group(1)
            mb_bron = bron.stat().st_size / 1e6
            ks = sorted({max(1, round(d / mb_bron)) for d in DOEL_MB})
            vorige: dict[tuple, tuple] = {}  # (opdracht, soort) -> (mb, piek)
            for k in ks:
                for opdracht, soort in METINGEN:
                    verwacht = None
                    if (opdracht, soort) in vorige:
                        mb0, piek0 = vorige[(opdracht, soort)]
                        if piek0 is None:
                            continue
                        verwacht = piek0 * (k * mb_bron) / mb0
                        if verwacht > STOP_MB:
                            continue
                    r = gedaan.get((bron.name, k, opdracht, soort))
                    if r:
                        ok = r["exit"] in ("0", "1") and r["piek_mb"]
                        vorige[(opdracht, soort)] = (float(r["mb"]), float(r["piek_mb"]) if ok else None)
                        continue
                    alleen = verwacht is not None and verwacht >= ALLEEN_MB
                    if alleen and IN_EEN_PLEK:
                        print(f"{bron.name} k={k} {opdracht} {soort}: overgeslagen, vraagt --alleen", flush=True)
                        vorige[(opdracht, soort)] = (0, None)
                        continue
                    map_ = werk / f"{bron.stem}-k{k}-{soort}"
                    if not (map_ / "hun.ifc").exists():
                        subprocess.run(
                            [*([] if IN_EEN_PLEK else [ZWAAR]), sys.executable, "-I", str(HIER / "maak_invoer.py"), str(bron), str(k), soort,
                             str(map_)],
                            check=True, stdout=subprocess.DEVNULL,
                        )
                    mb = (map_ / "basis.ifc").stat().st_size / 1e6
                    if opdracht == "verschil":
                        args = ["verschil", str(map_ / "basis.ifc"), str(map_ / "ons.ifc")]
                    else:
                        args = ["voeg-samen", *(str(map_ / f"{z}.ifc") for z in ("basis", "ons", "hun")),
                                "-o", str(map_ / "uit.ifc")]
                    seconden, piek, code = meet(programma, args, map_ / f"tijd-{opdracht}.txt", alleen)
                    w.writerow([bron.name, schema, k, f"{mb:.1f}", opdracht, soort,
                                f"{seconden:.2f}" if seconden is not None else "",
                                f"{piek:.0f}" if piek is not None else "", code])
                    f.flush()
                    (map_ / "uit.ifc").unlink(missing_ok=True)
                    print(f"{bron.name} k={k} {mb:.1f} MB {opdracht} {soort}: {seconden} s, {piek} MB, exit {code}",
                          flush=True)
                    # Exit 0 = gelukt, 1 = conflict (verwacht bij soort conflict). Iets anders (bijv. 137 = de grens):
                    # niet verder opschalen.
                    vorige[(opdracht, soort)] = (mb, piek if code in (0, 1) else None)


if __name__ == "__main__":
    main()
