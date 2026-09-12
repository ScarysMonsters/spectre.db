# API Reference

Complete reference for the `@sexfy/spectre.db` JavaScript API.

```js
const { Database, Table, hasNativeEngine, version } = require('@sexfy/spectre.db');
```

| Export | Type | Description |
|---|---|---|
| `Database` | class | The database handle |
| `Table` | class | Namespaced view (created via `db.table(name)`) |
| `hasNativeEngine` | boolean | `true` when the Rust addon is loaded |
| `version` | string | Package version (`'2.0.0'`) |

TypeScript definitions are shipped in `index.d.ts`.

---

## Constructor

```js
new Database(filePath = './database.json', options = {})
```

`filePath` is the database base path. Known extensions (`.json`, `.json.gz`,
`.gz`, `.db`, `.snapshot`) are stripped to derive the on-disk base name, e.g.
`./data/mydb.json` → `./data/mydb`.

### Constructor options

| Option | Type | Default | Description |
|---|---|---|---|
| `cache` | boolean | `true` | Enable the LRU cache (`false` disables it) |
| `maxCacheSize` | number | `1000` | Max cached entries (JS fallback engine) |
| `cacheTTL` | number | `0` | Cache TTL in ms, `0` = forever (JS fallback engine) |
| `autoSave` | number | `5000` | Compaction interval in ms (`0` disables periodic compaction) |
| `compactThreshold` | number | `500` | WAL ops before a compaction is triggered |
| `compactInterval` | number | `300000` | Interval between compaction checks (ms) |
| `backup` | boolean | `true` | Enable backup rotation |
| `backupCount` | number | `3` | Number of backup generations (`0` disables) |
| `compress` | boolean | `false` | Gzip snapshot compression (legacy / JSON mode) |
| `compression` | `'none' \| 'gzip' \| 'zstd'` | `'none'` | Compression per segment/block (v2 format) |
| `encryptionKey` | string \| Buffer | `null` | Key for sensitive values and encrypted backups |
| `encryptBackups` | boolean | `false` | Encrypt rotating backup files |
| `encryptSnapshot` | boolean | `false` | Encrypt the whole v2 snapshot |
| `scryptLogN` | number | `14` | scrypt KDF log2(N) |
| `scryptR` | number | `8` | scrypt r parameter |
| `scryptP` | number | `1` | scrypt p parameter |
| `format` | `'auto' \| 'v2' \| 'json'` | `'auto'` | Storage format |
| `durability` | `'process' \| 'durable'` | `'process'` | WAL flush policy |
| `syncWal` | boolean | `false` | Deprecated alias of `durability: 'durable'` (emits `warn`) |
| `walBuffering` | boolean | `false` | Batch WAL writes into 64 KiB chunks (throughput mode) |
| `compactMode` | `'auto' \| 'legacy' \| 'segments'` | `'auto'` | Compaction strategy |
| `legacyThreshold` | number | `100000` | Entry count under which full rewrites are used (`auto` mode) |
| `segmentMergeEvery` | number | `8` | Segments before a full merge |
| `lockTimeout` | number | `30000` | Lock acquisition timeout (ms) |
| `warmKeys` | string[] | `[]` | Keys to pre-load into the cache on startup |

Notes:

- `cache: false` sets `maxCacheSize` to `0`; `backup: false` sets `backupCount` to `0`.
- `autoSave > 0` sets `compactInterval = autoSave` and `compactThreshold = 50`.
- `maxCacheSize` / `cacheTTL` are honored by the JavaScript fallback engine; the
  native engine manages its own cache internally.
- Passing `syncWal: true` is equivalent to `durability: 'durable'` and emits a
  `warn` event.

### Properties

| Property | Type | Description |
|---|---|---|
| `db.ready` | `Promise<Database>` | Resolves when the snapshot/WAL are loaded and the lock is held |

```js
const db = new Database('./data/mydb');
await db.ready;
```

---

## Core methods

### `get(key)`

Returns the value at `key`, or `null` if missing. Reading a branch returns the
nested object.

```js
db.set('user.name', 'Alice');
db.set('user.age', 30);

db.get('user.name'); // 'Alice'
db.get('user');      // { name: 'Alice', age: 30 }
db.get('missing');   // null
```

### `set(key, value)`

Writes a value and returns it. Updates the in-memory store immediately; the WAL
append is queued.

```js
db.set('config.debug', true);
db.set('user.tags', ['a', 'b']);
```

`set(key, undefined)` throws a `TypeError` (v1.1.0 silently dropped such values
at compaction).

### `has(key)`

Returns `true` if the key resolves to a value.

### `delete(key)`

Deletes a key and returns `true` when something was removed.

```js
db.delete('temp.data'); // true
db.delete('temp.data'); // false
```

### `add(key, n)` / `sub(key, n)`

Adds/subtracts a finite number. Missing keys start at `0`; non-number values
throw a `TypeError`.

```js
db.set('coins', 10);
db.add('coins', 5);  // 15
db.sub('coins', 3);  // 12
```

### `push(key, value)` / `pull(key, predicate)`

Array helpers. `push` returns the new length; `pull` removes the first matching
element (value or predicate) and returns a boolean.

```js
db.set('roles', ['member']);
db.push('roles', 'admin');            // 2
db.pull('roles', 'member');           // true
db.pull('roles', (r) => r === 'admin')// true
```

---

## Query methods

### `all(prefix?)`

Returns every leaf entry as `{ ID, data }`. Object values are split into
field-level entries.

```js
db.all();            // all entries
db.all('user.');     // entries with ID starting with 'user.'
```

### `startsWith(prefix)`

Alias of `all(prefix)`.

### `filter(predicate)`

Filters entries with `(data, ID) => boolean`.

```js
db.filter((data, id) => id.endsWith('.coins') && data > 100);
```

### `find(predicateOrObject)`

With a function: returns the first matching `{ ID, data }` or `null`.
With an object: performs a field match (`{ status: 'active' }`) and uses a
secondary index when one exists for the field.

```js
db.find((data, id) => id === 'user.1.name');
db.find({ status: 'active' });
```

### `count(prefix?)`

Number of leaf entries, without materializing values (native engine).

### `paginate(prefix, page = 1, limit = 10, sortBy = 'data', sortDesc = true)`

```js
const page = db.paginate('user.', 1, 10, 'data', true);
// {
//   data: [{ ID, data }, ...],
//   pagination: { page, limit, total, pages, hasNext, hasPrev }
// }
```

### `range(path, { gte, lt, limit })`

Range over a (possibly indexed) field. On the native engine, the index is
created automatically if missing.

```js
db.range('user.age', { gte: 18, lt: 65, limit: 100 }); // [{ ID, data }]
```

---

## Lazy iteration

### `scan({ prefix?, after?, pageSize?, limit? })`

Returns an async iterator that fetches entries page by page — nothing is fully
materialized. `iterate()` is an alias.

```js
for await (const { ID, data } of db.scan({ prefix: 'user.', pageSize: 500 })) {
  // ...
}
```

Resume with a cursor:

```js
for await (const row of db.scan({ prefix: 'user.', after: lastID })) { ... }
```

### `cursor(prefix?, { after?, limit? })`

Manual paging.

```js
const page = db.cursor('user.', { limit: 100 });
// { rows: [{ ID, data }, ...], cursor: 'user.150', done: false }
const next = db.cursor('user.', { after: page.cursor, limit: 100 });
```

---

## Raw binary values

Native engine only. Raw values live in a reserved internal namespace
(prefix `0x00`) and are invisible to the JSON API.

```js
db.setRaw('thumb:1', imageBuffer);   // Buffer / Uint8Array
db.getRaw('thumb:1');                // Buffer | null
db.deleteRaw('thumb:1');             // true | false
```

The JavaScript fallback throws `setRaw() requires the native engine` for these
methods.

---

## Secondary indexes

Native engine only. Indexes are sorted B-trees maintained on every write and
persisted in the segment manifest.

### `db.index(path)`

Creates (or returns) an index and returns a handle:

```js
const status = db.index('user.status');

status.get('active');                 // values of matching entries
status.keys('active');                // matching keys
status.range({ gte: 'a', lt: 'm' });  // keys in range
status.drop();                        // remove the index
```

### `db.listIndexes()`

Returns the list of indexed field paths, e.g. `['user.status']`.

---

## Transactions

### Function form

```js
await db.transaction((tx) => {
  const balance = tx.get('user.balance') ?? 0;
  if (balance < amount) throw new Error('Insufficient balance');
  tx.set('user.balance', balance - amount);
  tx.set('user.lastTransaction', Date.now());
});
```

The transaction context exposes `get`, `set`, `delete`, `add`, `sub`, `push`
and `pull`. The function return value is passed through. Throwing rolls back
everything staged.

> **Native engine:** the function must be synchronous. Returning a promise
> rejects with a `TypeError` — use the array form for async work.
> **Fallback engine:** async functions are supported.

### Array form (legacy)

```js
await db.transaction([
  { type: 'set',    key: 'config.debug', value: true },
  { type: 'delete', key: 'cache.tmp' },
]);
```

- Native engine: only `set` and `delete` are accepted; `add`/`sub`/`push`/`pull`
  throw (precompute the value and use `set`). Returns an array of results
  (`set` → value, `delete` → boolean).
- Fallback engine: `set`, `delete`, `add`, `sub`, `push`, `pull`.

All operations in a transaction are committed as a single WAL batch record
(atomic on replay).

---

## Tables

```js
const users = db.table('users');

users.set('1.name', 'Alice');   // stored as "users.1.name"
users.get('1.name');            // 'Alice'
users.has('1.name');            // true
users.delete('1.name');         // true
users.add('1.coins', 100);
users.push('1.roles', 'admin');
users.pull('1.roles', 'member');
users.all();                    // entries with ID starting with 'users.'
users.count();                  // number of entries
await users.clear();            // removes all users.*

await users.transaction((tx) => {
  tx.add('1.coins', 100);
  tx.set('1.lastDaily', Date.now());
});
```

---

## Persistence & lifecycle

### `compact()` / `save()`

Force a compaction now (rotates backups, writes a snapshot, truncates the WAL).
`save()` is an alias of `compact()`.

```js
await db.save();
```

### `compactAsync()`

Native engine only. Runs serialization on a worker thread; reads/writes
continue during the operation. Resolves to `'committed'`, `'stale'` or
`'legacy'`.

```js
const status = await db.compactAsync();
```

### `clear()`

Removes all entries (the operation itself is WAL-logged).

### `close()`

Flushes pending writes, compacts, releases the lock and removes listeners.
Idempotent. Always call it on shutdown.

### `migrate(format)`

Native engine only. Converts the database between formats and removes the old
files.

```js
await db.migrate('v2');   // JSON (.snapshot/.wal) → binary (.spdb/.spwal)
await db.migrate('json'); // binary → JSON
```

### `table(name)`

Creates a `Table` view (see above).

---

## Statistics

### `getStats()`

v1.1.0-shaped stats.

| Field | Description |
|---|---|
| `driver` | Always `'spectre.db'` |
| `engine` | Engine string, e.g. `'spectre.db/2.0.0 (rust)'` |
| `format` | `'v2'` or `'json'` |
| `compress` / `compression` | Compression flags |
| `encrypted` / `snapshotEncrypted` | Encryption flags |
| `entries` | Number of stored leaf entries |
| `cacheSize` / `maxCacheSize` | Cache information |
| `fileSize` / `storeBytes` | Snapshot size |
| `walOps` / `walBytes` | Pending WAL operations/bytes |
| `compactThreshold` | Configured compaction threshold |
| `snapshotPath` / `walPath` | On-disk paths |

### `stats()`

Full observability surface (native engine):

| Field | Description |
|---|---|
| `durability` | `'process'` or `'durable'` |
| `rawEntries` | Number of raw (binary) entries |
| `pendingWrites` | Writes not yet flushed |
| `cacheHits` / `cacheMisses` | Cache counters |
| `compressionRatio` | Snapshot compression ratio |
| `segmentCount` | Number of live segments |
| `generation` | Manifest generation |
| `lastCompactionMs` | Timestamp of the last compaction |
| `lastLsn` | Last WAL log sequence number |
| `recoveryCount` | Number of recoveries on open |
| `indexCount` | Number of secondary indexes |

The fallback engine returns the same shape with neutral values.

---

## Events

| Event | Payload | When |
|---|---|---|
| `change` | `{ type, key, value? }` | A value changed (`set`, `delete`, `setRaw`, `deleteRaw`) |
| `clear` | — | The database was cleared |
| `save` | stats | A compaction completed |
| `transaction` | `{ ops }` | A transaction committed |
| `warn` | string \| Error | Non-fatal warning (corruption recovery, deprecations, …) |
| `restore` | — | The snapshot was restored from a backup |
| `reset` | — | No usable snapshot/backup: the database was reset |

```js
db.on('change', ({ type, key }) => {});
db.once('save', () => {});
db.off('warn', handler);
```

On the native engine, initialization events (`warn`, `restore`, `reset`) are
drained at construction time and re-emitted synchronously.

---

## Validation rules

### Keys

| Rule | Error |
|---|---|
| Non-empty string | `INVALID_KEY` (1000) |
| Max 1000 characters | `KEY_TOO_LONG` (1001) |
| No empty segment (`a..b`) | `KEY_EMPTY_SEGMENT` (1002) |
| No `__proto__`, `constructor`, `prototype` segment | `KEY_FORBIDDEN_SEGMENT` (1003) |
| No control characters (`\x00`–`\x1F`, `\x7F`) | `KEY_CONTROL_CHARS` (1004) |
| No invisible characters (zero-width, BOM, LS/PS) | `KEY_INVISIBLE_CHARS` (1005) |

Keys are normalized to Unicode NFC before storage.

### Values

| Rule | Error |
|---|---|
| Serialized size ≤ 10 MB | `VALUE_TOO_LARGE` (1101) |
| No circular references | `CIRCULAR_REFERENCE` (1102) |
| No `BigInt` | `UNSUPPORTED_TYPE` (1103) |
| No `undefined` | `TypeError` |

### Sensitive keys

Keys **starting with** `password`, `secret`, `token`, `apikey`, `api_key` or
`private` (followed by `.`, `_` or end of key) are automatically encrypted with
AES-256-GCM when `encryptionKey` is set. See [ENCRYPTION.md](./ENCRYPTION.md).

```js
db.set('password', '...');       // encrypted
db.set('user.password', '...');  // not auto-encrypted (does not start with the word)
```

---

## Error codes

Errors thrown by the native engine carry a numeric `code` property.

### Validation (1000–1199)

| Code | Name | Description |
|---|---|---|
| 1000 | `INVALID_KEY` | Key is not a non-empty string |
| 1001 | `KEY_TOO_LONG` | Key exceeds 1000 characters |
| 1002 | `KEY_EMPTY_SEGMENT` | Key contains an empty segment |
| 1003 | `KEY_FORBIDDEN_SEGMENT` | Prototype-pollution guard |
| 1004 | `KEY_CONTROL_CHARS` | Control characters in key |
| 1005 | `KEY_INVISIBLE_CHARS` | Invisible characters in key |
| 1100 | `INVALID_VALUE` | Invalid value (fallback engine) |
| 1101 | `VALUE_TOO_LARGE` | Value exceeds 10 MB |
| 1102 | `CIRCULAR_REFERENCE` | Circular reference detected |
| 1103 | `UNSUPPORTED_TYPE` | Unsupported type (e.g. `BigInt`) |

### Path (2000–2099)

| Code | Name | Description |
|---|---|---|
| 2000 | `PATH_TRAVERSAL` | Path escapes the allowed base directory |
| 2001 | `INVALID_PATH` | Invalid path |
| 2002 | `SYMLINK_DETECTED` | Symlink detected (fallback) |
| 2003 | `DIRECTORY_NOT_FOUND` | Directory not found (fallback) |

### Storage (3000–3099)

| Code | Name | Description |
|---|---|---|
| 3000 | `SNAPSHOT_CORRUPTED` | Corrupted snapshot / v2 refused by fallback |
| 3001 | `WAL_CORRUPTED` | Corrupted WAL |
| 3002 | `BACKUP_CORRUPTED` | Corrupted backup |
| 3003 | `WRITE_FAILED` | Write failure |
| 3004 | `READ_FAILED` | Read failure |
| 3005 | `FILE_LOCKED` | File locked |

### Encryption (4000–4099)

| Code | Name | Description |
|---|---|---|
| 4000 | `ENCRYPTION_FAILED` | Encryption failure |
| 4001 | `DECRYPTION_FAILED` | Wrong/missing key or corrupted ciphertext |
| 4002 | `INVALID_KEY` (encryption) | Invalid encryption key |
| 4003 | `KEY_DERIVATION_FAILED` | scrypt failure |

### Transactions (5000–5099)

| Code | Name | Description |
|---|---|---|
| 5000–5002 | `TRANSACTION_*` | Transaction state errors (fallback) |
| 5003 | `TRANSACTION_COMMIT_FAILED` | Commit failure |

### Cache / Lock / State / Operation

| Code | Name | Description |
|---|---|---|
| 6000–6001 | `CACHE_*` | Cache errors (fallback) |
| 7000 | `LOCK_ACQUISITION_FAILED` | Could not acquire the file lock |
| 7001 | `LOCK_TIMEOUT` | Lock acquisition timeout (`lockTimeout`) |
| 7002 | `LOCK_RELEASE_FAILED` | Lock release failure (fallback) |
| 8000 | `DATABASE_CLOSED` | Operation on a closed database |
| 8001 | `DATABASE_NOT_READY` | Operation before `ready` |
| 9000 | `OPERATION_FAILED` | Generic operation failure |
| 9002 | `UNKNOWN_OPERATION` | Unknown operation type |

```js
try {
  db.set('__proto__.x', 1);
} catch (err) {
  console.error(err.code);    // 1003
  console.error(err.message);
}
```

---

## Engine differences

| Feature | Native (Rust) | Fallback (v1.1.0 JS) |
|---|---|---|
| v2 binary format | ✅ | ❌ (clear error) |
| JSON v1.1.0 format | ✅ | ✅ |
| `scan()` / `cursor()` | ✅ (true lazy) | ✅ (emulated, async) |
| Secondary indexes / `find({...})` / `range()` | ✅ | `range()` emulated, indexes unavailable |
| `setRaw` / `getRaw` / `deleteRaw` | ✅ | ❌ |
| `compactAsync()` | ✅ | `compact()` only |
| `migrate()` | ✅ | ❌ |
| Async transaction function | ❌ (sync only) | ✅ |
| Array transaction `add`/`sub`/`push`/`pull` | ❌ (`set`/`delete` only) | ✅ |
| Full `stats()` fields | ✅ | neutral values |

Check the active engine with `hasNativeEngine` and branch when needed.
