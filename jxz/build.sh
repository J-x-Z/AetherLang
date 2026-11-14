#!/bin/sh
# Rebuild from Aether sources through the normal compiler driver.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
: "${AETHC:?Set AETHC to the built aethc executable}"
out=${1:-"$root/target/jxz"}
mkdir -p -- "$(dirname -- "$out")"
if [ -n "${AETHER_BACKEND:-}" ]; then
    "$AETHC" build -O2 --backend "$AETHER_BACKEND" -o "$out" "$root/jxz/src/main.aeth"
else
    "$AETHC" build -O2 -o "$out" "$root/jxz/src/main.aeth"
fi
printf '%s\n' "Built $out"
