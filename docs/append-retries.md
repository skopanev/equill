# Append retries and atomic legacy imports

Catch-up revision targets are monotonic write counters, not retained-record totals.
Keyed writes and atomic imports reserve their target before append and durably
publish that same target before transaction cleanup. Recovery repeats publication
without counting retries as new records.

`equill record --store <store> --input draft.json --idempotency-key <opaque-key>`
binds an operation to that store, authorized actor and canonical draft. A retry
returns the original record ID, receipt coordinate and durable result. A changed
draft with the same key is refused. Omitting a key keeps identical restatements
as independent records. UUIDs and recording timestamps are not request identity.

Core callers use `record::append_request` with `AppendRequest`. MCP `record`
accepts `idempotency_key` beside `draft`. JSONL record inputs retain partial-success
semantics; a keyed line is `{"draft":{...},"idempotency_key":"..."}`. Keys are
per entry, and the batch-level CLI key option is refused for JSONL batches.

Operation files persist only key/request digests, coordinates and durable outcome
metadata. Pending receipt and operation recovery runs under the writer lock before
another append. A retry must still satisfy the store's current actor grants.

An intentional physical compaction expires keys for removed records and reports
how many expired. Every associated outcome, coordinate and digest is removed.
Retained records keep their operation keys with updated ledger positions and
envelope digests. After expiry, a client-supplied request with that key is a new
validated write; no deleted content is restored from stored retry state.

Legacy `import` preflights all new lines against one locked immutable snapshot,
including same-file supersedes links. A rejected final line commits nothing.
One canonical writer append and one ledger sync commit the accepted set, followed
by receipt publication and one SQLite projection transaction. A projection failure
leaves durable truth intact and diagnosably degraded.

A private transient staged JSONL file proves ownership of a torn batch tail.
Keyed single-record operations use the same journal so a process crash during
append can recover before retrying, without leaving an unusable partial tail.
Recovery truncates only an exact uncommitted prefix of that file, or finishes the
whole batch when all its bytes are present. Corrupt or unrelated tails fail closed.
The staged data and coordinate/hash marker are removed after durable settlement.

Public ledger reads capture fixed descriptor lengths under a nonblocking shared
writer lock, then release it before parsing, hashing, indexing, or embedding.
An active writer returns `committed snapshot busy: writer active; retry` rather
than waiting behind a long import. An unresolved transaction returns
`write recovery pending; committed snapshot unavailable`. These are errors,
not empty successful results or permission to fall back to an older projection.
Reads never create lock files or recover/alter the store. The next authorized
write uses the existing recovery protocol. A reader that captured earlier
cannot grow into later transaction bytes, even when their complete lines exist.

Internal text drains and explicit vector sync/rebuild operations may wait for
the active writer while capturing their descriptors and bounds. They release
that lock before parsing, indexing, or embedding, and retain the captured target
and filter identity. Normal writer contention is not a provider outage.
