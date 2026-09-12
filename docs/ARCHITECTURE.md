# Architecture

This document describes how the project is organized, how data flows through
the different layers, and how the Rust and JavaScript parts cooperate.

## Repository layout

```
spectre.db/
├── index.js               ← JS facade: Database + Table, option mapping, engine loader
├── index.d.ts             ← TypeScript definitions
├── fallback/              ← frozen v1.1.0 JavaScript engine (JSON format only)
│   ├── index.js           ← Database subclass + v2-format guard
│   └── src/
│       ├── core/engine.js       ← core engine (WAL, LRU, crypto, transactions)
│       ├── storage/wal.js       ← append-only WAL writer
│       ├── storage/lock.js      ← file lock
│       ├── storage/backup.js    ← backup rotation/encryption
│       ├── crypto/              ← scrypt KDF + AES-256-GCM
│       ├── cache/               ← LRU cache + prefix index
│       ├── queue/write-queue.js ← serialized async write queue
│       ├── transaction/         ← transaction object
│       ├── events/              ← bounded event emitter
│       └── utils/               ← validator, path normalizer, JSON safety, error codes
├── crates/
│   ├── spectre-db-core/   ← pure Rust engine (no napi dependency)
│   │   └── src/
│   │       ├── engine.rs      ← Engine: open, CRUD, batch, scan, compaction, stats
│   │       ├── store.rs       ← flat BTreeMap store + JSON field splitter
│   │       ├── formats.rs     ← v2 snapshot/WAL/segment/manifest encoding
│   │       ├── segments.rs    ← incremental compaction jobs (async phases)
│   │       ├── lock.rs        ← cross-platform file lock + stale pid detection
│   │       ├── crypto.rs      ← scrypt KDF, AES-256-GCM, backup/snapshot encryption
│   │       ├── index.rs       ← secondary index B-tree
│   │       ├── validator.rs   ← key/value validation, sensitive keys
│   │       ├── json_compat.rs ← v1.1.0 JSON snapshot/WAL compatibility
│   │       ├── pathnorm.rs    ← path resolution and extension stripping
│   │       └── error.rs       ← structured error codes
│   └── spectre-db-napi/   ← napi-rs v3 bindings
│       ├── src/lib.rs         ← SpectreEngine class, CompactTask, error mapping
│       └── build.rs           ← napi_build::setup() (linker flags)
├── npm/                   ← per-platform binary packages (optionalDependencies)
│   ├── spectre-db-linux-x64-gnu/
│   ├── spectre-db-linux-x64-musl/
│   ├── spectre-db-darwin-x64/
│   ├── spectre-db-darwin-arm64/
│   └── spectre-db-win32-x64-msvc/
├── scripts/
│   ├── build.sh           ← cargo build + install build/Release/spectre.db-rs.node
│   ├── smoke.js           ← end-to-end smoke test
│   ├── check-versions.js  ← keeps all package versions in sync
│   └── bump-version.js    ← bumps root, platform packages, Cargo.toml and lock
├── tests/                 ← Jest suites (native, fallback, fault injection)
├── examples/              ← runnable examples
└── docs/                  ← this documentation
```

## Layers

```
┌───────────────────────────── Node.js / Bun ─────────────────────────────┐
│  index.js (Database + Table, EventEmitter, option mapping)              │
│      │ JSON.stringify once ─── getJson/setJson/allJson/scanJson         │
│  ┌───▼──────────────────────────────────────────────────────────────┐   │
│  │  crates/spectre-db-napi (napi-rs v3)                             │   │
│  └───┬──────────────────────────────────────────────────────────────┘   │
│  ┌───▼──────────────────────────────────────────────────────────────┐   │
│  │  crates/spectre-db-core (pure Rust)                              │   │
│  │  engine · store(BTreeMap) · formats v2 · json_compat · crypto    │   │
│  │  wal · snapshot · backup · lock · validator · index              │   │
│  └──────────────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────────────┘
```

### 1. JS facade (`index.js`)

- Resolves the native binding (`build/Release/`, `prebuilds/`, then the
  `spectre-db-<platform>-<arch>` packages).
- Maps legacy v1.1.0 options to engine options.
- Implements `Database` and `Table` on top of the engine.
- Keeps values as strings across the boundary: the JS layer calls
  `JSON.stringify` once and Rust stores the exact bytes.
- Adds `compact()`/`save()`, `transaction()` staging, index handles, raw
  methods, `stats()`/`getStats()`, `migrate()` and the event bridge.
- Falls back to `fallback/` when no native binary is available.

### 2. napi bridge (`crates/spectre-db-napi`)

- Exposes `SpectreEngine` with `setJson`, `getJson`, `allJson`, `scanJson`,
  `batch`, `setManyJson`, `stats`, `compact`, `compactAsync`, `migrate`, raw
  methods and index methods.
- Wraps the core engine in `Arc<Mutex<Engine>>`: single writer, short critical
  sections.
- `compactAsync()` returns a napi `AsyncTask`; serialization runs on a libuv
  worker thread while the JS event loop keeps running.
- Converts `SpectreError` to a napi error message formatted as
  `"<code>|<message>"`; `index.js` decodes it back into an `Error` with a
  numeric `code` property.

### 3. Rust core (`crates/spectre-db-core`)

| Module | Responsibility |
|---|---|
| `engine.rs` | Lifecycle, CRUD, batch, scan pages, compaction orchestration, stats, crash points |
| `store.rs` | Flat `BTreeMap` of encoded keys → JSON bytes; raw namespace; JSON field splitting for `all()` |
| `formats.rs` | Snapshot/WAL/segment/manifest encoding, CRC32, gzip/zstd compression |
| `segments.rs` | Incremental compaction jobs (`prepare_compact` / `compute` / `finish_compact`) |
| `lock.rs` | Exclusive lock file with stale-pid takeover (Unix `kill`, Windows `OpenProcess`) |
| `crypto.rs` | scrypt key derivation, AES-256-GCM value/backup/snapshot encryption |
| `index.rs` | Sorted secondary index with sortable numeric encoding |
| `validator.rs` | Key/value validation, sensitive-key detection |
| `json_compat.rs` | Byte-compatible v1.1.0 JSON snapshots and WAL |
| `pathnorm.rs` | Path resolution, extension stripping, `.lock` naming |

### 4. JavaScript fallback (`fallback/`)

A frozen copy of the v1.1.0 engine used when the native addon cannot be loaded.
It speaks the JSON format only and refuses v2 files with a clear error.

Compared to the original v1.1.0 code, the fallback carries three bug fixes
required for correct behavior on modern Node.js (see
[FORMATS.md §8](./FORMATS.md#8-v110-bugs-fixed-by-the-v2-engine-format-unchanged)):

- `ready` now awaits initialization (v1.1.0 resolved early);
- single-flight WAL open (Node 26 turns leaked `FileHandle`s into hard errors);
- `WriteQueue` no longer leaks unhandled rejections or poisons subsequent writes.

## Data flow

### Write path

```
db.set('x', value)
  └─ validate + JSON.stringify once (JS)
  └─ native setJson(key, json)
       └─ validate key, wrap sensitive values, store in BTreeMap
       └─ append to WAL (one write() of key‖value bytes)
  └─ emit 'change'
```

### Read path

```
db.get('x')
  └─ native getJson('x') → Option<JsString>
  └─ JSON.parse once
```

### Enumeration path

```
db.all() / db.scan()
  └─ Rust builds the whole result (or a page) as one JSON document
  └─ a single JSON.parse in V8
```

This keeps the JS↔Rust boundary crossing to one call per operation.

### Compaction path

```
compact()
  └─ rotate backups
  └─ serialize store (or dirty keys in segmented mode)
  └─ write temp file → atomic rename
  └─ update manifest (segmented mode)
  └─ truncate/reset WAL
```

## Concurrency model

- **Single writer per process**: the engine is guarded by a mutex; writes are
  serialized.
- **Single writer across processes**: the `.lock` file serializes processes;
  stale locks from dead pids are taken over.
- **Non-blocking compaction**: `compactAsync()` runs serialization on a libuv
  worker thread in three phases (short lock → compute → short lock). The WAL is
  not truncated, so a crash during compaction replays idempotently.
- **WAL replay**: opening replays the WAL on top of the snapshot and stops at
  the first bad/truncated CRC frame, emitting a `warn`.

## Build and engine resolution

```sh
npm run build   # scripts/build.sh: cargo build -p spectre-db-napi --release
```

The build installs the addon at `build/Release/spectre.db-rs.node`. At runtime
`index.js` tries, in order:

1. `build/Release/spectre.db-rs.node`
2. `prebuilds/spectre.db-rs-<platform>-<arch>.node`
3. `spectre-db-<platform>-<arch>[-gnu|-musl]` npm packages
4. the JavaScript fallback

Set `SPECTRE_FORCE_FALLBACK=1` to skip the native engine entirely.

## Tests

| Suite | Command | Coverage |
|---|---|---|
| Native + fallback integration | `npm test` | CRUD, queries, transactions, formats, locking, events, encryption |
| Fallback only | `npm run test:fallback` | v1.1.0 API behavior on the JS engine |
| Fault injection | `npm run test:fault` | Kill at instrumented points, recovery guarantees |
| Rust unit tests | `cargo test -p spectre-db-core` | Store, formats, segments, crypto, validation, locking |
| Smoke | `npm run smoke` | End-to-end against the resolved engine |

The fault-injection suite uses `SPECTRE_CRASH_AT=point[:skip]`; see
[DURABILITY.md](./DURABILITY.md#fault-injection) for the list of points.

## Design principles

- **One boundary crossing per operation** — serialize once in JS, store bytes
  as-is in Rust.
- **Crash safety over cleverness** — append-only WAL, atomic renames, CRC32 on
  every frame and snapshot.
- **API stability** — the v1.1.0 surface is frozen; new features are additive.
- **No runtime dependencies** — the JS layer only uses Node.js built-ins; the
  Rust core only uses well-established crates.
- **Graceful degradation** — when the native addon is unavailable, the fallback
  engine keeps JSON databases working.
