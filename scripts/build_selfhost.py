#!/usr/bin/env python3
"""Build the regular Aether compiler and compare consecutive self-generated programs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
source = root / "src_aether/core/aethc.aeth"

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def build(seed, work, out, backend):
    work.mkdir(parents=True, exist_ok=True)
    out.parent.mkdir(parents=True, exist_ok=True)
    options = ["--backend", backend, "-O2", "--sysroot", root]
    invocations = []
    def run(*args):
        command = list(map(str, args))
        subprocess.run(command, check=True)
        invocations.append(command)

    # Use a distinct directory per invocation. A failed or concurrent build
    # cannot replace a previously verified installed compiler.
    with tempfile.TemporaryDirectory(prefix="generations-", dir=work) as scratch:
        scratch = Path(scratch)
        first, second = (scratch / name for name in ("stage1", "stage2"))
        run(seed, "build", source, *options, "-o", first)
        run(first, "build", source, *options, "-o", second)
        representations = {}
        for mode, suffix in (("--emit-c", "c"), ("--emit-llvm", "ll")):
            left, right = (scratch / f"stage{i}.{suffix}" for i in (1, 2))
            for compiler, dest in ((first, left), (second, right)):
                run(compiler, source, mode, "--sysroot", root, "-o", dest)
            if sha(left) != sha(right):
                raise RuntimeError(f"consecutive compilers differ on {mode}")
            representations[suffix] = {"sha256": sha(right), "bytes": right.stat().st_size}
        run(second, "--version")
        with tempfile.NamedTemporaryFile(dir=out.parent, prefix=f".{out.name}-", delete=False) as dest:
            tmp = Path(dest.name)
        try:
            shutil.copy2(second, tmp)
            os.replace(tmp, out)
        finally:
            tmp.unlink(missing_ok=True)
        return dict(entry=str(source.relative_to(root)), seed_sha256=sha(seed),
                    executable_sha256=sha(out), backend=backend, fixed_point=representations,
                    commands=invocations)

if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--seed", type=Path, required=True)
    cli.add_argument("--backend", choices=["c", "llvm"], default="llvm")
    cli.add_argument("--work-dir", type=Path, required=True)
    cli.add_argument("--output", type=Path, required=True)
    args = cli.parse_args()
    result = build(args.seed.resolve(strict=True), args.work_dir.resolve(), args.output.resolve(), args.backend)
    (args.work_dir / "regular-bootstrap.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"Verified {args.output} (C and LLVM fixed points)")
