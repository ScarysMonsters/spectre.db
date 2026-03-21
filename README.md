> [!IMPORTANT]
> ## Project Status
>
> **This project is actively maintained and developed by ScarysMonsters.**

> [!NOTE]
> **spectre.db is a zero-dependency, production-grade file-based JSON database for Node.js bots and small applications.**
> **Built on a WAL (Write-Ahead Log) architecture with atomic writes, LRU cache, real transactions, and optional AES-256 encryption.**

## About

<strong>Welcome to `spectre.db`, a Node.js module that provides a lightweight, persistent key-value database engineered for Discord bots and backend services.</strong>

- spectre.db is a [Node.js](https://nodejs.org) module with **zero external dependencies** — powered entirely by Node.js built-ins.
- Uses a **WAL + snapshot** architecture so your data is never at risk from an unclean shutdown.

<div align="center">
  <p>
    <a href="https://www.npmjs.com/package/spectre.db"><img src="https://img.shields.io/npm/v/spectre.db.svg" alt="npm version" /></a>
    <a href="https://www.npmjs.com/package/spectre.db"><img src="https://img.shields.io/npm/dt/spectre.db.svg" alt="npm downloads" /></a>
    <a href="https://github.com/ScarysMonsters/spectre.db"><img src="https://img.shields.io/github/stars/ScarysMonsters/spectre.db?style=flat" alt="GitHub stars" /></a>
    <a href="https://github.com/ScarysMonsters/spectre.db/blob/main/LICENSE"><img src="https://img.shields.io/npm/l/spectre.db.svg" alt="license" /></a>
  </p>
</div>

### <strong>[Example Code](https://github.com/ScarysMonsters/spectre.db/tree/main/examples)</strong>

---

## Features

- [x] Zero external dependencies (pure Node.js built-ins)
- [x] WAL-based persistence — never rewrites the full file on every change
- [x] Atomic writes — crash-safe temp file + rename pattern
- [x] Backup rotation — configurable generations (`.1.bak`, `.2.bak`, ...)
- [x] O(1) LRU cache with prefix-index invalidation
- [x] Real transactions — function-based with automatic rollback on error
- [x] Legacy array transaction API — fully backward compatible
- [x] AES-256-GCM encryption — auto-applied to sensitive keys (`token`, `password`, `secret`, ...)
- [x] Table abstraction — scoped key namespacing
- [x] Dot-notation keys — `users.123.coins` works out of the box
- [x] Prototype pollution prevention by design
- [x] Events: `change`, `save`, `rollback`, `restore`, `clear`
- [ ] TypeScript types (planned)

---

## Installation

> [!NOTE]
> **Node.js 18.0.0 or newer is required**

```sh-session
npm install spectre.db@latest
```

---

## Quick Start

```js
const { Database } = require('spectre.db');

const db = new Database('./data/mydb', {
  cache:    true,
  autoSave: 5000,
  backup:   true,
});

db.ready.then(() => {
  db.set('users.1.name', 'Alice');
  console.log(db.get('users.1.name')); // 'Alice'
});
```

---

## Discord Bot Example

```js
const { Client, GatewayIntentBits } = require('discord.js');
const { Database } = require('spectre.db');

const client = new Client({ intents: [GatewayIntentBits.Guilds] });

const db = new Database('./src/data/database', {
  cache:    true,
  autoSave: 5000,
  backup:   true,
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
| `cache` | boolean | `true` | Enable LRU cache |
| `maxCacheSize` | number | `1000` | Max cached entries |
| `cacheTTL` | number | `0` | TTL in ms (0 = forever) |
| `autoSave` | number | `5000` | Compaction interval in ms |
| `backup` | boolean | `true` | Enable backup rotation |
| `backupCount` | number | `3` | Number of backup generations |
| `compress` | boolean | `false` | Gzip snapshot compression |
| `encryptionKey` | string/Buffer | `null` | AES-256-GCM key for sensitive values |
| `warmKeys` | string[] | `[]` | Keys to pre-load into cache on startup |

---

### Core Methods

```js
db.get('users.1.name')              // → value or null
db.set('users.1.name', 'Alice')     // → value
db.delete('users.1.name')           // → true / false
db.has('users.1.name')              // → true / false
db.add('users.1.coins', 100)        // → new value
db.sub('users.1.coins', 50)         // → new value
db.push('users.1.roles', 'admin')   // → new array length
db.pull('users.1.roles', 'member')  // → true / false
```

### Query Methods

```js
db.all()                                              // → [{ ID, data }]
db.startsWith('users.')                               // → [{ ID, data }]
db.filter((data, id) => id.endsWith('.coins'))        // → [{ ID, data }]
db.find((data, id)   => id === 'users.1.name')        // → { ID, data } | null
db.paginate('users.', page, limit, sortBy, sortDesc)  // → { data, pagination }
```

### Transactions

```js
// Function-based (recommended)
await db.transaction(async (tx) => {
  const coins = tx.get('users.1.coins') ?? 0;
  tx.set('users.1.coins', coins - 50);
  tx.set('users.2.coins', (tx.get('users.2.coins') ?? 0) + 50);
});

// Array-based (legacy — fully supported)
await db.transaction([
  { type: 'set',    key: 'config.debug',  value: true },
  { type: 'add',    key: 'stats.logins',  value: 1    },
  { type: 'delete', key: 'cache.tmp'                  },
]);
```

### Tables

```js
const users = db.table('users');

users.set('1.name', 'Alice')   // stored as "users.1.name"
users.get('1.name')            // 'Alice'
users.count()                  // number of entries
await users.clear()            // removes all users.*

await users.transaction(async (tx) => {
  tx.add('1.coins', 100);
});
```

### Persistence & Lifecycle

```js
await db.save()    // force compaction now
await db.close()   // flush + compact + release (always call on shutdown)
db.getStats()      // { entries, cacheSize, fileSize, walOps, ... }
```

---

## How it works

### Files on disk

```
data/
├── mydb.snapshot         ← compacted JSON state
├── mydb.wal              ← append-only operation log
├── mydb.snapshot.1.bak   ← most recent backup
├── mydb.snapshot.2.bak
└── mydb.snapshot.3.bak
```

### Write flow

```
db.set('x', 1)
  └─ updates in-memory store immediately (synchronous)
  └─ WAL append queued (async, non-blocking)

Every compactInterval:
  └─ if walOps >= compactThreshold:
       └─ serialize → unique temp file → atomic rename → truncate WAL
```

### Startup flow

```
new Database(path)
  └─ load .snapshot
  └─ replay .wal on top of snapshot
  └─ open WAL for appending
  └─ db.ready resolves
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

## Star History

<a href="https://www.star-history.com/?repos=ScarysMonsters%2Fspectre.db&type=date&legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/image?repos=ScarysMonsters/spectre.db&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/image?repos=ScarysMonsters/spectre.db&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/image?repos=ScarysMonsters/spectre.db&type=date&legend=top-left" />
 </picture>
</a>