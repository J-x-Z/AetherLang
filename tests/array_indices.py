#!/usr/bin/env python3
"""Fixed-array checks and signed/unsigned indexing through both native drivers."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--compiler", required=True, type=Path)
parser.add_argument("--work-dir", required=True, type=Path)
parser.add_argument("--backends", nargs="+", choices=["c", "llvm"], default=["c", "llvm"])
args = parser.parse_args()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
source = Path(__file__).resolve().parent / "regressions" / "array_indices.aeth"
# Clang uses either a breakpoint or an illegal instruction for a language trap.
traps = {0x80000003, 0xc000001d} if os.name == "nt" else {-signal.SIGILL, -signal.SIGTRAP}
records = []

for backend in args.backends:
    for level in (0, 2):
        binary = work / (f"indices-{backend}-{level}" + (".exe" if os.name == "nt" else ""))
        subprocess.run([args.compiler.resolve(), "build", source, "--backend", backend,
                        f"-O{level}", "-o", binary], check=True, timeout=60)
        for case in [[], ["0"], ["1"], ["2"], ["3"], ["4"]]:
            result = subprocess.run([binary, *case], capture_output=True, timeout=10)
            status = result.returncode & 0xffffffff if os.name == "nt" else result.returncode
            records.append(dict(backend=backend, optimization=level, case=case, status=status))
            assert status in (traps if case else {0}), records[-1]

(work / "results.json").write_text(json.dumps(records, indent=2) + "\n")
print(f"{len(records)} array indexing executions passed")
