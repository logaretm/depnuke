# AGENTS.md

## Project

nuke-pkg — Rust CLI that nukes node_modules, lock files, and package manager caches for local npm package development.

## Build & Test

```sh
cargo build              # dev build
cargo build --release    # release build
cargo install --path .   # install locally
cargo clippy             # lint
cargo fmt -- --check     # check formatting
```

No test suite yet. Test manually by running against a project with node_modules:

```sh
nuke-pkg @sentry/vue          # full nuke
nuke-pkg --cache-only @scope/pkg  # cache only
nuke-pkg -d 0 pkg             # no dep traversal
nuke-pkg                      # just clean node_modules + lock file
```

## Architecture

Single-file crate: `src/main.rs` (~280 lines). No need to split until it exceeds ~500 lines.

**Key components:**
- `PackageManager` enum — detection (from lock files), cache command generation
- `find_pkg_json()` — resolves package.json across flat, pnpm store (.pnpm), and nested node_modules layouts
- `build_pnpm_index()` — pre-indexes `.pnpm` dir into a HashMap for O(1) lookups
- `collect_deps()` — BFS dependency traversal with depth limiting and dep-type filtering
- `clear_cache()` — concurrent subprocess spawning (up to 8 at a time)
- `main()` — orchestrates: collect deps → background-thread node_modules removal → parallel cache clearing

**Design decisions:**
- Dependencies are collected BEFORE node_modules is deleted (we read from it)
- node_modules removal runs in a background thread, overlapping with cache clearing
- `serde::de::IgnoredAny` for version values — we only need dep names, not versions
- By default only `dependencies` + `peerDependencies` are traversed. `--dev`, `--optional`, `--all` flags enable others.
- Scoped packages (`@scope/foo`) automatically pull in siblings from the same scope in root package.json

## Style

- No unnecessary abstractions — keep it flat and direct
- Prefer `eprintln!` for all user-facing output (stdout is reserved for potential future piping)
- Don't add deps unless truly needed. Current deps: `clap`, `serde`, `serde_json`
- No async runtime — std threads are sufficient for the concurrency needed here
