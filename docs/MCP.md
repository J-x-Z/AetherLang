# AetherLang MCP server

The local stdio server lets a client type-check a saved Aether project, find a
declaration or its uses, and search symbols by name. It works with Core
(`.aeth`) and Script (`.ath`) and uses the selected compiler's semantic
analysis. Python 3.9 or newer is sufficient. The server uses the standard
library. It implements the 2025-11-25 and 2025-06-18 MCP protocols.

## Start a server

Choose the project directory when registering the server. With a toolchain
built from this revision, a client that accepts `mcpServers` can use this
configuration, substituting its own absolute paths.

```json
{
  "mcpServers": {
    "aetherlang": {
      "command": "/absolute/path/to/toolchain/bin/aether-mcp",
      "args": ["--root", "/absolute/path/to/project"]
    }
  }
}
```

The packaged adapter uses the adjacent compiler and JXZ. From a source
checkout, select built executables explicitly when PATH points to an older
installation. Windows bundles include `aether-mcp.cmd` for the terminal. Clients
that require an executable can launch `python.exe` with the bundled
`tools/mcp/aether_mcp.py` and the same `--root` argument.

```sh
python3 tools/mcp/aether_mcp.py --root "$PWD/jxz" \
  --compiler /absolute/path/to/aethc --jxz /absolute/path/to/jxz
```

A root containing `Jxz.toml` uses `jxz check` and `jxz symbols` from this
revision. They honor the configured entry, module directories and dependency
mappings. Registry dependencies must already be available locally. A bare
root uses the compiler directly. Pass `--entry relative/file.ath` or supply
`entry` in a tool call to select a different entry. A bare root defaults to
the first existing file in `src/main.aeth`, `main.aeth`, `src/main.ath`,
`main.ath` order. Each query reads the saved project again, so changes to
sources, module paths and manifests are visible on the next call.

## Make a query

| Tool | Arguments besides optional `entry` | Result |
| --- | --- | --- |
| `aether_check` | None | `ok` and compiler diagnostics |
| `aether_symbols` | `query`, optional `offset`, `limit` | Page of declarations with name, kind, type and definition. Case-insensitive substring match with exact matches first |
| `aether_definition` | `file`, `line`, `character` | Resolved `symbol`, or `null` when there is no indexed occurrence at the position |
| `aether_references` | Same position, optional `offset`, `limit` | Selected symbol and a page of distinct reference locations, excluding its declaration |

Start with `aether_symbols({"query":"build_project"})` in JXZ. The definition's
`file` and `range.start` can be passed directly to `aether_references`.
Reference pages default to 20 and accept 1–100 items. A final page has
`nextOffset: null`. If the project changes between pages, restart at offset 0.

Every completed analysis includes `schemaVersion`, the compiler version string
and `positionEncoding`. Lines and UTF-16 columns are zero-based, and ranges
exclude their end. Paths are project-relative. A symbol defined in an external
dependency can be identified at its use site, with `{"external":true}` for its
definition. Searches and reference lists include locations inside the project
root. The compiler still reads its configured standard library and dependencies.
A null `entry` means JXZ selected the manifest entry.

A source error returns `ok:false` with the compiler's original diagnostic code,
message and range. Type displays longer than 512 characters and diagnostic
messages longer than 2,048 characters carry an explicit truncation flag. Type
spellings and diagnostic codes can differ between compilers. An invocation
error such as an invalid path, a split surrogate pair or an unavailable
driver returns `isError:true`. Requests have a 256 KiB line limit. Each
analysis has a 60-second time budget and a 32 MiB compiler stdout limit.

The adapter runs compiler analysis commands and doesn't execute project
programs. Normal protocol operations and tool results follow the
[MCP tool specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools).
