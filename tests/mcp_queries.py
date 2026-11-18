#!/usr/bin/env python3
"""Check real compiler/JXZ queries, UTF-16 ranges, pagination and on-disk edits."""
import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--compiler", required=True, type=Path)
parser.add_argument("--jxz", required=True, type=Path)
parser.add_argument("--work-dir", required=True, type=Path)
parser.add_argument("--server", type=Path, default=REPO/"tools/mcp/aether_mcp.py",
                    help="Packaged MCP launcher or Python entry point to exercise")
args = parser.parse_args()
server = args.server.resolve(strict=True)
server_command = [sys.executable, "-B", server] if server.suffix == ".py" else [server]
base = args.work_dir.resolve()
base.mkdir(parents=True, exist_ok=True)


class Client:
    def __init__(self, root):
        self.pipe = subprocess.Popen(
            [*server_command, "--root", root,
             "--compiler", args.compiler.resolve(), "--jxz", args.jxz.resolve()],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        self.number = 0

    def send(self, method, params, notify=False):
        message = {"jsonrpc": "2.0", "method": method, "params": params}
        if not notify:
            self.number += 1
            message["id"] = self.number
        self.pipe.stdin.write(json.dumps(message).encode() + b"\n")
        self.pipe.stdin.flush()
        if not notify:
            reply = json.loads(self.pipe.stdout.readline())
            assert reply["id"] == self.number and "result" in reply, reply
            return reply["result"]

    def __enter__(self):
        self.send("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                 "clientInfo": {"name": "aether-integration", "version": "1"}})
        self.send("notifications/initialized", {}, notify=True)
        return self

    def __exit__(self, *exc):
        self.pipe.stdin.close()
        assert self.pipe.wait(timeout=15) == 0
        self.pipe.stdout.close()

    def call(self, tool, expect_error=False, **values):
        reply = self.send("tools/call", {"name": "aether_" + tool, "arguments": values})
        assert reply["isError"] == expect_error, reply
        data = reply["structuredContent"]
        assert json.loads(reply["content"][0]["text"]) == data
        return data


def save(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode("utf-8"))


with tempfile.TemporaryDirectory(dir=base) as folder:
    root = Path(folder) / "application"
    entry = root / "app Ω" / "start.aeth"
    lib = root / "vendor" / "geometry" / "src" / "lib.aeth"
    lib_rel = lib.relative_to(root).as_posix()
    main_rel = entry.relative_to(root).as_posix()
    save(root/"Jxz.toml", "[package]\nname='navigation'\n[build]\nmain='app Ω/start.aeth'\n"
         "link_objects=['not-yet-built.o']\n[dependencies]\ngeometry={path='vendor/geometry'}\n")
    save(lib.parents[1]/"Jxz.toml", "[package]\nname='geometry'\n")
    library = "pub fn square(x:i32)->i32{return x*x;}\n"
    save(lib, library)
    line = 'fn main()->i32{let s:*u8="😀";return square(3)+square(4);}'
    save(entry, "use geometry::square;\r\n" + line + "\r\n")
    before = {p.relative_to(root) for p in root.rglob("*")}
    pos = dict(file=main_rel, line=1, character=len(line[:line.index("square")].encode("utf-16-le"))//2)
    with Client(root) as client:
        assert client.call("check")["ok"]
        result = client.call("definition", **pos)
        symbol = result["symbol"]
        location = symbol["definition"]
        assert result["positionEncoding"] == "utf-16" and result["schemaVersion"] == 1
        assert symbol["name"] == "square" and location == {
            "file": lib_rel, "range": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 13}}}
        result = client.call("symbols", query="SQUARE", limit=1)
        assert result["total"] == 1 and result["items"] == [symbol] and result["nextOffset"] is None
        first = client.call("references", **pos, limit=1)
        assert first["total"] >= 2 and first["nextOffset"] == 1
        uses = first["items"] + client.call("references", **pos, offset=1)["items"]
        columns = [u["range"]["start"]["character"] for u in uses if u["file"] == main_rel and u["range"]["start"]["line"] == 1]
        assert columns == [pos["character"], pos["character"] + len("square(3)+")], uses
        assert client.call("references", **pos, offset=100)["items"] == []
        assert client.call("definition", file=main_rel, line=1, character=0)["symbol"] is None
        for bad_column in (line.index("😀") + 1, len(line) + 5):
            assert client.call("definition", file=main_rel, line=1, character=bad_column, expect_error=True)
        assert client.call("symbols", query="x", entry="../outside.aeth", expect_error=True)
        assert client.call("check", entry=main_rel)["ok"]
        save(lib, "\n" + library)
        moved = client.call("definition", **pos)["symbol"]["definition"]
        assert moved["range"]["start"]["line"] == 1, moved
        save(lib, "pub fn square(x:i32)->i32{return true;}\n")
        for tool, params in [("check", {}), ("definition", pos)]:
            broken = client.call(tool, **params)
            assert not broken["ok"] and broken["diagnostics"][0]["location"]["file"] == lib_rel, broken
        save(lib, library)
        assert client.call("check")["ok"]
    assert {p.relative_to(root) for p in root.rglob("*")} == before
    # Bare Script root exercises the other normal project entry on both compilers.
    script = Path(folder) / "script"
    save(script/"main.ath", "def answer() -> i32:\n    return 7\ndef main() -> i32:\n    return answer()\n")
    with Client(script) as client:
        check = client.call("check")
        assert check["ok"] and check["entry"] == "main.ath", check
        symbol = client.call("definition", file="main.ath", line=3, character=12)["symbol"]
        assert symbol["name"] == "answer" and symbol["definition"]["range"]["start"]["line"] == 0, symbol
    variants = Path(folder) / "variants"
    enums = ["pub enum Event<T>{Hit(T),Empty}", "pub enum Other{Hit(i32),Empty}",
             "pub enum Spare<T>{Retained(T),Vacant}"]
    save(variants/"kinds.aeth", "\n".join(enums))
    lines = [
        "use kinds::{Event,Other};",
        "fn number(e:Event<i32>)->i32{match e{Hit(x)=>x,Event::Empty=>0}}",
        "fn flag(e:Event<bool>)->bool{match e{Event::Hit(x)=>x,Event::Empty=>false}}",
        "fn second(o:Other)->i32{match o{Other::Hit(x)=>x,Other::Empty=>0}}",
        "fn main()->i32{let n:Event<i32>=Event::Hit(7);let b:Event<bool>=Event::Hit(true);"
        "let o:Other=Other::Hit(2);if flag(b){return number(n)+second(o);}return 0;}",
    ]
    save(variants/"main.aeth", "\n".join(lines))
    cases = {
        (0, "Hit"): [(1,"Hit(x)"), (2,"Hit(x)"), (4,"Hit(7)"), (4,"Hit(true)")],
        (0, "Empty"): [(1,"Empty"), (2,"Empty")],
        (1, "Hit"): [(3,"Hit(x)"), (4,"Hit(2)")],
        (1, "Empty"): [(3,"Empty")],
    }
    with Client(variants) as client:
        assert client.call("check")["ok"]
        unused = client.call("symbols", query="Retained")["items"]
        assert len(unused) == 1 and unused[0]["kind"] == "variant", unused
        assert unused[0]["definition"]["range"]["start"] == dict(line=2, character=enums[2].index("Retained"))
        for (row, label), sites in cases.items():
            source = dict(file="kinds.aeth", line=row, character=enums[row].index(label))
            uses = client.call("references", **source)["items"]
            expected = {(line, lines[line].index(token)) for line, token in sites}
            observed = {(u["range"]["start"]["line"], u["range"]["start"]["character"])
                        for u in uses if u["file"] == "main.aeth"}
            assert observed == expected, (source, uses)
            for line, column in expected:
                symbol = client.call("definition", file="main.aeth", line=line, character=column)["symbol"]
                location = symbol["definition"]
                assert symbol["kind"] == "variant" and symbol["name"] == label
                assert location["file"] == source["file"]
                assert location["range"]["start"] == dict(line=row, character=source["character"])
        # Compiler-generated match temporaries and anonymous payload slots are internal.
        for file, row, column in [("main.aeth",1,lines[1].index("match")), ("kinds.aeth",1,enums[1].index("i32"))]:
            assert client.call("definition",file=file,line=row,character=column)["symbol"] is None
    templates = Path(folder) / "templates"
    declarations = [
        "pub struct Pair<T>{left:T,right:T}",
        "pub fn identity<T>(input:T)->T{let copy:T=input;return copy;}",
        "pub struct Unused<T>{unread:T}",
        "pub fn inactive<T>(spare:T)->T{return spare;}",
    ]
    program = [
        "use generic::{Pair,identity};",
        "fn main()->i32{",
        "let n:Pair<i32>=Pair{left:1,right:2};",
        "let b:Pair<bool>=Pair{left:true,right:false};",
        "if identity(b.left){return identity(n.left);}return 0;}",
    ]
    save(templates/"generic.aeth", "\n".join(declarations))
    save(templates/"main.aeth", "\n".join(program))
    with Client(templates) as client:
        assert client.call("check")["ok"]
        for row, name, kind in [
            (0,"Pair","struct"), (0,"left","field"), (0,"right","field"),
            (1,"identity","function"), (1,"input","parameter"), (1,"copy","variable"),
            (2,"Unused","struct"), (2,"unread","field"),
            (3,"inactive","function"), (3,"spare","parameter"),
        ]:
            found = client.call("symbols", query=name)["items"]
            assert len(found)==1 and found[0]["name"]==name and found[0]["kind"]==kind, (name,found)
            assert found[0]["definition"] == {
                "file":"generic.aeth", "range":{
                    "start":{"line":row,"character":declarations[row].index(name)},
                    "end":{"line":row,"character":declarations[row].index(name)+len(name)}}}
            if kind not in ("struct",):
                assert "T" in found[0]["type"], (name,found)
        groups = [
            (0,"Pair",[(2,"Pair{"),(3,"Pair{")]),
            (0,"left",[(2,"left:"),(3,"left:"),(4,"left){"),(4,"left);")]),
            (1,"identity",[(4,"identity(b"),(4,"identity(n")]),
        ]
        for row, name, sites in groups:
            declaration = dict(file="generic.aeth",line=row,character=declarations[row].index(name))
            expected = {(line,program[line].index(token)) for line,token in sites}
            refs = client.call("references", **declaration)["items"]
            assert {(r["range"]["start"]["line"],r["range"]["start"]["character"])
                    for r in refs if r["file"]=="main.aeth"} == expected, (name,refs)
            for line,column in expected:
                selected=client.call("definition",file="main.aeth",line=line,character=column)["symbol"]
                assert selected["name"]==name and selected["definition"]["file"]=="generic.aeth", selected
print("MCP project, Script, enum and generic declaration queries passed")
