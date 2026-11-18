#!/usr/bin/env python3
"""Seal a verified Aether release with license texts, without regenerating its manifest.

Python 3.11+. Pass a tar/zip or an unpacked package directory and a new .tgz,
.tar.gz, or .zip output. The source repo must contain the manifest's commit.
All scratch files stay beside the output. Run --help for license inputs.
"""
import argparse
import contextlib
import gzip
import hashlib
import io
import json
import os
import re
import shutil
import stat
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile
from pathlib import Path, PurePosixPath

NOTICE = re.compile(r"^(?:LICEN[CS]E|COPYRIGHT|NOTICE|COPYING|UNLICENSE)(?:$|[._-])", re.I)
LEGAL = "THIRD-PARTY-LICENSES"
HEX = re.compile(r"[0-9a-f]{64}")


def check(ok, reason):
    if not ok:
        raise ValueError(reason)


def digest(path):
    with path.open("rb") as raw:
        return hashlib.file_digest(raw, "sha256").hexdigest()


def git_file(repo, commit, name):
    return subprocess.check_output(["git", "-C", str(repo), "show", commit + ":" + name])


def path_name(name):
    path = PurePosixPath(name)
    check(not path.is_absolute() and ".." not in path.parts and "\\" not in name
          and ":" not in name and "\x00" not in name, "invalid package path")
    return path


def unpack(src, tree):
    def put(name, mode, stream=None):
        path = tree / path_name(name)
        if stream is None:
            path.mkdir(parents=True, exist_ok=True)
            return
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb") as dst:
            shutil.copyfileobj(stream, dst, 1024 * 1024)
        path.chmod(mode & 0o777)

    if src.is_dir():
        check((src / "manifest.json").is_file(), "input directory must be the package root")
        for path in sorted(src.rglob("*")):
            check(not path.is_symlink(), "symlink in input directory")
            name = (Path(src.name) / path.relative_to(src)).as_posix()
            if path.is_dir():
                put(name, 0o755)
            else:
                check(path.is_file(), "special file in input")
                with path.open("rb") as raw:
                    put(name, path.stat().st_mode, raw)
    elif zipfile.is_zipfile(src):
        with zipfile.ZipFile(src) as arc:
            for entry in arc.infolist():
                mode = entry.external_attr >> 16
                check(stat.S_IFMT(mode) in (0, stat.S_IFREG, stat.S_IFDIR), "special ZIP entry")
                if entry.is_dir():
                    put(entry.filename, 0o755)
                else:
                    with arc.open(entry) as raw:
                        put(entry.filename, mode & 0o777 or 0o644, raw)
    else:
        with tarfile.open(src, "r:*") as arc:
            for entry in arc:
                if entry.isdir():
                    put(entry.name, 0o755)
                else:
                    check(entry.isfile(), "special tar entry")
                    with arc.extractfile(entry) as raw:
                        put(entry.name, entry.mode, raw)


def inventory(tree):
    return {p.relative_to(tree).as_posix(): (digest(p), p.stat().st_mode & 0o777)
            for p in tree.rglob("*") if p.is_file()}


def validate(tree):
    manifests = list(tree.glob("manifest.json")) + list(tree.glob("*/manifest.json"))
    check(len(manifests) == 1, "expected one root package manifest")
    path = manifests[0]
    root = path.parent
    obj = json.loads(path.read_bytes())
    commit = obj["source_commit"]
    check(re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", commit), "expected full source commit hash")
    windows = obj.get("format") == "aether-windows-native-bundle-1"
    records = obj["files"] if windows else obj["artifacts"]
    expected = {}
    for f in records:
        name = str(path_name(f["name"]))
        check(name not in expected and HEX.fullmatch(f["sha256"]), "invalid manifest entry")
        expected[name] = f["sha256"]
        check((root / name).stat().st_size == f["bytes"], "incorrect manifest size: " + name)
    if not windows:
        for name, sha in obj["stdlib"]["files"].items():
            name = str(path_name(name))
            check(name not in expected and HEX.fullmatch(sha), "invalid stdlib entry")
            expected[name] = sha
    expected["manifest.json"] = digest(path)
    if windows:
        sums = "\n".join(f'{f["sha256"]}  {f["name"]}' for f in records)
        sums += "\n" + expected["manifest.json"] + "  manifest.json\n"
        check((root / "SHA256SUMS").read_text() == sums, "Windows SHA256SUMS differs from manifest")
        expected["SHA256SUMS"] = digest(root / "SHA256SUMS")
    for name, sha in expected.items():
        check(digest(root / name) == sha, "manifest hash mismatch: " + name)
    files = inventory(tree)
    check(set(files) == {(root / name).relative_to(tree).as_posix() for name in expected},
          "input contains unmanifested files")
    return root, obj, windows, files


def local_licenses(folder):
    if folder.is_file():
        check(NOTICE.match(folder.name), "expected license/notice filename: " + folder.name)
        return {folder.name: folder}
    check(folder.is_dir(), "notice directory missing")
    found = {}
    for p in sorted(folder.rglob("*")):
        rel = p.relative_to(folder)
        if p.is_file() and (NOTICE.match(p.name) or any(
                s.lower() in ("licenses", "licences") for s in rel.parts[:-1])):
            check(not p.is_symlink(), "symlink in license input")
            found[rel.as_posix()] = p
    check(found, "no legal texts found in " + folder.name)
    return found


def add_notices(root, meta, windows, opt):
    commit = meta["source_commit"]
    check(not (root / "LICENSE").exists() and not (root / LEGAL).exists(),
          "release already contains a notice path")
    (root / "LICENSE").write_bytes(git_file(opt.repo, commit, "LICENSE"))
    index = {"source_commit": commit, "project_license_sha256": digest(root / "LICENSE"),
             "components": [], "notice_files_sha256": {}}

    def add(group, files, **details):
        check(files, "empty notice set: " + group)
        index["components"].append(dict(directory=group, **details))
        for name, path in sorted(files.items()):
            rel = str(path_name(group + "/" + name))
            dest = root / LEGAL / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            with dest.open("xb") as out, (io.BytesIO(path) if isinstance(path, bytes) else path.open("rb")) as raw:
                shutil.copyfileobj(raw, out)
            dest.chmod(0o644)
            index["notice_files_sha256"][rel] = digest(dest)

    if windows:
        check(opt.zig_prefix, "Windows package requires --zig-prefix")
        prefix = opt.zig_prefix
        version = subprocess.check_output([prefix / "bin/zig", "version"], text=True).strip()
        check(version == meta["zig"], "Zig version differs from Windows manifest")
        add("zig", {"LICENSE": prefix / "LICENSE"}, version=version)
        mingw = prefix / "lib/zig/libc/mingw"
        notices = {"COPYING": mingw / "COPYING"}
        # These runtime sources carry their notices in the first comment.
        for name in ("crt/crtexe.c", "gdtoa/gdtoa.c", "stdio/mingw_pformat.c", "winpthreads/thread.c"):
            header = re.match(rb"\s*(/\*.*?\*/)", (mingw / name).read_bytes(), re.DOTALL)
            check(header and re.search(rb"copyright|public domain", header[1], re.I),
                  "MinGW-w64 source notice missing: " + name)
            notices[name + ".NOTICE.txt"] = header[1] + b"\n"
        add("mingw-w64", notices, source="mingw-w64 runtime shipped by Zig " + version)
        add("zig-compiler-rt", {"musl-COPYRIGHT": prefix / "lib/zig/libc/musl/COPYRIGHT"},
            source="musl-derived routines in the Zig compiler runtime")
    else:
        check(opt.vendor and opt.rust_docs, "Rust seed package requires --vendor and --rust-docs")
        lock = tomllib.loads(git_file(opt.repo, commit, "Cargo.lock").decode())
        index["cargo_scope"] = "All registry packages locked at this source commit, including optional and build dependencies."
        for pkg in lock["package"]:
            if "source" not in pkg:
                continue
            check(pkg["source"] == "registry+https://github.com/rust-lang/crates.io-index",
                  "non-registry dependency needs separate attribution")
            name = pkg["name"] + "-" + pkg["version"]
            source = opt.vendor / name
            sums = json.loads((source / ".cargo-checksum.json").read_bytes())
            check(sums["package"] == pkg["checksum"], "registry checksum differs: " + name)
            check(digest(source / "Cargo.toml") == sums["files"]["Cargo.toml"], "changed Cargo metadata: " + name)
            manifest = tomllib.loads((source / "Cargo.toml").read_text())["package"]
            files = local_licenses(source)
            if manifest.get("license-file"):
                legal_file = str(path_name(manifest["license-file"]))
                files[legal_file] = source / legal_file
            for rel, path in files.items():
                check(sums["files"].get(rel) == digest(path), "unverified vendor notice: " + name + "/" + rel)
            add("cargo/" + name, files, declared_license=manifest.get("license"),
                registry_checksum=pkg["checksum"])
        rust = re.match(r"rustc (\d+\.\d+\.\d+)", meta["rust"])
        check(rust, "unrecognized Rust version in manifest")
        supplied = opt.rust_docs_version
        if not supplied:
            # A rustup sysroot records the compiler version in its installed channel manifest.
            channel = opt.rust_docs.parents[2] / "lib/rustlib/multirust-channel-manifest.toml"
            data = tomllib.loads(channel.read_text())
            supplied = data["pkg"]["rustc"]["version"].split()[0]
        exact = supplied == rust[1]
        check(exact,
              "Rust notice version differs from release; supply matching rust-docs")
        docs = opt.rust_docs
        check((docs / "COPYRIGHT-library.html").is_file(), "Rust standard-library notices missing")
        files = {"COPYRIGHT-library.html": docs / "COPYRIGHT-library.html"}
        if (docs / "COPYRIGHT").is_file():
            files["COPYRIGHT"] = docs / "COPYRIGHT"
        for file in sorted((docs / "licenses").glob("*.txt")):
            files["licenses/" + file.name] = file
        check("licenses/Apache-2.0.txt" in files and "licenses/MIT.txt" in files, "Rust license texts missing")
        add("rust", files, compiler_version=rust[1], notices_version=supplied,
            exact_version=exact)
    llvm_libs = [a["name"] for a in meta.get("artifacts", [])
                 if re.search(r"/libLLVM[^/]*\.so(?:\.|$)", a["name"])]
    if llvm_libs:
        check(opt.llvm_notices and opt.llvm_version, "bundled libLLVM requires --llvm-notices and --llvm-version")
        for lib in llvm_libs:
            major = re.search(r"(?:LLVM-|\.so\.)([0-9]+)", lib)
            check(major and opt.llvm_version.split(".")[0] == major[1], "LLVM notice major differs from library")
        add("llvm", local_licenses(opt.llvm_notices), version=opt.llvm_version, bundled_files=llvm_libs)
    dest = root / LEGAL / "INDEX.json"
    dest.write_text(json.dumps(index, ensure_ascii=False, sort_keys=True, indent=2) + "\n")
    dest.chmod(0o644)
    (root / "LICENSE").chmod(0o644)
    return index


def pack(tree, output, zipped):
    paths = sorted(tree.rglob("*"), key=lambda p: p.relative_to(tree).as_posix())
    if zipped:
        with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as arc:
            for p in paths:
                folder = p.is_dir()
                rel = p.relative_to(tree).as_posix() + ("/" if folder else "")
                info = zipfile.ZipInfo(rel, date_time=(1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.compress_type = zipfile.ZIP_DEFLATED
                mode = (stat.S_IFDIR | 0o755) if folder else (stat.S_IFREG | (p.stat().st_mode & 0o777))
                info.external_attr = (mode << 16) | (16 if folder else 0)
                if folder:
                    arc.writestr(info, b"")
                else:
                    info.file_size = p.stat().st_size
                    with arc.open(info, "w") as raw, p.open("rb") as src:
                        shutil.copyfileobj(src, raw, 1024 * 1024)
    else:
        with output.open("wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw,
                mtime=0, compresslevel=6) as gz, tarfile.open(fileobj=gz, mode="w|") as arc:
            for p in paths:
                folder = p.is_dir()
                info = tarfile.TarInfo(p.relative_to(tree).as_posix())
                info.mode = 0o755 if folder else p.stat().st_mode & 0o777
                info.type = tarfile.DIRTYPE if folder else tarfile.REGTYPE
                info.size = 0 if folder else p.stat().st_size
                with (contextlib.nullcontext() if folder else p.open("rb")) as src:
                    arc.addfile(info, src)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--vendor", type=Path, help="Cargo vendor directory with .cargo-checksum.json")
    parser.add_argument("--rust-docs", type=Path, help="rustup share/doc/rust or matching distro legal text directory")
    parser.add_argument("--rust-docs-version", help="Rust version of distro legal texts outside rustup")
    parser.add_argument("--llvm-notices", type=Path, help="libLLVM installation notice file or dedicated license directory")
    parser.add_argument("--llvm-version", help="installed libLLVM notice source version")
    parser.add_argument("--zig-prefix", type=Path, help="matching host Zig prefix for a Windows GNU package")
    opt = parser.parse_args()
    output = opt.output.resolve()
    check(output.name.endswith((".tgz", ".tar.gz", ".zip")), "choose a .tgz/.tar.gz or .zip output")
    check(not output.exists(), "output must be new")
    check(not (opt.package.is_dir() and output.is_relative_to(opt.package.resolve())), "output inside input")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".aether-notices-", dir=output.parent) as scratch:
        temp = Path(scratch)
        tree, target = temp / "input", temp / "sealed"
        tree.mkdir()
        unpack(opt.package, tree)
        root, meta, windows, original = validate(tree)
        idx = add_notices(root, meta, windows, opt)
        expected = inventory(tree)
        check(all(expected[key] == value for key, value in original.items()), "original payload changed")
        pack(tree, target, output.suffix == ".zip")
        restored = temp / "restored"
        restored.mkdir()
        unpack(target, restored)
        check(inventory(restored) == expected, "sealed archive readback failed")
        # Publish only after full readback. A concurrent writer's existing output is preserved.
        os.link(target, output)
        print(json.dumps({"output": output.name, "sha256": digest(output), "source_commit": meta["source_commit"],
                          "preserved_files": len(original), "preserved_manifest_sha256": digest(root / "manifest.json"),
                          "added_files": len(expected) - len(original), "notice_components": len(idx["components"]),
                          "rust_notices_exact": next((c["exact_version"] for c in idx["components"]
                                                     if c["directory"] == "rust"), None)}, indent=2))


if __name__ == "__main__":
    main()
