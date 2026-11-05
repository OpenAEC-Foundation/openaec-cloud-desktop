# Metingen: openaec-samenvoegen op grote modellen

Stand: 5 november 2026. Gemeten op commit `0655eda` van `crates/openaec-samenvoegen` (de tip van PR #7 bij de
start). Ontwerp: openaec-storage, `docs/samenwerken/ontwerp.md` §5.6 en §5.14.

## Samenvatting

De bibliotheek houdt alles in het geheugen: de bestanden zelf, en per versie een model met een rij en hashes per
instantie. Gemeten is tot 174 MB, op vijf kleine open modellen die ik heb vermenigvuldigd. Binnen die modellen groeit
het geheugen ongeveer lineair met de grootte. De opbouw van het bestand bepaalt hoe steil: per MB invoer vraagt
samenvoegen 4,4 MB bij een model met lange geometrieregels, 17 MB bij een dicht model, en 20 MB bij een planning van
OPS. Doorgetrokken (een schatting, geen meting) vraagt een dicht model van 1 GB zo'n 19 GB om samen te voegen en 9 GB
voor een verschil. Dat is geen bewezen grens voor echte modellen, maar wel een teken dat de huidige opzet niet tot
meerdere GB gaat. Een bestand van 4 GiB of meer kan de bibliotheek nooit lezen, en de opdrachtregel loopt dan eerst
het geheugen vol (`knownbugs.md`, punt 90, BLOKKEERT). De tijd is een kleiner probleem: samenvoegen kostte 3 tot 7 s
per 100 MB (bij een planning van OPS 19 s), en een derde daarvan is de zelfcontrole. De tijden zijn ruw: de machine
was druk en de bestandscache wisselde.

## Opzet

**Machine.** aia09-mp50: 8 kernen, 13 GB geheugen, Linux Mint 22.3. Op dezelfde machine draaiden tegelijk zo'n
16 andere agents, met builds en testruns (load 10 tot 50). Ook de bestandscache wisselde: `meet.py` maakt de invoer
vlak vóór de eerste meting, en hervatte metingen draaiden later op dezelfde bestanden. Zo las de ene meting de invoer
uit de cache en de andere van de schijf (bijv. `ifc2x3-infra-landscaping-k68-weinig`: 0 "File system inputs" bij
`verschil`, 526 104 bij `voeg-samen`). De tijden zijn daarom ruw: herhaal je een meting, dan kan hij 30 tot 50 %
anders uitkomen. Een vast protocol voor opwarmen en herhalen was er niet (`knownbugs.md`, punt 95). Het
piekgeheugen is wel stabiel (binnen 1 %).

**Grens.** Elke run had een harde grens van 2 GB (`systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0`).
Ging een run eroverheen, dan stopte systemd hem. Zo bleven de andere processen op de machine heel. Een stap waarvan de
piek (lineair doorgetrokken uit de vorige stap) boven 1,8 GB zou komen, is niet gemeten. Alles daarboven is een
schatting met de formules hieronder. Elke run ging via `zwaar.sh`; een run die 900 MB of meer verwachtte, of een
bestand van 1 GB of meer, via `zwaar.sh --alleen`.

**Wat gemeten is.** Tijd (wandklok) en piekgeheugen ("Maximum resident set size" van `/usr/bin/time -v`) van het
opdrachtregelprogramma, release-modus, één run per punt. Het geheugen staat in MiB (de kB van `time -v` gedeeld door
1024), de invoer in MB (10^6 bytes); het verschil is 5 %.

| Opdracht | Invoer (soort) | Wat ons en hun doen |
|---|---|---|
| `verschil basis ons` | weinig | ons wijzigt de Name van 0,1 % van de objecten |
| `verschil basis ons` | veel | ons wijzigt de Name van 10 % van de objecten |
| `voeg-samen` | weinig | ons en hun wijzigen elk een ander 0,1 % (geen conflict) |
| `voeg-samen` | veel | ons en hun wijzigen elk een ander 10 % (geen conflict) |
| `voeg-samen` | groot | ons als bij weinig; hun voegt een hele kopie van het bronmodel toe (één grote wijziging) |
| `voeg-samen` | conflict | ons en hun wijzigen dezelfde 0,1 % elk anders (elk een conflict) |

"Lezen" heeft geen eigen opdracht in de bibliotheek. Het zit in elke opdracht. De tijd en het geheugen van het
lezen alleen staan bij het profiel (fase `lezen`) en bij de grote bestanden (opdracht `tel` van de meetkopie).

**Invoer.** Een bronmodel, k keer vermenigvuldigd (`scripts/meting/maak_invoer.py`). Kopie 0 is de bron zelf. Elke
volgende kopie krijgt nieuwe nummers en nieuwe GlobalIds, en hangt onder het IfcProject van de bron. Zo groeit het
bestand ongeveer k keer, met dezelfde verhouding tussen objecten, knopen en bytes als de bron.

| Bron | Schema | Grootte | Instanties | Objecten | Bytes per instantie | Herkomst en licentie |
|---|---|---|---|---|---|---|
| `ifc2x3-infra-landscaping.ifc` | IFC2X3 | 1,2 MB | 27 419 | 185 | 43 | buildingSMART/Sample-Test-Files, `IFC 2.3.0.1 (IFC 2x3 TC1)/Simple-Scene/Infra-Landscaping.ifc`, CC BY 4.0 |
| `ifc4-infra-bridge.ifc` | IFC4 | 1,7 MB | 919 | 192 | 1 896 | idem, `IFC 4.0.2.1 (IFC 4 ADD2 TC1)/Simple-Scene/Infra-Bridge.ifc`, CC BY 4.0 |
| `ifc4-infra-landscaping.ifc` | IFC4 | 2,3 MB | 1 186 | 202 | 1 981 | idem, `IFC 4.0.2.1 (IFC 4 ADD2 TC1)/Simple-Scene/Infra-Landscaping.ifc`, CC BY 4.0 |
| `ifc4x3-infra-bridge.ifc` | IFC4X3_ADD2 | 1,7 MB | 902 | 175 | 1 929 | idem, `IFC 4.3.2.0 (IFC 4.3 ADD2)/Simple-Scene/Infra-Bridge.ifc`, CC BY 4.0 |
| `ops-13-supermarkt-albert-heijn.ifc` | IFC4X3_ADD2 | 29 kB | 292 | 165 | 98 | Open Planner Studio, `examples/`, LGPL-3.0 (al in `tests/bestanden/`) |

De modellen van buildingSMART komen van commit `80d976a9b193a26a8e928c3e79bff67af1de68a8`, © buildingSMART
International Ltd. `scripts/meting/haal_modellen.sh` haalt ze op.

Grotere echte modellen heb ik gezocht, maar niet gebruikt. In ThatOpen/engine_web-ifc (`tests/ifcfiles/public`,
tot 73 MB), opensourceBIM/TestFiles (tot 64 MB) en IfcOpenShell/files (tot 36 MB) staan grote modellen, maar zonder
licentie per model, en twee van de drie repo's hebben helemaal geen licentie. De opdracht vroeg een duidelijke
licentie.

Het dichte model (43 bytes per instantie) lijkt het meest op een export uit een BIM-programma: veel kleine regels met
punten, richtingen en plaatsingen. De brugmodellen hebben lange regels met getrianguleerde geometrie (bijna 2 kB per
instantie). Het verschil tussen de twee bepaalt het geheugen, zie hieronder.

## Resultaten

Eén run per punt. Per cel: de tijd, het piekgeheugen, en tussen haakjes het piekgeheugen per MB invoer. "-" is niet
gemeten: de stap zou boven 1,8 GB komen. De grootste stappen (vanaf 900 MB verwacht) zijn gemeten met
`zwaar.sh --alleen`, toen de machine rustiger was (load rond 3 in plaats van 30 tot 60).

#### ifc4-infra-bridge.ifc (IFC4, 192 objecten en 727 knopen per kopie)

| MB | instanties | verschil weinig | verschil veel | voeg-samen weinig | voeg-samen veel | voeg-samen groot | voeg-samen conflict |
|---|---|---|---|---|---|---|---|
| 1.7 | 919 | 0.04 s, 11 MB (6.5/MB) | 0.04 s, 11 MB (6.5/MB) | 0.07 s, 14 MB (8.2/MB) | 0.09 s, 14 MB (8.2/MB) | 0.16 s, 22 MB (12.9/MB) | 0.05 s, 13 MB (7.6/MB) |
| 5.2 | 2755 | 0.11 s, 18 MB (3.5/MB) | 0.12 s, 18 MB (3.5/MB) | 0.22 s, 29 MB (5.6/MB) | 0.19 s, 30 MB (5.8/MB) | 0.25 s, 36 MB (6.9/MB) | 0.13 s, 24 MB (4.6/MB) |
| 10.5 | 5509 | 0.19 s, 30 MB (2.9/MB) | 0.16 s, 29 MB (2.8/MB) | 0.48 s, 53 MB (5.0/MB) | 0.40 s, 53 MB (5.0/MB) | 0.52 s, 58 MB (5.5/MB) | 0.25 s, 41 MB (3.9/MB) |
| 19.2 | 10099 | 0.43 s, 48 MB (2.5/MB) | 0.39 s, 48 MB (2.5/MB) | 0.73 s, 91 MB (4.7/MB) | 0.96 s, 91 MB (4.7/MB) | 0.75 s, 96 MB (5.0/MB) | 0.44 s, 70 MB (3.6/MB) |
| 40.2 | 21115 | 0.60 s, 93 MB (2.3/MB) | 0.58 s, 94 MB (2.3/MB) | 1.49 s, 184 MB (4.6/MB) | 1.44 s, 183 MB (4.6/MB) | 1.93 s, 188 MB (4.7/MB) | 0.83 s, 140 MB (3.5/MB) |
| 80.4 | 42229 | 1.91 s, 181 MB (2.3/MB) | 2.45 s, 181 MB (2.3/MB) | 7.00 s, 361 MB (4.5/MB) | 4.54 s, 360 MB (4.5/MB) | 3.23 s, 366 MB (4.6/MB) | 1.93 s, 273 MB (3.4/MB) |
| 150.2 | 78949 | 3.10 s, 333 MB (2.2/MB) | 2.01 s, 335 MB (2.2/MB) | 11.80 s, 670 MB (4.5/MB) | 8.36 s, 670 MB (4.5/MB) | 5.75 s, 675 MB (4.5/MB) | 5.46 s, 506 MB (3.4/MB) |

#### ifc2x3-infra-landscaping.ifc (IFC2X3, 185 objecten en 27234 knopen per kopie)

| MB | instanties | verschil weinig | verschil veel | voeg-samen weinig | voeg-samen veel | voeg-samen groot | voeg-samen conflict |
|---|---|---|---|---|---|---|---|
| 1.2 | 27419 | 0.13 s, 15 MB (12.5/MB) | 0.05 s, 16 MB (13.3/MB) | 0.10 s, 26 MB (21.7/MB) | 0.10 s, 26 MB (21.7/MB) | 0.29 s, 43 MB (35.8/MB) | 0.07 s, 21 MB (17.5/MB) |
| 2.5 | 54837 | 0.09 s, 27 MB (10.8/MB) | 0.10 s, 27 MB (10.8/MB) | 0.22 s, 50 MB (20.0/MB) | 0.23 s, 50 MB (20.0/MB) | 0.44 s, 67 MB (26.8/MB) | 0.11 s, 38 MB (15.2/MB) |
| 5.0 | 109673 | 0.20 s, 50 MB (10.0/MB) | 0.20 s, 50 MB (10.0/MB) | 0.42 s, 96 MB (19.2/MB) | 0.57 s, 96 MB (19.2/MB) | 0.80 s, 114 MB (22.8/MB) | 0.22 s, 73 MB (14.6/MB) |
| 10.2 | 219345 | 0.42 s, 94 MB (9.2/MB) | 0.38 s, 95 MB (9.3/MB) | 0.87 s, 187 MB (18.3/MB) | 0.95 s, 187 MB (18.3/MB) | 1.55 s, 211 MB (20.7/MB) | 0.44 s, 140 MB (13.7/MB) |
| 22.3 | 466107 | 0.66 s, 197 MB (8.8/MB) | 0.64 s, 197 MB (8.8/MB) | 1.80 s, 397 MB (17.8/MB) | 1.87 s, 398 MB (17.8/MB) | 3.06 s, 430 MB (19.3/MB) | 0.85 s, 295 MB (13.2/MB) |
| 45.4 | 932213 | 1.32 s, 391 MB (8.6/MB) | 1.30 s, 392 MB (8.6/MB) | 3.57 s, 785 MB (17.3/MB) | 3.62 s, 792 MB (17.4/MB) | 5.46 s, 822 MB (18.1/MB) | 1.67 s, 590 MB (13.0/MB) |
| 91.7 | 1864425 | 2.50 s, 782 MB (8.5/MB) | 2.44 s, 782 MB (8.5/MB) | 6.23 s, 1578 MB (17.2/MB) | 6.51 s, 1565 MB (17.1/MB) | 9.27 s, 1606 MB (17.5/MB) | 2.64 s, 1179 MB (12.9/MB) |
| 174.0 | 3482087 | 4.60 s, 1462 MB (8.4/MB) | 4.38 s, 1464 MB (8.4/MB) | - | - | - | - |

#### ifc4-infra-landscaping.ifc (IFC4, 202 objecten en 984 knopen per kopie)

| MB | instanties | verschil weinig | verschil veel | voeg-samen weinig | voeg-samen veel | voeg-samen groot | voeg-samen conflict |
|---|---|---|---|---|---|---|---|
| 2.3 | 1186 | 0.05 s, 12 MB (5.2/MB) | 0.04 s, 12 MB (5.2/MB) | 0.08 s, 17 MB (7.4/MB) | 0.08 s, 16 MB (7.0/MB) | 0.17 s, 26 MB (11.3/MB) | 0.05 s, 14 MB (6.1/MB) |
| 4.7 | 2371 | 0.08 s, 17 MB (3.6/MB) | 0.07 s, 17 MB (3.6/MB) | 0.13 s, 27 MB (5.7/MB) | 0.14 s, 27 MB (5.7/MB) | 0.21 s, 36 MB (7.7/MB) | 0.08 s, 22 MB (4.7/MB) |
| 9.4 | 4741 | 0.13 s, 27 MB (2.9/MB) | 0.14 s, 26 MB (2.8/MB) | 0.27 s, 47 MB (5.0/MB) | 0.26 s, 47 MB (5.0/MB) | 0.35 s, 56 MB (6.0/MB) | 0.16 s, 37 MB (3.9/MB) |
| 21.2 | 10666 | 0.30 s, 52 MB (2.5/MB) | 0.29 s, 52 MB (2.5/MB) | 0.60 s, 99 MB (4.7/MB) | 9.08 s, 100 MB (4.7/MB) | 0.70 s, 106 MB (5.0/MB) | 0.33 s, 77 MB (3.6/MB) |
| 40.1 | 20146 | 0.53 s, 93 MB (2.3/MB) | 0.51 s, 93 MB (2.3/MB) | 1.10 s, 182 MB (4.5/MB) | 1.18 s, 182 MB (4.5/MB) | 1.23 s, 187 MB (4.7/MB) | 0.60 s, 139 MB (3.5/MB) |
| 80.1 | 40291 | 1.05 s, 179 MB (2.2/MB) | 1.14 s, 179 MB (2.2/MB) | 2.25 s, 357 MB (4.5/MB) | 3.84 s, 357 MB (4.5/MB) | 2.42 s, 363 MB (4.5/MB) | 2.43 s, 271 MB (3.4/MB) |
| 150.9 | 75841 | 1.99 s, 332 MB (2.2/MB) | 1.95 s, 333 MB (2.2/MB) | 4.32 s, 668 MB (4.4/MB) | 4.44 s, 668 MB (4.4/MB) | 4.54 s, 675 MB (4.5/MB) | 2.46 s, 506 MB (3.4/MB) |

#### ifc4x3-infra-bridge.ifc (IFC4X3_ADD2, 175 objecten en 727 knopen per kopie)

| MB | instanties | verschil weinig | verschil veel | voeg-samen weinig | voeg-samen veel | voeg-samen groot | voeg-samen conflict |
|---|---|---|---|---|---|---|---|
| 1.7 | 902 | 0.04 s, 11 MB (6.5/MB) | 0.04 s, 11 MB (6.5/MB) | 0.11 s, 15 MB (8.8/MB) | 0.06 s, 14 MB (8.2/MB) | 0.12 s, 21 MB (12.4/MB) | 0.05 s, 13 MB (7.6/MB) |
| 5.2 | 2704 | 0.09 s, 18 MB (3.5/MB) | 0.07 s, 18 MB (3.5/MB) | 0.16 s, 29 MB (5.6/MB) | 0.14 s, 29 MB (5.6/MB) | 0.22 s, 36 MB (6.9/MB) | 0.10 s, 24 MB (4.6/MB) |
| 10.4 | 5407 | 0.15 s, 30 MB (2.9/MB) | 0.14 s, 30 MB (2.9/MB) | 0.28 s, 52 MB (5.0/MB) | 0.29 s, 52 MB (5.0/MB) | 0.36 s, 58 MB (5.6/MB) | 0.16 s, 41 MB (3.9/MB) |
| 19.2 | 9912 | 0.25 s, 48 MB (2.5/MB) | 0.25 s, 48 MB (2.5/MB) | 0.52 s, 90 MB (4.7/MB) | 0.52 s, 90 MB (4.7/MB) | 0.59 s, 96 MB (5.0/MB) | 0.30 s, 70 MB (3.6/MB) |
| 40.1 | 20724 | 0.52 s, 93 MB (2.3/MB) | 0.51 s, 93 MB (2.3/MB) | 1.09 s, 182 MB (4.5/MB) | 4.64 s, 182 MB (4.5/MB) | 1.24 s, 186 MB (4.6/MB) | 0.66 s, 139 MB (3.5/MB) |
| 80.2 | 41447 | 1.11 s, 180 MB (2.2/MB) | 1.05 s, 181 MB (2.3/MB) | 2.29 s, 357 MB (4.5/MB) | 2.59 s, 357 MB (4.5/MB) | 2.92 s, 361 MB (4.5/MB) | 3.54 s, 271 MB (3.4/MB) |
| 150.0 | 77487 | 2.61 s, 332 MB (2.2/MB) | 2.49 s, 333 MB (2.2/MB) | 5.31 s, 664 MB (4.4/MB) | 5.54 s, 664 MB (4.4/MB) | 4.07 s, 668 MB (4.5/MB) | 9.36 s, 502 MB (3.3/MB) |

#### ops-13-supermarkt-albert-heijn.ifc (IFC4X3_ADD2, 165 objecten en 127 knopen per kopie)

| MB | instanties | verschil weinig | verschil veel | voeg-samen weinig | voeg-samen veel | voeg-samen groot | voeg-samen conflict |
|---|---|---|---|---|---|---|---|
| 1.1 | 10186 | 0.08 s, 16 MB (14.5/MB) | 0.07 s, 17 MB (15.5/MB) | 0.15 s, 28 MB (25.5/MB) | 0.18 s, 28 MB (25.5/MB) | 0.17 s, 28 MB (25.5/MB) | 0.12 s, 25 MB (22.7/MB) |
| 2.1 | 20371 | 0.14 s, 26 MB (12.4/MB) | 0.14 s, 27 MB (12.9/MB) | 0.30 s, 51 MB (24.3/MB) | 0.31 s, 51 MB (24.3/MB) | 0.32 s, 50 MB (23.8/MB) | 0.21 s, 44 MB (21.0/MB) |
| 5.4 | 50926 | 0.32 s, 62 MB (11.5/MB) | 0.36 s, 64 MB (11.9/MB) | 0.82 s, 126 MB (23.3/MB) | 0.84 s, 126 MB (23.3/MB) | 0.92 s, 127 MB (23.5/MB) | 0.50 s, 106 MB (19.6/MB) |
| 10.8 | 101560 | 0.63 s, 107 MB (9.9/MB) | 0.70 s, 112 MB (10.4/MB) | 1.58 s, 227 MB (21.0/MB) | 1.69 s, 228 MB (21.1/MB) | 1.80 s, 239 MB (22.1/MB) | 1.00 s, 193 MB (17.9/MB) |
| 21.7 | 203119 | 1.41 s, 209 MB (9.6/MB) | 1.53 s, 219 MB (10.1/MB) | 3.41 s, 449 MB (20.7/MB) | 3.80 s, 449 MB (20.7/MB) | 3.53 s, 449 MB (20.7/MB) | 2.48 s, 381 MB (17.6/MB) |
| 43.8 | 406528 | 2.78 s, 413 MB (9.4/MB) | 3.05 s, 432 MB (9.9/MB) | 7.27 s, 894 MB (20.4/MB) | 7.68 s, 895 MB (20.4/MB) | 7.52 s, 895 MB (20.4/MB) | 4.45 s, 758 MB (17.3/MB) |
| 88.4 | 813055 | 6.23 s, 818 MB (9.3/MB) | 6.90 s, 860 MB (9.7/MB) | 17.15 s, 1791 MB (20.3/MB) | 17.03 s, 1781 MB (20.1/MB) | 16.80 s, 1793 MB (20.3/MB) | 9.72 s, 1508 MB (17.1/MB) |
| 166.5 | 1524550 | 12.26 s, 1511 MB (9.1/MB) | 12.44 s, 1557 MB (9.4/MB) | - | - | - | - |

**Spreiding.** Drie keer dezelfde samenvoeging, met `zwaar.sh --alleen` en load 3: het brugmodel van 150 MB in 5,0,
6,8 en 6,3 s (piek 668 tot 670 MB), het dichte model van 45 MB in 3,5, 4,3 en 3,7 s (piek 787 tot 793 MB). Het
geheugen herhaalt zich dus binnen 1 %, de tijd binnen ongeveer 35 %. In de reeks staan een paar uitschieters
(bijv. 9,08 s voor 21 MB): die vielen samen met zware builds van andere agents.

**De soort wijziging maakt weinig uit.** Weinig of veel wijzigingen (0,1 of 10 % van de objecten) geven hetzelfde
geheugen en ongeveer dezelfde tijd (binnen de ruis van belasting en cache): de bibliotheek leest en hasht altijd alle drie de versies helemaal. Eén grote
wijziging (hun voegt een hele kopie toe) vraagt iets meer, omdat hun groter is. Een conflict is goedkoper: de
bibliotheek stopt vóór het schrijven en de zelfcontrole, dus zonder het vierde model.

**Lezen alleen** (de opdracht `tel` van de meetkopie: het bestand inlezen, en één keer `Model::lees`, dus ontleden en
indelen in objecten en knopen, zonder hashes). Opnieuw gemeten in fixronde 1 (5 november 2026, 17:15), onder één
plek van `zwaar.sh`; de logs staan in `groot/meting/tel-r1/`:

| Model | Grootte | Tijd | Piek | MiB per MB |
|---|---|---|---|---|
| dicht (IFC2X3) | 174 MB | 2,1 s | 481 MiB | 2,8 |
| planning (OPS) | 167 MB | 2,9 s | 547 MiB | 3,3 |
| brug (IFC4) | 150 MB | 1,0 s | 161 MiB | 1,1 |
| dicht, 1,2 GB | 1 208 MB | niet opnieuw gemeten: er draaiden andere zware runs | | |

De eerste versie van `tel` (tot `2ca0c40`) ontleedde het bestand twee keer: eerst los, en daarna nog eens in
`Model::lees`. Dat gaf 3,2, 3,5 en 1,4 s en 501, 547 en 163 MiB. De tijd was dus 20 tot 35 % te hoog; het
piekgeheugen nauwelijks, want de eerste ronde was al opgeruimd voordat de tweede begon. Het bestand zelf is ongeveer
1 MB per MB. Bij het dichte model kost het lezen daarbovenop ongeveer 95 bytes per instantie (de rij met de plek van
elke instantie, de attributen, de verwijzingen en de indeling); bij de planning en de brug meer per instantie, omdat
objecten meer kosten dan knopen.

## Groei

**Lineair.** Het piekgeheugen per MB daalt bij de kleine stappen (een vaste voet van een paar MB) en wordt daarna
constant: dicht 21,7 → 17,2 MB per MB voor samenvoegen van 1,2 tot 92 MB, brug 8,2 → 4,5 van 1,7 tot 150 MB. De
tijd per MB is ook ongeveer constant: dicht 0,085 → 0,068 s per MB, brug 0,04 tot 0,08 (ruis). Alleen de planning
van OPS groeit iets sneller dan lineair in tijd: 0,15 s per MB bij 11 MB, 0,19 bij 88 MB (vermoeden: de tabellen op
GlobalId, met 458 217 objecten en 354 838 knopen bij 88 MB; niet nagegaan). Ook deze tijden zijn ruw.

**Het schema, alleen voor de brug.** Hetzelfde brugmodel in IFC4 en in IFC4X3_ADD2 geeft binnen 1 % hetzelfde
geheugen, en dezelfde tijd binnen de ruis (150 MB: 664 tegen 670 MB). Een vergelijkbare proef met IFC2X3 (hetzelfde
model in IFC2X3 en IFC4) is niet gedaan. Tussen de bronnen telt vooral hoe het bestand is opgebouwd: veel kleine
instanties of weinig grote.

**Een formule voor het geheugen.** Het geheugen hangt af van drie dingen: de bytes (S), het aantal knopen (K) en het
aantal objecten (O). Per opdracht een fit met de kleinste kwadraten (`scripts/meting/tabel.py`): zes fits op elk 35
of 37 punten uit dezelfde vijf bronmodellen, samen 214 punten:

#### Fit: piek = a*S + b*K + c*O + d

| opdracht | punten | a (MB per MB) | b (bytes per knoop) | c (bytes per object) | d (MB) | grootste afwijking, punten boven 50 MB | grootste afwijking, alle punten |
|---|---|---|---|---|---|---|---|
| verschil weinig | 37 | 1.92 | 331 | 1196 | 7 | 8 % | 18.7 % |
| verschil veel | 37 | 1.92 | 331 | 1262 | 7 | 8 % | 15.0 % |
| voeg-samen weinig | 35 | 3.87 | 669 | 2781 | 6 | 7 % | 10.1 % |
| voeg-samen veel | 35 | 3.87 | 665 | 2767 | 7 | 7 % | 12.3 % |
| voeg-samen groot | 35 | 3.83 | 689 | 2751 | 15 | 9 % | 32.3 % |
| voeg-samen conflict | 35 | 2.88 | 497 | 2471 | 6 | 6 % | 10.0 % |

#### Fit van de tijd: seconden = a*S + b*K + c*O (zonder vaste term; ruw: drukke machine en wisselende bestandscache)

| opdracht | a (s per 100 MB) | b (µs per knoop) | c (µs per object) |
|---|---|---|---|
| verschil weinig | 1.56 | 0.49 | 10.6 |
| verschil veel | 1.40 | 0.51 | 11.4 |
| voeg-samen weinig | 4.33 | 1.17 | 26.7 |
| voeg-samen veel | 4.12 | 1.41 | 27.2 |
| voeg-samen groot | 2.87 | 3.68 | 27.2 |
| voeg-samen conflict | 3.44 | -0.27 | 14.5 |

De fit beschrijft deze vermenigvuldigde modellen goed: boven 50 MB piek hoogstens 9 % afwijking. Over alle punten
is dat tot 32,3 %, bij de kleinste bestanden (een vaste voet die de fit slecht vangt). De fits zijn geen
onafhankelijke toets: ze komen uit dezelfde punten die ze beschrijven. Of echte modellen van dezelfde grootte
hetzelfde doen, is niet gemeten. Lees de formule zo:

- **verschil** ≈ 1,9 × S + 330 bytes per knoop + 1,2 kB per object. Dat is per versie het bestand zelf, ongeveer
  165 bytes per knoop en 600 bytes per object, voor twee versies.
- **voeg-samen** ≈ 3,9 × S + 670 bytes per knoop + 2,8 kB per object: vier keer zoveel als één versie. Dat zijn
  basis, ons, hun en het resultaat, dat de zelfcontrole opnieuw leest.
- Een **conflict** ≈ 2,9 × S + 500 bytes per knoop + 2,5 kB per object: drie versies, zonder resultaat.

Hoeveel MB per MB dat wordt, hangt dus af van het aantal bytes per instantie. De laatste kolom is een schatting met
de fit, voor een model dat op de bron lijkt:

| Soort model | Bytes per instantie | verschil | voeg-samen | Geschatte grens voeg-samen bij 8 GB vrij |
|---|---|---|---|---|
| Dicht, met veel kleine regels (`ifc2x3-infra-landscaping`) | 43 | 8,5 MB per MB | 17 tot 19 MB per MB | ongeveer 430 MB |
| Planning van OPS (veel objecten) | 98 | 9,5 MB per MB | 20 tot 22 MB per MB | ongeveer 375 MB |
| Veel geometrie in lange regels (`ifc4-infra-bridge`) | 1 900 | 2,2 MB per MB | 4,4 MB per MB | ongeveer 1,8 GB |

De tijd volgt eerder de bytes dan het aantal instanties: het hashen en het opnieuw lezen gaan over de tekst. Ruw
(de machine was druk): samenvoegen kost ongeveer 4 s per 100 MB, plus 1 tot 1,5 µs per knoop en 27 µs per object.

## Modellen van 1 en 5 GB

Gegenereerd uit het dichte model: `k847` is 1 208 MB (23,1 miljoen knopen, 156 000 objecten), `k4237` is 6 317 MB
(115 miljoen knopen, 780 000 objecten). Elke run met `zwaar.sh --alleen` en een harde grens van 2 GB
(`scripts/meting/meet_groot.sh`, met `/usr/bin/time` buiten de grens):

| Bestand | Opdracht | Wat er gebeurde | Tijd tot de stop | Piek |
|---|---|---|---|---|
| 1,2 GB | `tel` (alleen lezen), eerste versie die twee keer ontleedde | Het bestand staat na 2,9 s in het geheugen (1 154 MiB). Tijdens het eerste ontleden stopt de grens het programma (exit 137). Met de nieuwe `tel` niet opnieuw gemeten (er draaiden andere zware runs); ook die heeft volgens de leestabel ongeveer 2,8 × 1 208 = 3,3 GiB nodig en stopt dus ook | 7,5 s | 2,0 GiB |
| 1,2 GB | `verschil` | Stopt tijdens het inlezen van het tweede bestand: twee keer 1,2 GB past niet in 2 GB | 6,8 s | 2,0 GiB |
| 6,3 GB | `tel` | Stopt tijdens het inlezen van het bestand zelf (`std::fs::read`). De controle op 4 GiB (`Fout::TeGroot`) komt nooit aan de beurt | 6,9 s | 2,0 GiB |

Er is geen doorloop die alleen leest en telt in de bibliotheek zelf; `tel` zit alleen in de meetkopie
(`scripts/meting/meetkopie.patch`).

**Wat dat betekent** (schatting met de formule hierboven, niet gemeten; alleen voor modellen die op de bron
lijken):

| Model | Alleen lezen | verschil | voeg-samen |
|---|---|---|---|
| Dicht, 1 GB | 2,7 GB | 9 GB | 18,5 GB |
| Dicht, 5 GB | 13 GB | 46 GB | 92 GB |
| Planning (OPS), 1 GB | 3,2 GB | 10 GB | 21 GB |
| Veel geometrie, 1 GB | 1,0 GB | 2,1 GB | 4,3 GB |
| Veel geometrie, 5 GB | 5,2 GB | 11 GB | 22 GB |

- Een bestand van 4 GiB of groter kan de bibliotheek niet lezen: de plek van elke instantie is een `u32`. Een model
  van 5 GB geeft dus altijd een fout, ongeacht het geheugen. Dat is gemeten gedrag. De opdrachtregel leest het
  bestand eerst helemaal in; bij 6,3 GB werd het proces tijdens het lezen gedood (SIGKILL, exit 137) en kwam de
  nette fout nooit. Dat is een bestaand probleem van de opdrachtregel, geen regressie door deze meting, en voor
  werk op GB-schaal blokkeert het (`knownbugs.md`, punt 90). De fix hoort in de PR voor de GB-schaal.
- Volgens de schatting lukt samenvoegen van een dicht model van 1 GB alleen met meer dan 20 GB vrij, dus niet op een
  laptop met 16 GB.
- De tijd is het probleem niet: lineair doorgetrokken kost samenvoegen van 1 GB ongeveer 70 s (dicht) of 35 s (veel
  geometrie). Dat is ook een schatting.

Voor modellen van meerdere GB moet de bibliotheek dus anders werken: niet vier versies tegelijk in het geheugen, en
geen `u32` voor de plek. Een andere agent bouwt een versie die streamt en per object werkt; die kan op dezelfde
bestanden gemeten worden (zie "Zelf meten").

## Waar de tijd heen gaat

`perf` werkt niet zonder sudo op deze machine (`kernel.perf_event_paranoid = 4`). Daarom meet de meetkopie de tijd
en het geheugen (VmRSS en VmHWM) aan het eind van elke fase. Het profiel van de grootste samenvoegingen, met
`zwaar.sh --alleen` op een rustige machine:

| Fase | Dicht, 92 MB: tijd | geheugen erna | Brug, 150 MB: tijd | geheugen erna |
|---|---|---|---|---|
| Bestanden inlezen (opdrachtregel) | 0,72 s | 265 MB | 1,15 s | 432 MB |
| Lezen: ontleden en indelen, drie versies tegelijk | 1,09 s | 746 MB | 0,59 s | 472 MB |
| Relaties koppelen, hashes zonder ankers | 0,02 s | 747 MB | 0,04 s | 472 MB |
| Ankers afstemmen, hashes met ankers | 1,35 s | 1 171 MB | 1,52 s | 493 MB |
| Vluchtige velden, gelijk? | 0,05 s | 1 171 MB | 0,01 s | 493 MB |
| Besluit per object | 0,02 s | 1 173 MB | 0,03 s | 499 MB |
| Controles (koppelregel, relaties, verwijzingen) | 0,77 s | 1 180 MB (piek 1 377) | 0,13 s | 506 MB |
| Schrijven | 0,25 s | 1 273 MB | 0,12 s | 650 MB |
| Zelfcontrole (resultaat opnieuw lezen en vergelijken) | 2,08 s | 1 353 MB (piek 1 579) | 1,77 s | 655 MB (piek 670) |
| **Totaal** | **6,4 s** | **piek 1 579 MB** | **5,4 s** | **piek 670 MB** |

- **De zelfcontrole** kost een derde van de tijd (33 %), en het vierde model.
- **De hashes met ankers** kosten 21 tot 28 % van de tijd. Bij het dichte model zijn ze 27 % van de piek (425 MB
  voor drie versies, ongeveer 80 bytes per knoop per versie).
- **Ontleden en indelen**: 11 tot 17 % van de tijd, 480 MB bij het dichte model (ongeveer 90 bytes per instantie per
  versie).
- **De bestanden zelf**: bij de brug de helft van het geheugen, bij het dichte model een zesde.
- **De controles** bij het dichte model: 12 % van de tijd en een tijdelijke piek van 200 MB.

## Voorstellen

Niet gebouwd in de bibliotheek; een andere agent werkt erin. Per voorstel de verwachte winst, uit de metingen
hierboven.

1. **Controleer de grootte vóór het inlezen, en lees begrensd.** De opdrachtregel (en elke client) kan met
   `metadata().len()` de grens van 4 GiB nagaan voordat hij het bestand leest. Dat geeft een nette fout in plaats van
   een proces dat het geheugen volloopt. Dit hoort in de PR voor de GB-schaal (`samenvoegen/gb-schaal`, nu #20).
2. **Een smallere zelfcontrole.** Lees het resultaat niet helemaal opnieuw in, maar controleer alleen de objecten en
   knopen die het plan raakt, plus de tellingen. Winst: tot een derde van de tijd, en het vierde model (bij het dichte
   model ongeveer 400 MB van 1,6 GB). Risico: de zelfcontrole is het vangnet tegen fouten in het schrijven; de
   smallere versie moet hetzelfde vinden. Eerst een test die laat zien dat ze dat doet.
3. **Kleinere hashes.** De hashes zijn BLAKE3 van 32 bytes, per instantie twee keer (zonder en met ankers). 16 bytes
   is ruim genoeg tegen toevallige botsingen binnen één bestand. Winst bij het dichte model: ongeveer 200 MB van de
   1,6 GB (schatting).
4. **Hashen over meer kernen.** Elke versie hasht op één thread; er zijn drie versies en acht kernen. Hashen per
   deel van de instanties over alle kernen kan de fase "ankers" (een vijfde tot een kwart van de tijd) een paar keer
   sneller maken. Op een drukke laptop is de winst kleiner.
5. **Niet alle versies tegelijk.** Voor modellen van meerdere GB is dit de enige weg: per object streamen, of eerst
   basis en ons vergelijken, dan basis en hun, en alleen de hashes per object bewaren. Dat is het werk van de andere
   agent. Zijn meting hoort bij deze tabellen.
6. **Een andere geheugenbeheerder: geen winst.** Gemeten met mimalloc (in de meetkopie, `#[global_allocator]`):
   dicht 92 MB 8,2 s en 1 630 MB tegen 6,6 s en 1 579 MB met de systeembeheerder; brug 150 MB 5,8 s en 691 MB tegen
   5,6 s en 670 MB. Niet doen.

## Zelf meten

De bestanden staan buiten git, in `/home/aia09/worktrees/metingen-data/` op aia09-mp50:

| Pad | Wat |
|---|---|
| `modellen/` | De bronmodellen (zie boven) |
| `werk/<bron>-k<k>-<soort>/` | De invoer van de reeks: `basis.ifc`, `ons.ifc`, `hun.ifc` en de uitvoer van `/usr/bin/time -v` |
| `groot/ifc2x3-infra-landscaping-k847/`, `-k4237/` | De modellen van 1,2 en 6,3 GB (soort weinig) |
| `bin/openaec-samenvoegen-0655eda` | Het gemeten programma |
| `meetkopie-0655eda/` | De bibliotheek met `scripts/meting/meetkopie.patch`: tijd en geheugen per fase, en de opdracht `tel` |
| `meetkopie-0655eda-mimalloc/` | Idem, met mimalloc als geheugenbeheer (zie Voorstellen) |
| `reeks-0655eda.csv`, `.log` | De ruwe metingen van de reeks; `tabellen.md` is de uitvoer van `tabel.py` |
| `alleen-0655eda.sh`, `.log` | De run onder `zwaar.sh --alleen`: de grootste stappen, `tel`, de grote modellen, het profiel en de spreiding |
| `groot/meting/` | De metingen op de grote modellen (`*.tijd`, `*.fasen`); `groot/meting/tel/` is de eerste `tel` op de reeks (die twee keer ontleedde), `groot/meting/tel-r1/` de nieuwe |
| `profiel/` | Het profiel per fase (`*.fasen`), met de systeembeheerder en met mimalloc |

Een nieuwe versie van de bibliotheek meet je op dezelfde invoer zo:

```text
cargo build --release --features cli
cp target/release/openaec-samenvoegen /home/aia09/worktrees/metingen-data/bin/openaec-samenvoegen-<commit>
cd /home/aia09/worktrees/metingen-data
M=<repo>/crates/openaec-samenvoegen/scripts/meting
# De reeks onder één plek; stappen die --alleen vragen, slaat dit over.
/home/aia09/worktrees/zwaar.sh env MEET_IN_EEN_PLEK=1 python3 -I $M/meet.py \
    bin/openaec-samenvoegen-<commit> werk reeks-<commit>.csv modellen/*.ifc
# Die stappen onder --alleen (zie alleen-0655eda.sh voor de rest: tel, grote modellen, profiel).
/home/aia09/worktrees/zwaar.sh --alleen env MEET_IN_EEN_PLEK=alleen python3 -I $M/meet.py \
    bin/openaec-samenvoegen-<commit> werk reeks-<commit>.csv modellen/*.ifc
python3 -I $M/tabel.py reeks-<commit>.csv
```

De invoer in `werk/` bestaat al (elke map heeft het bestand `klaar`), dus het script maakt hem niet opnieuw.
`maak_invoer.py` schrijft alleen in een nieuwe of lege map en overschrijft niets; `proef_maak_invoer.py` toetst dat. Voor het profiel per fase pas je
`meetkopie.patch` toe op een kopie van `src/` (`patch -p1 < meetkopie.patch`); de patch hoort bij `0655eda` en past
misschien niet meer op een latere versie.
