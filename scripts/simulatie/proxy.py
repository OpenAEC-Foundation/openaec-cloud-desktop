"""Een kleine TCP-proxy voor de simulatie en de chaosproef (bewijs 2): tussen de sync-clients en de server (API of S3).

Per verbinding beslist hij met een vast zaad of er iets misgaat, en wat:
  knip   verbreek na een deel van het verzoek (midden in een upload, ook midden in een deel naar S3)
  stil   stuur het verzoek door, maar gooi het antwoord weg en verbreek: de server deed het, de client weet het niet
  traag  wacht 0,5 tot 3 s voordat het antwoord doorgaat
  reset  verbreek meteen, met een RST
De kans staat in het bestand --stand; hij leest het bij elke nieuwe verbinding. Zo zet de simulatie de chaos uit voor
de rustige periode. Per verbinding één regel JSON in --log: methode, pad, status, foutcodes, fout en bytes.

  proxy.py --luister 18201 --doel 18200 --log api.jsonl --stand chaos.txt --zaad 1
"""

import argparse
import asyncio
import json
import random
import re
import socket
import struct
import time

FOUTCODE = re.compile(rb'"foutcode":\s*"([a-z_]+)"')
LENGTE = re.compile(rb"\r\ncontent-length:\s*(\d+)", re.I)


def lees_stand(pad: str) -> float:
	try:
		with open(pad) as f:
			return float(f.read().strip() or 0)
	except (OSError, ValueError):
		return 0.0


def hard_dicht(schrijver: asyncio.StreamWriter) -> None:
	"""Verbreek met een RST (SO_LINGER 0), zoals een netwerk dat wegvalt; niet met een nette FIN."""
	sock = schrijver.get_extra_info("socket")
	if sock is not None:
		try:
			sock.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
		except OSError:
			pass
	schrijver.transport.abort()


class Proxy:
	def __init__(self, args):
		self.doel, self.stand, self.rng = args.doel, args.stand, random.Random(args.zaad)
		self.log = open(args.log, "a", buffering=1)

	def kies(self, pad: str) -> str | None:
		if self.rng.random() >= lees_stand(self.stand):
			return None
		if "upload_klaar" in pad:
			# Hier zit het lastigste geval: de server legt de versie vast, de client hoort het niet.
			return self.rng.choice(["stil", "stil", "knip", "traag", "reset"])
		return self.rng.choice(["knip", "stil", "traag", "reset"])

	async def verbinding(self, client_r: asyncio.StreamReader, client_w: asyncio.StreamWriter) -> None:
		begin = time.time()
		regel = {"t": round(begin, 3), "poort": client_w.get_extra_info("sockname")[1]}
		try:
			server_r, server_w = await asyncio.open_connection("127.0.0.1", self.doel)
		except OSError as e:
			regel["fout"] = f"doel: {e}"
			hard_dicht(client_w)
			self.schrijf(regel, begin)
			return
		eerste = await client_r.read(65536)
		kop = eerste.split(b"\r\n", 1)[0].decode("latin-1").split(" ")
		methode, pad = (kop + ["", ""])[:2]
		regel["methode"] = methode
		regel["pad"] = pad.split("?")[0].removeprefix("/api/method/openaec_storage.api.")
		fout = self.kies(pad) if eerste else None
		regel["fout"] = fout
		regel["in"] = regel["uit"] = 0
		if fout == "reset":
			hard_dicht(client_w)
			server_w.transport.abort()
			self.schrijf(regel, begin)
			return
		grens = None
		if fout == "knip":
			m = LENGTE.search(eerste)
			kop_eind = eerste.find(b"\r\n\r\n") + 4
			totaal = kop_eind + (int(m.group(1)) if m else 0)
			grens = self.rng.randint(max(1, kop_eind // 2), max(1, totaal - 1))
		antwoord = bytearray()

		async def heen():
			stuk = eerste
			while stuk:
				if grens is not None and regel["in"] + len(stuk) >= grens:
					server_w.write(stuk[: grens - regel["in"]])
					regel["in"] = grens
					raise ConnectionAbortedError("knip")
				server_w.write(stuk)
				regel["in"] += len(stuk)
				await server_w.drain()
				stuk = await client_r.read(65536)
			server_w.write_eof()

		async def terug():
			eerst = True
			while stuk := await server_r.read(65536):
				if len(antwoord) < 65536:
					antwoord.extend(stuk[: 65536 - len(antwoord)])
				if fout == "stil":
					# Het antwoord is er: de server heeft het verzoek verwerkt. Even doorlezen, dan de client afkappen.
					await asyncio.sleep(0.2)
					raise ConnectionAbortedError("stil")
				if fout == "traag" and eerst:
					await asyncio.sleep(self.rng.uniform(0.5, 3.0))
				eerst = False
				client_w.write(stuk)
				regel["uit"] += len(stuk)
				await client_w.drain()

		taken = [asyncio.ensure_future(heen()), asyncio.ensure_future(terug())]
		try:
			# Klaar als de server het antwoord gaf en sloot (gunicorn sluit na elk verzoek), of bij een fout. Sluit
			# alleen de client netjes zijn kant, dan nog wachten op het antwoord.
			klaar, _ = await asyncio.wait(taken, return_when=asyncio.FIRST_COMPLETED)
			if taken[0] in klaar and not taken[0].exception():
				await asyncio.wait([taken[1]])
			for t in taken:
				if t.done() and t.exception():
					raise t.exception()
		except (ConnectionError, OSError) as e:
			regel.setdefault("onderbroken", str(e))
		finally:
			for t in taken:
				t.cancel()
			await asyncio.gather(*taken, return_exceptions=True)
			if fout in ("knip", "stil"):
				hard_dicht(client_w)
				server_w.transport.abort()
			else:
				client_w.close()
				server_w.close()
		status = antwoord.split(b"\r\n", 1)[0].split(b" ")
		regel["status"] = int(status[1]) if len(status) > 1 and status[1].isdigit() else None
		regel["foutcodes"] = [c.decode() for c in FOUTCODE.findall(antwoord)]
		self.schrijf(regel, begin)

	def schrijf(self, regel: dict, begin: float) -> None:
		regel["duur"] = round(time.time() - begin, 3)
		self.log.write(json.dumps(regel) + "\n")


async def main():
	p = argparse.ArgumentParser()
	p.add_argument("--luister", type=int, required=True)
	p.add_argument("--doel", type=int, required=True)
	p.add_argument("--log", required=True)
	p.add_argument("--stand", required=True)
	p.add_argument("--zaad", type=int, default=1)
	args = p.parse_args()
	proxy = Proxy(args)
	server = await asyncio.start_server(proxy.verbinding, "127.0.0.1", args.luister, backlog=512)
	async with server:
		await server.serve_forever()


if __name__ == "__main__":
	asyncio.run(main())
