#!/bin/sh
# Reproducible semantic, CLI and real-program validation of one build.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
. "$root/scripts/toolchain.sh"
: "${CARGO_BUILD_JOBS:=3}"
: "${RUST_TEST_THREADS:=3}"
: "${CARGO_TARGET_DIR:=$root/target}"
: "${TMPDIR:=$CARGO_TARGET_DIR/tmp}"
export CARGO_BUILD_JOBS RUST_TEST_THREADS CARGO_TARGET_DIR TMPDIR
mkdir -p "$TMPDIR"
case "${1:-llvm}" in
    llvm)
        if [ -z "${LLVM_SYS_211_PREFIX:-}" ] && [ -x /opt/homebrew/opt/llvm@21/bin/llvm-config ]; then
            export LLVM_SYS_211_PREFIX=/opt/homebrew/opt/llvm@21
        fi
        cargo test --locked --features llvm
        backends='llvm c' ;;
    c)
        cargo test --locked --no-default-features
        backends='c' ;;
    *) printf '%s\n' 'Usage: scripts/test.sh [llvm|c]' >&2; exit 2 ;;
esac
compiler="$CARGO_TARGET_DIR/debug/aethc"
# $backends is an intentional list of fixed tokens selected above.
python3 scripts/validate.py --compiler "$compiler" --work-dir "$CARGO_TARGET_DIR/validation" --backends $backends
for backend in $backends; do
    jxz="$CARGO_TARGET_DIR/jxz-$backend"
    AETHC="$compiler" AETHER_BACKEND="$backend" jxz/build.sh "$jxz"
    AETHER_BACKEND="$backend" python3 jxz/tests/integration.py --jxz "$jxz" --aethc "$compiler" --work-dir "$CARGO_TARGET_DIR/jxz-tests-$backend"
done
