# Architecture

## Boundary

Equill is a universal container by form, not by meaning. The kernel accepts only types
registered through an owner-reviewed schema registry. A type owns its payload schema
and its canonical selectors. Consumers cannot invent a second interpretation of the
same type.

Equill owns:

- immutable append and content-addressed objects;
- schema validation and grants;
- deterministic selection and context receipts;
- rebuildable projections and integrity checks.

Domain packages own schemas, selectors, gates, and the location of their data store.

## Record envelope

The envelope is stable and small. Meaning lives in the versioned payload type.

```json
{
  "id": "record identifier",
  "namespace": "agent.memory",
  "type": "agent.lesson.v1",
  "actor": "writer identity supplied by the orchestrator",
  "observed_at": "2026-01-01T12:00:00Z",
  "valid_at": "2026-01-01T00:00:00Z",
  "payload": { "rule": "Run checks before publishing." },
  "evidence": [],
  "tags": [],
  "supersedes": null
}
```

Records never change in place. A new record may supersede or revoke an earlier record.
Objects too large for the envelope are stored by digest and referenced from evidence or
payload fields defined by the type.

### Lifecycle policy

Lifecycle semantics belong to the registered type definition:

- `dag` is the backward-compatible default and permits multiple descendants;
- `append_only` rejects every `supersedes` edge;
- `linear` requires a `key_pointer`, equal keys, and one current head;
- `allowed_predecessor_types` explicitly permits named cross-type migrations.

Every target must exist. Edges cannot cross namespaces, cross-type edges are denied
unless the successor type names the predecessor, and a record whose own type is
`append_only` cannot be superseded by any type. The writer acquires the exclusive store
lock, then validates the prospective graph — the stored records plus the candidate —
so competing linear updates cannot both commit and no append can leave behind a graph
that a read would reject.

Every canonical read applies the same rules and fails closed: dangling targets,
self-edges, cycles, invalid boundaries, and ambiguous linear heads are rejected by
`doctor --full`, by context assembly, by import, and by compaction planning alike. An
invalid graph yields an integrity error, never a partial answer.

A tombstone is not a special envelope or command. It is a schema-valid successor with
the `equill:revoked` tag and a `supersedes` link to the current head:

```json
{
  "namespace": "agent.memory",
  "type": "agent.lesson.v2",
  "observed_at": "2026-01-02T00:00:00Z",
  "payload": { "key": "build-check", "rule": "Withdraw this rule." },
  "tags": ["equill:revoked"],
  "supersedes": "01941f29-7c00-7000-8000-000000000001"
}
```

Context excludes both the superseded ancestor and the revoked tombstone. A later
schema-valid correction can supersede the tombstone; physical history remains immutable
until governed compaction.

### Type evolution

A registered type is immutable, and understanding is not. The answer is
explicit rather than discovered by deleting a file:

- While a type has no records, deleting its registry entry is a legitimate
  correction. `schema list` shows what a store holds; a type with no records is
  a draft nobody depends on yet.
- Once records exist, the type is permanent. There is no in-place edit, because
  every stored record was validated against the definition as it was, and a
  retroactive change would make the ledger disagree with its own receipts.
- Moving forward means a new version — `agent.lesson.v2` beside `v1` — and the
  new type names the old one in `allowed_predecessor_types`. Readers then see a
  chain rather than two unrelated types, and a record can be migrated by
  superseding its predecessor across the version boundary.
- Nothing rewrites old records. A reader that wants both versions selects both;
  a reader that wants only current state gets it, because a superseded record is
  hidden by default.

Design types so the project or tenant is a field rather than part of the type
name: a name is forever, a field is a filter.

## Store placement

The executable is installed globally; stores are not. A caller opens an explicit store
root. A domain can keep that root beside its own ignored runtime data while another can
place it in an OS data directory. Equill does not silently choose a global destination.

Ownership belongs to the store, not the Equill source repository. `equill init` creates
the store metadata, root owner, and first namespace. Versioned schemas are registered by
a separate governed operation because a store can contain several types with different
schema owners.

Stores are security boundaries. Each store has its own schema registry, selectors,
grants, owners, projections, and receipts. There is no global schema registry, automatic
store discovery, cross-store search, or cross-store join. A consumer wrapper selects one
explicit store for one operation.

```text
<store>/
  store.json     format version and root ownership
  records/       append-only typed records
  objects/       content-addressed bytes
  receipts/      context and write receipts
  projections/   disposable SQLite and vector indexes
```

## Context assembly

A context profile defines a hard total budget and reserves space per tier. No tier may
consume the entire bundle. The initial contract uses:

- `required_cap`: maximum space for registered mandatory policy;
- `core_cap`: maximum space for canonical type selections;
- `relevant_floor`: space protected for request-specific evidence;
- `receipt_reserve`: space for provenance and degradation reporting.

The total budget is hard. An overflowing required set is a configuration fault, not a
reason to exceed the caller's context window. Equill returns a deterministic bounded
bundle, marks it degraded in the receipt, and makes `doctor` fail until the profile is
repaired.

Selection order is fixed:

```text
grants → active/superseded/revoked → valid time → type selector → budget
```

Every selector ships with scenario gates containing expected inclusions and exclusions.
Request coordinates are opaque keys. A selector may map them to payload fields with
JSON pointers; the kernel compares values and never learns domain concepts such as a
project, phase, or harness.

## Access

Orchestrators select registered profiles. Agents receive assembled context and do not
choose their own grants. Direct callers are subject to the same namespace, type, and
operation grant table for every transport.

The store root owner can write every registered type. Legacy `writers` remain
store-wide for compatibility. `write_grants` add least-privilege append access scoped
by actor, namespace, and type; each dimension supports `*`. They are additive: an actor
already present in `writers` is not narrowed by a scoped grant. Store metadata without
`write_grants` continues to load with an empty scoped grant list.

Selector coordinates are not ACLs. They narrow records only after a profile's read
grant succeeds and never authorize reads, writes, schema changes, or governance.

CLI and MCP call the same core operations. In particular, MCP `record` uses the same
schema registry, grants, and immutable writer as CLI `record`.

## Projections

SQLite is the first projection and provides structured lookup plus full-text search.
Vector search is optional and rebuildable. No projection can become the only copy of a
record or object.

`init` creates a versioned embedded SQLite database and an FTS5 index. `record` updates
them only after the JSONL append and write receipt are durable. An index failure marks
the projection degraded but cannot roll back or invalidate the record. `doctor --full`
compares every projected record and FTS body with the immutable log; `rebuild` replaces
the disposable database from that log under the store writer lock.

## Source compaction

Imported JSONL is an owner-governed source set, not an endlessly growing second ledger.
`compact` takes one complete import manifest and proves the rewrite before changing any
source: duplicate or missing inputs, malformed records, invalid supersedes graphs, and
ambiguous lifecycle anchors fail the whole plan.

Removal is conservative. Superseded ancestors, records beyond a declared expiry warning
window, and anchors documented dead by an explicit resolver may be removed. Active or
unknown anchors remain. When an ancestor is removed, the retained live descendant is
materialized without a dangling legacy `supersedes` link; Git preserves the original
chain.

Apply stages same-directory files, rebuilds a shadow store through the canonical import
writer, rebuilds projections, and requires a green full doctor before swapping the
source files and rebuilt store structures. A compact receipt records only coordinates,
reason codes, hashes, actor, and verification outcome. No scheduler calls compaction.

## Transport

MCP is a local stdio adapter for dynamic retrieval. An optional event bus may later
publish opaque references after a durable append. Notifications never carry record
payloads, never make a write succeed or fail, and never count as replication.

## Consolidation (observations)

Raw records accumulate; beliefs must not silently duplicate. A derived
`observation.v1` type consolidates records that assert the same underlying claim:

- each observation carries its supporting evidence — record ids, exact quotes,
  and a proof count — never the records themselves;
- new evidence refines an existing observation (strengthen, weaken, extend)
  rather than overwriting it; refinement is itself an immutable record that
  supersedes the previous observation revision;
- observations are projections in spirit: recomputable from the record log, and
  a domain may pin a curated observation as a `must` record through the normal
  registry path.

Consolidation runs as an explicit command or scheduled pass over a namespace.
It never runs inline on the write path: `record` stays deterministic and
LLM-free.

Consolidation is scoped. Observations accumulate within an explicit tag scope;
without scoping, volatile per-session tags would mint a near-identical
observation per session. The scope is part of the observation's identity.

Near-duplicate reconciliation: two observations saying the same thing in
slightly different words are merged (folding both evidence sets) or kept apart
by a check that reads both in full — a number, a negation, or a named entity
must keep them separate. Reconciliation is deterministic where possible and
never destroys evidence.

Contradictory evidence refines rather than replaces: the new observation
records the journey ("was X, now Y, because Z"), preserving what was believed
before and why it changed. The raw record log always holds the original
statements and their times.

## Context layering and freshness

Selection prefers the most settled layer that covers the question:

```text
pinned (must)  →  observations  →  raw records
```

Each layer is cheaper and more condensed than the one below. A layer is served
with a freshness signal: records have landed in its scope since it was last
consolidated. A stale observation is still included but flagged in the receipt;
the consumer is told to verify it against the records below rather than trust
it blindly. Freshness is reported, never silently dropped.

Incremental rewrites preserve text physically. When a derived document
(observation, knowledge page) is refreshed, unchanged portions are left
byte-identical rather than regenerated: regeneration drifts — bullets become
numbers, casing shifts, sentences quietly paraphrase. A long-lived document
stays the document it was, with only the changed parts new.

## Memory defense

Every write passes a sanitization stage before the append:

- a pattern library (secrets, tokens, keys; per-store extendable) is matched
  against payload, evidence, and tags;
- a match is either redacted inline with a visible marker or blocks the write,
  per store policy — both outcomes leave a receipt;
- the writer identity stays intact: defense edits content, never provenance.

This is a kernel concern, not a domain concern: no type can opt out, because
a type schema cannot promise what its writers will paste.

Defense has two speeds. The inline path uses a focused bundled pack and a budget
measured in tens of milliseconds. The offline path, `equill doctor --deep`, scans the
immutable log with the full bundled Gitleaks and Kingfisher catalog. It never rewrites
history: a retrospective match produces a content-free audit receipt and alert. The
owner then decides whether to supersede the affected record. Repeating a scan over the
same corpus and rules reuses the same receipt instead of producing duplicate alerts.

The bundled pattern set covers provider API keys (anthropic, openai, google,
groq, xai...), cloud credentials (aws, digitalocean), source-control and
package tokens (github, gitlab, npm, pypi), payment secrets (stripe, square,
braintree), messaging tokens (slack, twilio, sendgrid, telegram), database
connection strings, PEM private keys, JWTs, and card/SSN-shaped numbers. A
store starts with the bundled set; redaction markers name the pattern that
fired (`[REDACTED:github_token]`).

## Recall strategies

FTS is one retrieval strategy, not the contract. The query path is
strategy-pluggable and always composable:

- initial strategies: full-text (SQLite FTS5), exact/tag, recency;
- optional, rebuildable: vector similarity and graph traversal over record
  relations — added per store, never required;
- a selector states which strategies it uses; a receipt reports which
  strategies answered, so a degraded projection is visible rather than silent.

A single-strategy store is honest and complete; multi-strategy is a scale
concern, and the record log remains the truth all strategies serve.

## First-run experience

`equill init` brings up a complete working store with zero configuration:
embedded SQLite projection, bundled defense patterns, one namespace, the root
owner. Nothing to provision before the first `record`. Upgrades swap the
projection engine or add optional strategies behind explicit store
configuration — the first-run default stays a single static binary and a
directory.
