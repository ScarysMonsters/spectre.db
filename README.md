> [!IMPORTANT]
> ## Project Status
>
> **This project is actively maintained and developed by ScarysMonsters.**

> [!NOTE]
> **spectre.db v2 is a high-performance key-value store written in Rust, exposed to Node.js and Bun through napi-rs.**
> **It speaks a binary storage format (CRC32-protected snapshots, WAL v3 with LSN, segmented incremental compaction) and stays 100% API-compatible with spectre.db v1.1.0 — including its JSON file format.**

## About

<strong>Welcome to `@sexfy/spectre.db`, the Rust rewrite of [spectre.db](https://www.npmjs.com/package/spectre.db) — a persistent key-value database engineered for Discord bots, small services and embedded workloads.</strong>

- **Drop-in API** — `get`, `set`, `transaction`, `table`, events: every v1.1.0 call keeps working.
- **Native speed** — the store lives in Rust; values cross the JS↔Rust boundary once.
- **Dual engine** — native addon when available, frozen v1.1.0 JavaScript engine as a fallback.
- **Crash-safe by design** — WAL + atomic snapshot renames, backup rotation, file locking.
- **Still zero runtime dependencies** — the JS layer only uses Node.js built-ins.

<div align="center">
  <p>
    <a href="https://www.npmjs.com/package/@sexfy/spectre.db"><img src="https://img.shields.io/npm/v/@sexfy/spectre.db.svg" alt="npm version" /></a>
    <a href="https://www.npmjs.com/package/@sexfy/spectre.db"><img src="https://img.shields.io/npm/dt/@sexfy/spectre.db.svg" alt="npm downloads" /></a>
    <a href="https://github.com/ScarysMonsters/spectre.db"><img src="https://img.shields.io/github/stars/ScarysMonsters/spectre.db?style=flat" alt="GitHub stars" /></a>
    <a href="https://github.com/ScarysMonsters/spectre.db/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-Custom-blue.svg" alt="license" /></a>
  </p>
</div>

### <strong>[Example Code](https://github.com/ScarysMonsters/spectre.db/tree/main/examples)</strong>

---

## Documentation

- [Documentation index](./docs/README.md)
- [Getting Started](./docs/GETTING-STARTED.md) — installation, first database, migration
- [API Reference](./docs/API.md) — every method, option, event and error code
- [Advanced Usage](./docs/ADVANCED.md) — scan/cursor, indexes, raw values, compaction, multi-process
- [Durability & Crash Recovery](./docs/DURABILITY.md) — WAL, durability modes, backups, fault injection
- [Encryption](./docs/ENCRYPTION.md) — sensitive keys, backups, whole-snapshot encryption
- [Architecture](./docs/ARCHITECTURE.md) — project layout, Rust core, napi bridge, fallback engine
- [Storage Formats](./docs/FORMATS.md) — on-disk specification
- [Troubleshooting](./docs/TROUBLESHOOTING.md) — error reference and FAQ
- [Releasing](./docs/RELEASING.md) — versioning, CI, prebuilds, npm publishing

---

## Features

### Core (v1.1.0 compatible)

- [x] Drop-in API — no code changes required when migrating from `spectre.db` v1.1.0
- [x] WAL-based persistence — never rewrites the full file on every change
- [x] Atomic writes — crash-safe temp file + rename pattern
- [x] Backup rotation — configurable generations (`.1.bak`, `.2.bak`, ...)
- [x] Backup encryption — AES-256-GCM encryption for backup files
- [x] Multi-process support — file locking with stale-lock takeover
- [x] O(1) LRU cache with prefix-index invalidation
- [x] Real transactions — function-based and legacy array-based
- [x] AES-256-GCM encryption — auto-applied to sensitive keys (`token`, `password`, `secret`, ...)
- [x] Table abstraction — scoped key namespacing
- [x] Dot-notation keys — `users.123.coins` works out of the box
- [x] Prototype pollution prevention by design
- [x] Events: `change`, `clear`, `save`, `transaction`, `warn`, `restore`, `reset`

### New in v2

- [x] **Binary v2 format** (`.spdb` / `.spwal`) — CRC32-protected, no JSON parsing on the cold path
- [x] **WAL v3** — monotonic LSN per frame, generation header, single-CRC batches
- [x] **Segmented incremental compaction** — `.spman` manifest + immutable `.spseg-N` segments, background merge
- [x] **Lazy iteration** — `scan()`, `iterate()`, `cursor()` with prefix, limit and cursor resume
- [x] **Secondary indexes** — `db.index()`, `db.find({ ... })`, `db.range()` (sorted B-tree, maintained on write)
- [x] **Raw binary values** — `setRaw()` / `getRaw()` / `deleteRaw()` for `Buffer` / `Uint8Array`
- [x] **zstd / gzip compression** — per segment or per block
- [x] **Durability modes** — `durability: "process" | "durable"` (fsync per WAL flush)
- [x] **Async compaction** — `compactAsync()` runs serialization on a libuv worker thread
- [x] **Full snapshot encryption** — `encryptSnapshot` (AES-256-GCM, configurable scrypt)
- [x] **Migration** — `db.migrate("v2" | "json")` converts databases both ways
- [x] **Rich observability** — `db.stats()` with WAL bytes, cache hits, compression ratio, LSN, recovery count
- [x] **TypeScript types** — first-class `index.d.ts`
- [x] **Prebuilt binaries** — Linux (glibc/musl), macOS (x64/arm64), Windows (x64); no compilation on install

### Current release — v2.1.3

Durability release: no on-disk format change, no API break.

- **Data-loss fixes** — raw values and nested raw keys across segments, snapshot reload and WAL replay; `compactAsync()` full merges no longer resurrect deleted keys; `close()` or `compact()` during `compactAsync()` no longer drops or overwrites data; `compactMode: "auto"` retires stale segments when falling back to legacy
- **OS-level file lock** — `flock` / `LockFileEx` on top of the PID file: no stale-lock takeover race, crashed owners released by the kernel, still compatible with the JS fallback
- **Stricter writes** — unknown batch ops and `undefined` values are rejected instead of being stored; snapshot encoding errors are reported instead of writing an unreadable file
- **Encrypted fields inside objects** — `get("user.password")` returns the decrypted value
- **Non-blocking background compaction** — the `compactInterval` timer uses `compactAsync()`

### Previous release — v2.1.2
- **Crash-safe recovery** — WAL tail truncation, safe backup rotation, directory fsync, and segment/WAL ordering fixes
- **Encryption coverage** — sensitive values are encrypted through `setMany`, `transaction`, and batch paths
- **NFC keys** — string keys, prefixes, scans, cursors, bulk helpers, and transactions normalize to Unicode NFC
- **Recovery diagnostics** — `*.spwal.corrupt` copies, `*.migrate-bak` migration backups, `db.engine`, `db.nativeLoadError`
- **O(1) index maintenance** — reverse key map, contiguous buckets, one shared `Arc` key per leaf across indexes
- **Index integrity** — parent replacement and subtree deletion remove descendant index entries (single and batch)
- **WAL-first writes** — WAL append happens before store/index mutation; an I/O error leaves the database unchanged
- **Parent-count fast path** — leaf sets/deletes skip descendant B-tree scans
- **Direct WAL frames** — set/delete encoded without an intermediate `Op`
- **Prefix-bounded scans** — `page`, `scan`, `all` stop at the prefix boundary
- **Bulk APIs** — `getMany()`, `setMany()`, `deleteMany()`, `scanSync()`
- **Lower memory** — shared `Arc` store keys, shared indexed values, `Arc`-shared compaction snapshots
- **`count(prefix)`** — prefix-bounded counting
- **Crate layout** — `crates/core` and `crates/napi` (package names unchanged)

For the complete per-version history and release notes, see [`CHANGELOG.md`](CHANGELOG.md).

---

## Benchmarks

Cross-version medians on the standard 100k-key harness (Node.js 26, Windows, 3 runs, ~185-byte values). The 2.0.0/2.0.1 columns are published/local MSVC builds; 2.1.0/2.1.1/2.1.2/2.1.3 are local GNU builds, so small deltas include toolchain and thermal variance. Raw JSON: [`benchmarks/results/`](benchmarks/results/).

### Standard workloads — all versions (ops/s)

| Workload | 2.0.0 | 2.0.1 | 2.1.0 | 2.1.1 | 2.1.2 | **2.1.3** |
|---|---:|---:|---:|---:|---:|---:|
| write (single) | 90 547 | 117 176 | 108 104 | 118 125 | 129 918 | **124 844** |
| write (batch 1 000) | 162 509 | 244 365 | 272 581 | 344 583 | 323 315 | **341 601** |
| read (random) | 268 230 | 640 771 | 655 303 | 716 800 | 714 055 | **584 908** |
| read (batch 1 000) | — | 1 070 848 | 1 061 829 | 1 218 256 | 1 091 934 | **982 857** |
| scan (full prefix) | 576 765 | 1 404 492 | 1 484 578 | 1 636 249 | 1 569 893 | **1 332 502** |
| count (queries/s) | 24 | 276 | 298 | 296 | 302 | **262** |
| count (entries/s) | 2 437 641 | 27 648 827 | 29 794 714 | 29 567 516 | 30 204 545 | **26 234 531** |
| mixed 80/20 | 172 736 | 283 742 | 289 964 | 336 144 | 325 189 | **281 118** |
| delete (batched) | 321 517 | 414 003 | 581 126 | 821 717 | 773 932 | **685 084** |

2.1.3 is a durability release (no hot-path change). Every engine in this run, SQLite included, measured 10-15% below the 2.1.2 session, so the cross-version drop is machine state, not the code: a back-to-back A/B of 2.1.2 and 2.1.3 built with the same GNU toolchain gives the same index, compaction and insert figures within run-to-run noise. Compare versions against SQLite/LMDB from the same run below.

### 2.1.3 vs SQLite and LMDB (100k standard, ops/s)

Same 3-run harness and dataset. SQLite uses WAL + `synchronous=NORMAL`; LMDB uses its default sync mode; spectre.db 2.1.3 uses `durability: "process"`. Higher is better.

| Workload | **spectre.db 2.1.3** | SQLite | LMDB | Winner |
|---|---:|---:|---:|---|
| write (single) | **124 844** | 21 903 | 5 428 | spectre.db |
| write (batch 1 000) | **341 601** | 267 060 | 99 049 | spectre.db |
| read (random) | **584 908** | 155 619 | 570 347 | spectre.db |
| read (batch 1 000) | **982 857** | 160 379 | 341 427 | spectre.db |
| scan (full prefix) | **1 332 502** | 889 688 | 1 211 569 | spectre.db |
| count (queries/s) | **262** | 110 | 131 | spectre.db |
| count (entries/s) | **26 234 531** | 10 977 720 | 13 098 040 | spectre.db |
| mixed 80/20 | **281 118** | 56 223 | 24 831 | spectre.db |
| delete (batched) | **685 084** | 243 578 | 163 661 | spectre.db |
| on-disk size | **21.5 MB** | 21.1 MB | 35.8 MB | — |

spectre.db wins **9/9** standard workloads in this run; LMDB is within 3% on random reads. In `durability: "durable"` mode the trade-off changes: LMDB keeps the single-write and mixed-write lead (per-op fsync vs. LMDB's asynchronous flush), while spectre.db wins bulk reads, count, delete and batch ingestion. Raw engine summaries are in `benchmarks/results/2.1.3/standard.json`.

### Index, prefix and compaction — all versions

| Focus | 2.0.1 | 2.1.0 | 2.1.1 | 2.1.2 | **2.1.3** |
|---|---:|---:|---:|---:|---:|
| Indexed insert (5 indexes) | 263 ops/s | 93 366 ops/s | 122 696 ops/s | 142 419 ops/s | **115 002 ops/s** |
| Indexed update | 613 ops/s | 46 174 ops/s | 53 708 ops/s | 56 134 ops/s | **43 186 ops/s** |
| Update P99 | 1 024.6 µs | 40.8 µs | 28.4 µs | 36.3 µs | **49.6 µs** |
| Indexed lookup (10k results) | 4.90 ms | 6.56 ms | 4.17 ms | 4.19 ms | **6.90 ms** |
| Index range (1k results) | 3.77 ms | 5.02 ms | 3.04 ms | 3.07 ms | **4.79 ms** |
| Prefix 0.1% of 1M keys | 57.2 ms | 0.60 ms | 0.607 ms | 0.509 ms | **0.905 ms** |
| Prefix 1% of 1M keys | 37.2 ms | 7.72 ms | 7.56 ms | 6.05 ms | **7.94 ms** |
| Prefix 10% of 1M keys | 151 ms | 65.8 ms | 65.7 ms | 58.9 ms | **73.1 ms** |
| Compaction peak RSS (50k × 4 KB random) | 643 MB | 420 MB | 121 MB* | 130 MB | **112 MB** |
| Compaction time (50k × 4 KB random) | 1 086 ms | 858 ms | 165 ms* | 154 ms | **258 ms** |

\* The 2.1.1 compaction run used repeated `x` values, which compress; 2.1.2 uses incompressible random values by default, so its time/RSS are not directly comparable and are the honest baseline going forward. Index/prefix/compaction data lives under the OS temp directory to avoid cloud-sync filesystem effects; standard workload data lives in `benchmarks/data`. Index, prefix and compaction figures are single runs and are noisy on this laptop: in a same-session A/B with identical GNU builds, index lookup ranged 4.1-7.7 ms for both 2.1.2 and 2.1.3, and compaction 164-203 ms. Read the 2.1.3 column as "no regression", not as a slowdown. Raw results: [`benchmarks/results/2.1.3/`](benchmarks/results/2.1.3/).

```sh
cd benchmarks
npm install
npm run bench                          # published @sexfy/spectre.db baseline
SPECTRE_BENCH_LOCAL=1 node bench.js    # local build (2.1.2+)
```

On Windows PowerShell: `$env:SPECTRE_BENCH_LOCAL=1; node bench.js`.

---

## Installation

> [!NOTE]
> **Node.js 18.0.0 or newer is required.** Bun 1.0+ is supported as well.

```sh-session
npm install @sexfy/spectre.db@latest
```

Prebuilt native binaries are installed automatically as optional dependencies — there is no build step and no postinstall download.

---

## Quick Start

```js
const { Database } = require('@sexfy/spectre.db');

const db = new Database('./data/mydb', {
  cache: true,
  autoSave: 5000,
  backup: true,
});

await db.ready;

db.set('users.1.name', 'Alice');
console.log(db.get('users.1.name')); // 'Alice'

await db.close();
```

The default `format: 'auto'` opens existing v1.1.0 JSON databases as-is and creates new databases in the v2 binary format.

---

## Discord Bot Example

```js
const { Client, GatewayIntentBits } = require('discord.js');
const { Database } = require('@sexfy/spectre.db');

const client = new Client({ intents: [GatewayIntentBits.Guilds] });

const db = new Database('./src/data/database', {
  cache: true,
  autoSave: 5000,
  backup: true,
});

client.db = db;

db.ready.then(() => client.login(process.env.TOKEN));

client.once('ready', () => {
  console.log(`${client.user.tag} is ready!`);
});

process.on('SIGINT', async () => {
  await client.db.close();
  process.exit(0);
});
```

---

## API Reference

### Constructor

```js
new Database(path, options?)
```

| Option | Type | Default | Description |
|---|---|---|---|
| `cache` | boolean | `true` | Enable the LRU cache |
| `maxCacheSize` | number | `1000` | Max cached entries |
| `cacheTTL` | number | `0` | TTL in ms (0 = forever) |
| `autoSave` | number | `5000` | Compaction interval in ms |
| `compactThreshold` | number | `500` | WAL ops before a compaction is triggered |
| `compactInterval` | number | `300000` | Interval between compaction checks (ms) |
| `backup` | boolean | `true` | Enable backup rotation |
| `backupCount` | number | `3` | Number of backup generations |
| `compress` | boolean | `false` | Gzip snapshot compression (JSON mode) |
| `compression` | `'none' \| 'gzip' \| 'zstd'` | `'none'` | Compression algorithm per segment/block |
| `encryptionKey` | string/Buffer | `null` | Key for sensitive values and encrypted backups |
| `encryptBackups` | boolean | `false` | Encrypt backup files |
| `encryptSnapshot` | boolean | `false` | Encrypt the whole v2 snapshot |
| `scryptLogN` / `scryptR` / `scryptP` | number | `14` / `8` / `1` | scrypt KDF parameters |
| `format` | `'auto' \| 'v2' \| 'json'` | `'auto'` | Storage format |
| `durability` | `'process' \| 'durable'` | `'process'` | WAL flush policy |
| `syncWal` | boolean | `false` | Deprecated alias of `durability: 'durable'` (emits `warn`) |
| `walBuffering` | boolean | `false` | 64 KiB-batched WAL writes (throughput mode) |
| `compactMode` | `'auto' \| 'legacy' \| 'segments'` | `'auto'` | Compaction strategy |
| `legacyThreshold` | number | `100000` | Entry count under which full rewrites are used |
| `segmentMergeEvery` | number | `8` | Segments before a full merge |
| `lockTimeout` | number | `30000` | Lock acquisition timeout (ms) |
| `warmKeys` | string[] | `[]` | Keys to pre-load into the cache on startup |

---

### Core Methods

```js
db.get('users.1.name')              // → value or null
db.getMany(['users.1', 'users.2'])  // → values in order (batch lookup)
db.setMany([['users.1', 'Alice']])  // → bulk write
db.set('users.1.name', 'Alice')     // → value
db.deleteMany(['users.1.name', 'users.2.name']) // → true/false per key
db.delete('users.1.name')           // → true / false
db.has('users.1.name')              // → true / false
db.add('users.1.coins', 100)        // → new value
db.sub('users.1.coins', 50)         // → new value
db.push('users.1.roles', 'admin')   // → new array length
db.pull('users.1.roles', 'member')  // → true / false
```

> [!NOTE]
> `set(key, undefined)` throws a `TypeError`. v1.1.0 silently dropped such values at compaction; v2 refuses them up front.

### Query Methods

```js
db.all()                                              // → [{ ID, data }]
db.startsWith('users.')                               // → [{ ID, data }]
db.filter((data, id) => id.endsWith('.coins'))        // → [{ ID, data }]
db.find((data, id)   => id === 'users.1.name')        // → { ID, data } | null
db.find({ status: 'active' })                         // → indexed lookup if db.index('status') exists
db.count('users.')                                    // → number of leaf entries
db.paginate('users.', page, limit, sortBy, sortDesc)  // → { data, pagination }
```

### Lazy Iteration

```js
for await (const { ID, data } of db.scan({ prefix: 'user.', pageSize: 500 })) {
  // rows are fetched page by page — never all in memory
}

// Resume where you stopped:
for await (const row of db.scan({ prefix: 'user.', after: lastID })) { ... }

// Manual paging:
const page = db.cursor('user.', { limit: 100 });
// { rows, cursor, done }
```

### Secondary Indexes

```js
db.index('user.status');                       // create + persist a sorted index
db.find({ status: 'active' });                 // uses the index when defined
db.range('user.age', { gte: 18, lt: 65, limit: 100 }); // → [{ ID, data }]
db.listIndexes();                              // → ['user.status']
```

Indexes are maintained on write and persisted in the segment manifest.

### Raw Binary Values

```js
db.setRaw('thumb:1', imageBuffer);   // Buffer / Uint8Array — stored outside JSON
db.getRaw('thumb:1');                // → Buffer | null
db.deleteRaw('thumb:1');             // → true / false
```

Raw values live in a reserved internal namespace and are invisible to the JSON API.

### Transactions

```js
// Function-based (recommended)
await db.transaction((tx) => {
  const coins = tx.get('users.1.coins') ?? 0;
  tx.set('users.1.coins', coins - 50);
  tx.set('users.2.coins', (tx.get('users.2.coins') ?? 0) + 50);
});

// Array-based (legacy — fully supported)
await db.transaction([
  { type: 'set',    key: 'config.debug',  value: true },
  { type: 'delete', key: 'cache.tmp'                  },
]);
```

> [!NOTE]
> With the native engine the transaction function must be synchronous. Async function transactions are only available on the JavaScript fallback engine; use the array form for async work.

### Tables

```js
const users = db.table('users');

users.set('1.name', 'Alice')   // stored as "users.1.name"
users.get('1.name')            // 'Alice'
users.count()                  // number of entries
await users.clear()            // removes all users.*

await users.transaction((tx) => {
  tx.add('1.coins', 100);
});
```

### Persistence & Lifecycle

```js
await db.save()          // force compaction now
await db.compactAsync()  // non-blocking compaction on a worker thread
await db.close()         // flush + compact + release (always call on shutdown)
db.getStats()            // v1.1.0-shaped stats
db.stats()               // full v2 stats
await db.migrate('v2')   // convert the database to the v2 binary format
```

---

## Advanced Features

### Dual Engine

The package ships two engines:

1. **Native** — the Rust core loaded from `build/Release/`, `prebuilds/` or a platform package.
2. **Fallback** — a frozen, verbatim copy of the v1.1.0 JavaScript engine.

The fallback opens v1.1.0 JSON databases normally and **refuses v2 binary databases** with a clear error: migrate them first (`db.migrate('json')` with the native engine).

```js
const { hasNativeEngine } = require('@sexfy/spectre.db');
console.log(hasNativeEngine); // true when the Rust addon is loaded
```

### Durability Contract

| Mode | Process crash | Power loss |
|---|---|---|
| `durability: "process"` (default) | ✔ no acknowledged write lost | last writes may be lost (page cache) |
| `durability: "durable"` | ✔ | ✔ WAL fsynced on every flush |

`walBuffering: true` batches WAL writes into 64 KiB chunks for higher throughput, at the cost of a wider crash window.

### Compression

```js
const db = new Database('./data/mydb', {
  compression: 'zstd', // 'none' | 'gzip' | 'zstd'
});
```

Compression is applied per segment/block in v2 format. `compress: true` keeps the legacy gzip snapshot behavior for JSON databases.

### Encryption

```js
const db = new Database('./data/secure', {
  encryptionKey: process.env.DB_ENCRYPTION_KEY,
  encryptBackups: true,   // encrypted .N.bak files
  encryptSnapshot: true,  // encrypt the whole v2 snapshot
});

// These keys are automatically encrypted (AES-256-GCM):
db.set('user.password', 'secret123');
db.set('api.token', 'abc123');
db.set('auth.secret', 'xyz789');
```

Any dot-separated key segment that equals or starts with `password`, `secret`, `token`, `apikey`, `api_key` or `private` (followed by `_` or the end of the segment) is encrypted. Without a key, `encryptSnapshot` fails loudly instead of silently falling back to plaintext.

### Multi-Process Support

```js
const db = new Database('./shared.db');

// Process 1
await db.ready;
db.set('counter', 1);

// Process 2 (waits for the lock)
const db2 = new Database('./shared.db');
await db2.ready;
```

The lock file stores the owner pid; dead-pid locks are taken over automatically. `lockTimeout` controls how long to wait before erroring.

### Migration from v1.1.0

`format: 'auto'` (default) detects existing `.snapshot` / `.wal` files and keeps using them. When you are ready to switch:

```js
const db = new Database('./data/mydb', { format: 'auto' });
await db.ready;
await db.migrate('v2');  // writes .spdb/.spwal and removes the JSON files
```

Migration works both ways: `db.migrate('json')` converts a v2 database back to v1.1.0-compatible files.

---

## How it works

### Files on disk

```
data/
├── mydb.spdb             ← v2 binary snapshot (CRC32)
├── mydb.spwal            ← v2 binary WAL (v3 frames, LSN)
├── mydb.spman            ← segment manifest (v2, segmented mode)
├── mydb.spseg-1          ← immutable segments (v2, segmented mode)
├── mydb.lock             ← file lock for multi-process
├── mydb.spdb.1.bak       ← most recent backup (encrypted if enabled)
└── mydb.spdb.2.bak

# JSON mode (v1.1.0 layout):
├── mydb.snapshot
├── mydb.wal
└── mydb.snapshot.1.bak
```

The complete on-disk specification — snapshot layout, WAL v3 frames, segments, encryption envelopes, backups and compatibility matrix — lives in [docs/FORMATS.md](./docs/FORMATS.md).

### Write flow

```
db.set('x', 1)
  └─ validates + serializes in JS, stores in Rust (in-memory BTreeMap)
  └─ WAL append (one write() of key‖value bytes)

Every compactInterval:
  └─ if walOps >= compactThreshold:
       └─ compact(): rotate backups → write new snapshot → atomic rename → truncate WAL
```

### Startup flow

```
new Database(path)
  └─ acquire file lock
  └─ load snapshot (v2 binary or v1.1.0 JSON)
  └─ replay WAL on top (stops at the first bad/truncated CRC)
  └─ db.ready resolves
```

---

## Security Best Practices

### 1. Use Encryption for Sensitive Data

```js
const db = new Database('./data/secure', {
  encryptionKey: process.env.DB_ENCRYPTION_KEY,
  encryptBackups: true,
  encryptSnapshot: true,
});
```

### 2. Validate Input

```js
db.set('user.name', 'Alice');            // valid
db.set('__proto__.polluted', 'value');   // rejected — prototype pollution guard
```

### 3. Handle Errors Properly

```js
db.on('warn', (message) => console.warn('[@sexfy/spectre.db]', message));

db.on('change', ({ type, key }) => {
  console.log(type, key);
});
```

### 4. Use Transactions for Atomic Operations

```js
await db.transaction((tx) => {
  const balance = tx.get('user.balance') ?? 0;
  if (balance < amount) {
    throw new Error('Insufficient balance');
  }
  tx.set('user.balance', balance - amount);
});
```

---

## Testing

```bash
npm ci

# Native engine
npm run build && npm test

# JS fallback engine
npm run test:fallback

# Crash recovery / fault injection
npm run test:fault

# End-to-end smoke test
npm run smoke
```

---

## Contributing

- Before creating an issue, please ensure that it hasn't already been reported/suggested.
- See [the contribution guide](https://github.com/ScarysMonsters/spectre.db/blob/main/CONTRIBUTING.md) if you'd like to submit a PR.

## Need help?

GitHub Issues: [Here](https://github.com/ScarysMonsters/spectre.db/issues)

---

## Other project(s)

- 🤖 [***ScarysMonsters***](https://github.com/ScarysMonsters) <br/>
  More tools and bots.

---

## License

Source-available, custom license. See [LICENSE](./LICENSE) for the full terms
(attribution required, commercial use requires prior written consent).

---

## Star History

<a href="https://www.star-history.com/?repos=ScarysMonsters%2Fspectre.db&type=date&legend=top-left">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/image?repos=ScarysMonsters/spectre.db&type=date&theme=dark&legend=top-left" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/image?repos=ScarysMonsters/spectre.db&type=date&legend=top-left" />
    <img alt="Star History Chart" src="https://api.star-history.com/image?repos=ScarysMonsters/spectre.db&type=date&legend=top-left" />
  </picture>
</a>
