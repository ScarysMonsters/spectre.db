# Changelog — spectre.db

## 2.1.0 (2026-11-06)

Performance and memory release. No on-disk format change and no public API break: v1.1.0 (JSON) and v2.0.x (binary) databases open unchanged.

### Index maintenance
- **O(1) index removal**: `SecondaryIndex` now keeps a reverse `HashMap` from key to encoded value and bucket position, with contiguous `Vec` buckets. `remove_key()` no longer scans every indexed value; `insert_key()` no longer degrades with index cardinality.
- **Shared JSON parsing**: index updates parse the document once per leaf and reuse the parsed value for every index.
- **Batch deduplication**: repeated keys in a batch are resolved to final leaf values once; descendant keys of a deleted prefix are removed from every index.
- **Shared encoded values**: keys with the same indexed value share the encoded `Arc`, reducing index memory.

### Prefix scans
- `page()`, `page_ref()`, `all()` and `scan()` now use lexicographic prefix bounds instead of scanning to the end of the tree and filtering. Ancestor leaves are still included for dotted-path semantics.

### Memory
- `Store` keys are now shared `Arc<[u8]>` between the ordered `BTreeMap` and the lookup `HashMap`; keys are allocated once instead of twice.
- Full and delta compaction share immutable `Arc` values with the live store, lowering peak RSS during compaction.
- WAL buffers reserve the exact frame size before encoding large values.

### Added
- db.deleteMany(keys) — bulk delete in a single native call.

### Benchmarks (Windows, Node 26, 100k docs, 5 indexes)
- Indexed insert: **263 → 93,366 ops/s** in the 100k run (up to **355x**).
- Indexed update: **613 → 46,174 ops/s** (up to **75x**); P50 **319 → 19.6 µs**, P95 **702 → 25.5 µs**, P99 **1,025 → 40.8 µs** (100k run).
- Indexed insert overhead vs no index: from **×641** to **×1.85** depending on run and cardinality.
- Bulk delete: **414k → 581k ops/s (+40%)** with deleteMany(); standard workloads are otherwise flat to +11.5%, except single-write toolchain/thermal variance.
- Prefix scans on 1M keys: 0.1% prefix **57.2 → 0.60 ms** (95x), 1% **37.2 → 7.72 ms** (4.8x), 10% **151 → 65.8 ms** (2.3x).
- Forced-segment compaction (50k × 4 KB): peak RSS delta **643 → 420 MB** (-34.7%), compaction time **1,086 → 858 ms** (-21%).

### Repository
- Rust crates moved from `crates/spectre-db-core` / `crates/spectre-db-napi` to `crates/core` / `crates/napi`. Package names remain `spectre-db-core` and `spectre-db-napi`.
- Added reproducible focused benchmarks: `benchmarks/index-bench.js`, `benchmarks/prefix-bench.js`, `benchmarks/compact-bench.js`, with raw results in `benchmarks/results/2.1/`.

### Tests
- 87 Rust core tests, 35 Jest tests, 9 fallback tests, smoke test.

## 2.0.1 (2026-11-03)

Performance release. No on-disk format change and no API break: v1.1.0 (JSON) and v2.0.0 (binary) databases open unchanged.

### Performance
- **Point reads**: added an O(1) hash index alongside the ordered B-tree; `get()` now uses a zero-copy path and a single lookup. `db.get` improved from 268k to 641k ops/s in the benchmark (+2.4x), ahead of LMDB.
- **Key validation**: ASCII fast path without Unicode normalization or allocations on hot read paths.
- **Scan**: direct JSON page builder (no row clones, no per-key `serde_json` allocation) plus `scanSync()`; `scan()` no longer uses the O(n²) `Array.shift()`. Scan improved from 577k to 1.40M rows/s (+2.4x).
- **Count**: `count()` no longer materializes `all()`; improved from 2.4M to 27.6M entries/s (about 276 full-prefix count queries/s at 100k keys).
- **Batch writes**: `RawValue` batch parsing and the native `setManyPairs` path; batch ingestion is 2.8x faster than LMDB.
- **N-API**: values are handed to V8 directly from the store buffer, without an intermediate Rust `String`.

### Added
- `db.getMany(keys)` — bulk lookup in a single native call.
- `db.setMany(entries)` — bulk `[key, value]` writes in a single native call.
- `db.scanSync(opts)` — synchronous iterator with no per-row promises.
- `benchmarks/` — reproducible SQLite / LMDB comparison harness with committed results.

### Internal
- `Store` now keeps a `BTreeMap` and a `HashMap<Arc<[u8]>>`; values are shared between both indexes.
- Tests: 78 Rust core, 35 Jest, 9 fallback.

### Benchmarks
See the Benchmarks section in `README.md`. Raw JSON results are in `benchmarks/results/`.

## 2.0.0 (2026-09-12)

New engine generation: binary v2 format, scalability and crash-safety proven by fault-injection tests.

### P0 — scalability & robustness
- **Lazy iteration**: `scan()`, `iterate()` (`for await...of`), `cursor()` with
  `prefix`, `limit` and cursor resume (`after`). No full materialization.
  `all()` is unchanged (v1.1.0 compatibility) and implemented as a special case
  of scan.
- **Incremental segment compaction**: `active WAL → immutable segment (.spseg-N)
  → background full merge`. Versioned manifest (`.spman`, CRC32). Modes
  `compactMode: "auto" | "legacy" | "segments"` — full v2.0 rewrites remain
  available for small databases (`legacyThreshold`).
- **Rename atomicity** covered by fault injection; CI matrix on Linux / macOS
  (APFS) / Windows (NTFS). Guarantees: POSIX atomic rename; Windows:
  MoveFileEx REPLACE.

### P1 — durability, concurrency, batch
- **Batch fast path**: WAL v3 — a batch is encoded as ONE record with ONE CRC
  (v2 re-CRCed every sub-op) + move-based application (zero clone) +
  `setManyJson()` (single FFI document).
- **Durability contract**: `durability: "process" | "durable"` (`syncWal`
  remains accepted as a deprecated alias and emits a `warn`).
- **Binary API**: `getRaw()` / `setRaw()` / `deleteRaw()` (Buffer/Uint8Array),
  internal 0x00 namespace — invisible to the JSON API.
- **Documented concurrency model**: `Arc<Mutex<Engine>>` napi (single writer,
  short critical sections). **`compactAsync()`**: serialization on a libuv
  worker thread; JS reads/writes continue during compaction (3 phases,
  idempotent WAL replay).
- **Fault injection**: 8 instrumented crash points
  (`SPECTRE_CRASH_AT=name[:skip]`) — WAL before/after fsync, snapshot
  tmp/rename, segment, manifest. Suite `tests/fault-injection.test.js`: kill
  process + recovery verification.
- **Cross-platform npm packaging**: per OS/arch `optionalDependencies`
  (linux gnu+musl, macOS x64+arm64, Windows x64-msvc), glibc/musl detection,
  zero user-side compilation, zero postinstall download.

### P2 — advanced features
- **Secondary indexes**: `db.index("users.email")`, `db.find({status:"active"})`,
  `db.range("age", {gte, lt, limit})` — sorted B-tree, sortable numeric
  encoding, zero overhead without an index, definitions persisted in the
  manifest.
- **LSN + generation**: WAL v3 header (`SPDBWAL3`, versioned) — monotonic LSN
  per frame, manifest generation, `recovery_count`. v2.0 WALs (headerless)
  remain readable.
- **zstd compression**: `compression: "none" | "gzip" | "zstd"` per
  segment/block (no more monolithic gzip).
- **Observability**: `db.stats()` — `walBytes`, `pendingWrites`,
  `cacheHits/Misses`, `compressionRatio`, `segmentCount`, `generation`,
  `lastCompactionMs`, `lastLsn`, `recoveryCount`, `indexCount`. `getStats()`
  keeps the v1.1.0 shape.
- **Encryption**: configurable scrypt (`scryptLogN/R/P`), `encryptSnapshot`
  option (AES-256-GCM encryption of the whole snapshot, explicit failure
  without a key — no silent fallback).

### Compatibility
- v1.1.0 (JSON) and v2.0 (binary) databases: opening + behavior unchanged.
- Migration: `db.migrate("v2")` / `db.migrate("json")` tested both ways.
- The JS fallback cleanly refuses the binary format with an explicit message.
- No public API breakage from v1.1.0/v2.0.

## 2.0.0-beta (initial fork)
- Full Rust rewrite (napi-rs v3), binary v2.0 format + byte-for-byte v1.1.0
  JSON compatibility, dual engine (native Rust + frozen JS fallback).
