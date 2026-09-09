# Schema export

```sh
equill schema export --store ./store --output ./schemas
equill schema export --store ./store --output ./schema-history --all-registered
```

The destination must not exist, its parent must exist, and it must be outside
the source store. Export never starts vector catch-up or changes the registry.
The complete validated bundle is staged before atomic no-replace publication.

By default, only current definition files are exported. Current means the sole
unreferenced leaf in each connected `allowed_predecessor_types` lineage; an
isolated definition is current. Cycles, missing predecessors and ambiguous
branches refuse both modes. Full mode adds predecessor files labeled `legacy`.

Definitions retain their complete semantics, including predecessor references.
A current-only bundle may therefore mention types whose files it omits. It is
not a rewrite into a history-free schema, and a complete historical re-export
requires the full registry. No records, policies, receipts or store paths are
included.

Files contain canonical JSON followed by a newline. `manifest.json` uses
`equill.schema-export.v1`, with sorted entries containing `type`, `filename`,
`sha256` and `status` (`current` or `legacy`). Digests cover the exact exported
file bytes. Identical registered definitions yield identical bundles.

These output digests are not a stored source-registration attestation. The
existing registry has no independent trusted hash baseline for arbitrary type
definitions. Export validates schemas and their graph, but cannot detect a
prior valid semantic alteration merely by hashing the altered source itself.
