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
case "${1:-llvm}" in
    c) set -- --no-default-features ;;
    llvm)
        if [ -z "${LLVM_SYS_211_PREFIX:-}" ]; then
            if command -v llvm-config-21 >/dev/null 2>&1; then
                LLVM_SYS_211_PREFIX=$(llvm-config-21 --prefix)
            elif [ -x /opt/homebrew/opt/llvm@21/bin/llvm-config ]; then
                LLVM_SYS_211_PREFIX=/opt/homebrew/opt/llvm@21
            elif command -v llvm-config >/dev/null 2>&1 && [ "$(llvm-config --version | cut -d. -f1)" = 21 ]; then
                LLVM_SYS_211_PREFIX=$(llvm-config --prefix)
            else
                printf '%s\n' 'LLVM 21 is required. Set LLVM_SYS_211_PREFIX, or use scripts/build.sh c.' >&2
                exit 1
            fi
            export LLVM_SYS_211_PREFIX
        fi
        set -- --features llvm ;;
    *) printf '%s\n' 'Usage: scripts/build.sh [llvm|c]' >&2; exit 2 ;;
esac
cargo build --locked --release "$@"
printf '%s\n' "Compiler: $CARGO_TARGET_DIR/release/aethc"
