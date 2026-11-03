# Changelog — spectre.db

## 2.0.1 (2026-11-03)

Performance release. No on-disk format change and no API break: v1.1.0 (JSON) and v2.0.0 (binary) databases open unchanged.

### Performance
- **Point reads**: added an O(1) hash index alongside the ordered B-tree; `get()` now uses a zero-copy path and a single lookup. `db.get` improved from 268k to 641k ops/s in the benchmark (+2.4x), ahead of LMDB.
- **Key validation**: ASCII fast path without Unicode normalization or allocations on hot read paths.
- **Scan**: direct JSON page builder (no row clones, no per-key `serde_json` allocation) plus `scanSync()`; `scan()` no longer uses the O(n²) `Array.shift()`. Scan improved from 577k to 1.40M rows/s (+2.4x).
- **Count**: `count()` no longer materializes `all()`; improved from 2.4M to 27.6M ops/s.
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
