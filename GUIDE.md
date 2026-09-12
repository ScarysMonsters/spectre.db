# spectre.db v2 — Usage Guide

## Table of Contents

1. [Installation](#installation)
2. [Quick Start](#quick-start)
3. [Basic Operations](#basic-operations)
4. [Advanced Queries](#advanced-queries)
5. [Lazy Iteration](#lazy-iteration)
6. [Secondary Indexes](#secondary-indexes)
7. [Raw Binary Values](#raw-binary-values)
8. [Transactions](#transactions)
9. [Tables](#tables)
10. [Encryption](#encryption)
11. [Compression](#compression)
12. [Durability](#durability)
13. [Multi-Process](#multi-process)
14. [Migrating from v1.1.0](#migrating-from-v110)
15. [Performance](#performance)
16. [Best Practices](#best-practices)
17. [Complete Examples](#complete-examples)
18. [Troubleshooting](#troubleshooting)

---

## Installation

```bash
npm install spectre-db@latest
```

**Required:** Node.js 18.0.0 or newer (Bun 1.0+ is supported).

Prebuilt binaries for Linux (glibc/musl), macOS (x64/arm64) and Windows (x64)
are installed automatically. If no prebuilt binary matches your platform, the
package falls back to the frozen v1.1.0 JavaScript engine.

---

## Quick Start

```javascript
const { Database } = require('spectre-db');

// Create a database (format: 'auto' → v2 binary for new files)
const db = new Database('./data/mydb', {
  cache: true,
  autoSave: 5000,
  backup: true,
});

// Wait for the database to be ready
await db.ready;

// Write data
db.set('users.1.name', 'Alice');
db.set('users.1.coins', 100);

// Read data
const name = db.get('users.1.name'); // 'Alice'
const coins = db.get('users.1.coins'); // 100

// Close properly
await db.close();
```

---

## Basic Operations

### set(key, value)

Sets a value for a key.

```javascript
db.set('user.name', 'Alice');
db.set('user.age', 30);
db.set('config.debug', true);
db.set('data.items', [1, 2, 3]);
```

`undefined` is rejected with a `TypeError` — v1.1.0 silently lost such values
during compaction.

### get(key)

Retrieves a value by key. Reading a branch returns the nested object.

```javascript
db.set('user.name', 'Alice');
db.set('user.age', 30);

db.get('user.name'); // 'Alice'
db.get('user');      // { name: 'Alice', age: 30 }
db.get('missing');   // null
```

### delete(key)

Deletes a value.

```javascript
db.set('temp.data', 'value');
db.delete('temp.data'); // true
db.get('temp.data');    // null
```

### has(key)

Checks if a key exists.

```javascript
db.set('user.name', 'Alice');
db.has('user.name'); // true
db.has('user.age');  // false
```

### add(key, number)

Adds a number to an existing value (creates it at 0 if missing).

```javascript
db.set('counter', 10);
db.add('counter', 5);  // 15
db.add('counter', -3); // 12
```

### sub(key, number)

Subtracts a number from an existing value.

```javascript
db.set('counter', 10);
db.sub('counter', 5); // 5
```

### push(key, value)

Appends a value to an array.

```javascript
db.set('items', [1, 2, 3]);
db.push('items', 4); // 4 (new length)
db.get('items');     // [1, 2, 3, 4]
```

### pull(key, predicate)

Removes the first matching value from an array.

```javascript
db.set('items', [1, 2, 3, 4]);
db.pull('items', 3); // true
db.get('items');     // [1, 2, 4]

// With a predicate function
db.pull('items', (item) => item > 2);
```

---

## Advanced Queries

### all(prefix?)

Retrieves all entries. Object values are split into leaf-field entries
(v1.1.0 semantics).

```javascript
db.set('user.1.name', 'Alice');
db.set('user.1.age', 30);
db.set('user.2.name', 'Bob');

const all = db.all();
// [
//   { ID: 'user.1.name', data: 'Alice' },
//   { ID: 'user.1.age',  data: 30 },
//   { ID: 'user.2.name', data: 'Bob' }
// ]
```

### startsWith(prefix)

Retrieves entries starting with a prefix.

```javascript
const users = db.startsWith('user.');
```

### filter(predicate)

Filters entries with a function.

```javascript
const richUsers = db.filter((data, id) => {
  return id.endsWith('.coins') && data > 100;
});
```

### find(predicate | object)

Finds a single entry. Passing an object performs a field match and uses a
secondary index when one is defined for the field.

```javascript
const user = db.find((data, id) => id === 'user.1.name' && data === 'Alice');

db.index('user.status');
const active = db.find({ status: 'active' }); // → { ID, data } | null
```

### count(prefix?)

Counts leaf entries without materializing values.

```javascript
db.count('user.'); // 2
```

### paginate(prefix, page, limit, sortBy, sortDesc)

Paginates results.

```javascript
const page1 = db.paginate('user.', 1, 10, 'data', true);
// {
//   data: [...],
//   pagination: { page: 1, limit: 10, total: 25, pages: 3, hasNext: true, hasPrev: false }
// }
```

### range(path, { gte, lt, limit })

Numeric / lexicographic range over an indexed field. The index is created
automatically if missing.

```javascript
db.range('user.age', { gte: 18, lt: 65, limit: 100 }); // → [{ ID, data }]
```

---

## Lazy Iteration

`scan()` streams entries page by page — nothing is materialized in full.
Use it instead of `all()` on large datasets.

```javascript
for await (const { ID, data } of db.scan({ prefix: 'user.', pageSize: 500 })) {
  console.log(ID, data);
}
```

Resume from a cursor:

```javascript
let lastID = null;

for await (const row of db.scan({ prefix: 'user.', after: lastID })) {
  lastID = row.ID;
  // persist lastID if you need to resume later
}
```

Manual paging with `cursor()`:

```javascript
const page = db.cursor('user.', { limit: 100 });
// { rows: [{ ID, data }, ...], cursor: 'user.150', done: false }

const next = db.cursor('user.', { after: page.cursor, limit: 100 });
```

`iterate()` is an alias of `scan()`.

---

## Secondary Indexes

Indexes are sorted B-trees maintained on every write and persisted in the
segment manifest. They add no cost when unused.

```javascript
const status = db.index('user.status');

status.get('active');                 // → values of matching entries
status.keys('active');                // → matching keys
status.range({ gte: 'a', lt: 'm' });  // → keys in range
status.drop();                        // remove the index

db.listIndexes();                     // → ['user.status']
```

---

## Raw Binary Values

Store `Buffer` / `Uint8Array` payloads outside the JSON namespace — perfect
for thumbnails, attachments or encrypted blobs.

```javascript
db.setRaw('thumb:1', imageBuffer);
const buf = db.getRaw('thumb:1'); // Buffer | null
db.deleteRaw('thumb:1');          // true / false
```

Raw values require the native engine; the JavaScript fallback stores JSON only
and throws a clear error.

---

## Transactions

Transactions guarantee atomicity of operations.

### Function-based transaction

```javascript
await db.transaction((tx) => {
  const balance = tx.get('user.balance') ?? 0;
  if (balance < amount) {
    throw new Error('Insufficient balance');
  }
  tx.set('user.balance', balance - amount);
  tx.set('user.lastTransaction', Date.now());
});
```

> [!NOTE]
> With the native engine the function must be synchronous. Async function
> transactions are only supported by the JavaScript fallback engine — use the
> array form when you need to await work.

### Array-based transaction (legacy)

```javascript
await db.transaction([
  { type: 'set',    key: 'config.debug', value: true },
  { type: 'delete', key: 'cache.tmp' },
]);
```

Only `set` and `delete` are supported in the array form on the native engine.
Precompute `add`/`sub`/`push`/`pull` values before the transaction.

### Automatic rollback

```javascript
try {
  await db.transaction((tx) => {
    tx.set('user.balance', 100);
    throw new Error('Simulated error');
  });
} catch (err) {
  // Transaction was rolled back — nothing was written
  db.get('user.balance'); // previous value (or null)
}
```

---

## Tables

Tables create namespaces. Keys are automatically prefixed.

```javascript
const users = db.table('users');
const config = db.table('config');

users.set('1.name', 'Alice'); // stored as "users.1.name"
users.get('1.name');          // 'Alice'
users.has('1.name');          // true
users.delete('1.name');       // true

users.count();                // number of entries
await users.clear();          // removes all users.*

await users.transaction((tx) => {
  tx.add('1.coins', 100);
  tx.set('1.lastDaily', Date.now());
});
```

---

## Encryption

spectre.db automatically encrypts sensitive keys (AES-256-GCM).

### Sensitive keys

Keys starting with these words are automatically encrypted:

- `password`
- `secret`
- `token`
- `apikey`
- `api_key`
- `private`

```javascript
const db = new Database('./data/secure', {
  encryptionKey: 'your-32-byte-encryption-key-here',
});

// These keys are automatically encrypted
db.set('user.password', 'secret123');
db.set('api.token', 'abc123');
db.set('auth.secret', 'xyz789');

// These keys are not encrypted
db.set('user.name', 'Alice');
db.set('config.debug', true);
```

### Backup and snapshot encryption

```javascript
const db = new Database('./data/secure', {
  encryptionKey: 'your-32-byte-encryption-key-here',
  encryptBackups: true,  // encrypt .N.bak files
  encryptSnapshot: true, // encrypt the whole v2 snapshot
  backup: true,
  backupCount: 3,
});

await db.save(); // backups will be encrypted
```

`encryptSnapshot` fails with a clear error when no `encryptionKey` is set —
there is no silent plaintext fallback.

---

## Compression

v2 compresses per segment or per block:

```javascript
const db = new Database('./data/mydb', {
  compression: 'zstd', // 'none' | 'gzip' | 'zstd'
});
```

For v1.1.0 JSON databases, `compress: true` keeps the legacy gzip snapshot
behavior.

---

## Durability

| Mode | Process crash | Power loss |
|---|---|---|
| `durability: 'process'` (default) | no acknowledged write lost | last writes may be lost |
| `durability: 'durable'` | no acknowledged write lost | WAL fsynced on every flush |

```javascript
const db = new Database('./data/mydb', {
  durability: 'durable',
});

// Throughput mode: batch WAL writes into 64 KiB chunks
const fast = new Database('./data/fast', {
  walBuffering: true,
});
```

`syncWal: true` is accepted as a deprecated alias of `durability: 'durable'`
and emits a `warn` event.

---

## Multi-Process

spectre.db supports multi-process access with automatic file locking.

```javascript
// Process 1
const db1 = new Database('./shared.db');
await db1.ready;
db1.set('counter', 1);

// Process 2 (waits for the lock)
const db2 = new Database('./shared.db');
await db2.ready;
```

Lock errors:

```javascript
const db = new Database('./shared.db');

try {
  await db.ready;
} catch (err) {
  if (err.code === 7001) { // LOCK_TIMEOUT
    console.error('Failed to acquire lock');
  }
}
```

Stale locks (dead pid) are taken over automatically.

---

## Migrating from v1.1.0

The default `format: 'auto'` opens existing `.snapshot` / `.wal` databases
without any change. When you want to switch to the binary format:

```javascript
const db = new Database('./data/mydb', { format: 'auto' });
await db.ready;

await db.migrate('v2'); // writes .spdb/.spwal, removes the JSON files
```

Going back:

```javascript
await db.migrate('json'); // writes .snapshot/.wal, removes the binary files
```

The JavaScript fallback engine cannot open v2 databases — migrate to JSON
first if you need to run without the native addon.

---

## Performance

### Optimization for Discord bots

```javascript
const db = new Database('./data/discord', {
  cache: true,
  maxCacheSize: 10000,
  cacheTTL: 60000,
  autoSave: 10000,
  compactThreshold: 1000,
});
```

### Large datasets

```javascript
// Instead of materializing everything:
const all = db.all();

// Stream it:
for await (const { ID, data } of db.scan({ prefix: 'user.' })) { ... }

// Or page it:
const page1 = db.paginate('user.', 1, 100);
```

### Non-blocking compaction

```javascript
const status = await db.compactAsync(); // 'committed' | 'stale' | 'legacy'
```

Reads and writes continue while serialization runs on a worker thread.

---

## Best Practices

### 1. Always close the database

```javascript
process.on('SIGINT', async () => {
  await db.close();
  process.exit(0);
});
```

### 2. Use transactions for related writes

```javascript
// ❌ Not atomic
db.set('user.balance', balance - amount);
db.set('user.lastTransaction', Date.now());

// ✅ Atomic
await db.transaction((tx) => {
  tx.set('user.balance', balance - amount);
  tx.set('user.lastTransaction', Date.now());
});
```

### 3. Handle warnings and events

```javascript
db.on('warn', (message) => console.warn('[spectre-db]', message));

db.on('change', ({ type, key }) => {
  console.log(type, key);
});
```

### 4. Warm the cache with frequent keys

```javascript
const db = new Database('./data/db', {
  cache: true,
  warmKeys: ['config.settings', 'user.1.profile'],
});
```

### 5. Use tables for organization

```javascript
const users = db.table('users');
const config = db.table('config');
const cache = db.table('cache');
```

---

## Complete Examples

### Discord bot with economy

```javascript
const { Client, GatewayIntentBits } = require('discord.js');
const { Database } = require('spectre-db');

const client = new Client({
  intents: [GatewayIntentBits.Guilds, GatewayIntentBits.GuildMessages],
});

const db = new Database('./data/discord', {
  cache: true,
  autoSave: 10000,
  backup: true,
  encryptionKey: process.env.DB_KEY,
});

client.on('messageCreate', async (message) => {
  if (message.content === '!daily') {
    const userId = message.author.id;
    const lastKey = `users.${userId}.lastDaily`;
    const last = db.get(lastKey) ?? 0;
    const now = Date.now();
    const cooldown = 24 * 60 * 60 * 1000;

    if (now - last < cooldown) {
      const remaining = Math.ceil((cooldown - (now - last)) / 3600000);
      return message.reply(`Come back in ${remaining}h for your daily reward!`);
    }

    await db.transaction((tx) => {
      const coins = (tx.get(`users.${userId}.coins`) ?? 0) + 100;
      tx.set(`users.${userId}.coins`, coins);
      tx.set(lastKey, now);
    });

    message.reply('You received 100 coins!');
  }
});

process.on('SIGINT', async () => {
  await db.close();
  process.exit(0);
});

client.login(process.env.TOKEN);
```

### Backend application with cache

```javascript
const express = require('express');
const { Database } = require('spectre-db');

const app = express();
const db = new Database('./data/backend', {
  cache: true,
  maxCacheSize: 10000,
  cacheTTL: 300000,
});

app.get('/api/users/:id', async (req, res) => {
  const user = db.get(`users.${req.params.id}`);

  if (!user) {
    return res.status(404).json({ error: 'User not found' });
  }

  res.json(user);
});

app.post('/api/users/:id', async (req, res) => {
  const userId = req.params.id;

  await db.transaction((tx) => {
    tx.set(`users.${userId}`, req.body);
    tx.set(`users.${userId}.updatedAt`, Date.now());
  });

  res.json({ success: true });
});

process.on('SIGINT', async () => {
  await db.close();
  process.exit(0);
});

app.listen(3000);
```

---

## Troubleshooting

### Error: "Lock acquisition timeout"

The file lock could not be acquired. Check that no other process is using the
database, or raise `lockTimeout`.

### Error: "Value too large"

The value exceeds the maximum allowed size. Split it into multiple keys or use
`setRaw()` for binary payloads.

### Error: "Circular reference detected"

You are trying to store an object with circular references. Use a data
structure without cycles.

### The fallback engine refuses to open my database

The JavaScript fallback cannot read the v2 binary format. Open the database
with the native engine and run `await db.migrate('json')`.

### Slow performance

- Increase `maxCacheSize` and use `warmKeys`.
- Use `scan()` / `paginate()` instead of `all()` on large datasets.
- Enable `compression: 'zstd'` for disk-bound workloads.
- Use `compactAsync()` to avoid blocking the event loop.

---

## Support

- [GitHub Issues](https://github.com/ScarysMonsters/spectre.db/issues)
- [Main documentation](./README.md)
- [On-disk formats](./docs/FORMATS.md)
