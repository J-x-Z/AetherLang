#!/usr/bin/env python3
"""Serve Aether compiler queries over newline-delimited MCP stdio."""
import argparse
import json
import os
import sys
from pathlib import Path

VERSIONS = ("2025-11-25", "2025-06-18")
MAX_LINE = 256 * 1024


def string(description, maximum=4096):
    return {"type": "string", "minLength": 1, "maxLength": maximum, "description": description}


def integer(description, minimum=0, maximum=2**31 - 1, **options):
    return {"type": "integer", "minimum": minimum, "maximum": maximum,
            "description": description, **options}


ENTRY = {"entry": string("Project-relative source entry. Omit to use the configured project entry.")}
POSITION = {"file": string("Project-relative source file containing the occurrence."),
            "line": integer("Zero-based source line."),
            "character": integer("Zero-based UTF-16 code unit offset on that line.")}
PAGE = {"offset": integer("Number of matching results to skip.", default=0),
        "limit": integer("Maximum number of matching results to return.", 1, 100, default=20)}


def object_schema(properties):
    return {"type": "object", "properties": properties, "required": list(properties)}


def output_schema(name):
    text = {"type": "string"}
    number = {"type": "integer", "minimum": 0}
    loc = {"$ref": "#/$defs/location"}
    pos = {"$ref": "#/$defs/position"}
    defs = {"position": object_schema({"line": number, "character": number}),
            "location": {"oneOf": [object_schema({"external": {"const": True}}),
                                    object_schema({"file": text, "range": object_schema({"start": pos, "end": pos})})]}}
    diagnostic = object_schema({"code": {"type": ["string", "null"]}, "severity": {"type": ["string", "null"]},
                                "message": text, "location": {"anyOf": [loc, {"type": "null"}]}})
    diagnostic["properties"]["messageTruncated"] = {"type": "boolean"}
    fields = {"schemaVersion": {"const": 1}, "positionEncoding": {"const": "utf-16"},
              "compiler": text, "entry": {"type": ["string", "null"]}, "ok": {"type": "boolean"}}
    common = list(fields)
    fields["diagnostics"] = {"type": "array", "items": diagnostic}
    success = []
    if name == "aether_check":
        success = ["diagnostics"]
    else:
        defs["symbol"] = object_schema({"name": text, "kind": text, "type": text, "definition": loc})
        defs["symbol"]["properties"]["typeTruncated"] = {"type": "boolean"}
        symbol = {"$ref": "#/$defs/symbol"}
        if name != "aether_symbols":
            fields["symbol"] = {"anyOf": [symbol, {"type": "null"}]}
            success.append("symbol")
        if name != "aether_definition":
            fields.update(items={"type": "array", "items": symbol if name == "aether_symbols" else loc},
                          total=number, nextOffset={"type": ["integer", "null"], "minimum": 0})
            success.extend(("items", "total", "nextOffset"))
    fields["error"] = text
    # A completed analysis can have compiler diagnostics. Tool invocation failures
    # (e.g. invalid path or position) carry a separate, actionable error message.
    return {"type": "object", "$defs": defs, "properties": fields,
            "oneOf": [{"required": ["error"]},
                      {"required": common, "if": {"properties": {"ok": {"const": True}}},
                       "then": {"required": success}, "else": {"required": ["diagnostics"]}}]}


def tool(name, description, properties, required=()):
    return {"name": name, "description": description,
            "inputSchema": {"type": "object", "properties": properties,
                            "required": list(required), "additionalProperties": False},
            "outputSchema": output_schema(name),
            "annotations": {"readOnlyHint": True, "openWorldHint": False}}


TOOLS = {item["name"]: item for item in (
    tool("aether_check", "Type-check the entry and imported modules. Return compiler diagnostics.", ENTRY),
    tool("aether_definition", "Resolve the declaration at a source position in the entry's module graph.",
         {**ENTRY, **POSITION}, POSITION),
    tool("aether_references", "Find references to the declaration at a source position in the entry's module graph.",
         {**ENTRY, **POSITION, **PAGE}, POSITION),
    tool("aether_symbols", "Search declarations by name in the entry's module graph.",
         {**ENTRY, "query": string("Name or substring to search for.", 512), **PAGE}, ["query"]),
)}


def arguments_for(schema, arguments):
    properties = schema["properties"]
    for key in arguments:
        if key not in properties:
            raise ValueError(f"Unknown argument {key[:80]!r}.")
    for key in schema["required"]:
        if key not in arguments:
            raise ValueError(f"Missing required argument {key!r}.")
    result = {}
    for key, spec in properties.items():
        if key not in arguments:
            if "default" in spec:
                result[key] = spec["default"]
            continue
        value = arguments[key]
        if spec["type"] == "string":
            if not isinstance(value, str) or not spec["minLength"] <= len(value) <= spec["maxLength"]:
                raise ValueError(f"{key} must be a string of 1 to {spec['maxLength']} characters.")
            if "\x00" in value:
                raise ValueError(f"{key} contains a null character.")
            try:
                value.encode("utf-8")
            except UnicodeEncodeError as exc:
                raise ValueError(f"{key} contains an unpaired surrogate.") from exc
        elif type(value) is not int or not spec["minimum"] <= value <= spec["maximum"]:
            raise ValueError(f"{key} must be an integer from {spec['minimum']} to {spec['maximum']}.")
        result[key] = value
    return result


def compact(value):
    return json.dumps(value, separators=(",", ":"), ensure_ascii=False, allow_nan=False)


def error(request_id, code, message):
    return {"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}}


def tool_result(data, failed=False):
    return {"content": [{"type": "text", "text": compact(data)}],
            "structuredContent": data, "isError": failed}


class Session:
    def __init__(self, project):
        self.project = project
        self.phase = "new"

    def dispatch(self, message):
        if not isinstance(message, dict) or message.get("jsonrpc") != "2.0":
            return error(None, -32600, "Expected a JSON-RPC 2.0 request.")
        method = message.get("method")
        if not isinstance(method, str):
            # This server never sends requests. A response has no reply.
            if "id" in message and ("result" in message or "error" in message):
                return None
            return error(None, -32600, "Expected a request method.")
        params = message.get("params", {})
        if "id" not in message:
            if method == "notifications/initialized" and self.phase == "negotiated" and isinstance(params, dict):
                self.phase = "ready"
            return None
        request_id = message["id"]
        if type(request_id) not in (int, str):
            return error(None, -32600, "Request id must be an integer or a string.")
        if not isinstance(params, dict):
            return error(request_id, -32602, "Expected a parameter object.")
        if method == "ping":
            result = {}
        elif method == "initialize":
            if self.phase != "new":
                return error(request_id, -32600, "Session already initialized.")
            version = params.get("protocolVersion")
            client = params.get("clientInfo")
            if not (isinstance(version, str) and isinstance(params.get("capabilities"), dict)
                    and isinstance(client, dict) and isinstance(client.get("name"), str)
                    and isinstance(client.get("version"), str)):
                return error(request_id, -32602, "Expected protocolVersion, capabilities and clientInfo.")
            result = {"protocolVersion": version if version in VERSIONS else VERSIONS[0],
                      "capabilities": {"tools": {}},
                      "serverInfo": {"name": "aether-mcp", "version": "0.1.0"}}
            self.phase = "negotiated"
        elif self.phase != "ready":
            return error(request_id, -32002, "Initialize the session and send notifications/initialized first.")
        elif method == "tools/list":
            if "cursor" in params:
                return error(request_id, -32602, "All tools are returned in one page. Omit cursor.")
            result = {"tools": list(TOOLS.values())}
        elif method == "tools/call":
            name = params.get("name")
            arguments = params.get("arguments", {})
            if not isinstance(name, str) or name not in TOOLS:
                return error(request_id, -32602, "Unknown tool. Use tools/list.")
            if not isinstance(arguments, dict):
                return error(request_id, -32602, "Expected an arguments object.")
            try:
                values = arguments_for(TOOLS[name]["inputSchema"], arguments)
                result = tool_result(self.project.call(name, values))
            except (ValueError, OSError) as exc:
                result = tool_result({"error": str(exc)[:2048]}, failed=True)
            except Exception as exc:
                # Keep unrelated requests alive after an unexpected compiler-adapter failure.
                print(f"aether-mcp: {type(exc).__name__}: {str(exc)[:2048]}", file=sys.stderr)
                return error(request_id, -32603, "Compiler query failed unexpectedly. See server stderr.")
        else:
            return error(request_id, -32601, "Method not found.")
        return {"jsonrpc": "2.0", "id": request_id, "result": result}


def invalid_constant(value):
    raise ValueError(f"Invalid JSON literal {value}")


def serve(project, source, sink):
    session = Session(project)
    while True:
        line = source.readline(MAX_LINE + 1)
        if not line:
            return
        if len(line) > MAX_LINE:
            while line and not line.endswith(b"\n"):
                line = source.readline(MAX_LINE + 1)
            reply = error(None, -32700, "Input line exceeds the 256 KiB limit.")
        else:
            try:
                message = json.loads(line.decode("utf-8"), parse_constant=invalid_constant)
            except (ValueError, RecursionError):
                reply = error(None, -32700, "Invalid UTF-8 or JSON.")
            else:
                reply = session.dispatch(message)
        if reply is not None:
            # backslashreplace preserves JSON escapes for unpaired surrogates in request ids.
            sink.write(compact(reply).encode("utf-8", errors="backslashreplace") + b"\n")
            sink.flush()


def default_command(name):
    # Installed tools and unpacked archives have their matched drivers beside the manifest.
    bundle = Path(__file__).resolve().parent.parent.parent
    bundled = bundle / (name + ".exe" if os.name == "nt" else name)
    if (bundle / "manifest.json").is_file() and bundled.is_file():
        return str(bundled)
    return name


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path, help="Project directory")
    parser.add_argument("--entry", help="Default entry relative to the project directory")
    parser.add_argument("--compiler", default=default_command("aethc"), help="Compiler executable (default: bundled aethc or PATH)")
    parser.add_argument("--jxz", default=default_command("jxz"), help="JXZ executable (default: bundled jxz or PATH)")
    args = parser.parse_args()
    # Launching from a verified, unpacked release must preserve its inventory.
    sys.dont_write_bytecode = True
    from queries import Project

    try:
        project = Project(args.root, args.compiler, args.entry, jxz=args.jxz)
    except (ValueError, OSError) as exc:
        parser.exit(2, f"{parser.prog}: {exc}\n")
    try:
        serve(project, sys.stdin.buffer, sys.stdout.buffer)
    except BrokenPipeError:
        pass


if __name__ == "__main__":
    main()
