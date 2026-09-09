# Candidate-scoped canonical hydration

Semantic candidates use the text projection only to locate immutable truth.
The projection never supplies trusted record payloads.

`projection::locators(store, &LocatorRequest { ids })` returns Equill-owned
`LedgerLocator { record_id, ledger, record_sha256 }` values in request order.
Missing ids are omitted; malformed or repeated requested ids fail. The SQLite
provider selects only those three columns in chunks of 256 ids using its
existing schema. No migration or provider-specific type crosses this boundary.

`LocatorReport` includes projection state and the existing optional
`TextWatermark`. These describe the projection and do not prove absence or
authorize writes. In particular, a just-written predecessor may not yet have a
locator. Write confirmation must use independently validated lifecycle metadata
or establish projection freshness before relying on projected coordinates.

`record::read_located(store, locators)` validates all locator paths before opening
shards, groups them by filename, and opens every distinct named shard once. It
never enumerates `records/` or calls `record::read_all`. Directory-relative
`openat` calls with `O_NOFOLLOW` confine the internal `records` directory and
shard leaf even when path components are replaced concurrently. Shards must be
regular files with one link; names must be `records/YYYY-MM.jsonl`.

Every complete named-shard row is deserialized and checked with the existing
stored-record verifier. Invalid rows and repeated UUIDs refuse the result,
including duplicates unrelated to the requested candidates. The reader returns
the requested records in locator order only after canonical serialization hashes
match the locator digests. Missing records, invalid digests, mismatched months,
and duplicate locator ids fail closed. Refusals contain fixed diagnostic reasons,
never projected path strings, payloads, or validator messages.

A concurrent read ignores an incomplete trailing line, consistent with the
append-only ledger's existing reader contract. A caller already holding the
writer lock uses `record::read_located_exclusive`; it rejects an incomplete tail
as crash damage. Neither helper acquires a writer lock or rebuilds a lifecycle
graph.

Vector hydration then verifies namespace/type filters, provider record digests,
UUIDv7 coordinates and duplicate hits. Public semantic retrieval still
re-derives embedding-input digests and reports stale candidates as rejected.
Candidate order and existing fallback diagnostics remain intact.

Request-path freshness compares published target and indexed revisions without
scanning the ledger. Equal revisions report `current` and zero pending records.
A lagging answer omits `pending_records`: unknown is not zero. Revision gaps
include configuration invalidation and filtered-out writes, so they cannot be
reported as document counts. Full `status` retains its separate corpus-based
accounting. Marker-only freshness does not claim to detect unrecorded external
edits to ledger files; immutable candidate verification still checks returned data.

Deterministic tests cover an unrelated corrupt shard, shared/multiple shards,
corrupt rows and digests, missing/duplicate coordinates, path escapes, symlinks,
hardlinks and reader/writer tail semantics. A public `vector::retrieve` test
uses the actual `VectorProjection` hydration and embedding verification path,
substituting only the external index/model inputs, and proves zero full-ledger
reads. Full strategy freshness accounting is a separate caller responsibility;
these tests do not claim it is fixed by candidate hydration.

The work is bounded by the named shards, not by individual byte offsets. Its
cost therefore still depends on the size of those shards. Release measurements
must separate local read/hydration overhead from embedding and provider latency;
this change adds no inference timeout or fallback policy and makes no end-to-end
latency claim. Cold and warm local paths still require their release gates.
