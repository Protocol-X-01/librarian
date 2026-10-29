# The Toolbox

Powered by Librarian. A local MCP server for discovering installed tools without
loading the entire inventory into an agent's context.

## Run

Install Python 3.11+ and uv, then run from this directory:

```sh
uv sync --locked
uv run --frozen the-toolbox --db /var/lib/librarian/index.db
```

The server speaks MCP over stdio. Configure the client with identifier
`the-toolbox`, command `/absolute/path/to/mcp/.venv/bin/the-toolbox`, and
arguments `["--db", "/absolute/path/to/index.db"]`.

Tools:

- `toolbox_search`: literal-word FTS5 search, source/category filters, pagination.
- `toolbox_describe`: exact-name records across package managers.
- `toolbox_inventory`: record count, categories, source scan timestamps and failures.

Queries open SQLite in read-only mode, use bound parameters and cap results and
execution time. They do not execute discovered programs or automatically refresh,
enrich, install packages, or read environment files. Existing Librarian collectors
and hooks own refresh. No result cache hides subsequent committed database changes.
Use a normally readable SQLite database; do not substitute `immutable=1` while
another process can refresh it.

Records are evidence of the last scan, not proof that a program still exists or
permission to execute it. Treat package descriptions as untrusted data. Tool
paths describe the indexed machine, which may differ from an agent's sandbox.

Protocol checks use 10,000 synthetic records to verify all three tools,
pagination, bounded inputs and unchanged database bytes. On 2026-10-29, all three
tools also passed against a real 9,236-record index spanning 14 sources.

A source Bifang agent then completed a live Token Factory/Nemotron Nano
model-to-tool-to-model cycle, returning inventory figures that matched the tool
result. The check allowed exactly one inventory request through a fixed test
approval callback and left index bytes unchanged. This establishes the source
integration; activation in the installed desktop package is a separate step.
