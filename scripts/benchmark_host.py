#!/usr/bin/env python3
"""Compare two release compilers on JXZ and a streaming numerical workload."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import time

cli = argparse.ArgumentParser(description=__doc__)
cli.add_argument("--reference", type=Path, required=True)
cli.add_argument("--candidate", type=Path, required=True)
cli.add_argument("--work-dir", type=Path, required=True)
cli.add_argument("--samples", type=int, default=7)
args = cli.parse_args()
if args.samples < 1:
    cli.error("--samples must be positive")
root = Path(__file__).resolve().parents[1]
out = args.work_dir.resolve()
out.mkdir(parents=True, exist_ok=True)
tools = dict(reference=args.reference.resolve(), candidate=args.candidate.resolve())
data = out / "samples.csv"
count = 250000
with data.open("w") as stream:
    stream.write("x,y\n")
    for i in range(count):
        stream.write(f"{i},{2*i+3}\n")
env = dict(os.environ, LC_ALL="C", TMPDIR=str(out))
results = {}
raw = {}
def timed(command):
    timer = ["/usr/bin/time", "-l"] if platform.system() == "Darwin" else ["/usr/bin/time", "-f", "%M maxrss_kib"]
    start = time.perf_counter_ns()
    child = subprocess.run([*timer, *map(str, command)], check=True, cwd=out, env=env, capture_output=True, text=True)
    elapsed = (time.perf_counter_ns() - start) / 1e6
    mem = re.search(r"(\d+)\s+(maximum resident set size|maxrss_kib)", child.stderr)
    if not mem:
        raise RuntimeError(f"missing peak RSS: {child.stderr}")
    return dict(ms=elapsed, rss_bytes=int(mem[1]) * (1024 if mem[2] == "maxrss_kib" else 1)), child.stdout

def digest(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()

for key, compiler in tools.items():
    app = out / f"regress-{key}"
    timed([compiler, "build", root/"examples/analysis/regress.aeth", "--sysroot", root, "--backend", "llvm", "-O2", "-o", app])
    results[key] = dict(compiler_sha256=digest(compiler), compiler_bytes=compiler.stat().st_size, program_bytes=app.stat().st_size)
    raw[key] = {}
for task in ("check", "emit-c", "emit-obj", "run"):
    for index in range(args.samples+1):
        for key in (list(tools) if index%2 == 0 else list(reversed(tools))):
            compiler = tools[key]
            command = [compiler, "check" if task == "check" else "build", root/"jxz/src/main.aeth", "--sysroot", root]
            generated = out / f"{key}.{task}"
            if task.startswith("emit"):
                command.extend(["--"+task, "-O2", "-o", generated])
            if task == "run":
                command = [out/f"regress-{key}", data, "x", "y"]
            sample, stdout = timed(command)
            if task == "run":
                fit = json.loads(stdout)
                assert (fit["n"], fit["slope"], fit["intercept"], fit["r_squared"]) == (count, 2, 3, 1), fit
            if index:
                raw[key].setdefault(task, []).append(sample)
            if index == args.samples:
                measurements = raw[key][task]
                results[key][task] = {metric: statistics.median(row[metric] for row in measurements) for metric in ("ms","rss_bytes")}
                if task.startswith("emit"):
                    results[key][task]["output_bytes"] = generated.stat().st_size
report = dict(host=platform.platform(), data_rows=count, samples=args.samples, source_commit=subprocess.check_output(
    ["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(), compilers={k:str(v) for k,v in tools.items()},
    input_sha256=digest(data), results=results, samples_raw=raw)
(out/"measurements.json").write_text(json.dumps(report, indent=2)+"\n")
print(json.dumps(results, indent=2))
