# Source from the repository root. Prefer the pinned rustup toolchain when it is
# installed, even on hosts where a standalone Homebrew cargo shadows the proxy.
if command -v rustup >/dev/null 2>&1; then
    if aether_rustc=$(rustup which --toolchain 1.100.0 rustc 2>/dev/null); then
        PATH="$(dirname -- "$aether_rustc"):$PATH"
        export PATH
    fi
fi
cargo --version
rustc --version

# Select the SDK used by llvm-sys and by tools that consume its output.
aether_select_llvm() {
    case "$1" in
        llvm) aether_major=21; aether_prefix=${LLVM_SYS_211_PREFIX:-} ;;
        llvm23) aether_major=23; aether_prefix=${LLVM_SYS_231_PREFIX:-} ;;
        *) return 2 ;;
    esac
    if [ -n "$aether_prefix" ]; then
        set -- "$aether_prefix/bin/llvm-config"
    else
        set -- "llvm-config-$aether_major" "llvm-config" \
            "/opt/homebrew/opt/llvm@$aether_major/bin/llvm-config" "/opt/homebrew/opt/llvm/bin/llvm-config" \
            "/usr/local/opt/llvm@$aether_major/bin/llvm-config" "/usr/local/opt/llvm/bin/llvm-config"
    fi
    for aether_config in "$@"; do
        case "$("$aether_config" --version 2>/dev/null)" in
            "$aether_major".1.*)
                AETHER_LLVM_PREFIX=$("$aether_config" --prefix)
                export AETHER_LLVM_PREFIX
                case "$aether_major" in
                    21) export LLVM_SYS_211_PREFIX="$AETHER_LLVM_PREFIX" ;;
                    23) export LLVM_SYS_231_PREFIX="$AETHER_LLVM_PREFIX" ;;
                esac
                return 0 ;;
        esac
    done
    echo "LLVM $aether_major.1 required. Set LLVM_SYS_${aether_major}1_PREFIX to the installation prefix." >&2
    return 1
}
