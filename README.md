# librarian

A system-wide index of every executable installed across every package manager on
the box, indexed into a SQLite database any local agent — human or AI — can query
in milliseconds without sudo.

Built because the question *"what tool do I actually have for X?"* is constant in
security work, and the existing answers — `which`, `apropos`, the desktop menu,
`pacman -Ss`, `cargo install --list`, half a dozen others — each cover a slice of
the system. None of them know about the others. The Librarian unifies them.

## Quick example

```
$ librarian stats
Total tools: 10,539

By source:
  pacman         8198    pip             430    desktop      204
  path           1139    pip2            268    aur          189
  cargo            49    npm              28    uv            18
  foundry           5    rustup            4    conda          3
  bun               2    docker            1    sagemath       1

Top 20 categories:
  (none)                 9230    fuzzer                 206
  build-system           202     cracker                199
  webapp                 127     development            104
  wireless               73      reverse-engineering    50
  forensic               44      language-runtime       45
  ...
```

```
$ librarian search "sql injection"
sqlmap        [pacman/webapp]    1.10.5-1
    /usr/bin/sqlmap
    Automatic SQL injection and database takeover tool
sqlbrute      [pacman/webapp]    1.0-8
    /usr/bin/sqlbrute
    Brute forces data out of databases using blind SQL injection.
jsql-injection [pacman/webapp]   0.114-1
    /usr/bin/jsql-injection
    A Java application for automatic SQL database injection.
```

```
$ librarian list --category wireless | head -5
airbase-ng         [pacman/wireless]  /usr/bin/airbase-ng
aircrack-ng        [pacman/wireless]  /usr/bin/aircrack-ng
airdecap-ng        [pacman/wireless]  /usr/bin/airdecap-ng
airdecloak-ng      [pacman/wireless]  /usr/bin/airdecloak-ng
airdrop-ng         [pacman/wireless]  /usr/bin/airdrop-ng
```

Full scan time on a moderately loaded Arch + BlackArch system: ~8 seconds.
Search latency at 10K rows: a few milliseconds (FTS5 + SQLite indexes).

## Sources

| source             | what it captures                                                |
|--------------------|-----------------------------------------------------------------|
| `pacman`           | Official Arch + BlackArch repositories                          |
| `aur`              | Foreign packages installed via yay / makepkg                    |
| `path`             | `$PATH`-visible binaries not claimed by any other source        |
| `cargo` / `rustup` | `cargo install` binaries + Rust toolchains                      |
| `foundry`          | forge / cast / anvil / chisel via `foundryup`                   |
| `pip` / `pip2` / `pipx` | Python packages (system, legacy, isolated apps)            |
| `uv`               | `uv tool install` binaries                                      |
| `conda`            | Conda environments                                              |
| `npm` / `yarn` / `pnpm` / `bun` | JavaScript ecosystem global installs               |
| `docker`           | Locally pulled images, treated as runnable tools                |
| `desktop`          | `.desktop` entries from XDG application dirs                    |
| `extra_paths`      | User-defined directories from `/etc/librarian/sources.toml`     |
| `rpm`              | RPM packages (rare on Arch; useful for repacking)               |

Adding a new manager is one file — implement the `Collector` trait in
`crates/librarian-collectors/src/collectors/`, register it in `lib.rs`.

## Categorization

The `category` column is one of ~60 values, the bulk of them mirroring BlackArch's
package groups (`recon`, `fuzzer`, `webapp`, `cracker`, `wireless`, …). When a
package is in `blackarch-recon`, the tool inherits that category for free —
hundreds of tools categorized via the existing package metadata, no LLM required.

For non-BlackArch packages, a ~150-line name-based heuristic catches well-known
tools that don't carry group tags (nmap → recon, wireshark → sniffer, forge → 
blockchain-security, hashcat → cracker, etc.). Anything still uncategorized after
that is a candidate for the Bright Data enrichment pass.

## Bright Data enrichment

`librarian sync` does the same as `refresh`, plus an optional SERP-backed pass
that fetches a github URL + description snippet for every tool that doesn't
already have one. Powered by Bright Data's SERP API.

- **Keyed by tool name** — the enrichment cache survives every refresh cycle.
  Once "nmap" is researched, it stays cached for 30 days (configurable TTL).
- **Delta-driven** — subsequent syncs only enrich newly discovered tools.
  Steady state is "candidates: 0, enriched: 0" until you install something new.
- **Bounded by default** — 100 SERP calls per `sync` run, 200ms between calls.
  Tunable via `LIBRARIAN_ENRICH_MAX` and `LIBRARIAN_ENRICH_DELAY_MS`.
- **Graceful without credentials** — `sync` degrades to a plain `refresh` if
  `/etc/librarian/.env` is missing or empty.

## Installation

```bash
git clone git@github.com:Protocol-X-01/librarian.git
cd librarian
cargo build --release
sudo bash scripts/install.sh
```

Installs binary at `/usr/local/bin/librarian`, database at
`/var/lib/librarian/index.db`, manifest at `/var/lib/librarian/manifest.txt`,
and config templates at `/etc/librarian/`. Creates a `librarian` group and
adds the invoking user to it so subsequent `librarian refresh` / `librarian sync`
runs work without sudo (log out and back in for the group to take effect).

For Bright Data enrichment, edit `/etc/librarian/.env`:

```ini
BRIGHT_DATA_API_KEY=...
BRIGHT_DATA_UNLOCKER_ZONE=your_unlocker_zone
BRIGHT_DATA_SERP_ZONE=your_serp_zone
```

For custom build paths (anything not installed via a package manager), edit
`/etc/librarian/sources.toml`:

```toml
[extra_paths]
paths = [
    "/opt/custom-builds/bin",
    "/srv/tools",
]
```

## Staying fresh

| Trigger                                 | What runs                                       |
|-----------------------------------------|-------------------------------------------------|
| `pacman -S` / `yay -S` / any pacman transaction | Pacman hook → refresh pacman+aur sources |
| `cargo install` / `npm i -g` / `pipx install` / etc. | Shell wrapper → background refresh that source |
| Anything wrappers don't catch (CI, scripts, manual drops) | Lazy stale-check at next query — mtime on the source's root dir triggers rescan |
| Wanting enrichment for newly discovered tools | `librarian sync` |

No always-on daemon, no MCP server, no systemd timer. Just hooks plus mtime.

## AI access

Two equivalent entry points for any local LLM:

1. **Bulk dump** — `cat /var/lib/librarian/manifest.txt`. World-readable plain
   text, pipe-delimited (`name | source | category | version | path | description`).
   Greppable from any tool, suitable for one-shot context loads.

2. **Targeted query** — `librarian search`, `librarian list --category <cat>`,
   `librarian stats`. Read-only, no perms required. The database is opened with
   `file:…?immutable=1` for read commands, so even `nobody` can query.

`/etc/librarian/README.md` is a self-contained discovery document any LLM can
be pointed at to learn the query surface — includes a copy-pasteable system
prompt snippet for Ollama / llama.cpp / vLLM and similar.

## Architecture

Four crates in a Cargo workspace:

```
crates/
├── librarian-core/         Tool / Source / Category types, SQLite store, Collector trait
├── librarian-collectors/   One module per package manager + shared helpers
├── librarian-enrichment/   Bright Data SERP client + delta-driven enrichment service
└── librarian-cli/          Clap-based CLI binary
```

Data model is intentionally flat: every tool is one row keyed by
`(name, source, path)`. The same binary surfaced by multiple sources (e.g. on
`$PATH` and via `pacman`) yields multiple rows; the `describe` command merges
them at display time. Enrichment is keyed by name only — decoupled from row IDs
so the cache survives refresh cycles.

SQLite is used in DELETE journal mode (not WAL) so the database directory does
not need to be writable for read-only opens; combined with `immutable=1` URI
mode this lets any user query without permissions on `/var/lib/librarian/`.

## Status

Working:
- All 18 collector sources, live-tested on Arch + BlackArch
- Full-text search via SQLite FTS5
- BlackArch group → category mapping + heuristic fallback
- Bright Data delta-only enrichment with TTL-based refresh
- Pacman hook + 12 shell wrappers + lazy stale-check
- System install script with group management and idempotent re-runs
- LLM-facing discovery README installed system-wide

Pending:
- `librarian describe <name>` and `librarian categories` (CLI; logic exists in
  search/list, just not exposed as standalone commands yet)
- Formal Store unit tests (functionality is proven by live runs but no
  regression coverage)

## License

Dual-licensed under MIT OR Apache-2.0.
