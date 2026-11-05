"""Hulp bij scripts/proef-twee-clients.sh (stap 5b, ontwerp §5.6): opzet van de proefsite, een wijziging in een IFC-bestand
zoals een planner die opslaat, controles op de server via de API, en de racemeting (d).

Draait met de Python van de bench (frappe, requests en boto3 staan daar). Elke opdracht print één regel JSON of tekst;
een controle die faalt, eindigt met exitcode 1 en zegt waarom.

  opzet <sites-map> <site> <s3-adres>          gebruikers A en B met API-sleutel, ruimte "Proef", bucket in moto
  taak <bestand.ifc> <globalid>                naam en omschrijving van een IfcTask in een lokaal bestand
  wijzig <bestand.ifc> <globalid> <veld> <w>   veld "naam" of "omschrijving" van een IfcTask, opgeslagen als planner
  wacht-en-wijzig <map> <bestand.ifc> <globalid> <veld> <w>
                                               wacht tot de sync in <map>/.openaec/nieuw/ iets neerzet (hij schrijft dan
                                               naar de server) en wijzig dan het bestand: de gebruiker slaat op terwijl de
                                               sync samenvoegt
  server <adres> <sleutel> <ruimte> <opdracht> ...
      versies <pad>                            versies van een bestand (nieuwste eerst)
      taak <pad> <globalid> [versie_id]        naam en omschrijving van een IfcTask in de huidige (of die) versie
      paden                                    alle bestanden in de ruimte
      race <pad> <n>                           racemeting (d): n uploads tegelijk op dezelfde basis
"""

import ctypes
import json
import os
import re
import select
import sys
import threading

TAAK = re.compile(r"^(#\d+=IFCTASK\(')([^']*)(',[^,]*,)('(?:[^']|'')*'|\$)(,)('(?:[^']|'')*'|\$)(,.*)$")


def fout(melding: str) -> None:
	print(f"FOUT: {melding}")
	sys.exit(1)


# --- lokaal: een IFC-bestand lezen en wijzigen --------------------------------------------------------------------


def _tekst(waarde: str) -> str | None:
	return None if waarde == "$" else waarde[1:-1].replace("''", "'")


def _ifc(waarde: str) -> str:
	return "'" + waarde.replace("'", "''") + "'"


def taak_uit(inhoud: str, globalid: str) -> dict:
	for regel in inhoud.splitlines():
		m = TAAK.match(regel)
		if m and m.group(2) == globalid:
			return {"naam": _tekst(m.group(4)), "omschrijving": _tekst(m.group(6))}
	fout(f"geen IfcTask met GlobalId {globalid}")


def wijzig(pad: str, globalid: str, veld: str, waarde: str) -> None:
	"""Zoals een planner opslaat: de hele inhoud naar een tijdelijk bestand en dan op de plek (rename)."""
	with open(pad, encoding="utf-8", newline="") as f:
		regels = f.read().split("\n")
	gevonden = False
	for i, regel in enumerate(regels):
		m = TAAK.match(regel.rstrip("\r"))
		if not (m and m.group(2) == globalid):
			continue
		delen = list(m.groups())
		delen[3 if veld == "naam" else 5] = _ifc(waarde)
		regels[i] = "".join(delen) + ("\r" if regel.endswith("\r") else "")
		gevonden = True
	if not gevonden:
		fout(f"geen IfcTask met GlobalId {globalid} in {pad}")
	tijdelijk = pad + ".opslaan"
	with open(tijdelijk, "w", encoding="utf-8", newline="") as f:
		f.write("\n".join(regels))
	os.replace(tijdelijk, pad)


def wacht_en_wijzig(map_: str, pad: str, globalid: str, veld: str, waarde: str) -> None:
	"""inotify op <map>/.openaec/nieuw/: daar zet de sync de inhoud neer die hij naar de server schrijft (een vaste
	momentopname). Zodra daar een bestand verschijnt, slaat de 'gebruiker' op."""
	nieuw = os.path.join(map_, ".openaec", "nieuw")
	os.makedirs(nieuw, exist_ok=True)
	libc = ctypes.CDLL("libc.so.6", use_errno=True)
	fd = libc.inotify_init1(0)
	if fd < 0 or libc.inotify_add_watch(fd, nieuw.encode(), 0x100 | 0x80) < 0:  # IN_CREATE | IN_MOVED_TO
		fout("inotify lukt niet")
	print("wacht", flush=True)
	if not select.select([fd], [], [], 120)[0]:
		fout("de sync zette na 120 s nog niets in .openaec/nieuw/")
	os.read(fd, 4096)
	wijzig(pad, globalid, veld, waarde)
	print("opgeslagen tijdens het schrijven naar de server", flush=True)


# --- opzet ------------------------------------------------------------------------------------------------------------


def opzet(sites: str, site: str, s3: str) -> None:
	import boto3
	import frappe
	from frappe.core.doctype.user.user import generate_keys

	os.chdir(sites)
	frappe.init(site=site, sites_path=".")
	frappe.connect()
	frappe.set_user("Administrator")
	sleutels = {}
	for wie in ("a", "b"):
		email = f"proef-{wie}@proef.localhost"
		if not frappe.db.exists("User", email):
			frappe.get_doc(
				{"doctype": "User", "email": email, "first_name": f"Proef {wie.upper()}", "send_welcome_email": 0,
				 "user_type": "System User"}
			).insert(ignore_permissions=True)
		uit = generate_keys(email) or {}
		geheim = uit.get("api_secret")
		sleutel = uit.get("api_key") or frappe.db.get_value("User", email, "api_key")
		sleutels[wie] = f"{sleutel}:{geheim}"
	ruimte = frappe.get_doc(
		{
			"doctype": "Ruimte",
			"naam": "Proef",
			"soort": "project",
			"leden": [{"gebruiker": f"proef-{w}@proef.localhost", "rol": "schrijver"} for w in ("a", "b")],
		}
	).insert(ignore_permissions=True)
	frappe.db.commit()
	boto3.client(
		"s3", endpoint_url=s3, region_name="eu-central-1", aws_access_key_id="proef", aws_secret_access_key="proef"
	).create_bucket(Bucket="proef", CreateBucketConfiguration={"LocationConstraint": "eu-central-1"})
	print(json.dumps({"a": sleutels["a"], "b": sleutels["b"], "ruimte": ruimte.name}))


# --- op de server -----------------------------------------------------------------------------------------------------


class Api:
	def __init__(self, adres: str, sleutel: str, ruimte: str):
		import requests

		self.adres, self.ruimte = adres.rstrip("/"), ruimte
		self.http = requests.Session()
		self.http.headers["Authorization"] = f"token {sleutel}"

	def roep(self, methode: str, **velden):
		r = self.http.post(f"{self.adres}/api/method/openaec_storage.api.{methode}", json=velden, timeout=60)
		try:
			lichaam = r.json()
		except ValueError:
			lichaam = {"tekst": r.text[:300]}
		return r.status_code, lichaam

	def ok(self, methode: str, **velden):
		status, lichaam = self.roep(methode, **velden)
		if status != 200:
			fout(f"{methode}: HTTP {status} {lichaam.get('foutcode') or lichaam.get('exc_type') or lichaam}")
		return lichaam["message"]

	def bestand(self, pad: str) -> str:
		r = self.ok("zoek_pad", ruimte=self.ruimte, pad=pad, soort="bestand")
		if not r.get("gevonden"):
			fout(f"{pad} staat niet op de server")
		return r["bestand"]

	def inhoud(self, bestand: str, versie_id: str | None = None) -> bytes:
		link = self.ok("download", bestand=bestand, versie_id=versie_id)
		r = self.http.get(link["url"], headers={"Authorization": ""}, timeout=60)
		r.raise_for_status()
		return r.content

	def paden(self) -> list[str]:
		boom = self.ok("boom", ruimte=self.ruimte)
		mappen = {m["name"]: m for m in boom["mappen"]}

		def pad(map_):
			delen = []
			while map_:
				delen.insert(0, mappen[map_]["naam"])
				map_ = mappen[map_].get("bovenliggende")
			return "/".join(delen)

		return sorted("/".join(filter(None, [pad(b.get("map")), b["naam"]])) for b in boom["bestanden"])


def race(api: Api, pad: str, n: int) -> None:
	"""n uploads tegelijk, alle op dezelfde basis (de huidige versie), om en om met bij_conflict "weiger" en "kopie".
	Verwacht: precies één wordt de nieuwe versie; de andere zijn geweigerd (verouderde_basis) of een conflictkopie met
	hun eigen inhoud; er verdwijnt geen versie."""
	import hashlib

	bestand = api.bestand(pad)
	voor = api.ok("versies", bestand=bestand)
	basis = voor[0]
	origineel = api.inhoud(bestand).decode()
	gid = next(m.group(2) for m in map(TAAK.match, origineel.splitlines()) if m)
	map_ = api.ok("info", bestand=bestand).get("map")
	naam = pad.rsplit("/", 1)[-1]

	uploads = []
	for i in range(n):
		regels = origineel.split("\n")
		for j, regel in enumerate(regels):
			m = TAAK.match(regel.rstrip("\r"))
			if m and m.group(2) == gid:
				delen = list(m.groups())
				delen[5] = _ifc(f"race {i}")
				regels[j] = "".join(delen) + ("\r" if regel.endswith("\r") else "")
		inhoud = "\n".join(regels).encode()
		modus = "weiger" if i % 2 == 0 else "kopie"
		start = api.ok(
			"upload_start", ruimte=api.ruimte, naam=naam, map=map_, grootte=len(inhoud), bron="sync",
			basis_versie=basis["volgnummer"], basis_bestand=bestand, basis_versie_id=basis["versie_id"],
			bij_conflict=modus, sha256=hashlib.sha256(inhoud).hexdigest(),
		)
		etags = []
		for deel in start["delen"]:
			blok = inhoud[(deel["nummer"] - 1) * start["deelgrootte"] : deel["nummer"] * start["deelgrootte"]]
			r = api.http.put(deel["url"], data=blok, headers={"Authorization": ""}, timeout=60)
			r.raise_for_status()
			etags.append({"nummer": deel["nummer"], "etag": r.headers["ETag"]})
		uploads.append({"i": i, "modus": modus, "inhoud": inhoud, "upload": start["upload"], "delen": etags})

	# Alle uploads klaarmelden op hetzelfde moment.
	poort = threading.Barrier(n)
	uitkomsten = [None] * n

	def klaar(u):
		poort.wait()
		uitkomsten[u["i"]] = api.roep("upload_klaar", upload=u["upload"], delen=u["delen"])

	draden = [threading.Thread(target=klaar, args=(u,)) for u in uploads]
	for d in draden:
		d.start()
	for d in draden:
		d.join()

	winnaars, geweigerd, kopieen, anders = [], [], [], []
	for u, (status, lichaam) in zip(uploads, uitkomsten):
		r = lichaam.get("message") if status == 200 else None
		if r and not r["conflict"] and r["bestand"] == bestand:
			winnaars.append((u, r))
		elif r and r["conflict"] and u["modus"] == "kopie":
			kopieen.append((u, r))
		elif status == 409 and lichaam.get("foutcode") == "verouderde_basis" and u["modus"] == "weiger":
			geweigerd.append((u, lichaam))
		else:
			anders.append((u["i"], u["modus"], status, lichaam.get("foutcode") or lichaam.get("exc_type") or r))
	print(f"race: {n} uploads op basis versie {basis['volgnummer']}: {len(winnaars)} nieuwe versie, "
		  f"{len(geweigerd)} geweigerd (verouderde_basis), {len(kopieen)} conflictkopie, {len(anders)} anders")
	if anders:
		fout(f"onverwachte uitkomsten: {anders}")
	if len(winnaars) != 1:
		fout(f"verwacht precies één nieuwe versie, kreeg {len(winnaars)}")
	na = api.ok("versies", bestand=bestand)
	if len(na) != len(voor) + 1 or na[0]["versie_id"] != winnaars[0][1]["versie_id"]:
		fout(f"versies: voor {len(voor)}, na {len(na)}; verwacht er één bij, de winnaar bovenaan")
	if api.inhoud(bestand) != winnaars[0][0]["inhoud"]:
		fout("de huidige versie is niet de inhoud van de winnaar")
	for u, r in kopieen:
		if api.inhoud(r["bestand"]) != u["inhoud"]:
			fout(f"conflictkopie {r['naam']} heeft niet de inhoud van upload {u['i']}")
	for u, lichaam in geweigerd:
		if lichaam.get("huidige_versie_id") is None:
			fout(f"weigering van upload {u['i']} zonder huidige_versie_id")
	print(f"race: OK; winnaar upload {winnaars[0][0]['i']} ({winnaars[0][0]['modus']}) is versie "
		  f"{winnaars[0][1]['versie']}, geen versie verdwenen")


def server(adres: str, sleutel: str, ruimte: str, opdracht: str, *rest: str) -> None:
	api = Api(adres, sleutel, ruimte)
	if opdracht == "versies":
		bestand = api.bestand(rest[0])
		uit = [
			{k: v.get(k) for k in ("volgnummer", "versie_id", "maker", "samengevoegd_uit")}
			for v in api.ok("versies", bestand=bestand)
		]
		print(json.dumps({"bestand": bestand, "versies": uit}))
	elif opdracht == "taak":
		bestand = api.bestand(rest[0])
		print(json.dumps(taak_uit(api.inhoud(bestand, rest[2] if len(rest) > 2 else None).decode(), rest[1])))
	elif opdracht == "paden":
		print("\n".join(api.paden()))
	elif opdracht == "race":
		race(api, rest[0], int(rest[1]))
	else:
		fout(f"onbekende opdracht {opdracht}")


if __name__ == "__main__":
	opdracht, *args = sys.argv[1:]
	if opdracht == "opzet":
		opzet(*args)
	elif opdracht == "taak":
		with open(args[0], encoding="utf-8") as f:
			print(json.dumps(taak_uit(f.read(), args[1])))
	elif opdracht == "wijzig":
		wijzig(*args)
	elif opdracht == "wacht-en-wijzig":
		wacht_en_wijzig(*args)
	elif opdracht == "server":
		server(*args)
	else:
		fout(f"onbekende opdracht {opdracht}")
