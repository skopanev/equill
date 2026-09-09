# Voyage embeddings with local Qdrant

Voyage is an opt-in embedding provider, not a hosted store. Candle and Ollama
remain available. SQLite/FTS, immutable records and Qdrant stay where the caller
configured them. No extra daemon or provider framework is required.

The provider sends canonical record text (namespace, type, tags, payload leaves)
and search query text to `https://api.voyageai.com/v1/embeddings`. It does not send
record provenance or the credential to another endpoint. Before using private
records, review your Voyage account's data policy and select **Organization →
Terms of Service → Opted Out** if no training/zero-day retention is required.
See [Voyage's data-policy FAQ](https://docs.voyageai.com/docs/faq).

## Configuration

Keep the store's existing `store_id` and local Qdrant endpoint. Use a new
collection alias when moving from another embedding model. Set `dimensions` to
`2048`, `distance` to `cosine`, and replace only the `embedding` object:

```json
{
  "provider": "voyage",
  "model_id": "voyage-4-large",
  "input_schema": "equill.record.embedding.v1",
  "api_key_file": "/absolute/private/path/voyage.token",
  "allow_remote": true
}
```

The key file contains the API key, not a JSON object. Never put the key itself
in configuration, shell arguments, the repository, or logs. Configure it once
locally and keep it readable only by the user running Equill. Configuration
loading and status do not read this credential or call the embedding API.

The embedding `allow_remote` is separate from the top-level Qdrant `allow_remote`:
the former permits cloud embedding, while the latter controls remote storage.
For local Qdrant, leave top-level `allow_remote` false.

Install the complete config as the store's root actor, then rebuild:

```sh
EQUILL_ACTOR=owner equill vector configure --store /path/to/store --file /path/to/vector.json
EQUILL_ACTOR=owner equill vector rebuild --store /path/to/store
EQUILL_ACTOR=owner equill vector sync --store /path/to/store
```

Replace the example owner and paths with your store's actual configuration.
The post-rebuild sync settles records appended while the rebuild was running.
Switching provider/model is not a zero-cost config flip: Qwen vectors and Voyage
vectors are different spaces. Rebuild before relying on vector search. Keep the
previous config/collection until the replacement has passed verification.

Cosine thresholds are model-specific. The existing default `0.48` is not a
calibrated Voyage threshold. During acceptance, compare ranked results with
`--vector-score-threshold 0` and then choose a store retrieval threshold using
labelled relevant and irrelevant queries. A zero threshold is useful for
diagnosis, not proof that every returned record is relevant. Do not silently
carry over the old model's cutoff or change it for other stores.

## Embedding contract

- `voyage-4-large`, 2048 float dimensions, cosine; provider-side truncation is
  disabled. Equill bounds search query text to its first 2,000 Unicode scalar
  values before provider-specific instructions, for all embedding providers.
  This does not trim record documents, change FTS input, or filter source code
  and service text. Long queries lose their tail; place the question first.
- Documents use `input_type=document`; queries use `input_type=query` with raw
  query text. Voyage supplies its retrieval instruction, not the Qwen prefix.
- The existing incremental sync compares canonical input hashes and embeds
  changed current records; a provider change requires a full rebuild.
- The descriptor's historical `model_sha256` and `tokenizer_sha256` fields hold
  an API/preprocessing contract fingerprint for Voyage, **not a weight hash**.
  Hosted weights cannot be verified locally. A provider-side model revision
  requires an operator-led rebuild; changing credentials alone does not.
- The fixed HTTPS endpoint, finite timeout, disabled redirects/proxies and
  sanitized failures apply equally to document and query embedding.

Cloud rate limits, outages and billing errors can prevent vector updates. They
must not be reported as a current index; existing FTS/fallback and vector
freshness reporting remain the source of operational status.
