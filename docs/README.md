# spectre.db — Documentation

**spectre.db** is a high-performance, crash-safe key-value store written in Rust,
exposed to Node.js and Bun through napi-rs, with a frozen v1.1.0 JavaScript
engine as fallback. The npm package is [`@sexfy/spectre.db`](https://www.npmjs.com/package/@sexfy/spectre.db).

This directory contains the full project documentation.

## Documentation map

| Document | What it covers |
|---|---|
| [GETTING-STARTED.md](./GETTING-STARTED.md) | Installation, first database, core concepts, migration from v1.1.0 |
| [API.md](./API.md) | Complete API reference: `Database`, `Table`, options, methods, events, stats, error codes |
| [ADVANCED.md](./ADVANCED.md) | Lazy iteration, secondary indexes, raw values, compression, segmented compaction, async compaction, multi-process |
| [DURABILITY.md](./DURABILITY.md) | WAL, durability modes, crash recovery, backups, fault injection |
| [ENCRYPTION.md](./ENCRYPTION.md) | Key derivation, sensitive keys, encrypted backups, whole-snapshot encryption |
| [ARCHITECTURE.md](./ARCHITECTURE.md) | Repository layout, Rust core, napi bridge, JS facade, fallback engine, concurrency model |
| [FORMATS.md](./FORMATS.md) | On-disk format specification (v2 binary, v1.1.0 JSON, WAL, segments, manifest, locks) |
| [TROUBLESHOOTING.md](./TROUBLESHOOTING.md) | Error reference, common failures, diagnostics, FAQ |
| [RELEASING.md](./RELEASING.md) | Versioning, build, CI workflows, prebuilt binaries, npm publishing |

Other entry points at the repository root:

- [README.md](../README.md) — project overview and feature list
- [GUIDE.md](../GUIDE.md) — task-oriented usage guide
- [CONTRIBUTING.md](../CONTRIBUTING.md) — contribution process and development setup
- [CHANGELOG.md](../CHANGELOG.md) — release history
- [examples/](../examples/) — runnable examples

## At a glance

```js
const { Database } = require('@sexfy/spectre.db');

const db = new Database('./data/mydb', {
  cache: true,
  autoSave: 5000,
  backup: true,
});

await db.ready;

db.set('users.1.name', 'Alice');
db.get('users.1.name'); // 'Alice'

await db.close();
```

- **Drop-in v1.1.0 API** — `get`, `set`, `transaction`, `table`, events.
- **Native speed** — the store lives in Rust; values cross the JS↔Rust boundary once.
- **Dual engine** — native addon when available, frozen v1.1.0 JavaScript engine otherwise.
- **Crash-safe** — WAL + atomic snapshot renames, backup rotation, file locking.
- **Zero runtime dependencies** — the JS layer only uses Node.js built-ins.

## Requirements

- Node.js >= 18.0.0 (native prebuilds are installed automatically)
- Bun >= 1.0.0 is supported
- Supported prebuilds: Linux x64 (glibc and musl), macOS x64 and arm64, Windows x64 (MSVC)
