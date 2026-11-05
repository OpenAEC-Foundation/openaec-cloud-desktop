# Metingen: openaec-samenvoegen op grote modellen

Stand: 5 november 2026. Gemeten op commit `0655eda` van `crates/openaec-samenvoegen` (de tip van PR #7 bij de
start). Ontwerp: openaec-storage, `docs/samenwerken/ontwerp.md` §5.6 en §5.14.

## Samenvatting

<!-- SAMENVATTING -->

## Opzet

**Machine.** aia09-mp50: 8 kernen, 13 GB geheugen, Linux Mint 22.3. Op dezelfde machine draaiden tegelijk zo'n
16 andere agents, met builds en testruns (load 10 tot 50). De tijden zijn daarom ruw: herhaal je een meting, dan kan
hij 30 tot 50 % anders uitkomen. Het piekgeheugen is wel stabiel (binnen 1 %).

**Grens.** Elke run had een harde grens van 2 GB (`systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0`).
Ging een run eroverheen, dan stopte systemd hem. Zo bleven de andere processen op de machine heel. Een stap waarvan de
piek (lineair doorgetrokken uit de vorige stap) boven 1,8 GB zou komen, is niet gemeten. Alles daarboven is een
schatting met de formules hieronder. Elke run ging via `zwaar.sh`; een run die 900 MB of meer verwachtte, of een
bestand van 1 GB of meer, via `zwaar.sh --alleen`.

**Wat gemeten is.** Tijd (wandklok) en piekgeheugen ("Maximum resident set size" van `/usr/bin/time -v`) van het
opdrachtregelprogramma, release-modus, één run per punt:

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

<!-- TABELLEN -->

## Groei

<!-- GROEI -->

## Modellen van 1 en 5 GB

<!-- GROOT -->

## Waar de tijd heen gaat

<!-- PROFIEL -->

## Voorstellen

<!-- VOORSTELLEN -->

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
| `reeks-0655eda.csv`, `.log` | De ruwe metingen |
| `groot/meting/` | De metingen op de grote modellen (`*.tijd`, `*.fasen`) |

Een nieuwe versie van de bibliotheek meet je op dezelfde invoer zo:

```text
cargo build --release --features cli
cp target/release/openaec-samenvoegen /home/aia09/worktrees/metingen-data/bin/openaec-samenvoegen-<commit>
cd /home/aia09/worktrees/metingen-data
/home/aia09/worktrees/zwaar.sh env MEET_IN_EEN_PLEK=1 python3 -I <repo>/crates/openaec-samenvoegen/scripts/meting/meet.py \
    bin/openaec-samenvoegen-<commit> werk reeks-<commit>.csv modellen/*.ifc
python3 -I <repo>/crates/openaec-samenvoegen/scripts/meting/meet.py \
    bin/openaec-samenvoegen-<commit> werk reeks-<commit>.csv modellen/*.ifc   # de stappen die --alleen vragen
python3 -I <repo>/crates/openaec-samenvoegen/scripts/meting/tabel.py reeks-<commit>.csv
```

De invoer in `werk/` bestaat al, dus het script maakt hem niet opnieuw. Voor het profiel per fase pas je
`meetkopie.patch` toe op een kopie van `src/` (`patch -p1 < meetkopie.patch`); de patch hoort bij `0655eda` en past
misschien niet meer op een latere versie.
