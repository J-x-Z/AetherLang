#!/bin/sh
# Build and test the release archive on the target Linux host. All outputs
# except Cargo's normal cache live under the supplied new directory.
set -eu
if [ "$#" -ne 1 ] || [ "$(uname -s)" != Linux ]; then
    echo "Usage: sh scripts/release_linux.sh NEW_WORK_DIR (native Linux)" >&2
    exit 2
fi
mkdir -- "$1"
work=$(cd -- "$1" && pwd)
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
. scripts/toolchain.sh
aether_select_llvm llvm
export PATH="$AETHER_LLVM_PREFIX/bin:$PATH"
export PYTHONDONTWRITEBYTECODE=1
version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')
package="aetherlang-v$version-linux-$(uname -m)"
archive="$work/artifacts/$package.tgz"
proof="$work/artifacts/evidence"
mkdir -p "$proof" "$work/extracted"
export TMPDIR="$work/tmp"
mkdir "$TMPDIR"
unset LD_LIBRARY_PATH AETHER_PATH AETHC
python3 scripts/package.py --bundle-linux-llvm --work-dir "$work/bootstrap" --output "$work/$package"
cargo vendor --locked --versioned-dirs "$work/vendor" > "$work/vendor-config.toml"
python3 scripts/package_notices.py --package "$work/$package" --output "$archive" \
    --vendor "$work/vendor" --rust-docs "$(rustc --print sysroot)/share/doc/rust" \
    --llvm-notices "${AETHER_LLVM_NOTICE:-/usr/share/doc/libllvm21/copyright}" \
    --llvm-version "$(llvm-config --version)" > "$proof/notice-seal.json"

# Install from the archived bytes, move the complete prefix, and exercise
# the installed executables and their relative dependencies at the new path.
tar -xzf "$archive" -C "$work/extracted"
extracted="$work/extracted/$package"
manifest_sha=$(sha256sum "$extracted/manifest.json" | cut -d ' ' -f 1)
sh "$extracted/install.sh" --package "$extracted" --prefix "$work/installed" --manifest-sha256 "$manifest_sha"
mv "$work/installed" "$work/relocated install"
bin="$work/relocated install/bin"
python3 scripts/linux_abi.py --package "$bin" --output "$proof/elf-abi.json"
python3 tests/mcp_protocol.py > "$proof/mcp-protocol.log" 2>&1
for compiler in aethc aethc-native; do
    case "$compiler" in aethc) driver=jxz ;; *) driver=jxz-native ;; esac
    python3 tests/mcp_queries.py --compiler "$bin/$compiler" --jxz "$bin/$driver" \
        --server "$bin/aether-mcp" --work-dir "$work/mcp-$compiler" > "$proof/mcp-$compiler.log" 2>&1
    python3 jxz/tests/integration.py --jxz "$bin/$driver" --aethc "$bin/$compiler" \
        --backend llvm --work-dir "$work/jxz-$compiler" > "$proof/jxz-$compiler.log" 2>&1
done
cp "$extracted/manifest.json" "$proof/manifest.json"
cp "$work/bootstrap/regular-bootstrap.json" "$proof/regular-bootstrap.json"
for backend in c llvm; do
    cp "$work/bootstrap/bootstrap-$backend/results.json" "$proof/bootstrap-$backend.json"
done
(cd "$work/artifacts" && sha256sum "$package.tgz" > "$package.sha256")
cat "$work/artifacts/$package.sha256"
