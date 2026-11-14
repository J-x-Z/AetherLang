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
