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

<!-- NTK -->

### Tickets

**CRITICAL:** ALL task management MUST use `ntk` CLI. NO other tools. NO Notion API. NO exceptions.

Drafting depth depends on the task. For trivial one-liners ("just add XXX", "update README", "bump version") — DO NOT scour the codebase, take the task as-is. For substantive tickets — draft the description independently using codebase context. NEVER ask the user to fill in details that can be inferred from the codebase or the internet. Only ask if the info genuinely cannot be found.

**Description Format (`-d`):**
```
## Summary
Business-level what/why. Max 2 sentences.

## Expected Outcome
The concrete result/value delivered when this is done.

## Details
- Implementation specifics, affected files/modules, technical approach
- Reference code by symbol — function/class/block name or a unique snippet — NEVER by line number. Line numbers go stale as files shift.
- Edge cases, constraints, dependencies

## Acceptance Criteria
- [ ] Independently verifiable checklist item
- [ ] Independently verifiable checklist item. NO vague "works correctly". Define "correct".
```

**Always pass `-d` (and `-A`) via a heredoc:**
```
ntk create "title" -p med -d "$(cat <<'EOF'
## Summary
...
EOF
)"
```

**Ticket Rules:**
- **Trigger:** Run `ntk create` ONLY when the user explicitly asks to file a ticket. Fixing bugs, reviewing code, answering questions — NOT a trigger.
- **Initiative:** Expand user one-liners into full tickets using codebase context — but only when the task warrants it (see above).
- **Clarity:** NEVER create vague tickets. Ask questions FIRST if ACs cannot be written.
- **Closing:** Before running `ntk close` (or moving to `done`) you MUST append a comment via `ntk update <id> -A "..." --force` describing WHAT WAS DONE — how it was fixed, key files/decisions. No comment, no close.

Project is auto-set via `.ntkrc`. Outside a repo pass `-P <name>` (list via `ntk projects`).

No `.ntkrc` and user named a project? Match it against `ntk projects` (case-insensitive, fuzzy), then pass `-P <matched-name>`.

**Workspaces (databases):** Repo → one DB via `workspace` in `.ntkrc` (else default). Workspaces NEVER mix; don't switch it yourself. Override per-command with `-W <name>`; `-W all` reads all (`ls` only). List: `ntk workspaces`.

**Vocabulary:** Statuses, types and priorities are per-database. `ntk help` prints the ones this workspace actually has; `ntk schema` shows them live. Never assume a fixed list.

**Commands:**
- `ntk ls [-s status,status] [-a initials,initials] [-t tags] [--since YYYY-MM-DD] [--progress]` — List (comma-separated for multiple statuses/assignees; `--since` filters by creation date). `--progress` replaces the table with a closed/total ratio + per-status breakdown for the filtered set (respects `-P`/`-t`/`-a`; works with `-W all`).
- `ntk show <id> [id...]` — View (pass multiple ids space- or comma-separated)
- `ntk start <id>` — Mark `in_progress`
- `ntk close <id>` — Mark `done`
- `ntk rm <id> [id...] [-y]` — Move ticket(s) to the Notion trash (restorable there). Notion has no permanent-delete API. Prefer `ntk close` for finished work; `rm` is for tickets that should never have existed.
- `ntk next [-a initials] [-P project]` — Pick next
- `ntk deps <id> | -t <tag>[,tag] [-P proj]` — Show dependency tree for one ticket (with `N/M done`, `[ready]`/`(waiting on N)`) or a forest for all tickets carrying the tag(s); external blockers shown as `↗`
- `ntk users` — List assignees
- `ntk schema` — Show the live database fields, options and status groups (also refreshes the cached schema)
- `--reload-schema` (global, before the command) — refetch the cached schema after the Notion database changes
- `ntk projects` — List projects from Notion (refreshes global config)
- `ntk workspaces` — List configured workspaces (databases); marks the default and the active one
- `-W <name>` (global, before the command) — run against a specific workspace; `-W all` reads across all (`ls` only)
- `ntk create <title> [-p priority] [-a initials] [-s status] [-T type] [-t tags] [-d text] [-i file] [-P project] [--deps tid,tid] [--due YYYY-MM-DD]`. `-i <path>` attaches a file/image (repeatable or comma-separated, ≤50MB each).
- `ntk update <id> [id...]` — Modify (same flags as create + `-d text` to REPLACE body + `-A text` to append + `--title text`). By default only not-yet-started tickets may be updated — every status in the schema's "To-do" status group (`open`, plus whatever else that group holds in this workspace); pass `--force` to intentionally update a started or finished one. Multiple ids: same flags applied to each; if any id doesn't resolve or any ticket is protected, nothing is updated. `--deps` accepts `tid,tid` (replace), `+tid,-tid` (add/remove), or `""` (clear); `--due` accepts a date or `""` to clear. `-d` refuses to replace a body it cannot write back — including one whose blocks carry comments, since deleting a block takes its discussion with it — — nesting, toggles, tables, attachments, child pages, formatted text, or text that would come back as different blocks — since replacing it would destroy that content (adjacent paragraphs or quotes are the one tolerated change: they merge into a single block, keeping every character); use `-A`, edit in Notion, or pass `--force-body` (separate from `--force`, which only covers status). Body text is read as markers: a line starting with `#`, `-`, `1.`, `>`, ``` or a line that is exactly `---` becomes the matching Notion block, in `-d`, `-A` and `create -d` alike. JSON output carries the ticket's own `uuid`, and `deps` come back as ticket ids, read in full even past the 25 entries a page object carries; `update --json` on several tickets prints one array rather than a stream of documents (a UUID left in place means that dependency is not in this database). `ntk show` prints `[nested content: ...]` where a body was too deep or too large to fetch, and `--json` carries those markers inside `body`; a discussion that could not be read in full is marked too (`comments_truncated` in `--json`), including when nothing at all could be read — an integration without comment permission is the one case that stays unmarked, since it has nothing to lose. If some old blocks cannot be removed the replacement is reported as incomplete and the exit code is non-zero.

### Reviewing Agent Work

Trigger: user asks "what's done?" / "let's check it" / similar.

Process **one ticket at a time** — never batch. Start with the first ticket in `ntk ls -s to_test -t agent-done`, finish it (GO or NO-GO), then move on.

Queue >1 ticket on separate branches? Review **ONLY in a worktree**: `git worktree add ../<repo>-review <base>`. Drop it when done.

Agent branches are stale — base could change during run. Reconcile overlaps yourself. Escalate only if blocked.

1. `ntk show <id>` — read request + agent log.
2. `git fetch origin && git diff --stat origin/<base>..origin/ntk/<id>` for the default path (base = project's main branch).
   - If the agent note says `Agent: ветка <base-override>@<commit>`, this was a `base:<branch>` task. Review that commit/branch directly; do not expect `origin/ntk/<id>`.
3. Summarize: what changed, flag anything off-topic or junk (files unrelated to the ticket).
4. Give your own short, concise verdict — one sentence — then ask the user: **GO / NO-GO?**
   - **GO:** sync base (`git switch <base> && git pull --rebase`), `git checkout origin/ntk/<id> -- <task files only>` (skip junk). If file already modified — edit by hand, no `checkout`. Commit + push, `ntk update <id> -s done -A "merged: ..." --force`, `git push origin --delete ntk/<id>` (only after merge push confirmed).
   - **GO for `base:<branch>` task:** do not merge/delete `origin/ntk/<id>`. The commit is already on the shared branch; after review, mark the ticket `done` with a comment like `reviewed: <branch>@<commit>`.
   - **NO-GO:** show the issues, discuss. Nothing else.

<!-- /NTK -->
