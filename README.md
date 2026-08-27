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

Each store is isolated and owns its own schemas, selectors, grants, and projections.
Consumer-specific wrappers choose the explicit `--store`; Equill never discovers or
joins stores automatically.

## Command surface

```text
equill init      create a store, root owner, and first namespace
equill schema    register an immutable versioned type
equill record    append one validated record
equill import    migrate a legacy JSONL batch through the writer
equill compact   preview or apply governed JSONL compaction
equill profile   register a context budget and grants
equill selector  register canonical per-type selection
equill context   assemble bounded deterministic context
equill search    inspect matching records
equill rebuild   rebuild disposable projections
equill doctor    validate a store and its policies
equill status    show installed, optional, and planned components
```

Direct `get` and the stdio MCP adapter remain planned.

Commands print concise human-readable output by default. Pass the global `--json` flag
for the stable machine-readable response, for example `equill status --json` or
`equill doctor --full --json`.

`equill doctor` runs quick health checks. `equill doctor --full` additionally scans
all stored structures and proves that SQLite/FTS matches the immutable log. `equill
doctor --deep` also runs the offline full-catalog
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
equill import --store .equill --input lessons.jsonl
equill search --store .equill --query "Run checks"
equill doctor --store .equill --full
```

`init` creates the embedded SQLite/FTS5 projection. A successful immutable append stays
successful if projection indexing fails; the projection becomes `degraded` and
`equill rebuild --store .equill` reconstructs it from the record log.

`import` accepts legacy JSONL envelopes, never trusts their actor as writer identity,
and preserves the legacy id, actor, timestamp, and source-line digest as evidence. The
digest makes a repeated import idempotent; reusing a legacy id with changed content is
rejected instead of silently duplicating it.

For a governed set of inputs, pass a JSONL manifest instead of one file:

```jsonl
{"path":"rules.jsonl","role":"rules"}
{"path":"lessons.jsonl","role":"lessons"}
```

```bash
equill import --store .equill --manifest inputs.jsonl
```

Paths are resolved relative to the manifest. `role` is optional metadata. A successful
set writes one immutable receipt containing the manifest SHA-256, every input SHA-256,
and every source-line digest. `equill doctor --full` proves those lines still exist in
the immutable ledger. Duplicate paths and partially imported sets never produce a set
receipt; rerunning after a fix is safe because each completed line is idempotent.

## Context assembly

A selector owns generic retrieval policy for one type. JSON pointers connect request
coordinates to payload fields without teaching Equill domain words:

```json
{
  "id": "agent.lesson.inject.v1",
  "version": "1",
  "type": "agent.lesson.v1",
  "strategies": ["fts", "exact", "tag", "recency"],
  "required_tags": ["must"],
  "core_tags": ["core"],
  "rank_pointer": "/confidence",
  "coordinate_pointers": { "scope": "/scope" }
}
```

Context content contains payloads only. Receipts retain selected coordinates; source
records retain evidence and tags without consuming the content budget. A numeric
`rank_pointer` sorts records descending within a tier before `observed_at` and id.

A profile binds selectors to read grants and a hard context budget:

```json
{
  "id": "worker.v1",
  "version": "1",
  "actors": ["local-worker"],
  "grants": [{ "namespace": "agent.memory", "types": ["agent.lesson.v1"] }],
  "selectors": ["agent.lesson.inject.v1"],
  "budget": {
    "total": 8000,
    "required_cap": 1500,
    "core_cap": 3000,
    "relevant_floor": 2500,
    "receipt_reserve": 500
  }
}
```

```bash
equill selector register --store .equill --file selector.json
equill profile register --store .equill --file profile.json
equill context --store .equill --profile worker.v1 --request request.json
```

Coordinate matching is exact by default. A selector may opt individual keys
into `set_or_wildcard`: record arrays then match a requested scalar by
membership, while a missing or `null` record coordinate applies to every
request. The mode is explicit because widening a scope must never be implicit.

The receipt names every included and excluded coordinate, strategy degradation, budget
use, and the bundle digest without copying payloads into the receipt. `search` remains a
simple SQLite/FTS inspection command; both surfaces reuse the same projection operation.

## Explicit compaction

Compaction operates only on the complete input manifest named by the owner:

```jsonl
{"path":"rules.jsonl","role":"rules","expiry":{"pointer":"/expires_at","warning_days":30},"anchor_resolver":"anchors.jsonl"}
{"path":"lessons.jsonl","role":"lessons"}
```

```bash
equill compact --store .equill --manifest inputs.jsonl --dry-run
equill compact --store .equill --manifest inputs.jsonl --apply
```

Dry-run is read-only and reports record IDs plus reason codes. Apply stages every source
beside its original, validates the complete result, rebuilds the ledger through the
canonical writer, rebuilds SQLite/FTS, runs `doctor --full`, then writes a content-free
receipt. Git is the archive for removed source lines; compaction is never automatic.

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
