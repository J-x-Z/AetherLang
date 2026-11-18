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
mkdir -p "$CARGO_TARGET_DIR" "$TMPDIR"
CARGO_TARGET_DIR=$(CDPATH= cd -- "$CARGO_TARGET_DIR" && pwd)
TMPDIR=$(CDPATH= cd -- "$TMPDIR" && pwd)
export CARGO_BUILD_JOBS RUST_TEST_THREADS CARGO_TARGET_DIR TMPDIR
case "${1:-llvm}" in
    llvm)
        if [ -z "${LLVM_SYS_211_PREFIX:-}" ] && [ -x /opt/homebrew/opt/llvm@21/bin/llvm-config ]; then
            export LLVM_SYS_211_PREFIX=/opt/homebrew/opt/llvm@21
        fi
        cargo test --locked --features llvm
        backends='llvm c'; bootstrap_backend=llvm ;;
    c)
        cargo test --locked --no-default-features
        backends='c'; bootstrap_backend=c ;;
    *) printf '%s\n' 'Usage: scripts/test.sh [llvm|c]' >&2; exit 2 ;;
esac
python3 "$root/scripts/test_build_paths.py"
python3 tests/package_install.py --work-dir "$CARGO_TARGET_DIR/package-install"
compiler="$CARGO_TARGET_DIR/debug/aethc"
# $backends is an intentional list of fixed tokens selected above.
python3 scripts/validate.py --compiler "$compiler" --work-dir "$CARGO_TARGET_DIR/validation" --backends $backends
for suite in dynamic_values closure_values array_repeat aggregate_abi system_values global_linkage semantic_values runtime_foundation generic_runtime memory_runtime simd_values platform_intrinsics process_runtime math_values quantum_values cuda_bindings literal_escapes extern_definitions c_void_pointers install_output vec_alignment vec32 darwin_target freestanding target_layout interfaces_methods ffi_modules contracts legacy_runtime text_map host_builtins sha256 compute_runtime compute_applications compute_jit compute_planning; do
    python3 "tests/$suite.py" --compiler "$compiler" --work-dir "$CARGO_TARGET_DIR/$suite" --backends $backends
done
case "$(uname -s)/$(uname -m)" in
    Darwin/arm64|Darwin/x86_64|Linux/x86_64)
        python3 tests/sysv_abi.py --compiler "$compiler" --work-dir "$CARGO_TARGET_DIR/native-aggregate-abi" --backends $backends ;;
esac
python3 tests/owned_allocations.py --compiler "$compiler" --work-dir "$CARGO_TARGET_DIR/owned-allocations"
for backend in $backends; do
    jxz="$CARGO_TARGET_DIR/jxz-$backend"
    AETHC="$compiler" AETHER_BACKEND="$backend" jxz/build.sh "$jxz"
    AETHER_BACKEND="$backend" python3 jxz/tests/integration.py --jxz "$jxz" --aethc "$compiler" --work-dir "$CARGO_TARGET_DIR/jxz-tests-$backend"
    python3 jxz/tests/registry.py --jxz "$jxz" --aethc "$compiler" --backend "$backend" --work-dir "$CARGO_TARGET_DIR/jxz-registry-$backend"
done

python3 tests/bootstrap_core.py --compiler "$compiler" --work-dir "$CARGO_TARGET_DIR/bootstrap-core" --backend "$bootstrap_backend"
if clang --version >/dev/null 2>&1; then
    CC=clang python3 tests/bootstrap_storage.py --compiler "$CARGO_TARGET_DIR/bootstrap-core/stage4" --work-dir "$CARGO_TARGET_DIR/bootstrap-storage"
    CC=clang python3 tests/bootstrap_errno.py --compiler "$CARGO_TARGET_DIR/bootstrap-core/stage4" --work-dir "$CARGO_TARGET_DIR/bootstrap-errno"
    python3 tests/bootstrap_driver.py --compiler "$CARGO_TARGET_DIR/bootstrap-core/stage4" --jxz "$CARGO_TARGET_DIR/jxz-$bootstrap_backend" --work-dir "$CARGO_TARGET_DIR/bootstrap-driver"
    python3 tests/bootstrap_llvm.py --compiler "$CARGO_TARGET_DIR/bootstrap-core/stage4" --work-dir "$CARGO_TARGET_DIR/bootstrap-llvm" --cc clang
fi
if [ -n "${AETHER_CUDAQ_BRIDGE:-}" ]; then
    case "${AETHER_CUDAQ_GPU:-0}" in
        0) cudaq_mode= ;;
        1) cudaq_mode=--gpu ;;
        *) printf '%s\n' 'AETHER_CUDAQ_GPU must be 0 or 1' >&2; exit 2 ;;
    esac
    python3 tests/cudaq_runtime.py --compiler "$compiler" --bridge "$AETHER_CUDAQ_BRIDGE" --work-dir "$CARGO_TARGET_DIR/cudaq-runtime" --backends $backends $cudaq_mode
    python3 tests/cudaq_boundaries.py --compiler "$compiler" --library "$AETHER_CUDAQ_BRIDGE" --work-dir "$CARGO_TARGET_DIR/cudaq-boundaries" --backends $backends
fi
