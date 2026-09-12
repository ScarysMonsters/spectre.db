# Troubleshooting

## Quick diagnostics

```js
const { Database, hasNativeEngine, version } = require('@sexfy/spectre.db');

console.log('package:', version);
console.log('native engine:', hasNativeEngine);
```

```js
const db = new Database('./data/mydb');
await db.ready;
console.log(db.stats()); // format, engine, durability, recoveryCount, …
```

Useful commands from the repository:

```sh
npm run smoke            # end-to-end sanity check
npm run check-versions   # verify all package versions match
npm test                 # full integration suite
```

When reporting a bug, include: `hasNativeEngine`, `version`, `db.stats()`, the
OS/arch, the Node.js version and the exact error with its `code`.

---

## Engine loading

### `hasNativeEngine` is `false`

The Rust addon could not be loaded. Causes:

- No prebuilt binary matches your platform (only Linux x64 gnu/musl, macOS
  x64/arm64 and Windows x64 MSVC are published).
- `npm install` skipped optional dependencies (`--no-optional`,
  `--omit=optional`, or a lockfile without them).
- The `SPECTRE_FORCE_FALLBACK=1` environment variable is set.
- A local build is missing: run `npm run build`.

The fallback engine keeps JSON databases working; v2 databases are refused (see
below).

### `SNAPSHOT_CORRUPTED` (3000) — "This database uses the spectre.db v2 binary format"

You are running the JavaScript fallback against a v2 database. Either:

- install the native prebuild (`npm install` without `--no-optional`), or
- convert the database back to JSON with the native engine:
  `await db.migrate('json')`.

---

## Key and value errors

| Code | Meaning | Fix |
|---|---|---|
| 1000 | Invalid key | Use a non-empty string |
| 1001 | Key too long | Keys are limited to 1000 characters |
| 1002 | Empty key segment | Remove `..` from the key |
| 1003 | Forbidden segment | `__proto__`, `constructor` and `prototype` are rejected by design |
| 1004 | Control characters | Strip `\x00`–`\x1F` and `\x7F` |
| 1005 | Invisible characters | Strip zero-width/BOM/LS/PS characters |
| 1101 | Value too large | Split the value (10 MB max) or use `setRaw()` |
| 1102 | Circular reference | Remove cycles from the object |
| 1103 | Unsupported type | `BigInt` is not JSON-serializable — convert to string/number |
| — | `set(key, undefined)` throws | Use `delete(key)` or a JSON value |

### `Value at "x" is not a number` / `is not an array`

`add`/`sub` require a number (missing keys start at 0), `push`/`pull` require an
array. Type mismatches throw a `TypeError`.

---

## Path errors

The fallback engine refuses paths outside the current working directory
(`PATH_TRAVERSAL`, code 2000) — a v1.1.0 security behavior. Use a path under
the project directory, or run with the native engine (which supports absolute
paths).

---

## Locking

### `LOCK_TIMEOUT` (7001)

Another process holds the lock (or a previous run left a stale lock with a pid
that still exists). Options:

- Ensure the other process calls `close()` (or exits).
- Increase `lockTimeout` (ms):
  ```js
  const db = new Database('./data/mydb', { lockTimeout: 60000 });
  ```
- Stale locks from dead processes are taken over automatically; if you see
  repeated timeouts, check that the pid in `<base>.lock` is really dead.

### Can I open the same database twice in one process?

No. A `Database` instance holds an exclusive lock; a second instance on the same
path in the same process will time out. Share one instance instead.

---

## Durability and recovery

### `recoveryCount` is greater than 0

The database was recovered from the WAL and/or a backup on open — usually
because the process exited without `close()`. This is expected after a crash;
check for `warn`/`restore`/`reset` events if you want details.

### A `restore` event fired

The snapshot was corrupted and the engine restored the newest valid backup.
Recent writes not yet compacted may have been replayed from the WAL.

### A `reset` event fired

No usable snapshot or backup was found; the database was reset to empty. Check
your backups and disk health.

### Writes seem lost after a power cut

That is the `durability: 'process'` contract (no fsync by default). Switch to
`durability: 'durable'` for power-loss safety.

### `DECRYPTION_FAILED` (4001)

The `encryptionKey` (or the scrypt parameters) differs from the one used to
write the data, or the ciphertext is corrupted. There is no recovery without
the original key.

---

## State errors

| Code | Meaning | Fix |
|---|---|---|
| 8000 | `DATABASE_CLOSED` | You called a method after `close()` |
| 8001 | `DATABASE_NOT_READY` | `await db.ready` before using the instance |

---

## Performance / event loop

### The event loop stalls on large databases

- Use `scan()` / `cursor()` instead of `all()`.
- Use `compactAsync()` instead of `compact()`/`save()`.
- Increase `maxCacheSize` / use `warmKeys` (fallback engine).
- Enable `compression: 'zstd'` for disk-bound workloads.

### Many small writes are slow

- Group them in a transaction (`db.transaction([...])` with `set` operations
  uses the fast `setManyJson` path).
- Consider `walBuffering: true` (widens the crash window).
- Increase `compactThreshold` to reduce compaction frequency.

---

## Fallback engine specifics

- `setRaw`/`getRaw`/`deleteRaw`, `index`, `migrate`, `compactAsync` throw
  "requires the native engine".
- `scan()`/`cursor()` are emulated: correct but not truly lazy.
- Async transaction functions work on the fallback, not on the native engine
  (which requires sync functions).
- `stats()` returns neutral values for v2-only fields.

---

## FAQ

**Is it safe to read the files with another tool?**
JSON-mode databases (`.snapshot`/`.wal`) are documented in
[FORMATS.md](./FORMATS.md). v2 binary files are also fully specified there, but
do not edit them by hand.

**Can several processes write?**
Only one at a time — the lock serializes writers. Reads happen from each
process's own snapshot+WAL view.

**How do I back up a database?**
Stop the process (or call `close()`), then copy the files. With `backup: true`,
rotating backups are kept automatically on every compaction.

**How do I force the fallback engine?**
`SPECTRE_FORCE_FALLBACK=1`.

**Where are the files?**
`db.stats().snapshotPath` and `db.stats().walPath` show the exact paths.
