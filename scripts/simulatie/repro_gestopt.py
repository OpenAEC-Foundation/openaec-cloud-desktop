"""Reproductie van bekend punt 61: na kill -9 midden in een ronde wordt een bestand dat die ronde al ophaalde, in de
volgende ronde een conflictkopie met dezelfde inhoud.

Twee clients in een nieuwe ruimte op de proefbench, met de echte motor (het voorbeeld `sync`). A zet vijf nieuwe
notities op de server. B start een ronde; zodra het eerste van die bestanden in zijn map staat (inotify), stopt de
proef B met kill -9. Daarna een gewone ronde van B. Dan telt de proef op de server de conflictkopieën met dezelfde
inhoud als het origineel.

  bench.sh python repro_gestopt.py --werk <map>     (of via draai.sh)
Exitcode 0 als de proef liep; de uitkomst staat in de laatste regel (GEREPRODUCEERD of NIET GEREPRODUCEERD).
"""

import argparse
import ctypes
import hashlib
import os
import select
import shutil
import struct
import subprocess
import sys
import time
import types

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import simulatie as s  # noqa: E402

IN_CREATE, IN_MOVED_TO = 0x100, 0x80


def wacht_op_bestand(map_: str, voorvoegsel: str, proces: subprocess.Popen, tot: float) -> str | None:
	"""Geef de naam van het eerste bestand met `voorvoegsel` dat in `map_` verschijnt (of None)."""
	libc = ctypes.CDLL("libc.so.6", use_errno=True)
	fd = libc.inotify_init1(0)
	if fd < 0 or libc.inotify_add_watch(fd, map_.encode(), IN_CREATE | IN_MOVED_TO) < 0:
		raise OSError("inotify lukt niet")
	try:
		while time.time() < tot and proces.poll() is None:
			if not select.select([fd], [], [], 0.05)[0]:
				continue
			data, i = os.read(fd, 65536), 0
			while i < len(data):
				_, _, _, lengte = struct.unpack_from("iIII", data, i)
				naam = data[i + 16 : i + 16 + lengte].rstrip(b"\0").decode()
				i += 16 + lengte
				if naam.startswith(voorvoegsel):
					return naam
		return None
	finally:
		os.close(fd)


def main() -> int:
	p = argparse.ArgumentParser()
	p.add_argument("--werk", required=True)
	p.add_argument("--zaad", type=int, default=61)
	args = p.parse_args()
	shutil.rmtree(args.werk, ignore_errors=True)
	os.makedirs(args.werk)
	stand = os.path.join(args.werk, "chaos.txt")
	with open(stand, "w") as f:
		f.write("0")
	proxies = s.start_proxies(args.werk, stand, args.zaad)
	try:
		frappe = s.verbind()
		ruimte, sleutels = s.opzet(frappe, 2, args.zaad)
		opties = types.SimpleNamespace(zaad=args.zaad, beurten=0, kill=0, tegelijk=0, pauze=0)
		a, b = (s.Client(i, args.werk, sleutels[i], ruimte, opties) for i in range(2))
		time.sleep(0.5)
		s.sla_op(os.path.join(a.map, "notities/begin.txt"), b"begin\n", a.tmp)
		s.log("A:", a.ronde()["rapport"])
		s.log("B:", b.ronde()["rapport"])
		inhoud = {}
		for n in range(1, 6):
			pad = f"notities/nieuw-{n}.txt"
			inhoud[pad] = f"nieuw {n}\n".encode() * 50
			s.sla_op(os.path.join(a.map, pad), inhoud[pad], a.tmp)
		s.log("A zet vijf nieuwe notities op de server:", a.ronde()["rapport"])

		# B haalt op; kill -9 zodra het eerste nieuwe bestand er staat.
		env = {**os.environ, "OPENAEC_AUTORISATIE": f"token {b.token}"}
		proces = subprocess.Popen([s.SYNC, s.API, ruimte, b.map], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
		naam = wacht_op_bestand(os.path.join(b.map, "notities"), "nieuw-", proces, time.time() + 60)
		proces.kill()
		proces.wait()
		staan = sorted(n for n in os.listdir(os.path.join(b.map, "notities")) if n.startswith("nieuw-"))
		s.log(f"B gestopt met kill -9 na {naam!r}; in de map van B staan nu: {staan}")
		if not naam:
			s.log("NIET GEREPRODUCEERD: B zette geen nieuw bestand neer voor de kill")
			return 0
		s.log("B, ronde na de kill:", b.ronde()["rapport"])

		versies, bestanden, _ = s.serverstand(frappe, ruimte)
		s3 = s.boto3.client("s3", endpoint_url=s.S3, region_name="eu-central-1", aws_access_key_id="proef", aws_secret_access_key="proef")
		sleutel = {v.name: v.s3_sleutel for v in versies}
		kopieen = []
		for bf in bestanden:
			if "(conflict " not in bf.naam or bf.verwijderd_op:
				continue
			data = s3.get_object(Bucket=s.BUCKET, Key=sleutel[bf.huidige_versie_id])["Body"].read()
			origineel = next((p for p, d in inhoud.items() if d == data), None)
			kopieen.append((bf.pad, origineel))
			s.log(f"conflictkopie {bf.pad}: {'dezelfde inhoud als ' + origineel if origineel else 'andere inhoud'} "
				  f"(sha256 {hashlib.sha256(data).hexdigest()[:12]})")
		overbodig = [k for k, o in kopieen if o]
		if overbodig:
			s.log(f"GEREPRODUCEERD: {len(overbodig)} overbodige conflictkopieën na één ronde met kill -9: {overbodig}")
		else:
			s.log(f"NIET GEREPRODUCEERD: {len(kopieen)} conflictkopieën, geen met dezelfde inhoud")
		return 0
	finally:
		for p_ in proxies:
			p_.terminate()


if __name__ == "__main__":
	sys.exit(main())
