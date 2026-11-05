"""Simulatie met 5 tot 10 sync-clients tegelijk (bewijs 2, stap B) en de chaosproef (stap C).

Elke client is de echte motor van openaec-sync (het voorbeeld `sync`: één ronde per proces), met een eigen map en een
eigen gebruiker met API-sleutel, in één ruimte op de proefbench (bench.sh). Al het verkeer van de clients gaat door
proxy.py: naar de API (poort 18201) en naar S3 (18211, het adres in de ondertekende links). Met --chaos verbreekt,
vertraagt of reset de proxy verbindingen; met --kill stopt de simulatie een ronde soms met kill -9.

Elke client doet beurten (met een vast zaad per client): een IFC-object hernoemen in het model of de planning, een
regel toevoegen aan een notitie, het grote bestand wijzigen, een nieuw bestand, hernoemen, wissen; dan een
sync-ronde, soms met een opslag tijdens die ronde. Zoals de app leest hij vóór elke beurt de wijzigingenlijst vanaf
zijn cursor. Daarna een rustige periode zonder chaos: een client draait alleen een ronde als zijn cursor iets nieuws
ziet of als zijn vorige ronde nog werk had, tot alles stil staat.

Controles op de server (database en S3 direct) en lokaal:
  verloren_werk     elke opslag die een ronde zag, staat in een serverversie, een conflictkopie of .openaec/eigen/
  gelijk            elke client heeft dezelfde bestanden met dezelfde inhoud als de server
  s3                geen versie zonder S3-object, geen S3-object zonder versie
  wijzigingenlijst  elke client kreeg elke regel van de ruimte precies één keer, in volgorde
Tellingen: rondes, samenvoegingen, conflictkopieën met reden, weigeringen, pogingen, chaos.

  bench.sh python simulatie.py --clients 8 --beurten 20 --zaad 1 [--chaos 0.08] [--kill 0.15] --werk <map>
Exitcode 0 als elke controle slaagt, anders 1.
"""

import argparse
import collections
import hashlib
import json
import os
import random
import re
import shutil
import subprocess
import sys
import threading
import time

import boto3
import requests

HIER = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HIER))
SYNC = os.path.join(REPO, "crates/openaec-sync/target/debug/examples/sync")
SITES = os.path.join(os.environ.get("BENCH_PROEF", "/home/aia09/bench-proef"), "frappe-bench/sites")
SITE = "sim.localhost"
API_POORT, S3_POORT, API_PROXY, S3_PROXY = 18200, 18210, 18201, 18211
API = f"http://127.0.0.1:{API_PROXY}"
S3 = f"http://127.0.0.1:{S3_POORT}"  # de controles lezen S3 direct, niet via de proxy
BUCKET = "sim"
MODEL = os.path.join(REPO, "crates/openaec-samenvoegen/tests/bestanden/ifc2x3-building-architecture.ifc")
PLANNING = "/home/aia09/projects/open-planner-studio/examples/03-kantoorgebouw-zuidas.ifc"
ORIGINEEL = {"model/gebouw.ifc", "planning/zuidas.ifc", "bijlagen/scan.bin"}
GROOT = 2_500_000  # groter dan 1 MiB: gaat niet in één verzoek, maar in delen naar S3

MARKER = re.compile(rb"c\d+b\d+x\d+")
# Een object in IFC (een subtype van IfcRoot): GlobalId, OwnerHistory, dan Name. De simulatie wijzigt de Name.
OBJECT = re.compile(r"^(#\d+=(IFC[A-Z0-9]+)\('[0-9A-Za-z_$]{22}',(?:#\d+|\$),)('(?:[^']|'')*'|\$)(.*)$")
POOL = 40  # de eerste 40 objecten per bestand: klein genoeg dat twee clients soms hetzelfde object raken


def log(*delen):
	print(time.strftime("%H:%M:%S"), *delen, flush=True)


def is_extra(pad: str) -> bool:
	return "(conflict " in pad or pad.endswith(".samenvoegrapport.json")


def bestanden(root: str) -> dict[str, str]:
	"""Alle bestanden onder root (zonder .openaec/), met '/' als scheiding, en hun sha256."""
	uit = {}
	for map_, submappen, namen in os.walk(root):
		if map_ == root:
			submappen[:] = [m for m in submappen if m != ".openaec"]
		for naam in namen:
			pad = os.path.join(map_, naam)
			with open(pad, "rb") as f:
				uit[os.path.relpath(pad, root).replace(os.sep, "/")] = hashlib.sha256(f.read()).hexdigest()
	return uit


def sla_op(pad: str, inhoud: bytes, tmp: str) -> None:
	"""Zoals een programma opslaat: de hele inhoud naar een tijdelijk bestand buiten de map, dan op de plek."""
	os.makedirs(os.path.dirname(pad), exist_ok=True)
	t = os.path.join(tmp, f"opslaan-{threading.get_ident()}")
	with open(t, "wb") as f:
		f.write(inhoud)
	os.replace(t, pad)


class Client:
	def __init__(self, i: int, werk: str, sleutel: str, ruimte: str, args):
		self.i, self.ruimte, self.args = i, ruimte, args
		self.map = os.path.join(werk, f"client-{i}")
		self.tmp = os.path.join(werk, f"tmp-{i}")
		self.log = open(os.path.join(werk, f"client-{i}.log"), "a", buffering=1)
		os.makedirs(self.map, exist_ok=True)
		os.makedirs(self.tmp, exist_ok=True)
		self.token = sleutel
		self.rng = random.Random(f"{args.zaad}-{i}")
		self.http = requests.Session()
		self.http.headers["Authorization"] = f"token {sleutel}"
		self.cursor, self.gezien = 0, []
		self.open, self.vereist = [], []  # opslagen die nog geen ronde zag, en opslagen die een ronde zag
		self.teller, self.beurt = 0, 0
		self.rondes = []
		self.laatste = None
		self.bezig = False  # draait er nu een ronde (dan is een opslag meteen vereist)

	# --- de wijzigingenlijst, zoals de app hem leest ------------------------------------------------------------------

	def lees_wijzigingen(self) -> bool:
		nieuw = False
		while True:
			try:
				r = self.http.post(
					f"{API}/api/method/openaec_storage.api.wijzigingen",
					json={"ruimte": self.ruimte, "sinds": self.cursor},
					timeout=60,
				)
				m = r.json()["message"] if r.status_code == 200 else None
			except (requests.RequestException, ValueError, KeyError):
				m = None
			if m is None:
				return nieuw  # de cursor blijft staan; de volgende keer opnieuw
			for w in m["wijzigingen"]:
				self.gezien.append(w["volgnummer"])
				nieuw = True
			self.cursor = max(self.cursor, m["cursor"])
			if not m["meer"]:
				return nieuw

	# --- wat een gebruiker doet ---------------------------------------------------------------------------------------

	def marker(self) -> str:
		self.teller += 1
		return f"c{self.i}b{self.beurt}x{self.teller}"

	def opgeslagen(self, pad: str, marker: str, soort: str) -> None:
		opslag = {"client": self.i, "beurt": self.beurt, "pad": pad, "marker": marker, "soort": soort, "t": time.time()}
		# Tijdens een ronde: alleen de sync kan dit nog weghalen, dus meteen vereist.
		(self.vereist if self.bezig else self.open).append(opslag)
		self.log.write(f"opslag {json.dumps(opslag)}\n")

	def lokaal(self, filter_) -> list[str]:
		return sorted(p for p in bestanden(self.map) if filter_(p) and not is_extra(p))

	def wijzig_ifc(self) -> None:
		kandidaten = self.lokaal(lambda p: p.endswith(".ifc"))
		if not kandidaten:
			return
		pad = self.rng.choice(kandidaten)
		vol = os.path.join(self.map, pad)
		try:
			with open(vol, encoding="utf-8", newline="") as f:
				regels = f.read().split("\n")
		except FileNotFoundError:
			return
		plekken = [n for n, r in enumerate(regels) if (m := OBJECT.match(r.rstrip("\r"))) and not m.group(2).startswith("IFCREL")]
		if not plekken:
			return
		n = self.rng.choice(plekken[:POOL])
		m = OBJECT.match(regels[n].rstrip("\r"))
		marker = self.marker()
		regels[n] = m.group(1) + f"'{marker}'" + m.group(4) + ("\r" if regels[n].endswith("\r") else "")
		sla_op(vol, "\n".join(regels).encode(), self.tmp)
		self.opgeslagen(pad, marker, "ifc")

	def wijzig_tekst(self) -> None:
		kandidaten = self.lokaal(lambda p: p.endswith(".txt"))
		if not kandidaten:
			return self.nieuw()
		pad = self.rng.choice(kandidaten)
		vol = os.path.join(self.map, pad)
		try:
			with open(vol, "rb") as f:
				oud = f.read()
		except FileNotFoundError:
			return
		marker = self.marker()
		sla_op(vol, oud + f"{marker}\n".encode(), self.tmp)
		self.opgeslagen(pad, marker, "tekst")

	def wijzig_groot(self) -> None:
		vol = os.path.join(self.map, "bijlagen/scan.bin")
		if not os.path.exists(vol):
			return
		marker = self.marker()
		vul = random.Random(marker).randbytes(GROOT)
		sla_op(vol, f"{marker}\n".encode() + vul, self.tmp)
		self.opgeslagen("bijlagen/scan.bin", marker, "groot")

	def nieuw(self) -> None:
		marker = self.marker()
		if self.rng.random() < 0.2 and (ifc := self.lokaal(lambda p: p.endswith(".ifc"))):
			# Een nieuw IFC-bestand: een kopie van een bestaand, met een gewijzigd object.
			bron = self.rng.choice(ifc)
			try:
				with open(os.path.join(self.map, bron), encoding="utf-8", newline="") as f:
					tekst = f.read()
			except FileNotFoundError:
				return
			tekst = re.sub(r"(#\d+=IFCPROJECT\('[^']{22}',(?:#\d+|\$),)('(?:[^']|'')*'|\$)", lambda m: m.group(1) + f"'{marker}'", tekst, count=1)
			pad = f"model/kopie-{marker}.ifc"
			sla_op(os.path.join(self.map, pad), tekst.encode(), self.tmp)
			soort = "ifc"
		else:
			pad = f"notities/n-{marker}.txt"
			sla_op(os.path.join(self.map, pad), f"{marker}\n".encode(), self.tmp)
			soort = "tekst"
		self.opgeslagen(pad, marker, soort)

	def hernoem(self) -> None:
		kandidaten = self.lokaal(lambda p: p not in ORIGINEEL)
		if not kandidaten:
			return
		pad = self.rng.choice(kandidaten)
		stam, ext = os.path.splitext(pad)
		self.teller += 1
		nieuw = f"{stam}-h{self.i}.{self.teller}{ext}"
		try:
			os.rename(os.path.join(self.map, pad), os.path.join(self.map, nieuw))
		except FileNotFoundError:
			return
		for o in self.open:
			if o["pad"] == pad:
				o["pad"] = nieuw
		self.log.write(f"hernoem {pad} -> {nieuw}\n")

	def wis(self) -> None:
		kandidaten = self.lokaal(lambda p: p not in ORIGINEEL)
		if not kandidaten:
			return
		pad = self.rng.choice(kandidaten)
		try:
			os.remove(os.path.join(self.map, pad))
		except FileNotFoundError:
			return
		self.log.write(f"wis {pad}\n")

	ACTIES = [("wijzig_ifc", 35), ("wijzig_tekst", 20), ("nieuw", 12), ("hernoem", 8), ("wis", 7), ("wijzig_groot", 5), ("niets", 13)]

	def doe_iets(self) -> None:
		namen, gewichten = zip(*self.ACTIES)
		for _ in range(self.rng.randint(1, 3)):
			actie = self.rng.choices(namen, gewichten)[0]
			if actie != "niets":
				getattr(self, actie)()

	# --- een sync-ronde -----------------------------------------------------------------------------------------------

	def beoordeel_open(self) -> None:
		"""Bij de start van een ronde: elke opslag waarvan de marker nog in het bestand staat, is werk dat de sync moet
		bewaren. Is hij weg (de gebruiker wiste of overschreef het zelf), dan telt hij niet."""
		for o in self.open:
			try:
				with open(os.path.join(self.map, o["pad"]), "rb") as f:
					if o["marker"].encode() in f.read():
						self.vereist.append(o)
			except FileNotFoundError:
				pass
		self.open = []

	def ronde(self, kill: bool = False, tegelijk: bool = False) -> dict:
		self.beoordeel_open()
		n = len(self.rondes)
		uit, err = os.path.join(self.tmp, f"ronde-{n}.out"), os.path.join(self.tmp, f"ronde-{n}.err")
		start = time.time()
		with open(uit, "wb") as fo, open(err, "wb") as fe:
			env = {**os.environ, "OPENAEC_AUTORISATIE": f"token {self.token}"}
			p = subprocess.Popen([SYNC, API, self.ruimte, self.map], stdout=fo, stderr=fe, env=env)
			self.bezig = True
			gedood = False
			try:
				if tegelijk:
					time.sleep(self.rng.uniform(0.0, 1.0))
					if p.poll() is None:
						(self.wijzig_ifc if self.rng.random() < 0.7 else self.wijzig_tekst)()
				if kill:
					time.sleep(self.rng.uniform(0.05, 2.0))
					if p.poll() is None:
						p.kill()
						gedood = True
				code = p.wait(timeout=900)
			finally:
				self.bezig = False
		with open(uit, "rb") as f:
			stdout = f.read().decode(errors="replace").strip()
		with open(err, "rb") as f:
			stderr = f.read().decode(errors="replace")
		rapport = None
		if code == 0 and stdout:
			try:
				rapport = json.loads(stdout.splitlines()[-1])
			except ValueError:
				pass
		werk = bool(
			rapport is None
			or rapport["geupload"] + rapport["gedownload"] + rapport["verwijderd"] + rapport["conflicten"]
			or rapport["fouten"]
			or rapport["wachtend"]
		)
		r = {
			"client": self.i,
			"n": n,
			"start": round(start, 3),
			"duur": round(time.time() - start, 3),
			"code": code,
			"gedood": gedood,
			"tegelijk": tegelijk,
			"rapport": rapport,
			"werk": werk,
			"samengevoegd": stderr.count("samengevoegd met het werk van een ander"),
			"niet_samengevoegd": re.findall(r"niet samengevoegd \(([a-z_]+)\)", stderr),
		}
		self.rondes.append(r)
		self.laatste = r
		self.log.write(f"ronde {json.dumps(r)}\n")
		if stderr.strip():
			self.log.write("".join(f"  | {regel}\n" for regel in stderr.strip().splitlines()))
		return r

	def wacht_tot(self) -> float:
		"""Wanneer de motor een bestand dat steeds mislukte opnieuw probeert (0: geen wachtend bestand)."""
		w = (self.laatste or {}).get("rapport") or {}
		return min((m["opnieuw_om"] for m in w.get("wachtend", [])), default=0)

	def speel(self) -> None:
		for self.beurt in range(1, self.args.beurten + 1):
			self.lees_wijzigingen()
			self.doe_iets()
			kill = self.rng.random() < self.args.kill
			tegelijk = self.rng.random() < self.args.tegelijk
			self.ronde(kill=kill, tegelijk=tegelijk)
			time.sleep(self.rng.uniform(0, self.args.pauze))


# --- opzet ---------------------------------------------------------------------------------------------------------------


def verbind():
	import frappe

	os.chdir(SITES)
	frappe.init(site=SITE, sites_path=".")
	frappe.connect()
	frappe.set_user("Administrator")
	return frappe


def opzet(frappe, n: int, zaad: int) -> tuple[str, list[str]]:
	from frappe.core.doctype.user.user import generate_keys

	sleutels = []
	for i in range(n):
		email = f"sim-{i}@sim.localhost"
		if not frappe.db.exists("User", email):
			frappe.get_doc(
				{"doctype": "User", "email": email, "first_name": f"Sim{i}", "send_welcome_email": 0, "user_type": "System User"}
			).insert(ignore_permissions=True)
		uit = generate_keys(email) or {}
		sleutel = uit.get("api_key") or frappe.db.get_value("User", email, "api_key")
		sleutels.append(f"{sleutel}:{uit.get('api_secret')}")
	ruimte = frappe.get_doc(
		{
			"doctype": "Ruimte",
			"naam": f"Sim {zaad} {time.strftime('%Y%m%d-%H%M%S')}",
			"soort": "project",
			"leden": [{"gebruiker": f"sim-{i}@sim.localhost", "rol": "schrijver"} for i in range(n)],
		}
	).insert(ignore_permissions=True)
	frappe.db.commit()
	s3 = boto3.client("s3", endpoint_url=S3, region_name="eu-central-1", aws_access_key_id="proef", aws_secret_access_key="proef")
	try:
		s3.create_bucket(Bucket=BUCKET, CreateBucketConfiguration={"LocationConstraint": "eu-central-1"})
	except s3.exceptions.BucketAlreadyOwnedByYou:
		pass
	return ruimte.name, sleutels


def startbestanden(c: Client) -> None:
	with open(MODEL, "rb") as f:
		sla_op(os.path.join(c.map, "model/gebouw.ifc"), f.read(), c.tmp)
	with open(PLANNING, "rb") as f:
		sla_op(os.path.join(c.map, "planning/zuidas.ifc"), f.read(), c.tmp)
	for n in range(1, 4):
		marker = c.marker()
		sla_op(os.path.join(c.map, f"notities/notitie-{n}.txt"), f"{marker}\n".encode(), c.tmp)
		c.opgeslagen(f"notities/notitie-{n}.txt", marker, "tekst")
	c.wijzig_groot()


# --- de rustige periode --------------------------------------------------------------------------------------------------


def rustig(clients: list[Client], tot: float) -> bool:
	"""Zoals de app: een ronde als de cursor iets nieuws ziet of als de vorige ronde nog werk had. Klaar als twee keer
	achter elkaar geen client werk had en niemand op een wachtend bestand wacht."""
	stil = 0
	while time.time() < tot:
		werk = []

		def een(c: Client):
			nieuw = c.lees_wijzigingen()
			if nieuw or c.open or (c.laatste or {}).get("werk", True):
				r = c.ronde()
				if r["werk"]:
					werk.append(c.i)

		draden = [threading.Thread(target=een, args=(c,)) for c in clients]
		for d in draden:
			d.start()
		for d in draden:
			d.join()
		wachten = [c.wacht_tot() for c in clients if c.wacht_tot()]
		if not werk and not wachten:
			stil += 1
			if stil >= 2:
				return True
		else:
			stil = 0
		if wachten and not [i for i in werk if not clients[i].wacht_tot()]:
			slaap = max(1.0, min(min(wachten) - time.time() + 1, tot - time.time()))
			log(f"rustig: clients wachten op een nieuwe poging; {slaap:.0f} s wachten")
			time.sleep(max(0, slaap))
		else:
			time.sleep(1)
	return False


# --- controles ------------------------------------------------------------------------------------------------------------


def serverstand(frappe, ruimte: str):
	frappe.db.rollback()  # een nieuwe momentopname
	versies = frappe.db.sql(
		"""select v.name, v.bestand, v.volgnummer, v.s3_sleutel, v.sha256, v.samengevoegd_uit, v.maker,
			b.naam, b.map, b.verwijderd_op, b.huidige_versie_id
		from `tabBestandsversie` v join `tabOpslagbestand` b on b.name = v.bestand where v.ruimte = %s""",
		ruimte,
		as_dict=True,
	)
	mappen = {m.name: m for m in frappe.get_all("Opslagmap", filters={"ruimte": ruimte}, fields=["name", "naam", "bovenliggende"])}

	def mappad(m):
		delen = []
		while m:
			delen.insert(0, mappen[m].naam)
			m = mappen[m].bovenliggende
		return "/".join(delen)

	bestanden_ = frappe.get_all(
		"Opslagbestand", filters={"ruimte": ruimte}, fields=["name", "naam", "map", "verwijderd_op", "huidige_versie_id"]
	)
	for b in bestanden_:
		b.pad = "/".join(filter(None, [mappad(b.map), b.naam]))
	regels = frappe.db.sql(
		"select commitnummer, soort, doel, naam from `tabOpslagwijziging` where ruimte = %s order by commitnummer", ruimte, as_dict=True
	)
	return versies, bestanden_, regels


def controleer(frappe, ruimte: str, clients: list[Client]) -> dict:
	s3 = boto3.client("s3", endpoint_url=S3, region_name="eu-central-1", aws_access_key_id="proef", aws_secret_access_key="proef")
	versies, bestanden_, regels = serverstand(frappe, ruimte)
	uit = {"fouten": {}}

	# s3: geen versie zonder object, geen object zonder versie
	objecten = {}
	for pagina in s3.get_paginator("list_objects_v2").paginate(Bucket=BUCKET):
		for o in pagina.get("Contents", []):
			objecten[o["Key"]] = o["Size"]
	sleutels = {v.s3_sleutel for v in versies}
	zonder_object = sorted(sleutels - objecten.keys())
	# Een object zonder versie: geen enkele versie op de site wijst ernaar (moto begint per run leeg; een eerdere,
	# afgebroken run in een andere ruimte telt zo niet mee als hij zijn objecten wel bij een versie heeft).
	alle = {r[0] for r in frappe.db.sql("select s3_sleutel from `tabBestandsversie`")}
	zonder_versie = sorted(objecten.keys() - alle)
	onaf = len(s3.list_multipart_uploads(Bucket=BUCKET).get("Uploads", []))
	uit["s3"] = {"versies": len(versies), "objecten": len(objecten), "zonder_object": zonder_object,
				 "zonder_versie": zonder_versie, "onafgemaakte_multipart": onaf}
	if zonder_object or zonder_versie:
		uit["fouten"]["s3"] = f"{len(zonder_object)} versies zonder object, {len(zonder_versie)} objecten zonder versie"

	# Inhoud per S3-sleutel (één keer lezen), met de markers erin.
	inhoud_hash, markers_server = {}, collections.defaultdict(set)
	for v in versies:
		if v.s3_sleutel in inhoud_hash or v.s3_sleutel not in objecten:
			continue
		data = s3.get_object(Bucket=BUCKET, Key=v.s3_sleutel)["Body"].read()
		inhoud_hash[v.s3_sleutel] = hashlib.sha256(data).hexdigest()
		for m in set(MARKER.findall(data)):
			markers_server[m.decode()].add(v.naam)
	sha_fout = [v.name for v in versies if v.sha256 and v.s3_sleutel in inhoud_hash and v.sha256 != inhoud_hash[v.s3_sleutel]]
	if sha_fout:
		uit["fouten"]["sha256"] = f"{len(sha_fout)} versies met een andere inhoud dan hun sha256: {sha_fout[:5]}"

	# gelijk: de huidige bestanden op de server tegen elke lokale map
	versie_sleutel = {v.name: v.s3_sleutel for v in versies}
	server = {b.pad: inhoud_hash.get(versie_sleutel.get(b.huidige_versie_id)) for b in bestanden_ if not b.verwijderd_op}
	verschillen = {}
	for c in clients:
		lokaal = bestanden(c.map)
		anders = sorted(p for p in server.keys() | lokaal.keys() if server.get(p) != lokaal.get(p))
		if anders:
			verschillen[c.i] = [{"pad": p, "server": (server.get(p) or "-")[:12], "lokaal": (lokaal.get(p) or "-")[:12]} for p in anders[:10]]
	uit["gelijk"] = {"bestanden_op_server": len(server), "verschillen": verschillen}
	if verschillen:
		uit["fouten"]["gelijk"] = f"{len(verschillen)} clients wijken af van de server"

	# verloren werk
	eigen = collections.defaultdict(set)
	for c in clients:
		for map_, _, namen in os.walk(os.path.join(c.map, ".openaec", "eigen")):
			for naam in namen:
				with open(os.path.join(map_, naam), "rb") as f:
					for m in set(MARKER.findall(f.read())):
						eigen[m.decode()].add(c.i)
	vereist = [o for c in clients for o in c.vereist]
	weg = [o for o in vereist if o["marker"] not in markers_server and o["marker"] not in eigen]
	alleen_eigen = [o for o in vereist if o["marker"] not in markers_server and o["marker"] in eigen]
	uit["verloren_werk"] = {"opslagen_gezien": len(vereist), "op_server": len(vereist) - len(weg) - len(alleen_eigen),
							"alleen_in_eigen": [o["marker"] for o in alleen_eigen], "nergens": weg[:20], "aantal_nergens": len(weg)}
	if weg:
		uit["fouten"]["verloren_werk"] = f"{len(weg)} opslagen staan nergens"

	# wijzigingenlijst: elke client kreeg elke regel precies één keer, in volgorde
	nummers = [r.commitnummer for r in regels]
	nul = sum(1 for n in nummers if not n)
	per_client = {}
	for c in clients:
		gezien = c.gezien
		mist = sorted(set(n for n in nummers if n) - set(gezien))
		dubbel = len(gezien) - len(set(gezien))
		volgorde = all(a < b for a, b in zip(gezien, gezien[1:]))
		per_client[c.i] = {"gezien": len(gezien), "cursor": c.cursor, "mist": mist[:10], "aantal_mist": len(mist), "dubbel": dubbel, "op_volgorde": volgorde}
		if mist or dubbel or not volgorde:
			uit["fouten"].setdefault("wijzigingenlijst", []).append(f"client {c.i}: {len(mist)} gemist, {dubbel} dubbel, volgorde {volgorde}")
	uit["wijzigingenlijst"] = {"regels": len(nummers), "uniek": len(set(nummers)) == len(nummers), "zonder_nummer": nul, "per_client": per_client}
	if nul or len(set(nummers)) != len(nummers):
		uit["fouten"].setdefault("wijzigingenlijst", []).append(f"{nul} regels zonder commitnummer; uniek {len(set(nummers)) == len(nummers)}")

	# tellingen van de server
	per_bestand = collections.defaultdict(list)
	for v in versies:
		per_bestand[v.bestand].append(v)
	dubbele_versies = 0
	for vs in per_bestand.values():
		vs.sort(key=lambda v: v.volgnummer)
		dubbele_versies += sum(1 for a, b in zip(vs, vs[1:]) if inhoud_hash.get(a.s3_sleutel) and inhoud_hash.get(a.s3_sleutel) == inhoud_hash.get(b.s3_sleutel))
	uit["server"] = {
		"bestanden": len(bestanden_),
		"verwijderd": sum(1 for b in bestanden_ if b.verwijderd_op),
		"conflictkopieen": sum(1 for b in bestanden_ if "(conflict " in b.naam and not b.naam.endswith(".samenvoegrapport.json")),
		"samenvoegrapporten": sum(1 for b in bestanden_ if b.naam.endswith(".samenvoegrapport.json")),
		"versies_samengevoegd_uit": sum(1 for v in versies if v.samengevoegd_uit),
		"opeenvolgende_versies_met_gelijke_inhoud": dubbele_versies,
	}
	return uit


def tel_proxy(pad: str) -> dict:
	tel = collections.Counter()
	klaar_status, foutcodes, chaos = collections.Counter(), collections.Counter(), collections.Counter()
	try:
		with open(pad) as f:
			for regel in f:
				r = json.loads(regel)
				if (r.get("pad") or "").startswith("/"):
					r["pad"] = r.get("methode")  # S3: per methode (PUT is een deel, GET een download)
				tel[r.get("pad") or "?"] += 1
				if r.get("fout"):
					chaos[f"{r['fout']} {r.get('pad')}"] += 1
				if r.get("pad") == "upload_klaar":
					klaar_status[str(r.get("status"))] += 1
				for code in r.get("foutcodes") or []:
					foutcodes[f"{r.get('pad')} {code}"] += 1
	except FileNotFoundError:
		pass
	return {"verzoeken": dict(tel.most_common(25)), "upload_klaar_status": dict(klaar_status), "foutcodes": dict(foutcodes), "chaos": dict(chaos)}


def main() -> int:
	p = argparse.ArgumentParser()
	p.add_argument("--clients", type=int, default=8)
	p.add_argument("--beurten", type=int, default=20)
	p.add_argument("--zaad", type=int, default=1)
	p.add_argument("--chaos", type=float, default=0.0, help="kans per verbinding op een fout in de proxy")
	p.add_argument("--kill", type=float, default=0.0, help="kans per ronde op kill -9")
	p.add_argument("--tegelijk", type=float, default=0.15, help="kans per ronde op een opslag tijdens de ronde")
	p.add_argument("--pauze", type=float, default=1.5)
	p.add_argument("--rustig", type=float, default=1800, help="hoogstens zoveel seconden voor de rustige periode")
	p.add_argument("--werk", required=True)
	args = p.parse_args()

	shutil.rmtree(args.werk, ignore_errors=True)
	os.makedirs(args.werk)
	stand = os.path.join(args.werk, "chaos.txt")
	with open(stand, "w") as f:
		f.write("0")
	proxies = [
		subprocess.Popen([sys.executable, os.path.join(HIER, "proxy.py"), "--luister", str(luister), "--doel", str(doel),
						  "--log", os.path.join(args.werk, f"{naam}.jsonl"), "--stand", stand, "--zaad", str(args.zaad + k)])
		for k, (naam, luister, doel) in enumerate([("api", API_PROXY, API_POORT), ("s3", S3_PROXY, S3_POORT)])
	]
	try:
		frappe = verbind()
		ruimte, sleutels = opzet(frappe, args.clients, args.zaad)
		log(f"ruimte {ruimte}, {args.clients} clients, {args.beurten} beurten, zaad {args.zaad}, chaos {args.chaos}, kill {args.kill}")
		clients = [Client(i, args.werk, s, ruimte, args) for i, s in enumerate(sleutels)]
		time.sleep(0.5)

		startbestanden(clients[0])
		clients[0].ronde()
		for c in clients[1:]:
			c.ronde()
		log("startbestanden staan bij iedereen; nu de beurten")

		with open(stand, "w") as f:
			f.write(str(args.chaos))
		begin = time.time()
		draden = [threading.Thread(target=c.speel) for c in clients]
		for d in draden:
			d.start()
		for d in draden:
			d.join()
		with open(stand, "w") as f:
			f.write("0")
		log(f"beurten klaar in {time.time() - begin:.0f} s; nu de rustige periode, zonder chaos")

		stil = rustig(clients, time.time() + args.rustig)
		for c in clients:
			c.lees_wijzigingen()
		log(f"rustige periode: {'alles staat stil' if stil else 'NIET stil binnen de tijd'}")
		uitkomst = controleer(frappe, ruimte, clients)
		if not stil:
			uitkomst["fouten"]["rustig"] = "de clients kwamen niet tot stilstand"
	finally:
		for p_ in proxies:
			p_.terminate()

	rondes = [r for c in clients for r in c.rondes]
	redenen = collections.Counter(x for r in rondes for x in r["niet_samengevoegd"])
	uitkomst["rondes"] = {
		"totaal": len(rondes),
		"gedood": sum(r["gedood"] for r in rondes),
		"opslag_tijdens_ronde": sum(r["tegelijk"] for r in rondes),
		"exitcode_niet_0": sum(1 for r in rondes if r["code"] != 0 and not r["gedood"]),
		"met_fouten": sum(1 for r in rondes if (r["rapport"] or {}).get("fouten")),
		"samengevoegd": sum(r["samengevoegd"] for r in rondes),
		"niet_samengevoegd": dict(redenen),
		"conflicten_volgens_rapport": sum((r["rapport"] or {}).get("conflicten", 0) for r in rondes),
		"duur_gemiddeld_s": round(sum(r["duur"] for r in rondes) / max(1, len(rondes)), 2),
	}
	foutregels = collections.Counter()
	for r in rondes:
		for f_ in (r["rapport"] or {}).get("fouten", []):
			foutregels[re.sub(r"^[^:]*: ", "", f_)[:120]] += 1
	uitkomst["rondes"]["fouten_soorten"] = dict(foutregels.most_common(10))
	uitkomst["proxy_api"] = tel_proxy(os.path.join(args.werk, "api.jsonl"))
	uitkomst["proxy_s3"] = tel_proxy(os.path.join(args.werk, "s3.jsonl"))
	uitkomst["args"] = vars(args)
	print(json.dumps(uitkomst, indent=1, default=str))
	if uitkomst["fouten"]:
		log("CONTROLES GEFAALD: " + "; ".join(f"{k}: {v}" for k, v in uitkomst["fouten"].items()))
		return 1
	log("alle controles geslaagd")
	return 0


if __name__ == "__main__":
	sys.exit(main())
