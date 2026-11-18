#!/bin/sh
# One build entry point. Set CARGO_TARGET_DIR and TMPDIR to an external volume
# on space-constrained hosts; existing toolchains and caches are reused.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
. "$root/scripts/toolchain.sh"
: "${CARGO_BUILD_JOBS:=3}"
: "${CARGO_TARGET_DIR:=$root/target}"
: "${TMPDIR:=$CARGO_TARGET_DIR/tmp}"
mkdir -p "$CARGO_TARGET_DIR" "$TMPDIR"
CARGO_TARGET_DIR=$(CDPATH= cd -- "$CARGO_TARGET_DIR" && pwd)
TMPDIR=$(CDPATH= cd -- "$TMPDIR" && pwd)
export CARGO_BUILD_JOBS CARGO_TARGET_DIR TMPDIR
backend=${1:-llvm}
case "$backend" in
    c) set -- --no-default-features ;;
    llvm|llvm23)
        aether_select_llvm "$backend"
        set -- --no-default-features --features "$backend" ;;
    *) printf '%s\n' 'Usage: scripts/build.sh [llvm|llvm23|c]' >&2; exit 2 ;;
esac
cargo build --locked --release "$@"
printf '%s\n' "Compiler: $CARGO_TARGET_DIR/release/aethc"
