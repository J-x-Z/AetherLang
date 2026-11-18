#!/bin/sh
# Install the matched compiler, JXZ and standard library from a local release.
exec python3 - "$@" <<'PY'
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import sys

parser = argparse.ArgumentParser(prog="install.sh", description="Install a verified local AetherLang package")
parser.add_argument("--package", required=True, type=Path, help="directory containing manifest.json")
parser.add_argument("--prefix", type=Path, default=Path.home() / ".aetherlang")
parser.add_argument("--manifest-sha256", help="expected digest from the release receipt")
args = parser.parse_args()

def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest() if hasattr(hashlib, "file_digest") else hashlib.sha256(file.read()).hexdigest()

def check_file(base, name, expected):
    path = base / name
    if not path.is_file() or path.is_symlink() or path.resolve() != path:
        raise ValueError(f"expected regular package file: {name}")
    if digest(path) != expected:
        raise ValueError(f"checksum mismatch: {name}")
    return path

def install():
    package = args.package.resolve(strict=True)
    raw = (package / "manifest.json").read_bytes()
    key = hashlib.sha256(raw).hexdigest()
    if args.manifest_sha256 and key != args.manifest_sha256.lower():
        raise ValueError("manifest checksum mismatch")
    m = json.loads(raw)
    if (m["platform"]["system"], m["platform"]["machine"]) != (platform.system(), platform.machine()):
        raise ValueError("package platform does not match this host")
    entries = [(a["name"], a["sha256"]) for a in m["artifacts"]] + list(m["stdlib"]["files"].items())
    names = set()
    for name, checksum in entries:
        path = PurePosixPath(name)
        if (path.is_absolute() or ".." in path.parts or str(path) != name or not path.parts
                or name in names or name == "manifest.json"):
            raise ValueError(f"invalid or duplicate package path: {name}")
        names.add(name)
    if not {"aethc", "jxz", "stdlib/vec.aeth"} <= names:
        raise ValueError("package is missing its compiler, JXZ or standard library")

    # Older packages may contain only the two required tools. Every compiler or
    # driver actually present in a new package must remain runnable as installed.
    executable_names = names & {"aethc", "aethc-c", "aethc-core", "aethc-native",
                                "jxz", "jxz-c", "jxz-native", "jxz-native-c", "aether-mcp"}

    prefix = args.prefix.resolve()
    prefix.mkdir(parents=True, exist_ok=True)
    # The directory lock serializes installs without a stale lock file after SIGKILL.
    lock = os.open(prefix, os.O_RDONLY | os.O_DIRECTORY)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX)
        store = prefix / ".aether-releases"
        current = prefix / "bin"
        if os.path.lexists(current):
            target = PurePosixPath(os.readlink(current)) if current.is_symlink() else None
            if target is None or target.parent != PurePosixPath(store.name) or len(target.name) != 64 or any(c not in "0123456789abcdef" for c in target.name):
                raise ValueError(f"{current} already exists outside this installer; choose a separate prefix")
        if store.is_symlink():
            raise ValueError(f"unexpected symlink at {store}")
        store.mkdir(mode=0o700, exist_ok=True)
        marker = store / ".format"
        if not marker.exists() and not any(store.iterdir()):
            marker.write_text("aetherlang-install-v1\n")
        if marker.is_symlink() or not marker.is_file() or marker.read_text() != "aetherlang-install-v1\n":
            raise ValueError(f"unrecognized installation store: {store}")
        release = store / key
        if release.is_symlink():
            raise ValueError("unexpected symbolic release directory")
        for name, checksum in entries:
            check_file(package, name, checksum)

        # Reserved staging paths live inside the private version store. The
        # active bin link always refers to a complete immutable version.
        stage = store / ".staging"
        if stage.exists():
            shutil.rmtree(stage)
        stage.mkdir(mode=0o700)
        try:
            if not release.exists():
                for name, checksum in entries:
                    dest = stage / name
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(package / name, dest)
                    check_file(stage, name, checksum)
                if not all(os.access(stage / n, os.X_OK) for n in executable_names):
                    raise ValueError("package tools are missing executable permission")
                (stage / "manifest.json").write_bytes(raw)
                stage.rename(release)
            else:
                for name, checksum in entries:
                    check_file(release, name, checksum)
                check_file(release, "manifest.json", key)
            if not all(os.access(release / n, os.X_OK) for n in executable_names):
                raise ValueError("installed tools are missing executable permission")
            stage.mkdir(exist_ok=True)
            link = stage / "bin"
            link.symlink_to(Path(store.name) / key)
            os.replace(link, current)
        finally:
            if stage.exists():
                shutil.rmtree(stage)
    finally:
        os.close(lock)
    print(f"Installed {m['release_id']} at {current} (manifest {key})")

try:
    install()
except (OSError, ValueError, KeyError, TypeError) as error:
    sys.exit(f"install: {error}")
PY
