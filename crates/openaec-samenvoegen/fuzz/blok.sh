#!/usr/bin/env bash
# blok.sh <seconden> <doel>...: bouwt de fuzzdoelen en fuzzt elk doel een blok van <seconden>, met één job.
# Op aia: altijd via /home/aia09/worktrees/zwaar.sh. De uitvoer van libFuzzer komt in logs/<doel>-<tijd>.log.
set -uo pipefail
cd "$(dirname "$0")"
duur=$1; shift
cargo +nightly fuzz build -s none -a "$@" || exit $?
mkdir -p logs
for doel in "$@"; do
  case $doel in samenvoegen) max=200000 ;; *) max=65536 ;; esac
  log="logs/$doel-$(date +%H%M).log"
  cargo +nightly fuzz run -s none -a "$doel" "corpus/$doel" -- \
    -dict=step.dict -max_len=$max -rss_limit_mb=1024 -timeout=10 -max_total_time="$duur" \
    -print_final_stats=1 > "$log" 2>&1
  echo "$doel EXIT:$? $log"
done
