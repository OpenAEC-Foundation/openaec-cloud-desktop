"""Lastproef met honderden lichte clients (bewijs 2, stap D): echte HTTP-verzoeken naar de proefbench, zonder sync-motor,
in één proces met asyncio. Elke client is een eigen gebruiker met API-sleutel, lid van één ruimte.

Drie metingen, per aantal clients (--aantallen):
  schrijvers  iedereen schrijft hetzelfde bestand via vergelijk-en-schrijf, zoals de opslaan-lus: upload_start met de
              basis en bij_conflict "weiger", het deel naar S3, upload_klaar. Bij verouderde_basis leest de client de
              nieuwe versie (download, zoals "lees hun") en probeert opnieuw. Gemeten: pogingen per geslaagde
              opslag, wie verhongert (geen enkele opslag), wachttijd tot een opslag lukt, doorvoer.
  doorvoer    iedereen leest de wijzigingenlijst vanaf zijn cursor (zoals de app pollt), en 1 op 10 schrijft
              daarnaast een eigen bestand (geen conflict): verzoeken per seconde en wachttijden.
  lijst       de wijzigingenlijst groeit (regels er direct in de database bij, met commitnummer 0, zoals vastgelegd
              werk dat nog niemand las): hoe lang de veger over een achterstand doet, en wat wijzigingen() kost bij
              een lange lijst, ook voor een stille ruimte die met cursor 0 begint.

  bench.sh python last.py --aantallen 100,200,300 --duur 45 [--fasen doorvoer] --werk <map>
Per fase meet hij ook de CPU van gunicorn, moto en MariaDB (systemd-units van bench.sh, /proc), zodat te zien is of
de server of de lastgever de grens is.
Het verslag komt als JSON op stdout. Exitcode 0 als de meting liep (de grenzen staan in het verslag), 1 bij een fout
in de meting zelf.
"""

import argparse
import asyncio
import collections
import hashlib
import json
import os
import random
import resource
import statistics
import subprocess
import sys
import time

import httpx

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from simulatie import BUCKET, S3, log, verbind  # noqa: E402

API = "http://127.0.0.1:18200/api/method/openaec_storage.api."  # direct naar gunicorn, zonder proxy


def pct(waarden: list[float], p: float) -> float | None:
	if not waarden:
		return None
	w = sorted(waarden)
	return round(w[min(len(w) - 1, int(p / 100 * len(w)))], 3)


def cpu_server() -> dict:
	"""CPU-seconden tot nu toe van gunicorn en moto (de user-units van bench.sh) en van MariaDB (gedeeld met de
	eerste bench, dus ook werk van anderen telt mee)."""
	uit = {}
	for naam, unit in (("web", "bench-proef-web"), ("s3", "bench-proef-s3")):
		try:
			w = subprocess.run(["systemctl", "--user", "show", unit, "-p", "CPUUsageNSec", "--value"],
							   capture_output=True, text=True, timeout=10).stdout.strip()
			uit[naam] = int(w) / 1e9 if w.isdigit() else None
		except (OSError, subprocess.SubprocessError):
			uit[naam] = None
	try:
		pid = subprocess.run(["pgrep", "-o", "mariadbd"], capture_output=True, text=True, timeout=10).stdout.strip()
		velden = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
		uit["mariadb"] = (int(velden[11]) + int(velden[12])) / os.sysconf("SC_CLK_TCK")
	except (OSError, ValueError, IndexError, subprocess.SubprocessError):
		uit["mariadb"] = None
	uit["lastgever"] = resource.getrusage(resource.RUSAGE_SELF).ru_utime + resource.getrusage(resource.RUSAGE_SELF).ru_stime
	return uit


def cpu_verschil(voor: dict, na: dict, looptijd: float, verzoeken: int) -> dict:
	"""CPU per onderdeel tijdens een fase: seconden, gemiddeld aantal kernen bezig, ms per API-verzoek."""
	uit = {}
	for k in na:
		if voor.get(k) is None or na.get(k) is None:
			continue
		d = na[k] - voor[k]
		uit[k] = {"s": round(d, 1), "kernen": round(d / looptijd, 2), "ms_per_verzoek": round(1000 * d / max(1, verzoeken), 2)}
	uit["load_1min_na"] = round(os.getloadavg()[0], 1)
	return uit


def samenvatting(waarden: list[float]) -> dict:
	return {"n": len(waarden), "p50": pct(waarden, 50), "p95": pct(waarden, 95), "p99": pct(waarden, 99), "max": pct(waarden, 100)}


class Meter:
	def __init__(self):
		self.tijden = collections.defaultdict(list)
		self.status = collections.Counter()


class Client:
	def __init__(self, i: int, sleutel: str, ruimte: str, http: httpx.AsyncClient, meter: Meter):
		self.i, self.ruimte, self.http, self.meter = i, ruimte, http, meter
		self.kop = {"Authorization": f"token {sleutel}"}
		self.cursor = 0
		self.basis = None  # (bestand, volgnummer, versie_id)
		self.tel = collections.Counter()
		self.reeks, self.langste_reeks = 0, 0
		self.wachten = []  # seconden van de eerste poging tot een geslaagde opslag
		self.pogingen_per_opslag = []  # de opslaan-lus geeft na 5 pogingen op (LusOpties): dan een conflictkopie

	async def roep(self, methode: str, **velden):
		t = time.perf_counter()
		try:
			r = await self.http.post(API + methode, json=velden, headers=self.kop)
			status = r.status_code
			try:
				lichaam = r.json()
			except ValueError:
				lichaam = {}
		except httpx.HTTPError as e:
			status, lichaam = type(e).__name__, {}
		self.meter.tijden[methode].append(time.perf_counter() - t)
		self.meter.status[f"{methode} {status}"] += 1
		return status, lichaam

	async def put(self, url: str, inhoud: bytes) -> str | None:
		url = url.replace("127.0.0.1:18211", "127.0.0.1:18210")  # direct naar moto, niet door de proxy
		t = time.perf_counter()
		try:
			r = await self.http.put(url, content=inhoud, headers={"Content-Length": str(len(inhoud))})
			etag = r.headers.get("ETag") if r.status_code == 200 else None
		except httpx.HTTPError:
			etag = None
		self.meter.tijden["s3_put"].append(time.perf_counter() - t)
		return etag

	async def schrijf(self, naam: str, inhoud: bytes, basis=None) -> tuple[str, dict]:
		"""Eén poging van vergelijk-en-schrijf. Uitkomst: "ok", "weiger_start", "weiger_klaar" of "fout"."""
		velden = {"ruimte": self.ruimte, "naam": naam, "grootte": len(inhoud), "bron": "sync",
				  "sha256": hashlib.sha256(inhoud).hexdigest()}
		if basis:
			velden.update(basis_bestand=basis[0], basis_versie=basis[1], basis_versie_id=basis[2], bij_conflict="weiger")
		status, l = await self.roep("upload_start", **velden)
		if status != 200:
			return ("weiger_start" if l.get("foutcode") == "verouderde_basis" else "fout"), l
		start = l["message"]
		delen = []
		for deel in start["delen"]:
			blok = inhoud[(deel["nummer"] - 1) * start["deelgrootte"]: deel["nummer"] * start["deelgrootte"]]
			etag = await self.put(deel["url"], blok)
			if not etag:
				return "fout", {}
			delen.append({"nummer": deel["nummer"], "etag": etag})
		status, l = await self.roep("upload_klaar", upload=start["upload"], delen=delen)
		if status != 200:
			return ("weiger_klaar" if l.get("foutcode") == "verouderde_basis" else "fout"), l
		return "ok", l["message"]

	async def lees_huidige(self, bestand: str) -> None:
		"""Zoals "lees hun" in de lus: de versies, dan de inhoud van de huidige."""
		status, l = await self.roep("versies", bestand=bestand)
		if status != 200 or not l.get("message"):
			return
		v = l["message"][0]
		self.basis = (bestand, v["volgnummer"], v["versie_id"])
		status, d = await self.roep("download", bestand=bestand, versie_id=v["versie_id"])
		if status == 200:
			try:
				await self.http.get(d["message"]["url"].replace("127.0.0.1:18211", "127.0.0.1:18210"))
			except httpx.HTTPError:
				pass

	async def schrijver(self, bestand: str, naam: str, tot: float, denktijd: float, rng: random.Random) -> None:
		await self.lees_huidige(bestand)
		begin = time.perf_counter()
		n = poging = 0
		while time.perf_counter() < tot:
			n += 1
			poging += 1
			inhoud = f"client {self.i} opslag {n}\n".encode() * 64
			uitkomst, l = await self.schrijf(naam, inhoud, self.basis)
			self.tel["pogingen"] += 1
			self.tel[uitkomst] += 1
			if uitkomst == "ok":
				self.wachten.append(time.perf_counter() - begin)
				self.pogingen_per_opslag.append(poging)
				poging = 0
				self.basis = (l["bestand"], l["versie"], l["versie_id"])
				self.reeks = 0
				await asyncio.sleep(rng.uniform(0, denktijd))
				begin = time.perf_counter()
			elif uitkomst.startswith("weiger"):
				self.reeks += 1
				self.langste_reeks = max(self.langste_reeks, self.reeks)
				await self.lees_huidige(bestand)
			else:
				await asyncio.sleep(0.5)
				await self.lees_huidige(bestand)

	async def poller(self, tot: float, eigen: str | None, rng: random.Random) -> None:
		"""Lees de lijst vanaf de cursor; met `eigen` schrijft de client soms ook dat bestand (per fase een nieuwe naam:
		zonder basis op een bestaande naam maakt de server een conflictkopie)."""
		n = 0
		while time.perf_counter() < tot:
			if eigen and rng.random() < 0.3:
				n += 1
				uitkomst, l = await self.schrijf(eigen, f"{self.i} {n}\n".encode() * 32, self.basis)
				self.tel[f"eigen_{uitkomst}"] += 1
				if uitkomst == "ok":
					self.basis = (l["bestand"], l["versie"], l["versie_id"])
			status, l = await self.roep("wijzigingen", ruimte=self.ruimte, sinds=self.cursor)
			if status == 200:
				self.cursor = max(self.cursor, l["message"]["cursor"])
				self.tel["polls"] += 1


async def schrijvers(clients: list[Client], bestand: str, naam: str, duur: float, denktijd: float, zaad: int) -> dict:
	meter = clients[0].meter
	tot = time.perf_counter() + duur
	begin, cpu = time.perf_counter(), resource.getrusage(resource.RUSAGE_SELF).ru_utime
	await asyncio.gather(*(c.schrijver(bestand, naam, tot, denktijd, random.Random(f"{zaad}-{c.i}")) for c in clients))
	looptijd = time.perf_counter() - begin
	ok = [c.tel["ok"] for c in clients]
	pogingen = sum(c.tel["pogingen"] for c in clients)
	return {
		"clients": len(clients),
		"looptijd_s": round(looptijd, 1),
		"geslaagd": sum(ok),
		"pogingen": pogingen,
		"geslaagd_per_s": round(sum(ok) / looptijd, 2),
		"pogingen_per_geslaagde": round(pogingen / max(1, sum(ok)), 1),
		"geweigerd_bij_start": sum(c.tel["weiger_start"] for c in clients),
		"geweigerd_bij_klaar": sum(c.tel["weiger_klaar"] for c in clients),
		"fouten": sum(c.tel["fout"] for c in clients),
		"verhongerd": sum(1 for n in ok if n == 0),
		"opslagen_per_client": {"min": min(ok), "mediaan": statistics.median(ok), "max": max(ok)},
		"langste_reeks_weigeringen": max(c.langste_reeks for c in clients),
		"pogingen_per_opslag": samenvatting([p for c in clients for p in c.pogingen_per_opslag]),
		"aandeel_meer_dan_5_pogingen": round(
			sum(1 for c in clients for p in c.pogingen_per_opslag if p > 5) / max(1, sum(ok)), 3
		),
		"wachttijd_tot_opslag_s": samenvatting([w for c in clients for w in c.wachten]),
		"cpu_lastgever_s": round(resource.getrusage(resource.RUSAGE_SELF).ru_utime - cpu, 1),
		"verzoeken": dict(meter.status),
		"duur_per_methode_s": {m: samenvatting(t) for m, t in meter.tijden.items()},
	}


async def doorvoer(clients: list[Client], duur: float, zaad: int) -> dict:
	meter = clients[0].meter
	tot = time.perf_counter() + duur
	begin, cpu = time.perf_counter(), resource.getrusage(resource.RUSAGE_SELF).ru_utime
	await asyncio.gather(*(
		c.poller(tot, f"eigen-{len(clients)}-{c.i}.txt" if c.i % 10 == 0 else None, random.Random(f"{zaad}-p{c.i}"))
		for c in clients
	))
	looptijd = time.perf_counter() - begin
	polls = sum(c.tel["polls"] for c in clients)
	verzoeken = sum(meter.status.values())
	return {
		"clients": len(clients),
		"looptijd_s": round(looptijd, 1),
		"verzoeken_per_s": round(verzoeken / looptijd, 1),
		"polls_per_s": round(polls / looptijd, 1),
		"eigen_opslagen_ok": sum(c.tel["eigen_ok"] for c in clients),
		"eigen_opslagen_fout": sum(c.tel["eigen_fout"] + c.tel["eigen_weiger_start"] + c.tel["eigen_weiger_klaar"] for c in clients),
		"cpu_lastgever_s": round(resource.getrusage(resource.RUSAGE_SELF).ru_utime - cpu, 1),
		"verzoeken": dict(meter.status),
		"duur_per_methode_s": {m: samenvatting(t) for m, t in meter.tijden.items()},
	}


async def meet_lijst(c: Client, ruimte: str, stil: str, keer: int = 10) -> dict:
	"""Wat wijzigingen() kost: de eerste pagina vanaf 0, een lege pagina aan het eind (de gewone poll), de hele lijst
	vanaf 0 (een nieuwe client), en een stille ruimte vanaf 0."""
	uit = {}
	_, l = await c.roep("wijzigingen", ruimte=ruimte, sinds=0, limiet=1)
	eind = 0
	cursor, pagina_s, t0 = 0, [], time.perf_counter()
	while True:
		t = time.perf_counter()
		status, l = await c.roep("wijzigingen", ruimte=ruimte, sinds=cursor, limiet=1000)
		pagina_s.append(time.perf_counter() - t)
		if status != 200:
			uit["fout"] = f"wijzigingen: {status}"
			return uit
		cursor = l["message"]["cursor"]
		if not l["message"]["meer"]:
			break
	eind = cursor
	uit["hele_lijst_vanaf_0_s"] = round(time.perf_counter() - t0, 2)
	uit["paginas"] = len(pagina_s)
	for naam, velden in (("eerste_pagina", {"ruimte": ruimte, "sinds": 0, "limiet": 1000}),
						 ("lege_pagina_aan_eind", {"ruimte": ruimte, "sinds": eind}),
						 ("stille_ruimte_vanaf_0", {"ruimte": stil, "sinds": 0})):
		tijden = []
		for _ in range(keer):
			t = time.perf_counter()
			await c.roep("wijzigingen", **velden)
			tijden.append(time.perf_counter() - t)
		uit[naam] = samenvatting(tijden)
	return uit


def voeg_regels_toe(frappe, ruimte: str, doel: str, aantal: int) -> None:
	"""Regels direct in de wijzigingenlijst, met commitnummer 0: vastgelegd werk dat de veger nog moet nummeren."""
	while aantal > 0:
		stuk = min(aantal, 50_000)
		frappe.db.sql(
			f"""insert into `tabOpslagwijziging` (name, creation, modified, owner, modified_by, docstatus, idx, ruimte,
				soort, doel_doctype, doel, naam, commitnummer)
			select nextval(`opslagwijziging_id_seq`), now(6), now(6), 'Administrator', 'Administrator', 0, 0, %s,
				'gewijzigd', 'Opslagbestand', %s, 'last', 0 from seq_1_to_{stuk}""",
			(ruimte, doel),
		)
		frappe.db.commit()
		aantal -= stuk


async def lijst(frappe, c: Client, anderen: list[Client], ruimte: str, stil: str, doel: str, stappen: list[int]) -> list[dict]:
	uit = []
	for stap in stappen:
		frappe.db.rollback()
		nu = frappe.db.sql("select count(*) from `tabOpslagwijziging` where ruimte = %s", ruimte)[0][0]
		erbij = max(0, stap - nu)
		voeg_regels_toe(frappe, ruimte, doel, erbij)
		# De eerste lezing na de achterstand: de veger nummert alle regels met 0 in één keer. Intussen pollen andere
		# clients de stille ruimte (een poll per client): ook hun veger wacht op het tellerslot.
		t = time.perf_counter()
		klaar = asyncio.Event()

		async def eerste():
			r = await c.roep("wijzigingen", ruimte=ruimte, sinds=c.cursor)
			klaar.set()
			return r

		async def ander(a: Client):
			tijden, codes = [], collections.Counter()
			while not klaar.is_set():
				t0 = time.perf_counter()
				status, _ = await a.roep("wijzigingen", ruimte=stil, sinds=0)
				tijden.append(time.perf_counter() - t0)
				codes[str(status)] += 1
				await asyncio.sleep(0.5)
			return tijden, codes

		(status, _), *rest = await asyncio.gather(eerste(), *(ander(a) for a in anderen))
		veger = time.perf_counter() - t
		frappe.db.rollback()
		nul = frappe.db.sql("select count(*) from `tabOpslagwijziging` where commitnummer = 0")[0][0]
		totaal = frappe.db.sql("select count(*) from `tabOpslagwijziging`")[0][0]
		codes = sum((r[1] for r in rest), collections.Counter())
		regel = {"regels_in_ruimte": nu + erbij, "regels_op_site": totaal, "achterstand_voor_veger": erbij,
				 "eerste_lezing_s": round(veger, 2), "status": status, "na_veger_nog_nul": nul,
				 "polls_van_anderen_intussen": {**samenvatting([x for r in rest for x in r[0]]), "status": dict(codes)}}
		log(f"lijst: {regel}")
		if status == 200:
			regel.update(await meet_lijst(c, ruimte, stil))
		uit.append(regel)
		log(f"lijst: {json.dumps(regel)}")
	return uit


def opzet(frappe, n: int) -> tuple[str, str, list[str]]:
	from frappe.core.doctype.user.user import generate_keys

	sleutels = []
	# Een eerdere opzet (met frappe.flags.in_import) liet de standaardwaarden weg, ook enabled: zulke gebruikers krijgen
	# 401. Vóór de lus, zodat generate_keys hieronder elke gebruiker opnieuw opslaat (en zijn cache ververst).
	frappe.db.sql("update `tabUser` set enabled = 1 where name like 'last-%%@sim.localhost'")
	frappe.db.commit()
	for i in range(n):
		email = f"last-{i}@sim.localhost"
		if not frappe.db.exists("User", email):
			frappe.get_doc(
				{"doctype": "User", "email": email, "first_name": f"Last{i}", "send_welcome_email": 0, "user_type": "System User"}
			).insert(ignore_permissions=True)
		uit = generate_keys(email)
		sleutels.append(f"{uit['api_key']}:{uit['api_secret']}")
		if i % 50 == 49:
			frappe.db.commit()
	stempel = time.strftime("%Y%m%d-%H%M%S")
	ruimtes = []
	for naam in (f"Last {stempel}", f"Stil {stempel}"):
		ruimtes.append(
			frappe.get_doc(
				{"doctype": "Ruimte", "naam": naam, "soort": "project",
				 "leden": [{"gebruiker": f"last-{i}@sim.localhost", "rol": "schrijver"} for i in range(n)]}
			).insert(ignore_permissions=True).name
		)
	frappe.db.commit()
	frappe.clear_cache()
	import boto3

	s3 = boto3.client("s3", endpoint_url=S3, region_name="eu-central-1", aws_access_key_id="proef", aws_secret_access_key="proef")
	try:
		s3.create_bucket(Bucket=BUCKET, CreateBucketConfiguration={"LocationConstraint": "eu-central-1"})
	except s3.exceptions.BucketAlreadyOwnedByYou:
		pass
	return ruimtes[0], ruimtes[1], sleutels


async def hoofd(args) -> dict:
	aantallen = [int(a) for a in args.aantallen.split(",")]
	fasen = set(args.fasen.split(","))
	frappe = verbind()
	t = time.perf_counter()
	ruimte, stil, sleutels = opzet(frappe, max(aantallen))
	log(f"opzet: {max(aantallen)} gebruikers en ruimtes {ruimte}, {stil} in {time.perf_counter() - t:.0f} s")
	verslag = {"args": vars(args), "werkers": int(os.environ.get("WERKERS", 4)), "schrijvers": [], "doorvoer": []}
	limieten = httpx.Limits(max_connections=2000, max_keepalive_connections=0)
	async with httpx.AsyncClient(timeout=httpx.Timeout(180.0), limits=limieten) as http:
		eerste = Client(0, sleutels[0], ruimte, http, Meter())
		uitkomst, l = await eerste.schrijf("gedeeld.txt", b"begin\n")
		if uitkomst != "ok":
			raise RuntimeError(f"gedeeld.txt niet aan te maken: {uitkomst} {l}")
		bestand = l["bestand"]
		for n in aantallen:
			if "schrijvers" in fasen:
				meter = Meter()
				clients = [Client(i, sleutels[i], ruimte, http, meter) for i in range(n)]
				voor = cpu_server()
				r = await schrijvers(clients, bestand, "gedeeld.txt", args.duur, args.denktijd, args.zaad)
				r["cpu"] = cpu_verschil(voor, cpu_server(), r["looptijd_s"], sum(meter.status.values()))
				log(f"schrijvers {n}: {r['geslaagd']} geslaagd, {r['pogingen_per_geslaagde']} pogingen per opslag, "
					f"{r['verhongerd']} verhongerd, {r['fouten']} fouten; cpu {r['cpu']}")
				verslag["schrijvers"].append(r)
			if "doorvoer" in fasen:
				meter = Meter()
				clients = [Client(i, sleutels[i], ruimte, http, meter) for i in range(n)]
				voor = cpu_server()
				r = await doorvoer(clients, args.duur, args.zaad)
				r["cpu"] = cpu_verschil(voor, cpu_server(), r["looptijd_s"], sum(meter.status.values()))
				log(f"doorvoer {n}: {r['verzoeken_per_s']} verzoeken/s, {r['polls_per_s']} polls/s, "
					f"{r['eigen_opslagen_fout']} eigen opslagen fout; cpu {r['cpu']}")
				verslag["doorvoer"].append(r)
		if args.lijst and "lijst" in fasen:
			c = Client(0, sleutels[0], ruimte, http, Meter())
			anderen = [Client(i, sleutels[i], stil, http, Meter()) for i in range(1, min(len(sleutels), 21))]
			try:
				verslag["lijst"] = await lijst(frappe, c, anderen, ruimte, stil, bestand, [int(s) for s in args.lijst.split(",")])
			finally:
				# De proefregels weer weg: anders moet elke volgende lezing op deze site ze nog nummeren.
				frappe.db.rollback()
				frappe.db.sql("delete from `tabOpslagwijziging` where naam = 'last' and doel = %s", bestand)
				frappe.db.commit()
	return verslag


def main() -> int:
	p = argparse.ArgumentParser()
	p.add_argument("--aantallen", default="100,200,300")
	p.add_argument("--duur", type=float, default=45)
	p.add_argument("--denktijd", type=float, default=1.0, help="na een geslaagde opslag hoogstens zoveel s wachten")
	p.add_argument("--lijst", default="10000,50000,200000", help="stappen voor de lengte van de wijzigingenlijst")
	p.add_argument("--fasen", default="schrijvers,doorvoer,lijst", help="welke metingen (komma's ertussen)")
	p.add_argument("--zaad", type=int, default=1)
	p.add_argument("--werk", required=True)
	args = p.parse_args()
	os.makedirs(args.werk, exist_ok=True)
	verslag = asyncio.run(hoofd(args))
	print(json.dumps(verslag, indent=1, default=str))
	return 0


if __name__ == "__main__":
	sys.exit(main())
