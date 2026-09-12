# Advanced Usage

This document covers the features beyond basic CRUD: lazy iteration, secondary
indexes, raw binary values, compression, segmented compaction, asynchronous
compaction and multi-process access.

---

## Lazy iteration

`all()` materializes every entry in memory. For large datasets, use `scan()`
or `cursor()` instead — they page through the store.

### Async iteration

```js
for await (const { ID, data } of db.scan({ prefix: 'user.', pageSize: 500 })) {
  // process one entry at a time
}
```

Options:

| Option | Default | Description |
|---|---|---|
| `prefix` | — | Only yield IDs starting with this prefix |
| `after` | — | Resume after this ID (cursor) |
| `pageSize` | `500` | Rows fetched per page |
| `limit` | ∞ | Maximum number of rows to yield |

### Resumable scans

```js
let lastID = '';

for await (const row of db.scan({ prefix: 'user.', after: lastID })) {
  lastID = row.ID;
  // persist lastID to resume later
}
```

### Manual paging

```js
let page = db.cursor('user.', { limit: 100 });

while (!page.done) {
  for (const { ID, data } of page.rows) {
    // ...
  }
  page = db.cursor('user.', { after: page.cursor, limit: 100 });
}
```

On the native engine the page is produced in Rust (`scanJson`) and parsed once
in V8. The fallback engine emulates the same API on top of `all()`.

---

## Secondary indexes

Native engine only. An index is a sorted B-tree over a dotted field path,
maintained on every write and persisted in the segment manifest. Creating an
index scans the current store once; after that, lookups are O(log n).

```js
db.index('user.status');
db.index('user.age');

db.listIndexes(); // ['user.age', 'user.status']
```

### Lookups

```js
const status = db.index('user.status');

status.keys('active');   // ['user.1', 'user.4', ...]
status.get('active');    // [{ ID, data }, ...]
```

### Ranges

Numeric and lexicographic ranges are supported (numbers are encoded in a
sortable form):

```js
status.range({ gte: 'a', lt: 'm' });              // keys
db.range('user.age', { gte: 18, lt: 65, limit: 10 }); // [{ ID, data }]
```

### Using indexes implicitly

`db.find({ ... })` uses an index automatically when one of the matched fields
is indexed; otherwise it falls back to a full scan.

```js
db.index('user.status');
db.find({ status: 'active' }); // indexed lookup
```

### Dropping

```js
db.index('user.status').drop(); // true
```

Indexes add zero cost when unused. They are persisted across restarts and
rebuilt on write, so a corrupted index can always be recreated with
`db.index(path)` after a drop.

---

## Raw binary values

Native engine only. Raw values are stored outside the JSON namespace
(internal prefix `0x00`) and are invisible to `get`, `all`, `scan`, `count`,
`has` and JSON iteration.

```js
const image = fs.readFileSync('avatar.png');

db.setRaw('blob:avatar:1', image);
const back = db.getRaw('blob:avatar:1'); // Buffer
db.deleteRaw('blob:avatar:1');           // true
```

Use cases: thumbnails, attachments, encrypted blobs, protobuf/msgpack payloads,
anything that should not be JSON-encoded.

`stats().rawEntries` reports how many raw entries exist.

---

## Compression

v2 snapshots and segments support per-block compression:

```js
const db = new Database('./data/mydb', {
  compression: 'zstd', // 'none' | 'gzip' | 'zstd'
});
```

| Value | Algorithm | Notes |
|---|---|---|
| `'none'` | — | Fastest, largest files |
| `'gzip'` | DEFLATE | Legacy-compatible |
| `'zstd'` | Zstandard | Best ratio/speed trade-off |

`compress: true` keeps the legacy gzip snapshot behavior (mainly useful for
JSON-mode databases). `stats().compressionRatio` shows the current ratio.

---

## Compaction modes

v2 supports two compaction strategies, selected with `compactMode`:

| Mode | Behavior |
|---|---|
| `'auto'` (default) | Full rewrites below `legacyThreshold` entries, segmented deltas above |
| `'legacy'` | Always full rewrite (`.spdb` replaced) |
| `'segments'` | Always incremental segments (`.spseg-N` + `.spman` manifest) |

Segmented mode:

- Each `compact()` writes only the dirty keys as an immutable segment.
- Every `segmentMergeEvery` segments (default 8), a full merge runs: a fresh
  `.spdb`, segments purged, WAL truncated.
- Deletions are stored as tombstones so deleted keys cannot resurrect from the
  base snapshot.

```js
const db = new Database('./data/big', {
  compactMode: 'segments',
  segmentMergeEvery: 8,
});
```

Full on-disk details: [FORMATS.md](./FORMATS.md).

---

## Asynchronous compaction

`compactAsync()` performs serialization on a libuv worker thread so the event
loop keeps serving reads and writes:

```js
const status = await db.compactAsync();
// 'committed' — the compaction was applied
// 'stale'     — the store changed too much; nothing was applied
// 'legacy'    — the legacy full-rewrite path was used
```

Three phases:

1. **Short lock** — flush the WAL and snapshot the dirty keys.
2. **Worker thread** — serialize the snapshot/segments off the main thread.
3. **Short lock** — write files and update the manifest; the WAL is *not*
   truncated, so a crash replays idempotently.

Use it for large databases where a blocking `compact()` would stall the bot or
service.

---

## Multi-process access

A file lock (`<base>.lock`) serializes writers across processes. The lock stores
the owner pid; stale locks (dead pid) are taken over automatically.

```js
// Process 1
const db1 = new Database('./shared.db');
await db1.ready;
db1.set('counter', 1);

// Process 2 — waits for the lock
const db2 = new Database('./shared.db');
await db2.ready;
```

Control the wait with `lockTimeout` (ms, default 30000):

```js
const db = new Database('./shared.db', { lockTimeout: 5000 });

try {
  await db.ready;
} catch (err) {
  if (err.code === 7001) {
    // LOCK_TIMEOUT
  }
}
```

The lock is released on `close()` and on process exit. Note that the model is
single-writer: a second process blocks until the first releases the lock.

---

## Transactions in depth

### Staging and rollback

The function form stages all writes in memory and commits them as a single WAL
batch record. If the callback throws, nothing is written.

```js
await db.transaction((tx) => {
  const a = tx.get('accounts.a') ?? 0;
  const b = tx.get('accounts.b') ?? 0;
  if (a < 100) throw new Error('insufficient funds');
  tx.set('accounts.a', a - 100);
  tx.set('accounts.b', b + 100);
});
```

### Performance: `setManyJson`

When every operation is a `set`, the native engine sends the whole batch to
Rust as a single JSON document and writes one WAL frame — the fastest path.

```js
await db.transaction([
  { type: 'set', key: 'a', value: 1 },
  { type: 'set', key: 'b', value: 2 },
]);
```

### Choosing between forms

| Need | Form |
|---|---|
| Read-then-write logic | Function form (sync on native) |
| Bulk writes | Array form of `set`/`delete` |
| Async work inside the transaction | Array form (or fallback engine) |
| `add`/`sub`/`push`/`pull` on native | Precompute with `get`, then `set` |

---

## Caching

```js
const db = new Database('./data/discord', {
  cache: true,
  maxCacheSize: 10000,
  cacheTTL: 60000,          // 1 minute
  warmKeys: ['config.settings', 'user.1.profile'],
});
```

- `maxCacheSize` / `cacheTTL` apply to the JS fallback engine.
- `warmKeys` preloads frequent keys on startup.
- The native engine maintains its own cache; `stats()` exposes `cacheHits` and
  `cacheMisses`.

---

## Environment variables

| Variable | Effect |
|---|---|
| `SPECTRE_FORCE_FALLBACK=1` | Force the JavaScript fallback engine |
| `SPECTRE_CRASH_AT=point[:skip]` | Crash-injection testing (see [DURABILITY.md](./DURABILITY.md)) |

Test-only variables used by the suite: `SPECTRE_DB_PATH`, `SPECTRE_CRASH_MODE`,
`SPECTRE_COMPACT_MODE`, `SPECTRE_DURABILITY`.

---

## Fallback engine constraints

When `hasNativeEngine` is `false`:

- v2 databases cannot be opened (code 3000) — run `db.migrate('json')` with the
  native engine first.
- `setRaw`/`getRaw`/`deleteRaw`, `index`, `migrate` and `compactAsync` are
  unavailable (explicit errors).
- `scan()`/`cursor()` are emulated (correct results, `all()`-based cost).
- Async transaction functions are supported.

See the comparison table in [API.md](./API.md#engine-differences).
