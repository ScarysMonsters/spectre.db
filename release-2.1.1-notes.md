spectre.db 2.1.1 — Stability & Performance.

No on-disk format change and no public API break: v1.1.0 (JSON) and v2.1.x (binary) databases open unchanged.

## Reliability

- Index entries are removed when a parent key is replaced or a subtree is deleted, in single sets and batches.
- WAL is appended before store/index mutation for set/delete/clear/raw operations; a WAL I/O error leaves the in-memory database unchanged.
- Batch delete keys and nested operations are validated; exact delete flags are computed from the removal result.
- `deleteMany()` returns per-key booleans in both native and fallback engines.

## Performance

- Parent-count fast path skips descendant B-tree scans for leaf writes and deletes.
- Direct WAL set/delete frames avoid an intermediate `Op` and extra key/value copies.
- One shared `Arc` key per leaf update is used by all secondary indexes.
- `count(prefix)` uses prefix bounds instead of a full-map fallback.

## Measured (100k standard, local GNU build)

| Workload | 2.1.0 | 2.1.1 | Delta |
|---|---:|---:|---:|
| write (single) | 108 104 | 118 125 | +9.3% |
| write (batch 1 000) | 272 581 | 344 583 | +26.4% |
| read (random) | 655 303 | 716 800 | +9.4% |
| read (batch 1 000) | 1 061 829 | 1 218 256 | +14.7% |
| scan | 1 484 578 | 1 636 249 | +10.2% |
| mixed 80/20 | 289 964 | 336 144 | +15.9% |
| delete (batch) | 581 126 | 821 717 | +41.4% |
| indexed insert | 93 366 | 122 696 | +31.4% |
| indexed update | 46 174 | 53 708 | +16.3% |
| update P99 | 40.8 µs | 28.4 µs | -30.4% |
| 5-index overhead | ×1.85 | ×1.35 | -27% |
| indexed lookup | 6.56 ms | 4.17 ms | -36.4% |
| index range | 5.02 ms | 3.04 ms | -39.4% |
| compaction peak RSS | 420 MB | 121 MB | -71.2% |
| compaction time | 858 ms | 165 ms | -80.8% |

Prefix scan performance is flat versus 2.1.0; the 2.1.1 prefix targets were not reached. Count is flat within thermal variance. Standard single-write, batch-write, batch-read and mixed targets are close but not all met; measured values are reported as-is.

## Tests

94 Rust core, 35 Jest, 9 fallback, 8 fault-injection, smoke OK.

Raw benchmark JSON is attached to this release and committed under `benchmarks/results/2.1.1/`.
