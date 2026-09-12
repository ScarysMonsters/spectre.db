# spectre.db Storage Formats (v2)

This document specifies every on-disk format produced by spectre.db v2
(npm: `@sexfy/spectre.db`) and its compatibility with spectre.db v1.1.0 JSON files.

## File layout

For a database opened at `<path>` (extensions `.json` / `.db` / `.snapshot`
stripped from the base name, like v1.1.0):

| File               | Format | Mode |
| ------------------ | ------ | ---- |
| `<base>.spdb`      | v2 binary snapshot | v2 mode |
| `<base>.spwal`     | v2 binary WAL      | v2 mode |
| `<base>.snapshot`  | v1.1.0 JSON snapshot | JSON mode |
| `<base>.wal`       | v1.1.0 JSONL WAL     | JSON mode |
| `<base>.lock`      | lock file (both modes) | always |
| `<base>.spdb.N.bak` / `<base>.snapshot.N.bak` | rotating backups (N = 1..backupCount) | mode-dependent |

Format mode is chosen at open time:

* `format: "auto"` (default) — `v2` when a `.spdb`/`.spwal` exists, `json`
  when a `.snapshot`/`.wal` exists, otherwise **v2** for fresh databases.
* `format: "v2"` / `format: "json"` — explicit override (loading falls back
  to the other file set when the preferred one is absent; the next
  `compact()`/`migrate()` materializes the chosen format).
* `db.migrate("v2" | "json")` converts an existing database and removes the
  old files.

## 1. v2 binary snapshot (`.spdb`)

All integers are little-endian.

```
offset  size  field
0       8     magic  "SPDBSNAP"
8       2     version u16 (= 2)
10      2     flags   u16  (bit0 = payload is gzip-compressed, gzip level 1)
12      4     reserved u32 (0)
16      ...   payload:
                entry_count u64
                entry_count × {
                  key_len u32
                  key bytes (UTF-8, full dot path)
                  val_len u32
                  val bytes (JSON-encoded value)
                }
...     4     crc32 LE over bytes [0, len-4)
```

* Values are stored exactly as the engine received them from
  `JSON.stringify` — no reinterpretation. Loading is a bounds-checked memcpy
  per entry: no JSON parsing on the cold path.
* The file is written to `<base>.spdb.<pid>.<ts>.<rand>.tmp` then renamed
  (atomic replacement, same as v1.1.0).

## 2. v2 binary WAL (`.spwal`)

Append-only framed stream; one `write()` per record (or per batch flush when
`walBuffering` is enabled):

```
record := type u8 | payload_len u32 | payload | crc32 u32 (over type||payload)

type 1 (set):    payload = key_len u32 | key | value (JSON bytes)
type 2 (del):    payload = key
type 3 (clear):  payload = empty
type 4 (batch):  payload = op_count u32 | op_count × record(set|del)
```

* Torn writes: replay stops at the first frame with a bad/truncated CRC and
  emits a `warn` event. Earlier frames stay valid.
* `batch` is a single record → group commit preserves atomicity.
* Durability model matches v1.1.0: write + flush per op, **no fsync** (safe
  against process death, not against OS/power failure). `syncWal: true`
  fsyncs every flush for stronger guarantees. `walBuffering: true` batches
  writes into 64 KiB chunks (higher throughput, weaker crash window).

## 3. JSON v1 mode (byte-compatible with spectre.db v1.1.0)

* Snapshot: one compact JSON document of the nested tree
  (`JSON.stringify` equivalent; key order follows our sorted store rather
  than JS insertion order — semantically identical, byte order may differ).
* WAL: JSON Lines, fixed key order:
  * `{"op":"set","k":"<key>","v":<value>}`
  * `{"op":"del","k":"<key>"}`
  * `{"op":"clear"}`
  * `{"op":"batch","ops":[...]}`
* Value payloads are spliced as raw JSON bytes (already valid JSON).

## 4. Lock file (`.lock`)

Identical to v1.1.0: exclusive creation (`wx`), content `"<pid>\n"`, fsync,
stale takeover when the pid is dead, 100 ms polling, 30 s default timeout
(`lockTimeout` option).

## 5. Encryption

* KDF: `scrypt(rawKey, "spectre-db-kdf-v1", N=16384, r=8, p=1, 32)` —
  32-byte raw keys are used as-is (v1.1.0 parity).
* Sensitive values (keys starting with `password|secret|token|apikey|api_key|private`
  followed by `.`, `_` or end) are AES-256-GCM encrypted and stored as the
  envelope `{"__enc":1,"iv":"<b64>","ct":"<b64>","tag":"<b64>"}` (fixed key
  order, byte-identical to v1.1.0 envelopes).
* `encryptBackups: true` encrypts backup files as raw `iv(12) || tag(16) ||
  ciphertext` (v1.1.0 layout; in JSON mode the *uncompressed* content is
  encrypted then gzipped when `compress` is set).

## 6. Backups

Every `compact()` first rotates backups: `.snapshot → .1.bak → .2.bak → …`
(same for `.spdb`). `backupCount: 0` disables rotation. On a corrupted
snapshot at open, the engine restores the newest valid backup and emits
`warn` + `restore` (or `reset` when none is usable).

## 7. Cross-engine compatibility matrix

| Scenario | Result |
| --- | --- |
| Native (JSON mode) opens v1.1.0 files | ✅ |
| v1.1.0 opens native JSON-mode files | ✅ |
| Native opens v1.1.0 WAL (JSONL) | ✅ |
| v1.1.0 opens a v2 database | ❌ — clear error (guard in fallback), use native + `migrate("json")` |
| v1.1.0 envelope `{__enc:1}` values | ✅ decrypted with the same KDF |

## 8. v1.1.0 bugs fixed by the v2 engine (format unchanged)

1. `ready` race — `_initComponents` did not await `_init()`, so `await db.ready`
   could resolve before the snapshot/WAL finished loading. v2 init is
   synchronous; `ready` always means "data loaded".
2. `close()` set `_closed` before compaction, so the final compaction was
   silently skipped. v2 compacts *then* closes.
3. `encryptBackups + compress` restore double-gunzipped and failed. v2
   restores correctly.
4. `encryptBackups` was never applied during compaction (option ignored).
   v2 encrypts the rotated backup in place.
5. `set(key, undefined)` silently produced state that vanished at compaction.
   v2 rejects `undefined` with a `TypeError`.
6. Delete of a path that only exists as a branch reported `false` in some
   orderings; v2 matches v1.1.0's documented quirk (`true` when anything was
   removed) and never leaves dangling parents.

---

# §10-13 — 2.0.0 additions

## §10. WAL v3 (`.spwal`)
```
Header 32 bytes : "SPDBWAL3" | version u16 (=3) | flags u16 | generation u64
                  | base_lsn u64 | crc32 u32 (over bytes [0..28])
Frame           : type u8 | plen u32 | payload | lsn u64 | crc32(type||plen||payload||lsn)
Batch v3 (type 5): count u32 | raw sub-ops [type u8 | klen u32 | key | (vlen u32 | value)]
                  — ONE CRC per frame (v2 re-CRCed every sub-op)
```
A v2.0 WAL (headerless) is read as-is (generation 0, LSN 0).
`SPECTRE_CRASH_AT=point[:skip]` — crash injection (§9): `wal_after_write`,
`wal_after_sync`, `snap_tmp_written`, `snap_before_rename`, `snap_after_rename`,
`seg_written`, `man_before_rename`.

## §11. Segmented layout (`.spman` + `.spseg-N`)
- Each segment = a file in snapshot format §3 (reused as-is), immutable.
- Manifest: generation + ordered segment list (id, gen, entries, name, CRC32 of
  the file) + secondary index definitions. Atomic write (tmp+rename).
- Load order: `.spdb` (base) → segments in manifest order → WAL. Most recent
  wins. A segment with an invalid CRC is skipped (warn).
- Tombstone: sentinel value `00 DEL` (never valid JSON) — removes keys deleted
  from the base at load time (anti-resurrection).
- `compact()` (segments mode): delta = segment of dirty keys only; full merge =
  fresh `.spdb` + purge segments + truncate WAL (after manifest commit).
  `compactAsync()`: phase 1 (short lock: flush + dirty snapshot) → 2 (worker
  thread: serialization) → 3 (short lock: write + manifest; WAL NOT truncated —
  idempotent replay).

## §12. Raw namespace
Internal keys prefixed with `0x00` (never produced by JSON key validation):
invisible to `get/all/scan/count/has`, persisted in snapshots/WAL/segments.
Opaque values (`getRaw`/`setRaw`), no JSON validation.

## §13. Whole-snapshot encryption
Flag `FLAG_ENCRYPTED = 0b0100`: payload = compression then AES-256-GCM
`iv(12) || tag(16) || ct` (AAD = "SPDBSNAP"). Without a key: hard error 4001
(no silent backup restore). Configurable scrypt (log2 N, r, p).
