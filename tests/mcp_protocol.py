#!/usr/bin/env python3
"""Dependency-free wire tests for the Aether MCP transport."""
from contextlib import redirect_stderr
import io
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/mcp"))
from aether_mcp import MAX_LINE, VERSIONS, serve


class Project:
    def __init__(self):
        self.calls = []

    def call(self, name, arguments):
        if arguments.get("entry") == "missing.aeth":
            raise ValueError("Entry does not exist.")
        if arguments.get("entry") == "crash.aeth":
            raise RuntimeError("Test adapter failure.")
        self.calls.append((name, arguments))
        result = {"schemaVersion": 1, "positionEncoding": "utf-16", "compiler": "wire-stub",
                  "entry": arguments.get("entry"), "ok": True}
        if name == "aether_check":
            return {**result, "diagnostics": []}
        if name in ("aether_definition", "aether_references"):
            result["symbol"] = None
        if name in ("aether_symbols", "aether_references"):
            result.update(items=[], total=0, nextOffset=None)
        return result


def request(method, request_id, **params):
    return {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}


def hello(version=VERSIONS[0]):
    return request("initialize", 1, protocolVersion=version, capabilities={},
                   clientInfo={"name": "wire-test", "version": "1"})


READY = {"jsonrpc": "2.0", "method": "notifications/initialized"}


def replies(*messages, project=None):
    source = io.BytesIO(b"".join((m if isinstance(m, bytes) else json.dumps(m).encode()) + b"\n"
                                 for m in messages))
    sink = io.BytesIO()
    serve(project or Project(), source, sink)
    return [json.loads(line) for line in sink.getvalue().splitlines()]


class ProtocolTests(unittest.TestCase):
    def test_lifecycle(self):
        result = replies(request("ping", 0), request("tools/list", 2), hello(), request("tools/list", 3),
                         READY, request("tools/list", 4), hello(), request("unknown", 5))
        self.assertEqual([r["id"] for r in result], [0, 2, 1, 3, 4, 1, 5])
        self.assertEqual(result[0]["result"], {})
        self.assertEqual(result[1]["error"]["code"], -32002)
        self.assertEqual(result[3]["error"]["code"], -32002)
        self.assertEqual(result[2]["result"]["capabilities"], {"tools": {}})
        names = [t["name"] for t in result[4]["result"]["tools"]]
        self.assertEqual(names, ["aether_check", "aether_definition", "aether_references", "aether_symbols"])
        self.assertEqual(result[5]["error"]["code"], -32600)
        self.assertEqual(result[6]["error"]["code"], -32601)

    def test_negotiation(self):
        for requested in (*VERSIONS, "2099-01-01"):
            with self.subTest(version=requested):
                self.assertEqual(replies(hello(requested))[0]["result"]["protocolVersion"],
                                 requested if requested in VERSIONS else VERSIONS[0])

    def test_dispatch(self):
        position = {"file": "lib.aeth", "line": 0, "character": 8}
        cases = {"aether_check": {}, "aether_definition": position, "aether_references": position,
                 "aether_symbols": {"query": "核心", "offset": 20, "limit": 2}}
        project = Project()
        result = replies(hello(), READY, *(request("tools/call", name, name=name, arguments=args)
                                          for name, args in cases.items()), project=project)
        for message in result[1:]:
            name = message["id"]
            payload = message["result"]
            self.assertFalse(payload["isError"])
            self.assertEqual(json.loads(payload["content"][0]["text"]), payload["structuredContent"])
            expected = cases[name]
            if name == "aether_references":
                expected = {**position, "offset": 0, "limit": 20}
            self.assertEqual(dict(project.calls)[name], expected)
            self.assertTrue(payload["structuredContent"]["ok"])

    def test_schema_and_backend_errors(self):
        cases = [
            ("aether_check", {"extra": True}), ("aether_check", {"entry": ""}),
            ("aether_check", {"entry": "a\x00.aeth"}), ("aether_check", {"entry": "\ud800"}),
            ("aether_check", {"entry": "missing.aeth"}), ("aether_symbols", {}),
            ("aether_definition", {"file": "x", "line": 0, "character": False}),
            ("aether_references", {"file": "x", "line": -1, "character": 0}),
            ("aether_symbols", {"query": "x", "offset": 1.5}),
            ("aether_symbols", {"query": "x", "limit": 101}),
        ]
        result = replies(hello(), READY, *(request("tools/call", i, name=name, arguments=args)
                                          for i, (name, args) in enumerate(cases)))
        for reply in result[1:]:
            self.assertTrue(reply["result"]["isError"], reply)
            self.assertTrue(reply["result"]["structuredContent"]["error"])

    def test_invalid_requests_and_notifications(self):
        result = replies(hello(), READY, {"jsonrpc": "2.0", "method": "unknown"},
                         {"jsonrpc": "2.0", "id": 1, "result": {}},
                         request("tools/list", False), request("tools/list", None),
                         request("tools/list", 0, cursor="x"),
                         request("tools/call", "a", name="bad_tool"),
                         request("tools/call", "b", name="aether_check", arguments=[]),
                         request("ping", "still-alive"))
        self.assertEqual([r.get("error", {}).get("code") for r in result],
                         [None, -32600, -32600, -32602, -32602, -32602, None])

    def test_unexpected_adapter_failure_preserves_the_session(self):
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            result = replies(hello(), READY, request("tools/call", 2, name="aether_check",
                             arguments={"entry": "crash.aeth"}), request("ping", 3))
        self.assertEqual(result[1]["error"]["code"], -32603)
        self.assertIn("RuntimeError", stderr.getvalue())
        self.assertEqual(result[2]["result"], {})

    def test_large_and_malformed_frame_recovery(self):
        result = replies(b"x" * (3 * MAX_LINE), b"\xff", b"NaN", b"[", b"[" * 2000,
                         [], hello(), READY, request("ping", "ok"))
        self.assertEqual([r.get("error", {}).get("code") for r in result],
                         [-32700] * 5 + [-32600, None, None])
        self.assertEqual(result[-1], {"jsonrpc": "2.0", "id": "ok", "result": {}})


if __name__ == "__main__":
    unittest.main()
