# Native MCP hooks

An editor lifecycle hook can call this store directly, with no shell wrapper.
The tool is `hook_context`. It is a thin adapter: the `context` tool assembles
the bundle, and the hook returns that same text in the envelope a hook expects.

## Output

Always exactly one key, and always the event that was asked about:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PostToolBatch",
    "additionalContext": "…"
  }
}
```

`hookEventName` is echoed from the request. One editor's event name is never
substituted for another's — a harness that receives the wrong one ignores the
result, which looks like the store having nothing to say.

Accepted events: `UserPromptSubmit`, `PostToolBatch`, `PostToolUse`. Anything
else is refused, and the refusal names what was sent.

## Calling it

`hook_event_name` is the only argument this tool adds. Everything else is the
`context` tool's own argument list, unchanged — `profile`, coordinates, `query`,
`where`, `tags`, `at`, `budget`, `budget_records`, `strict`, the retrieval
overrides — and it behaves here exactly as it does there.

```json
{
  "hook_event_name": "PostToolBatch",
  "query": "finish the merge checks"
}
```

## The question comes from the launcher

The hook does not invent one. Nothing is read out of a tool loop: no tool
names, no file paths, no command strings, no responses. If the harness wants
context for the work it just did, it passes task text as `query`; with no
`query`, the coordinate and recency selectors still assemble the contract, as
they do for a `context` call without a query. Every cap, skip rule and budget
is the store's existing retrieval policy, applied once, in one place.

## The actor must be given explicitly

Every write and every assembly reads `EQUILL_ACTOR`. A hook runs inside the
MCP server process, and that process does not necessarily inherit the parent
environment — this was observed on Codex, where the first run failed until the
variable was set on the server entry itself. Set it there:

For Codex, in `config.toml`:

```toml
[mcp_servers.equill.env]
EQUILL_ACTOR = "reader"
```

For Claude, set the same variable in `mcpServers.equill.env` in its MCP
configuration. Replace `reader` with an actor authorized by the selected store.

Do not rely on inheritance. Coordinates the profile requires belong in the same
place if the harness does not pass them as arguments.

## Coverage

What is verified is that a hook fires after a tool loop and that its
`additionalContext` reaches the next model turn, on both clients tested. That
is two consecutive continuations — not every internal model request, not hosted
tools, and not any guarantee about future client versions. No generic
every-request injection is claimed or implemented.

Query telemetry records these calls as `mcp.context`, because the assembly is
the `context` tool's. The `hook_context` operation stays distinguishable in the
request audit.
