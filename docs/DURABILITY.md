# Durability & Crash Recovery

This document explains the durability contract, how the WAL and snapshots
interact, what happens after a crash, and how the fault-injection tests verify
those guarantees.

## Durability contract

| Mode | Process crash | Power loss |
|---|---|---|
| `durability: 'process'` (default) | ✔ no acknowledged write lost | last writes may be lost (page cache) |
| `durability: 'durable'` | ✔ no acknowledged write lost | ✔ WAL fsynced on every flush |

```js
const db = new Database('./data/mydb', {
  durability: 'durable',
});
```

- `durability: 'process'` is the v1.1.0 behavior: every write is flushed to the
  OS (safe against process death), but not fsynced.
- `durability: 'durable'` fsyncs the WAL after every flush. Slower, but a power
  cut cannot lose acknowledged writes.
- `walBuffering: true` batches WAL writes into 64 KiB chunks for higher
  throughput, widening the crash window. It is orthogonal to the durability
  mode.
- `syncWal: true` is accepted as a deprecated alias of `durability: 'durable'`
  and emits a `warn` event.

## Write path

```
db.set('x', 1)
  └─ in-memory store updated synchronously
  └─ WAL record appended (one write() per op, or per buffered flush)
       └─ durable mode: fsync after each flush
```

The WAL is append-only. Snapshots are only written during compaction:

```
compact()
  └─ rotate backups
  └─ serialize → temp file (fsync) → atomic rename
  └─ truncate/reset WAL
```

Atomic rename guarantees:

- POSIX: `rename(2)` is atomic.
- Windows: `MoveFileEx` with `REPLACE_EXISTING`.

## Startup recovery

```
new Database(path)
  └─ acquire lock
  └─ load snapshot (v2 binary or v1.1.0 JSON)
  └─ replay WAL on top
  └─ stop at the first frame with a bad/truncated CRC (warn)
  └─ ready resolves
```

- A torn WAL tail is truncated: earlier frames remain valid.
- `stats().recoveryCount` counts how many recoveries happened.
- If the snapshot is corrupted, the newest valid backup is restored and the
  engine emits `warn` + `restore` (see [FORMATS.md §6](./FORMATS.md)).
- If no snapshot or backup is usable, the database is reset and a `reset` event
  is emitted.

`close()` compacts *before* releasing resources, so a clean shutdown always
leaves a fresh snapshot and an empty WAL.

## Backups

Every compaction rotates backups first:

```
mydb.spdb → mydb.spdb.1.bak → mydb.spdb.2.bak → ...
```

- `backupCount` (default `3`) controls how many generations are kept;
  `backup: false` / `backupCount: 0` disables rotation.
- `encryptBackups: true` encrypts backup files (see [ENCRYPTION.md](./ENCRYPTION.md)).
- On startup, a corrupted snapshot triggers an automatic restore from the
  newest valid backup.

## Fault injection

The native engine has instrumented crash points for testing recovery. Set
`SPECTRE_CRASH_AT` to abort the process at a specific point:

```sh
SPECTRE_CRASH_AT=wal_after_write:2 node your-app.js
```

Format: `point[:skip]` — `skip` is the number of occurrences to let pass before
aborting (so `:2` aborts on the third occurrence).

| Point | Where |
|---|---|
| `wal_after_write` | After a WAL record is written |
| `wal_after_sync` | After an fsync in `durable` mode |
| `snap_tmp_written` | After the snapshot temp file is written |
| `snap_before_rename` | Just before the snapshot rename |
| `snap_after_rename` | Just after the snapshot rename |
| `seg_written` | After an incremental segment is written |
| `man_before_rename` | Just before the manifest rename |
| `man_after_rename` | Just after the manifest rename |

Run the suite:

```sh
npm run test:fault
```

The tests kill a worker at each point, reopen the database and verify that all
acknowledged writes are recovered and that the segmented layout stays coherent.

> On Windows, the snapshot/segment points are not observable from the JS test
> harness (the worker exits normally), so those cases are skipped there; the WAL
> points run on every platform.

## Operational recommendations

- Always `await db.close()` on `SIGINT`/`SIGTERM` — it compacts and releases the
  lock.
- Use `durability: 'durable'` when losing the last writes on power loss is not
  acceptable (payments, counters, audit logs).
- Keep `backup: true` (default) and, for sensitive data, `encryptBackups: true`.
- On large databases, prefer `compactAsync()` to avoid stalling the event loop.
- Monitor `stats().recoveryCount`, `pendingWrites`, `lastLsn` and `walBytes` in
  production if you want early warning of abnormal recovery patterns.
