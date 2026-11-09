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

### New in v2.1.1

- [x] **Index integrity** — parent replacement and subtree deletion remove descendant index entries
- [x] **WAL-first writes** — WAL append happens before store/index mutation; an I/O error no longer leaves a silently mutated database
- [x] **Parent-count fast path** — leaf writes/deletes skip descendant B-tree scans
- [x] **Direct WAL encoding** — set/delete frames written without an intermediate `Op`, fewer key/value copies
- [x] **`count(prefix)`** — prefix-bounded counting instead of a full-map fallback
- [x] **Shared index keys** — one `Arc` key allocation per leaf update across all secondary indexes
- [x] **`deleteMany()` parity** — same native/fallback results; batch delete no longer creates 1,000 JS objects per chunk

### New in v2.1.0

- [x] **O(1) index maintenance** — reverse key map + contiguous buckets
- [x] **Shared JSON parsing** — one parse per document for all indexes
- [x] **Prefix-bounded scans** — `page`, `scan`, `all` stop at the prefix boundary
- [x] **Lower memory** — shared `Arc` keys, shared indexed values, `Arc`-shared compaction snapshots
- [x] **Bulk APIs** — `getMany()`, `setMany()`, `scanSync()`
- [x] **Crate layout** — `crates/core` and `crates/napi` (package names unchanged)

---

## Benchmarks

### 2.1.0 → 2.1.1 engine improvements

Raw JSON: [`benchmarks/results/2.1.1/`](benchmarks/results/2.1.1/). The 2.1.0 column is the previous local GNU build, 2.1.1 is the current local GNU build; ratios are the meaningful figure.

| Focus | 2.1.0 | 2.1.1 | Delta |
|---|---:|---:|---:|
| Indexed insert, 100k docs, 5 indexes | 93 366 ops/s | **122 696 ops/s** | **+31.4%** |
| Indexed update, 100k docs, 5 indexes | 46 174 ops/s | **53 708 ops/s** | **+16.3%** |
| Update P50 / P95 / P99 (100k) | 19.6 / 25.5 / 40.8 µs | **16.9 / 21.2 / 28.4 µs** | P99 **-30.4%** |
| Indexed lookup (10k results) | 6.56 ms | **4.17 ms** | **-36.4%** |
| Index range (1k results) | 5.02 ms | **3.04 ms** | **-39.4%** |
| Indexed insert overhead vs no index | ×1.85 | **×1.35** | under the 1.5× target |
| Compaction peak RSS (50k × 4 KB) | 420 MB | **121 MB** | **-71.2%** |
| Compaction time (50k × 4 KB) | 858 ms | **165 ms** | **-80.8%** |
| Prefix scan 0.1% / 1% / 10% (1M keys) | 0.60 / 7.72 / 65.8 ms | 0.61 / 7.56 / 65.7 ms | flat (targets not reached) |

### 2.1.1 targets check

| Metric | Target | Achieved | Status |
|---|---:|---:|:--|
| Single write | 130k/s | 118 125/s | close |
| Batch write | 350k/s | 344 583/s | close |
| Random read | 680k/s | 716 800/s | met |
| Batch read | 1.30M/s | 1 218 256/s | close |
| Full scan | 1.55M/s | 1 636 249/s | met |
| Mixed workload | 350k/s | 336 144/s | close |
| DeleteMany | 800k/s | 821 717/s | met |
| Indexed insert | 130k/s | 122 696/s | close |
| Indexed update | 65k/s | 53 708/s | not met |
| Update P50 | 15 µs | 16.9 µs | close |
| Update P99 | 25 µs | 28.4 µs | close |
| Lookup 10k | 4 ms | 4.17 ms | close |
| Range 1k | 3 ms | 3.04 ms | close |
| 5-index overhead | <1.5× | 1.35× | met |
| Compaction peak RSS | 300 MB | 121 MB | met |
| Compaction time | 650 ms | 165 ms | met |

Allocation and large-delete memory targets were not instrumented in this cycle and remain open.

### Standard workloads — 2.1.1

Median of 3 runs on Windows, Node.js 26.4.0, 8 logical cores, 100,000 keys, ~185-byte JSON values. SQLite uses WAL + `synchronous=NORMAL`; LMDB uses its default sync mode; spectre.db runs the local 2.1.1 build with `autoSave: 0`. Higher is better (ops/s).

| Workload | spectre.db | SQLite | LMDB | Winner |
|---|---:|---:|---:|---|
| write (single op) | **118 125** | 25 096 | 5 907 | spectre.db |
| write (batch of 1,000) | **344 583** | 305 901 | 111 289 | spectre.db |
| read (random) | **716 800** | 180 006 | 635 332 | spectre.db |
| read (batch of 1,000) | **1 218 256** | 182 469 | 377 864 | spectre.db |
| scan (full prefix) | **1 636 249** | 1 081 143 | 1 558 778 | spectre.db |
| count (queries/s, 100k keys) | **296** | 131 | 148 | spectre.db |
| count (entries/s) | **29 567 516** | 13 149 607 | 14 837 037 | spectre.db |
| mixed 80/20 | **336 144** | 64 163 | 27 945 | spectre.db |
| delete (batched) | **821 717** | 258 359 | 176 214 | spectre.db |
| on-disk size | 21.5 MB | 21.1 MB | 35.8 MB | — |

spectre.db wins 9/9 workloads against LMDB in this run. Count is flat versus 2.1.0 (-0.7%) and within run-to-run variance.

### 2.1.0 → 2.1.1 standard workloads

| Workload | 2.1.0 | 2.1.1 | Delta |
|---|---:|---:|---:|
| write (single op) | 108 104 | 118 125 | **+9.3%** |
| write (batch of 1,000) | 272 581 | 344 583 | **+26.4%** |
| read (random) | 655 303 | 716 800 | **+9.4%** |
| read (batch of 1,000) | 1 061 829 | 1 218 256 | **+14.7%** |
| scan (full prefix) | 1 484 578 | 1 636 249 | **+10.2%** |
| count (queries/s) | 298 | 296 | -0.7%* |
| count (entries/s) | 29 794 714 | 29 567 516 | -0.8%* |
| mixed 80/20 | 289 964 | 336 144 | **+15.9%** |
| delete (batched) | 581 126 | 821 717 | **+41.4%** |

\* Count is statistically flat within thermal variance. The large structural gains are index maintenance, WAL-first writes, bulk delete and compaction.

Raw results: [`benchmarks/results/`](benchmarks/results/). Reproduce:

```sh
cd benchmarks
npm install
npm run bench                          # published @sexfy/spectre.db baseline
SPECTRE_BENCH_LOCAL=1 node bench.js    # local build (2.1.1+)
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

Sensitive keys start with `password`, `secret`, `token`, `apikey`, `api_key` or `private` (followed by `.`, `_` or end of key). Without a key, `encryptSnapshot` fails loudly instead of silently falling back to plaintext.

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
