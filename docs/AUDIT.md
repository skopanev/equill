# Isolated request audit

Every CLI invocation and every nonempty MCP request has one durable request
intent before its operation begins, followed by one immutable completion event.
The default destination is `.equill-log` inside the caller's home.
`EQUILL_AUDIT_DIR` selects an isolated alternative, including disposable test
directories. An audit destination inside a project store is refused.

`equill audit list` and `equill audit stats`, including their argument errors,
are deliberately exempt. Audit internals never invoke the CLI, start a worker,
or use Qdrant. A domain's existing detached `vector drain` is a separate CLI
invocation and has its own event. An MCP process has one CLI session event in
addition to one event per request, including notifications and malformed input.

## Querying

```sh
equill audit list --project demo --surface mcp --limit 20
equill --json audit stats --operation record --since 2026-01-01T00:00:00Z
```

Both commands accept `--since` (inclusive), `--until` (exclusive), `--surface`,
`--operation`, `--project`, `--role`, `--process`, `--outcome`, `--instance`,
`--session`, `--actor`, and `--lane`. Time bounds are RFC3339 timestamps, compared
as instants. List defaults to 20 events, newest first, and accepts limits 1–1000.
Stats uses the identical scope without list pagination: count, failures, error
rate, and minimum/mean/p95/maximum duration in microseconds. JSON and human output
describe the same selected events and aggregates.

Operation names come from the executable's catalogue, such as `record`,
`schema.register`, or MCP `schema_show`; unrecognized input is `invalid`.
Coordinate defaults come from `EQUILL_PROJECT`, `EQUILL_ROLE`, `EQUILL_PROCESS`,
`EQUILL_ACTOR`, `EQUILL_LANE`, `EQUILL_INSTANCE`, and `EQUILL_SESSION`.
Explicit project/role/process coordinates in CLI or MCP arguments override their
defaults. Actor and lane fields are claims, not proof of authentication. Process
ID is recorded separately; absent instance/session identifiers use a process
instance UUID.

## Bounded content

Schema version 1 records UTC event time, duration, surface, operation, coordinates,
process ID, invocation UUID, overall outcome, domain outcome, and error class.
Arbitrary arguments are represented only by their canonical SHA-256, byte length,
and item count. Responses keep a digest, byte count, result count, at most 16 UUID
references, optional durable-write flag, and a receipt-reference digest. The CLI
captures structured IDs/counts before text, JSONL, or LLM rendering. Neither
request bodies, query text, response bodies, input paths, nor raw error messages
are copied to the audit ledger or index.

Structured coordinate values remain readable only when they contain 1–64 ASCII
letters, digits, dots, underscores, or hyphens and pass credential screening.
Paths, free text, oversized values, and credential-shaped values become
`sha256:` references. Filters apply the same transformation to the supplied
original value. This is separate from the existing opt-in query telemetry;
query telemetry behavior is unchanged.

## Durability and recovery

Events append to UTC `YYYY-MM.jsonl` files in the flat audit directory. A pending
journal is fsynced before append; recovery verifies the committed prefix and
appends only missing bytes. It never truncates an immutable ledger. File locks
serialize appends across processes. All audit child files reject symlinks and
multiple hard links before writes, and new files are private to their owner.
An invocation pins its audit directory descriptor before dispatch; replacing its
root or an ancestor cannot redirect checkpoints, ledger appends, or recovery.

| Interruption point | Recovery |
| --- | --- |
| During unpublished intent staging | Remove the abandoned stage; domain dispatch never began and no completion is invented |
| Before domain dispatch, after durable intent | One `error`/`interrupted` event; domain outcome `unknown` |
| After bounded domain-outcome checkpoint | One `interrupted` event retaining known domain outcome and result coordinates |
| During final ledger append | Complete the exact pending event from its verified prefix |
| After ledger fsync | Remove the linked intent before clearing the pending journal; no duplicate event |

Active intents have file leases and are never declared interrupted. A later
invocation or audit query recovers abandoned intents after their process exits.
A crash before the domain outcome can be checkpointed cannot establish whether
the operation committed; the event says `unknown` rather than guessing.

Delivery failure is `transport` while the separate domain outcome and durable
record coordinates remain intact. A failure to finalize audit storage never
replaces a successful domain result: CLI retains stdout and prints a sanitized
pending-audit warning; MCP retains the result and annotates checkpoint failures
known before delivery. Failures after response delivery are reported on stderr
and remain recoverable from the durable intent/outcome files. This avoids making
a committed record appear rolled back and inviting a duplicate retry.

`index.sqlite3` is a rebuildable audit-owned projection. Missing or corrupt
projections rebuild from monthly ledgers. Queries take committed byte snapshots
under the append lock, then release it before scanning/indexing. A separate index
lock prevents simultaneous index writers without blocking domain operations
through a long audit rebuild. No query opens a project store.

## Verification and delivery

Run checks with an explicit disposable destination, never the live audit log:

```sh
audit_test_dir="$(mktemp -d)"
EQUILL_AUDIT_DIR="$audit_test_dir" cargo test --all-features
EQUILL_AUDIT_DIR="$audit_test_dir" cargo test --release --test audit_requests latency:: -- --nocapture --test-threads=1
```

The coverage suite walks every CLI leaf for successful help and parse failure,
executes representative native operations and execution failures, and exercises
every advertised MCP tool for success and failure. Additional tests cover
concurrent processes, actual abrupt process exits, partial append recovery,
domain-write success during audit publication failure, broken response pipes,
projection rebuild, monthly rotation, structured summary parity, and outside-file
confinement. All records and identifiers are synthetic.

The release test prints audit-only overhead separately from total finite-session
record/search/context latency. MCP startup and protocol initialization are printed
separately; the first real tool call is not warmed up and remains inside its gate.
All three operation distributions are printed before any latency assertion.
The later owner rule gates total maximum latency
at 250ms. The 50ms p95 and 100ms maximum are advisory targets, printed explicitly.
Run measurements on an otherwise idle host; build contention is not release
evidence.

Before delivery, obtain independent review and preserve the previous executable.
Smoke the candidate against a disposable store and audit destination, check one
success and one error event, remove only the disposable audit index, and confirm
identical list/stats after rebuild. Restore the previous executable for rollback;
retain the immutable audit ledgers. Installation and live activation are separate
operator actions, not effects of running the test suite.
