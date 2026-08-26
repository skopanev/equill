# AGENTS.md — Equill

## Public repository

Everything committed here is public forever. Never commit real records, prompts,
receipts, store snapshots, credentials, tokens, private hostnames, absolute user
paths, or identifiers copied from another project. Tests and examples use synthetic
data only. Deleting a leak in a later commit is not a fix; keep it out of history.

## Product boundary

Equill is a local-first engine for immutable typed records and reproducible context
assembly. Domain owners define record schemas and selection policies. Equill owns the
storage protocol, validation, grants, projections, receipts, and diagnostics.

The engine does not own a domain's data location. A caller selects a store; Equill
must never silently copy records into the source repository or a global store.

## Engineering rules

- One installed executable: `equill`.
- No daemon and no network access by default.
- SQLite and every index are rebuildable projections; records and objects are truth.
- All writes pass through one validated, grant-checked immutable writer.
- MCP is an adapter over core operations, never a second write path.
- Logs and errors contain coordinates and hashes, never record payloads.

## LOC250

Every handwritten code file is at most 250 physical lines, including blank lines and
comments. Split by responsibility; do not compress code to evade the limit. Generated
files and Markdown specifications are exempt. CI enforces the rule.

## Directory size

A directory contains at most 10 handwritten files; aim to split at 7. Module entry
files count. When a directory approaches the cap, group files by capability in named
subdirectories before adding more. Do not create vague buckets such as `misc`, `utils`,
or a global `providers` directory. Generated files are exempt.

## Replaceable projections

SQLite/FTS is today's rebuildable projection, not part of Equill's public contract.
Nobody outside the projection module may issue raw queries, depend on SQLite types, or
return database rows through core, CLI, or MCP. Those surfaces use Equill-owned request
and response structures only.

A future SQL server replaces the projection module and rebuilds from immutable records;
consumers must not notice the change. Preserve that boundary, but do not build a generic
provider framework or unused implementations before a real second provider exists.

Provider code is organized capability-first, and EVERY provider gets its own directory
from day one. Its entry file repeats the provider name, for example
`src/projection/provider/sqlite/sqlite.rs` or
`src/vector/provider/qdrant/qdrant.rs`. Supporting config, migrations, and tests grow
beside that entry file. Never create a global `src/providers/` directory.

## Required checks

Run before every commit:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```
