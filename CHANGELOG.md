# Changelog — spectre.db

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
