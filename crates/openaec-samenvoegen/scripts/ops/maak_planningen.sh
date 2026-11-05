#!/usr/bin/env bash
# Maakt tests/bestanden/planning/ opnieuw: wijzigingen zoals een planner ze in Open Planner Studio maakt, gelezen,
# doorgerekend en opgeslagen met de code van OPS zelf (ops.ts). De basis is 13-supermarkt-albert-heijn.ifc, één keer
# opnieuw opgeslagen.
#
#   scripts/ops/maak_planningen.sh <ops-repo>
set -euo pipefail
ops=$1
hier=$(cd "$(dirname "$0")" && pwd)
uit=$hier/../../tests/bestanden/planning
mkdir -p "$uit"
w() { node "$hier/ops.mjs" "$ops" wijzig "$1" "$uit/$2.ifc" "$3"; }
v() { w "$uit/basis.ifc" "$1" "$2"; }

w "$hier/../../tests/bestanden/13-supermarkt-albert-heijn.ifc" basis '{"geen":true}'
sleep 1 # een andere tijdstempel
v opnieuw '{"geen":true}'
v duur-ons '{"duur":"Structural steel, extension","dagen":20}'
v duur-hun '{"duur":"Structural steel, extension","dagen":18}'
v duur-ander-hun '{"duur":"Foundations, extension","dagen":14}'
v naam-hun '{"hernoem":"Structural steel, extension","naar":"Staalconstructie uitbouw"}'
v wis-ons '{"wis":"Partially break out floor"}'
v relatie-ons '{"relatie":["Start of refurbishment","Partially break out floor"],"type":"FINISH_START"}'
v relatie-hun '{"relatie":["Start of refurbishment","Partially break out floor"],"type":"FINISH_START"}'
v relatie-ss-ons '{"relatie":["Start of refurbishment","Partially break out floor"],"type":"START_START"}'
# Dezelfde relatie met gegevens in OPS_Sequences (een lag met een breuk, de P6-grens).
r='"relatie":["Start of refurbishment","Partially break out floor"],"type":"FINISH_START"'
v relatie-lag150-ons "{$r,\"lagMinuten\":150}"
v relatie-lag150.4-hun "{$r,\"lagMinuten\":150.4}"
v relatie-lag240-ons "{$r,\"lagMinuten\":240}"
v relatie-halvedag-hun "{$r,\"lagDagen\":0.5}"
v relatie-halvedag-ons "{$r,\"lagDagen\":0.5}"
v relatie-p6-hun "{$r,\"p6\":true}"
v verplaats-ons '{"verplaats":"Checkout island","onder":"3. Extension"}'
v verplaats-hun '{"verplaats":"Checkout island","onder":"4. Building services"}'
v nieuw-ons '{"nieuw":"Gevelreclame","onder":"3. Extension","zoals":"Floor, extension"}'
v nieuw-hun '{"nieuw":"Camerabewaking","onder":"4. Building services","zoals":"Fire alarm system"}'
v nieuw-zelfde-hun '{"nieuw":"Hekwerk","onder":"3. Extension","zoals":"Floor, extension"}'
v baseline-ons '{"baseline":"Ons"}'
v baseline-hun '{"baseline":"Hun"}'
node "$hier/ops.mjs" "$ops" lees "$uit"/*.ifc > /dev/null
echo "klaar: $uit"
