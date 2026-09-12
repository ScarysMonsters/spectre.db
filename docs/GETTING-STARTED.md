# Getting Started

## Installation

```sh
npm install @sexfy/spectre.db
```

Requirements:

- Node.js **>= 18.0.0** (Bun **>= 1.0.0** is supported)
- No build step: native prebuilds are installed automatically as optional
  dependencies for Linux x64 (glibc/musl), macOS x64/arm64 and Windows x64.
- If no prebuild matches the platform, the package falls back to the frozen
  v1.1.0 JavaScript engine.

Check which engine is active:

```js
const { hasNativeEngine, version } = require('@sexfy/spectre.db');

console.log(version);          // '2.0.0'
console.log(hasNativeEngine);  // true when the Rust addon is loaded
```

## Your first database

```js
const { Database } = require('@sexfy/spectre.db');

const db = new Database('./data/mydb', {
  cache: true,
  autoSave: 5000,
  backup: true,
});

await db.ready;

db.set('users.1.name', 'Alice');
db.set('users.1.coins', 100);
db.set('users.1.roles', ['member']);

console.log(db.get('users.1.name'));   // 'Alice'
console.log(db.get('users.1'));        // { name: 'Alice', coins: 100, roles: ['member'] }
console.log(db.has('users.1.coins'));  // true

db.add('users.1.coins', 50);           // 150
db.push('users.1.roles', 'admin');     // ['member', 'admin']
db.pull('users.1.roles', 'member');    // ['admin']

await db.close();
```

Always call `await db.close()` on shutdown (or on `SIGINT`/`SIGTERM`): it flushes
the WAL, compacts and releases the lock.

```js
process.on('SIGINT', async () => {
  await db.close();
  process.exit(0);
});
```

## Core concepts

### Keys are dot paths

Keys use dots to build a nested tree. `db.set('users.1.name', 'Alice')` and
`db.set('users.1.coins', 100)` produce:

```js
db.get('users.1'); // { name: 'Alice', coins: 100 }
```

Object values are flattened into leaf entries when you enumerate
(`all()`, `startsWith()`, `scan()`), matching v1.1.0 semantics:

```js
db.all();
// [
//   { ID: 'users.1.name',  data: 'Alice' },
//   { ID: 'users.1.coins', data: 100 }
// ]
```

### Values are JSON

Any JSON-serializable value is accepted: strings, numbers, booleans, `null`,
arrays and plain objects. Limits and rules:

- Serialized value size: **10 MB max** (`VALUE_TOO_LARGE`, code 1101)
- `undefined` is rejected (`TypeError`) — v1.1.0 silently dropped such values
- Circular references are rejected (code 1102), `BigInt` is rejected (code 1103)
- Keys: 1000 chars max, no empty segment, no control/invisible characters,
  no `__proto__` / `constructor` / `prototype` segments (prototype pollution guard)

### Tables

Tables are namespaced views over the same database:

```js
const users = db.table('users');

users.set('1.name', 'Alice');  // stored as "users.1.name"
users.get('1.name');           // 'Alice'
users.count();                 // number of entries
await users.clear();           // removes all users.*
```

### Events

```js
db.on('change', ({ type, key }) => console.log(type, key));
db.on('save', (stats) => console.log('compacted', stats.fileSize));
db.on('warn', (message) => console.warn('[spectre.db]', message));
```

See [API.md](./API.md#events) for the full event list.

## Files on disk

With the default `format: 'auto'`, new databases use the v2 binary format:

```
data/
├── mydb.spdb             ← binary snapshot (CRC32)
├── mydb.spwal            ← binary WAL (v3 frames, LSN)
├── mydb.spman            ← segment manifest (segmented mode)
├── mydb.spseg-1          ← immutable segments (segmented mode)
├── mydb.lock             ← file lock
├── mydb.spdb.1.bak       ← backup rotation
└── mydb.spdb.2.bak
```

Existing v1.1.0 databases (`.snapshot` / `.wal`) are detected and opened as-is.
Full details: [FORMATS.md](./FORMATS.md).

## Common options

```js
const db = new Database('./data/mydb', {
  // Caching
  cache: true,
  maxCacheSize: 1000,
  cacheTTL: 0,              // ms, 0 = forever
  warmKeys: ['config.settings'],

  // Persistence
  autoSave: 5000,           // compaction interval (ms)
  backup: true,
  backupCount: 3,

  // Storage
  format: 'auto',           // 'auto' | 'v2' | 'json'
  compression: 'zstd',      // 'none' | 'gzip' | 'zstd'
  durability: 'process',    // 'process' | 'durable'

  // Security
  encryptionKey: process.env.DB_KEY,
  encryptBackups: true,
});
```

All options and defaults: [API.md](./API.md#constructor-options).

## Migrating from spectre.db v1.1.0

The default `format: 'auto'` opens `.snapshot`/`.wal` databases without any
change. When you want to switch to the binary format:

```js
const db = new Database('./data/mydb'); // format: 'auto'
await db.ready;

await db.migrate('v2');   // writes .spdb/.spwal, removes the JSON files
// ... or back:
await db.migrate('json'); // writes .snapshot/.wal, removes the binary files
```

`migrate()` requires the native engine. The JavaScript fallback refuses to open
v2 databases with an explicit error (`SNAPSHOT_CORRUPTED`, code 3000) — migrate
back to JSON first if you need to run without the addon.

## Discord bot example

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

process.on('SIGINT', async () => {
  await client.db.close();
  process.exit(0);
});
```

More runnable code in [`examples/`](../examples/):

- `basic.js` — CRUD, queries, transactions, tables
- `advanced.js` — encryption, tables, compaction
- `discord-bot.js` — economy bot
- `v2-features.js` — scan, indexes, raw values, async compaction
- `migrate.js` — JSON ↔ v2 migration

## Next steps

- [API.md](./API.md) — every method, option, event and error code
- [ADVANCED.md](./ADVANCED.md) — indexes, cursors, raw values, multi-process
- [DURABILITY.md](./DURABILITY.md) — understand the durability contract
- [TROUBLESHOOTING.md](./TROUBLESHOOTING.md) — when something goes wrong
