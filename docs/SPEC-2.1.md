# spectre.db 2.1 Engineering Specification

Status: draft for implementation
Target versions: 2.1.x (compatibility-preserving), 2.2.0 for larger internal changes
Owner: maintainers of `@sexfy/spectre.db`

## 1. Goals

1. Make benchmark results reproducible, comparable and honest.
2. Prove behavior at 1M, 5M and 10M keys (latency, throughput, RSS, startup, compaction).
3. Harden crash, corruption and recovery guarantees.
4. Document and, where proven necessary, improve concurrency.
5. Make the package easy to adopt on Node.js and Bun without regressing the v1.1.0/v2.0 contract.

## 2. Compatibility contract (non-negotiable)

- Public JavaScript API of v1.1.0 and v2.0.0 stays source-compatible.
- Existing databases (JSON v1.1.0 and binary v2.0.x) must open without migration.
- Existing error codes and error shapes stay valid.
- New features are additive (`getMany`, `setMany`, `scanSync`, optional settings).
- No change to the v2 binary format without a versioned format bump and a migration path.
- Every performance change must be accompanied by a benchmark and a regression check.
- No secret, token, absolute local path or user identifier is committed to the repository or published in npm packages.

## 3. Workstream P0 — Benchmark credibility

### 3.1 Already corrected in 2.0.1

- `count` records are split into:
  - `count` = number of `db.count()` queries (queries/s),
  - `countEntries` = number of entries scanned (entries/s).
- The README and changelog label count results as entries/s and queries/s separately.

### 3.2 Benchmark matrix

Create one benchmark suite (`benchmarks/spectre-bench.js` or extend `benchmarks/bench.js`) with:

| Scenario | Keys | Value size |
|---|---:|---:|
| baseline | 100,000 | 185 B |
| scale-1m | 1,000,000 | 185 B |
| scale-5m | 5,000,000 | 185 B |
| scale-10m | 10,000,000 | 185 B |
| large-values | 1,000,000 | 1 KB |
| huge-values | 100,000 | 16 KB |

For every scenario measure:

- single write, single read, random read, update, delete;
- batched write and batched read (same batch size for all engines);
- full scan and cursor pagination;
- secondary-index lookup (`db.find`) where supported by the engine;
- count queries and entries scanned;
- startup/open time and recovery time;
- compaction time and peak memory.

### 3.3 Metrics

- Throughput: ops/s, rows/s, entries/s.
- Latency: P50, P95, P99 (per operation, in microseconds).
- Memory: RSS before/after open, after load, after scan, peak during compaction.
- Disk: database size after write and after compaction.
- Run metadata: engine versions, OS, CPU, Node/Bun version, value size, batch size, durability mode, cache settings.
- Report median and full run distribution; never report only the best run.
- Fail the benchmark when verification fails (round-trip, count, scan).

### 3.4 Fair comparison rules

- Same durability guarantees per comparison:
  - `process` mode vs LMDB default / SQLite `synchronous=NORMAL`;
  - `durable` mode vs SQLite `synchronous=FULL`; document that LMDB `putSync` uses an asynchronous flush.
- Compare bulk APIs to bulk APIs; never compare a native batch call to a naive JavaScript loop.
- Explicitly state cache, compaction and WAL settings.
- No cherry-picking: publish all raw JSON results, including failures.
- Benchmarks must run on a quiet machine; store thermal/load caveats in metadata.

### 3.5 Acceptance criteria

- The suite runs unattended and writes machine-readable JSON plus a Markdown summary.
- Results are committed under `benchmarks/results/` and attached to GitHub releases.
- README tables are generated only from committed raw results.
- A 10% regression on any reported metric fails CI.

## 4. Workstream P1 — Scale to millions of keys

### 4.1 Memory profiling

- Add a profiling harness that records RSS at 100k, 1M, 5M and 10M keys.
- Measure per-key overhead for:
  - `BTreeMap` keys,
  - `HashMap` keys and entries,
  - `Arc<[u8]>` values,
  - secondary indexes,
  - cache entries,
  - WAL and segment buffers.
- Measure peak RSS during compaction and during startup replay.
- Do not remove a structure without measuring its throughput contribution; the hash index exists because point reads depend on it.

### 4.2 Startup and loading

- Measure open time at each scale.
- If open time or RSS exceeds the budget, implement lazy/paged loading:
  - load key metadata first;
  - resolve values from segments on demand;
  - keep a bounded LRU of hot values;
  - preserve exact get/scan semantics.
- Any partial-loading design needs a format/version strategy and crash-safety review.

### 4.3 Cache

- Configurable real memory budget (`maxCacheBytes`) instead of entry-count-only.
- Eviction policy configurable (LRU default).
- Expose `cacheHits`, `cacheMisses`, `cacheBytes`, `evictions`.
- Optional selective preload (`warmKeys`) and documented behavior.
- Cache must never return stale data after updates/deletes/compaction.

### 4.4 Acceptance criteria

Provisional budgets (to be validated by measurements):

- 1M keys open under 2 s and RSS under 3x raw value bytes.
- 5M keys open under 10 s and RSS under 3x raw value bytes.
- 10M keys open under 25 s without OOM on a 16 GB machine.
- No regression above 5% on the 100k benchmark for point reads/writes.
- Compaction peak RSS documented and bounded.

## 5. Workstream P1 — Reliability and crash resistance

### 5.1 Fix permissive tests

- `tests/fault-injection.test.js`: replace `expect([null, 3]).toContain(db.get('c'))` with strict assertions on acknowledged writes.
- Replace `recoveryCount >= 0` with exact expected values or a monotonic invariant tied to the crash point.
- Every crash point must assert:
  - acknowledged writes are present after recovery (durable mode),
  - no unacknowledged write is observable as committed,
  - data before the crash point is intact,
  - the database reopens and passes a full consistency scan.

### 5.2 Crash/corruption matrix

Add tests for:

- crash before/after WAL fsync;
- truncated WAL at every frame boundary;
- corrupted WAL frame (bad CRC, bad length, partial header);
- corrupted snapshot header/body/CRC;
- corrupted manifest and missing segment;
- crash during segment merge and manifest rename;
- repeated crash/recover cycles;
- wrong encryption key and wrong scrypt parameters;
- backup restore when the primary snapshot is corrupted.

### 5.3 Property-based and fuzz testing

- `proptest` (Rust) for WAL/snapshot/manifest parsers: random valid/invalid inputs must never panic; invalid input must return a typed error.
- `cargo-fuzz` targets for `replay_wal`, snapshot decode, manifest decode and key validation.
- Model-based test: random sequence of set/delete/clear/compact, compare in-memory model to reopened database.
- Run fuzz smoke tests in CI (bounded time) and a nightly longer run.

### 5.4 Power-loss semantics

- Document exactly what `durability: "durable"` guarantees: file fsync, directory fsync, WAL ordering, snapshot rename.
- Add VM-level hard-reboot tests where possible; process kill is not a power-loss test.
- State clearly which guarantees are not covered.

### 5.5 Acceptance criteria

- No lost acknowledged write in `durable` mode under the tested crash points.
- No panic on corrupted or truncated inputs; typed errors only.
- Recovery is idempotent across repeated restarts.
- Fault-injection and corruption tests run on Linux, macOS and Windows.

## 6. Workstream P2 — Concurrency

### 6.1 Current model

- `Arc<Mutex<Engine>>` in the napi layer; one writer at a time; exclusive process lock.
- Document this clearly as the supported model.

### 6.2 Evaluation

- Prototype an `RwLock`/snapshot-read model behind a feature flag and measure:
  - read concurrency,
  - write latency regression,
  - snapshot lifetime and memory,
  - interaction with WAL replay and compaction.
- Do not merge without benchmarks proving a real gain and a safety review.

### 6.3 Event loop

- Measure event-loop delay during scans, large transactions, compaction, recovery and index rebuild.
- Move long operations to `compactAsync()` or worker threads where safe.
- Add a documented limit for synchronous operations.

### 6.4 Multi-process

- Test 2, 5 and 10 processes opening the same database.
- Test stale lock takeover after a killed process.
- Verify no concurrent-writer corruption; document `LOCK_TIMEOUT` behavior.
- Do not claim multi-writer support without a proven design.

### 6.5 Acceptance criteria

- Documented concurrency model and limits.
- No event-loop stall above an agreed budget for documented async APIs.
- Multi-process tests pass on all three platforms.
- Any concurrency change is opt-in and benchmarked.

## 7. Workstream P3 — Compatibility and adoption

### 7.1 CI matrix

Test at least:

| Environment | Coverage |
|---|---|
| Node 18 (minimum) | CRUD, API, fallback |
| Node 20/22/24 LTS/current | full suite |
| Bun stable | CRUD, transactions, native storage |
| Windows / Linux / macOS | storage, lock, recovery |
| CommonJS / ESM / TypeScript | import, types, build |

- Verify prebuilt binaries on real runners for each OS/arch.
- Keep the release workflow's setup-node version aligned with npm trusted publishing requirements.

### 7.2 Examples and docs

- Official examples: Discord.js economy/inventory, Node REST cache, Bun CLI/backend, TypeScript typed transactions.
- Keep migration guide v1.1.0 → v2.x and API reference current.
- Add a "when to use / when not to use" section (embedded KV, single-writer workloads).

### 7.3 Licensing

- Decide between Source Available, permissive Open Source or dual licensing.
- Document contribution and redistribution rules; keep the decision out of code changes.

### 7.4 Acceptance criteria

- CI green on every claimed environment.
- Published package installs without compilation on all claimed platforms.
- Examples run against the published package.

## 8. Workstream P0 — Release engineering (immediate)

### 8.1 Trusted publishing failure (observed on v2.0.1)

Cause: `actions/setup-node` injects a placeholder `NODE_AUTH_TOKEN`; npm then attempts token auth and returns 404 instead of using OIDC.

Fix already on main:

- `actions/setup-node@v6` with `node-version: 24`, `registry-url` and `package-manager-cache: false`.
- A step deletes `//registry.npmjs.org/:_authToken` and sets `NODE_AUTH_TOKEN` empty.
- Publish steps run with `NODE_AUTH_TOKEN: ''`.

### 8.2 Trusted publisher configuration

- Configure a trusted publisher for each of the six packages:
  - provider: GitHub Actions,
  - owner: `ScarysMonsters`,
  - repository: `spectre.db`,
  - workflow: `release.yml`,
  - environment: `npm`,
  - allowed actions: direct `npm publish` (and dist-tag if needed).
- Validate each configuration with a real publish; local token publishes do not validate OIDC configurations.
- If OIDC cannot be used, fall back to a granular automation token stored in the `npm` environment secret; never commit it.

### 8.3 Release checklist

- Bump versions (`scripts/bump-version.js`).
- Update changelog and benchmark tables from committed results.
- Run: `cargo test -p spectre-db-core`, `npm test`, `npm run test:fallback`, `npm run test:fault`, `npm run check-versions`.
- Commit, tag `vX.Y.Z`, push main and tag.
- Watch test → build → publish; verify all six npm versions and the GitHub release.
- Attach benchmark results to the GitHub release.
- Never force-push a tag after a successful npm publish.

### 8.4 Acceptance criteria

- Release workflow green on all five build targets.
- Six npm packages published with the same version and provenance.
- GitHub release contains notes and benchmark assets.
- No secret in logs or repository.

## 9. Implementation order

1. P0 benchmark fixes and release engineering (mostly done in 2.0.1).
2. P1 scale harness + memory profiling at 1M/5M/10M.
3. P1 reliability: strict fault injection, corruption matrix, fuzzing.
4. P2 concurrency evaluation and event-loop measurements.
5. P3 CI matrix, examples, docs, license decision.
6. P2 optimizations only after measurements prove the need.

## 10. Definition of done

- Benchmarks reproducible from a clean checkout and committed raw results.
- 1M/5M/10M runs documented with latency, throughput, RSS, startup and compaction.
- Crash/corruption/fuzz suites green on all platforms.
- Concurrency and multi-process limits documented and tested.
- CI green on every claimed Node/Bun/OS target.
- Release workflow publishes all packages with OIDC and provenance.
- No API or format regression for v1.1.0/v2.0 users.

## 11. Out of scope for 2.1

- Multi-writer concurrency.
- A new on-disk format (unless required by profile data and approved).
- Replacing the Rust core.
- Claiming power-loss guarantees that are not tested.
