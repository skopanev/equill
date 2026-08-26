# Performance contract

Equill is a local context engine. Model inference and remote API calls are outside the
runtime path.

Initial release-mode targets on a modern local SSD:

| Operation | p95 target |
| --- | ---: |
| process start and argument parsing | 15 ms |
| append one small validated record | 30 ms |
| retrieve by identifier | 10 ms |
| SQLite full-text search | 50 ms |
| ordinary context assembly | 150 ms |

Targets are budgets, not benchmark claims. Each claim must gain a reproducible benchmark
with synthetic data before it is described as achieved. Durability settings and corpus
size must be reported with every result.

