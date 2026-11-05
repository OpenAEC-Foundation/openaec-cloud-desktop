#!/usr/bin/env bash
# Alles wat de grote plek vraagt, in één run onder `zwaar.sh --alleen` (5 november 2026, commit 0655eda):
#   /home/aia09/worktrees/zwaar.sh --alleen bash alleen-0655eda.sh
# Elke run heeft een harde grens van 2 GB. Eén run tegelijk.
set -u
cd /home/aia09/worktrees/metingen-data
S=/home/aia09/worktrees/desktop-p4-metingen/crates/openaec-samenvoegen/scripts/meting
K=meetkopie-0655eda/target/release/openaec-samenvoegen
KM=meetkopie-0655eda-mimalloc/target/release/openaec-samenvoegen
G=(systemd-run --user --scope -q -p MemoryMax=2G -p MemorySwapMax=0)
export MEET_IN_EEN_PLEK=alleen
echo "== 1. de stappen van de reeks die --alleen vroegen"
python3 -I $S/meet.py bin/openaec-samenvoegen-0655eda werk reeks-0655eda.csv modellen/ifc4-infra-bridge.ifc \
  modellen/ifc2x3-infra-landscaping.ifc modellen/ifc4-infra-landscaping.ifc modellen/ifc4x3-infra-bridge.ifc \
  modellen/ops-13-supermarkt-albert-heijn.ifc
echo "== 2. alleen lezen en tellen (meetkopie tel) op de reeks"
for f in werk/ifc2x3-infra-landscaping-k*-weinig/basis.ifc werk/ifc4-infra-bridge-k*-weinig/basis.ifc \
         werk/ops-13-supermarkt-albert-heijn-k*-weinig/basis.ifc; do
  bash $S/meet_groot.sh $K groot/meting/tel tel "$f"
done
echo "== 3. de grote modellen"
bash $S/meet_groot.sh $K groot/meting tel groot/ifc2x3-infra-landscaping-k847/basis.ifc
bash $S/meet_groot.sh $K groot/meting verschil groot/ifc2x3-infra-landscaping-k847/basis.ifc
bash $S/meet_groot.sh $K groot/meting tel groot/ifc2x3-infra-landscaping-k4237/basis.ifc
echo "== 4. profiel per fase, en mimalloc"
mkdir -p profiel
for w in werk/ifc2x3-infra-landscaping-k68-weinig werk/ifc4-infra-bridge-k86-weinig; do
  for p in "$K:systeem" "$KM:mimalloc"; do
    prog=${p%%:*}; naam="$(basename $w)-${p##*:}"
    /usr/bin/time -v -o profiel/$naam.tijd "${G[@]}" $prog voeg-samen $w/basis.ifc $w/ons.ifc $w/hun.ifc \
      -o $w/uit.ifc > profiel/$naam.uit 2> profiel/$naam.fasen
    echo "$naam: exit $?, $(grep Elapsed profiel/$naam.tijd | sed 's/.*: //'), piek $(grep 'Maximum resident' profiel/$naam.tijd | sed 's/.*: //') kB"
    rm -f $w/uit.ifc
  done
done
echo "== 5. spreiding: drie keer hetzelfde"
for i in 1 2 3; do
  w=werk/ifc4-infra-bridge-k86-weinig
  /usr/bin/time -f "bridge-k86 voeg-samen: %e s, %M kB" "${G[@]}" bin/openaec-samenvoegen-0655eda voeg-samen \
    $w/basis.ifc $w/ons.ifc $w/hun.ifc -o $w/uit.ifc > /dev/null
  w=werk/ifc2x3-infra-landscaping-k34-weinig
  /usr/bin/time -f "landscaping-k34 voeg-samen: %e s, %M kB" "${G[@]}" bin/openaec-samenvoegen-0655eda voeg-samen \
    $w/basis.ifc $w/ons.ifc $w/hun.ifc -o $w/uit.ifc > /dev/null
done
# Alleen de uitvoer die stap 5 zelf maakte (stap 1 en 4 ruimen hun eigen uit.ifc al op).
rm -f werk/ifc4-infra-bridge-k86-weinig/uit.ifc werk/ifc2x3-infra-landscaping-k34-weinig/uit.ifc
