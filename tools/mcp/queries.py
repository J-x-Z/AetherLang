"""Project-scoped queries over aethc's checked source index."""
from bisect import bisect_right
from itertools import chain
from pathlib import Path
from threading import Thread
import json
import os
import shutil
import signal
import subprocess
import time


def executable(command):
    found = shutil.which(str(command))
    if not found:
        raise ValueError(f"Executable not found: {str(command)[:256]}")
    # Preserve symlink names. The installed tools may dispatch by argv[0].
    return os.path.abspath(found)


def stop_analysis(child):
    # JXZ execs the compiler on POSIX and waits for a compiler child on Windows.
    if os.name == "nt":
        taskkill = Path(os.environ["SystemRoot"]) / "System32" / "taskkill.exe"
        try:
            subprocess.run([taskkill, "/pid", str(child.pid), "/t", "/f"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
        finally:
            child.kill()
    else:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def invoke(argv, root, env, stdout_limit, timeout=60):
    # Bound both streams and their wall time, including any compiler child
    # that still holds a pipe inherited from the project driver.
    with subprocess.Popen(argv, cwd=root, env=env, stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          start_new_session=os.name != "nt") as child:
        streams = {}
        overflow = []

        def read(name, pipe, limit):
            data = pipe.read(limit + 1)
            if len(data) > limit:
                overflow.append(name)
                stop_analysis(child)
            streams[name] = data

        readers = [Thread(target=read, args=(name, pipe, limit)) for name, pipe, limit in
                   (("stdout", child.stdout, stdout_limit), ("stderr", child.stderr, 256 * 1024))]
        deadline = time.monotonic() + timeout
        for reader in readers:
            reader.start()
        try:
            for reader in readers:
                reader.join(max(0, deadline - time.monotonic()))
            if any(reader.is_alive() for reader in readers):
                raise subprocess.TimeoutExpired(argv, timeout)
            child.wait(timeout=max(0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired as exc:
            stop_analysis(child)
            for reader in readers:
                reader.join()
            raise ValueError(f"Compiler analysis exceeded {timeout} seconds.") from exc
        if overflow:
            raise ValueError("Compiler output exceeds the analysis limit; choose a smaller entry.")
        return child.returncode, streams["stdout"], streams["stderr"]

class Source:
    """Translate byte spans from either compiler through the actual UTF-8 file."""
    def __init__(self, path):
        self.data = path.read_bytes()
        self.starts = [0, *(i + 1 for i, b in enumerate(self.data) if b == 10)]

    def offset(self, line, character):
        if not 0 <= line < len(self.starts):
            raise ValueError("Line is outside the file.")
        start = self.starts[line]
        end = self.starts[line + 1] - 1 if line + 1 < len(self.starts) else len(self.data)
        if end > start and self.data[end - 1] == 13:
            end -= 1
        units = self.data[start:end].decode("utf-8").encode("utf-16-le")
        if not 0 <= 2 * character <= len(units):
            raise ValueError("Character is outside the line.")
        try:
            prefix = units[:2 * character].decode("utf-16-le")
        except UnicodeDecodeError as exc:
            raise ValueError("Character splits a UTF-16 surrogate pair.") from exc
        return start + len(prefix.encode("utf-8"))

    def position(self, byte):
        if not 0 <= byte <= len(self.data):
            raise ValueError("Source changed during analysis; repeat the query.")
        line = bisect_right(self.starts, byte) - 1
        prefix = self.data[self.starts[line]:byte].decode("utf-8")
        return {"line": line, "character": len(prefix.encode("utf-16-le")) // 2}


class Locations:
    def __init__(self, root):
        self.root = root
        self.names = {}
        self.sources = {}

    def relative(self, name):
        if name not in self.names:
            path = (self.root / name).resolve()
            self.names[name] = path.relative_to(self.root).as_posix() if path.is_relative_to(self.root) else None
        return self.names[name]

    def source(self, name):
        if name not in self.sources:
            self.sources[name] = Source(self.root / name)
        return self.sources[name]

    def show(self, location):
        if location is None:
            return None
        name = self.relative(location["file"])
        if name is None:
            return {"external": True}
        if "utf16_column" in location and "end_utf16_column" in location:
            start = {"line": location["line"] - 1, "character": location["utf16_column"]}
            end = {"line": location["end_line"] - 1, "character": location["end_utf16_column"]}
        else:
            # The 0.1.0 Rust seed only serializes byte spans and scalar columns.
            source = self.source(name)
            start, end = source.position(location["start"]), source.position(location["end"])
        return {"file": name, "range": {"start": start, "end": end}}


def page(items, args, render):
    start, limit = args["offset"], args["limit"]
    end = start + limit
    return {"items": [render(item) for item in items[start:end]], "total": len(items),
            "nextOffset": end if end < len(items) else None}


def location_key(location):
    return location["file"], location["start"], location["end"]


def symbol_at(index, locs, file, byte):
    # Resolve the narrowest half-open occurrence, preferring references on a tie.
    candidates = chain(((ref["location"], ref["symbol"], 0) for ref in index["references"]),
                       ((sym["definition"], sym["id"], 1) for sym in index["symbols"]))
    matches = ((loc["end"] - loc["start"], rank, sid) for loc, sid, rank in candidates
               if loc["start"] <= byte < loc["end"] and locs.relative(loc["file"]) == file)
    return min(matches, default=(0, 0, None))[2]


def display_text(key, value, limit):
    if len(value) <= limit:
        return {key: value}
    return {key: value[:limit], key + "Truncated": True}


class Project:
    def __init__(self, root, compiler="aethc", entry=None, jxz="jxz"):
        self.root = Path(root).resolve(strict=True)
        if not self.root.is_dir():
            raise ValueError("Project root must be a directory.")
        self.compiler = executable(compiler)
        self.jxz = jxz
        self.entry = entry
        self.env = {**os.environ, "AETHC": self.compiler}
        status, version, _ = invoke([self.compiler, "--version"], self.root, self.env, 4096)
        if status or not version.strip():
            raise ValueError("Could not identify the configured Aether compiler.")
        self.version = version.decode().strip()

    def source_name(self, value):
        path = Path(value)
        if path.is_absolute() or path.drive or ".." in path.parts:
            raise ValueError("Use a project-relative source path.")
        path = (self.root / path).resolve()
        if not path.is_relative_to(self.root) or not path.is_file() or path.suffix not in (".aeth", ".ath"):
            raise ValueError("Expected an existing .aeth or .ath file inside the project.")
        return path.relative_to(self.root).as_posix()

    def call(self, name, args):
        cmd = "check" if name == "aether_check" else "symbols"
        entry = args.get("entry", self.entry)
        file = self.source_name(args["file"]) if "file" in args else None
        locs = Locations(self.root)
        if file is not None:
            offset = locs.source(file).offset(args["line"], args["character"])
        if entry is not None:
            entry = self.source_name(entry)
        if (self.root / "Jxz.toml").is_file():
            argv = [executable(self.jxz), cmd]
            if entry is not None:
                argv.append(str(self.root / entry))
        else:
            if entry is None:
                entry = next((p for p in ("src/main.aeth", "main.aeth", "src/main.ath", "main.ath")
                              if (self.root / p).is_file()), None)
                if entry is None:
                    raise ValueError("Choose a project-relative entry or configure --entry.")
                entry = self.source_name(entry)
            argv = [self.compiler, cmd, "--error-format", "json", "--", str(self.root / entry)]
        status, stdout, stderr = invoke(argv, self.root, self.env, 32 * 1024 * 1024)
        result = {"schemaVersion": 1, "compiler": self.version, "positionEncoding": "utf-16",
                  "entry": entry, "ok": status == 0}
        if status:
            try:
                diagnostic = json.loads(stderr)
                if status != 1 or not isinstance(diagnostic, dict) or "message" not in diagnostic:
                    raise ValueError()
            except (UnicodeError, ValueError):
                message = (stderr or stdout).decode("utf-8", errors="replace").strip()[:1024]
                if status == 2 and "jxz: unknown command" in message:
                    raise ValueError("Select a JXZ with check and symbols support using --jxz.") from None
                raise ValueError(f"Analysis command failed ({status}): {message}") from None
            result["diagnostics"] = [{"code": diagnostic.get("code"), "severity": diagnostic.get("severity"),
                                      **display_text("message", diagnostic["message"], 2048), "location": locs.show(diagnostic.get("location"))}]
            return result
        if cmd == "check":
            return {**result, "diagnostics": []}

        index = json.loads(stdout)
        def describe(symbol):
            return {"name": symbol["name"], "kind": symbol["kind"], **display_text("type", symbol["type_name"], 512),
                    "definition": locs.show(symbol["definition"])}
        if name == "aether_symbols":
            term = args["query"].casefold()
            matches = [s for s in index["symbols"] if term in s["name"].casefold()
                       and locs.relative(s["definition"]["file"]) is not None]
            matches.sort(key=lambda s: (s["name"].casefold() != term, s["name"],
                                        *location_key(s["definition"])))
            return {**result, **page(matches, args, describe)}
        sid = symbol_at(index, locs, file, offset)
        symbol = next((s for s in index["symbols"] if s["id"] == sid), None)
        result["symbol"] = describe(symbol) if symbol is not None else None
        if name == "aether_references":
            refs = {location_key(r["location"]): r["location"] for r in index["references"]
                    if r["symbol"] == sid and locs.relative(r["location"]["file"]) is not None}
            result.update(page([refs[k] for k in sorted(refs)], args, locs.show))
        return result
