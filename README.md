# Equill

Equill is a local-first engine for immutable typed records and reproducible context
assembly. It is designed as one fast executable with no daemon and no network access
by default.

> Status: pre-alpha. The storage format and CLI are not stable yet.

## Why

Storing information is not enough. A consumer needs the right context for its current
purpose, within a bounded context window, with an explanation of what was selected.
Equill combines a durable record ledger with versioned selection policies and receipts.

## Principles

- Records are immutable; correction uses `supersedes` or revocation.
- Payloads are validated by versioned, owner-reviewed type schemas.
- Type selection is versioned with the type, not reimplemented by each consumer.
- SQLite, full-text search, and vector indexes are disposable projections.
- Every context bundle includes record coordinates and a selection receipt.
- The caller owns the store location and the data inside it.

## Planned command surface

```text
equill init      create a store, root owner, and first namespace
equill record    append one validated record
equill context   assemble a bounded context bundle
equill search    inspect matching records
equill get       retrieve a record and its evidence
equill rebuild   rebuild disposable projections
equill doctor    validate a store and its policies
equill status    show installed, optional, and planned components
equill mcp       expose the same operations over stdio MCP
```

The current pre-alpha slice implements `init`, immutable schema registration, validated
`record`, `doctor`, and `status`.

`equill doctor` runs quick health checks. `equill doctor --full` additionally scans
all stored structures. `equill doctor --deep` also runs the offline full-catalog
memory-defense audit and writes an immutable audit receipt plus an alert when needed.

## Initializing a store

Examples use synthetic agent memory only:

```bash
equill init \
  --store .equill \
  --owner local-orchestrator \
  --namespace agent.memory
```

Write operations take actor identity from the orchestrator environment. The first slice
permits only the store's root owner; governed grants will widen that deliberately later.

```bash
export EQUILL_ACTOR=local-orchestrator
equill schema register --store .equill --file agent.lesson.v1.schema.json
equill record --store .equill --input lesson.json
equill doctor --store .equill --full
```

## Development

Requires Rust 1.85 or newer.

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo run -- doctor
```

All committed fixtures must be synthetic. See [AGENTS.md](AGENTS.md) before changing
the repository and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the contract.
