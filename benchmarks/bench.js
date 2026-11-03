'use strict';

const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const SPECTRE_PACKAGE = process.env.SPECTRE_BENCH_LOCAL === '1' ? '..' : '@sexfy/spectre.db';
const { Database, hasNativeEngine, version: spectreVersion } = require(SPECTRE_PACKAGE);
const { open: openLmdb } = require('lmdb');
const sqlite = require('node:sqlite');

const BATCH = 1000;

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (!arg.startsWith('--')) continue;
    const eq = arg.indexOf('=');
    let key;
    let value;
    if (eq !== -1) {
      key = arg.slice(2, eq);
      value = arg.slice(eq + 1);
    } else {
      key = arg.slice(2);
      const next = argv[i + 1];
      if (next !== undefined && !next.startsWith('--')) {
        value = next;
        i++;
      }
    }
    const camel = key.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
    if (value === undefined) out[camel] = true;
    else out[camel] = value !== '' && !Number.isNaN(Number(value)) ? Number(value) : value;
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
const N = Number(args.n ?? 100000);
const READS = Number(args.reads ?? 500000);
const MIXED = Number(args.mixed ?? 200000);
const RUNS = Math.max(1, Number(args.runs ?? 3));
const BATCH_SIZE = Math.max(1, Number(args.batch ?? BATCH));

const ROOT = __dirname;
const DATA_DIR = path.join(ROOT, 'data');
const RESULTS_DIR = path.join(ROOT, 'results');
const DURABLE = Boolean(args.durable);

const ENGINE_NAMES = ['spectre', 'sqlite', 'lmdb'];
const PREFIX = { spectre: 'user.', sqlite: 'user:', lmdb: 'user:' };
const OTHER_PREFIX = { spectre: 'meta.', sqlite: 'meta:', lmdb: 'meta:' };

function hr() {
  return process.hrtime.bigint();
}

function elapsedMs(start) {
  return Number(process.hrtime.bigint() - start) / 1e6;
}

function makeValue(i) {
  return JSON.stringify({
    id: i,
    name: 'user_' + i,
    email: 'user' + i + '@example.com',
    age: 18 + (i % 60),
    active: (i & 1) === 0,
    score: (i * 7919) % 10000,
    bio: 'lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor',
  });
}

function mulberry32(seed) {
  let a = seed >>> 0;
  return function () {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function prefixEnd(prefix) {
  return prefix.slice(0, -1) + String.fromCharCode(prefix.charCodeAt(prefix.length - 1) + 1);
}

function dirSize(dir) {
  let total = 0;
  const stack = [dir];
  while (stack.length) {
    const current = stack.pop();
    let entries;
    try {
      entries = fs.readdirSync(current, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const full = path.join(current, entry.name);
      if (entry.isDirectory()) stack.push(full);
      else if (entry.isFile()) {
        try {
          total += fs.statSync(full).size;
        } catch {}
      }
    }
  }
  return total;
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

function fmt(n) {
  return n.toLocaleString('en-US', { maximumFractionDigits: 0 });
}

function mb(bytes) {
  return (bytes / (1024 * 1024)).toFixed(1);
}

async function createSpectre(dir) {
  fs.mkdirSync(dir, { recursive: true });
  const db = new Database(path.join(dir, 'bench'), {
    autoSave: 0,
    backup: false,
    cache: true,
    durability: DURABLE ? 'durable' : 'process',
  });
  await db.ready;
  return {
    engine: 'spectre',
    set: (key, value) => db.set(key, value),
    get: (key) => db.get(key) ?? null,
    getMany: typeof db.getMany === 'function' ? (keys) => db.getMany(keys) : null,
    count: (prefix) => db.count(prefix),
    batchSet: async (pairs) => {
      if (typeof db.setMany === 'function') {
        db.setMany(pairs);
        return;
      }
      await db.transaction(pairs.map(([key, value]) => ({ type: 'set', key, value })));
    },
    scan: async (prefix) => {
      let count = 0;
      if (typeof db.scanSync === 'function') {
        for (const _row of db.scanSync({ prefix, pageSize: 1000 })) count++;
        return count;
      }
      for await (const _row of db.scan({ prefix, pageSize: 1000 })) count++;
      return count;
    },
    batchDelete: async (keys) => {
      await db.transaction(keys.map((key) => ({ type: 'delete', key })));
    },
    flushForSize: async () => {
      await db.save();
    },
    diskBytes: () => dirSize(dir),
    close: async () => {
      await db.close();
    },
  };
}

function createSqlite(dir) {
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, 'bench.db');
  const db = new sqlite.DatabaseSync(file);
  db.exec('PRAGMA journal_mode = WAL');
  db.exec(`PRAGMA synchronous = ${DURABLE ? 'FULL' : 'NORMAL'}`);
  db.exec('PRAGMA temp_store = MEMORY');
  db.exec('PRAGMA cache_size = -65536');
  db.exec('CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID');

  const stmtSet = db.prepare('INSERT OR REPLACE INTO kv(key, value) VALUES (?, ?)');
  const stmtGet = db.prepare('SELECT value FROM kv WHERE key = ?');
  const stmtDelete = db.prepare('DELETE FROM kv WHERE key = ?');
  const stmtScan = db.prepare('SELECT key, value FROM kv WHERE key >= ? AND key < ? ORDER BY key');
  const stmtCount = db.prepare('SELECT COUNT(*) AS n FROM kv WHERE key >= ? AND key < ?');

  return {
    engine: 'sqlite',
    set: (key, value) => stmtSet.run(key, value),
    get: (key) => {
      const row = stmtGet.get(key);
      return row ? row.value : null;
    },
    getMany: (keys) => keys.map((key) => {
      const row = stmtGet.get(key);
      return row ? row.value : null;
    }),
    count: (prefix) => stmtCount.get(prefix, prefixEnd(prefix)).n,
    batchSet: (pairs) => {
      db.exec('BEGIN IMMEDIATE');
      try {
        for (const [key, value] of pairs) stmtSet.run(key, value);
        db.exec('COMMIT');
      } catch (error) {
        db.exec('ROLLBACK');
        throw error;
      }
    },
    scan: (prefix) => {
      let count = 0;
      for (const _row of stmtScan.iterate(prefix, prefixEnd(prefix))) count++;
      return count;
    },
    batchDelete: (keys) => {
      db.exec('BEGIN IMMEDIATE');
      try {
        for (const key of keys) stmtDelete.run(key);
        db.exec('COMMIT');
      } catch (error) {
        db.exec('ROLLBACK');
        throw error;
      }
    },
    flushForSize: () => {
      db.exec('PRAGMA wal_checkpoint(TRUNCATE)');
    },
    diskBytes: () => dirSize(dir),
    close: () => db.close(),
  };
}

function createLmdb(dir) {
  fs.mkdirSync(dir, { recursive: true });
  const db = openLmdb({ path: path.join(dir, 'bench'), compression: false });
  return {
    engine: 'lmdb',
    set: (key, value) => db.putSync(key, value),
    get: (key) => db.getSync(key) ?? null,
    getMany: (keys) => db.getMany(keys),
    count: (prefix) => db.getCount({ start: prefix, end: prefixEnd(prefix) }),
    batchSet: (pairs) => {
      db.transactionSync(() => {
        for (const [key, value] of pairs) db.putSync(key, value);
      });
    },
    scan: (prefix) => {
      const iterator = db.getRange({ start: prefix, end: prefixEnd(prefix) }).iterate();
      let count = 0;
      while (!iterator.next().done) count++;
      return count;
    },
    batchDelete: (keys) => {
      db.transactionSync(() => {
        for (const key of keys) db.removeSync(key);
      });
    },
    flushForSize: () => {},
    diskBytes: () => dirSize(dir),
    close: () => db.close(),
  };
}

const FACTORIES = { spectre: createSpectre, sqlite: createSqlite, lmdb: createLmdb };

function buildWorkload() {
  const values = new Array(N);
  for (let i = 0; i < N; i++) values[i] = makeValue(i);

  const keys = {};
  for (const engine of ENGINE_NAMES) {
    const prefix = PREFIX[engine];
    keys[engine] = new Array(N);
    for (let i = 0; i < N; i++) keys[engine][i] = prefix + i;
  }

  const rand = mulberry32(0xc0ffee);
  const readIndices = new Int32Array(READS);
  for (let i = 0; i < READS; i++) readIndices[i] = Math.floor(rand() * N);

  const mixedIndices = new Int32Array(MIXED);
  const mixedWrites = new Uint8Array(MIXED);
  for (let i = 0; i < MIXED; i++) {
    mixedIndices[i] = Math.floor(rand() * N);
    mixedWrites[i] = rand() < 0.2 ? 1 : 0;
  }

  const valueBytes = values.reduce((sum, value) => sum + Buffer.byteLength(value), 0) / N;

  return { values, keys, readIndices, mixedIndices, mixedWrites, valueBytes };
}

async function runMainWorkloads(engine, db, workload, records) {
  const { values, keys, readIndices, mixedIndices, mixedWrites } = workload;
  const key = keys[engine];
  const prefix = PREFIX[engine];

  let start = hr();
  for (let i = 0; i < N; i++) db.set(key[i], values[i]);
  records.write = { ms: elapsedMs(start), ops: N };

  if (db.get(key[N - 1]) !== values[N - 1]) records.verified = false;
  await db.flushForSize();
  records.diskBytes = db.diskBytes();

  const warmup = Math.min(Math.floor(READS / 10), 20000);
  for (let i = 0; i < warmup; i++) db.get(key[readIndices[i]]);

  start = hr();
  for (let i = 0; i < READS; i++) db.get(key[readIndices[i]]);
  records.read = { ms: elapsedMs(start), ops: READS };

  if (typeof db.getMany === 'function') {
    start = hr();
    for (let offset = 0; offset < READS; offset += BATCH_SIZE) {
      const chunk = [];
      const end = Math.min(offset + BATCH_SIZE, READS);
      for (let i = offset; i < end; i++) chunk.push(key[readIndices[i]]);
      const found = await db.getMany(chunk);
      if (found.length !== chunk.length) records.verified = false;
    }
    records.readMany = { ms: elapsedMs(start), ops: READS };
  }

  start = hr();
  const scanned = await db.scan(prefix);
  records.scan = { ms: elapsedMs(start), ops: scanned };
  if (scanned !== N) records.verified = false;

  start = hr();
  let counted = 0;
  const countRounds = 10;
  for (let round = 0; round < countRounds; round++) counted = db.count(prefix);
  const countMs = elapsedMs(start);
  records.count = { ms: countMs, ops: countRounds };
  records.countEntries = { ms: countMs, ops: N * countRounds };
  if (counted !== N) records.verified = false;

  start = hr();
  for (let i = 0; i < MIXED; i++) {
    const index = mixedIndices[i];
    if (mixedWrites[i]) db.set(key[index], values[(index + 1) % N]);
    else db.get(key[index]);
  }
  records.mixed = { ms: elapsedMs(start), ops: MIXED };
  if (db.get(key[0]) === null) records.verified = false;

  start = hr();
  for (let offset = 0; offset < N; offset += BATCH_SIZE) {
    await db.batchDelete(key.slice(offset, Math.min(offset + BATCH_SIZE, N)));
  }
  records.delete = { ms: elapsedMs(start), ops: N };

  const remaining = await db.scan(prefix);
  if (remaining !== 0) records.verified = false;
}

async function runBatchedWrite(engine, root, workload, records) {
  const dir = root + '-batch';
  fs.rmSync(dir, { recursive: true, force: true });
  const db = await FACTORIES[engine](dir);
  const { values, keys } = workload;
  const key = keys[engine];
  const prefix = PREFIX[engine];

  const start = hr();
  for (let offset = 0; offset < N; offset += BATCH_SIZE) {
    const pairs = [];
    const end = Math.min(offset + BATCH_SIZE, N);
    for (let i = offset; i < end; i++) pairs.push([key[i], values[i]]);
    await db.batchSet(pairs);
  }
  records.writeBatched = { ms: elapsedMs(start), ops: N };
  await db.flushForSize();
  records.diskBytesBatched = db.diskBytes();

  const scanned = await db.scan(prefix);
  if (scanned !== N) records.verified = false;
  if (db.get(key[N - 1]) !== values[N - 1]) records.verified = false;

  await db.close();
  fs.rmSync(dir, { recursive: true, force: true });
}

async function runEngine(engine, runIndex, workload) {
  const root = path.join(DATA_DIR, `${engine}-run${runIndex}`);
  fs.rmSync(root, { recursive: true, force: true });
  const records = { verified: true };
  let db;
  try {
    db = await FACTORIES[engine](root);
    await runMainWorkloads(engine, db, workload, records);
    await db.close();
    db = null;
    await runBatchedWrite(engine, root, workload, records);
  } catch (error) {
    records.error = error && error.stack ? error.stack : String(error);
    if (db) {
      try {
        await db.close();
      } catch {}
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
    fs.rmSync(root + '-batch', { recursive: true, force: true });
  }
  return records;
}

function opsPerSec(record) {
  if (!record || !record.ms) return 0;
  return (record.ops / record.ms) * 1000;
}

const WORKLOADS = [
  ['write', 'write (single op)'],
  ['writeBatched', 'write (batch of ' + fmt(BATCH_SIZE) + ')'],
  ['read', 'read (random)'],
  ['readMany', 'read (batch of ' + fmt(BATCH_SIZE) + ')'],
  ['scan', 'scan (full prefix)'],
  ['count', 'count (queries)'],
  ['countEntries', 'count (entries scanned)'],
  ['mixed', 'mixed 80/20'],
  ['delete', 'delete (batched)'],
];

function renderTable(header, rows) {
  const widths = header.map((cell, i) =>
    Math.max(cell.length, ...rows.map((row) => String(row[i]).length)),
  );
  const line = (cells) =>
    '| ' +
    cells.map((cell, i) => String(cell).padEnd(widths[i])).join(' | ') +
    ' |';
  return [
    line(header),
    '| ' + widths.map((w) => '-'.repeat(w)).join(' | ') + ' |',
    ...rows.map(line),
  ].join('\n');
}

async function main() {
  const workload = buildWorkload();
  const runs = {};
  for (const engine of ENGINE_NAMES) runs[engine] = [];

  const orders = [
    ['spectre', 'sqlite', 'lmdb'],
    ['sqlite', 'lmdb', 'spectre'],
    ['lmdb', 'spectre', 'sqlite'],
  ];

  const startedAt = Date.now();
  for (let runIndex = 0; runIndex < RUNS; runIndex++) {
    const order = orders[runIndex % orders.length];
    for (const engine of order) {
      process.stdout.write(`run ${runIndex + 1}/${RUNS} - ${engine} ... `);
      const started = Date.now();
      const records = await runEngine(engine, runIndex, workload);
      records.run = runIndex;
      records.elapsedMs = Date.now() - started;
      runs[engine].push(records);
      const status = records.error ? 'ERROR' : records.verified ? 'ok' : 'CHECK FAILED';
      process.stdout.write(`${status} (${(records.elapsedMs / 1000).toFixed(1)}s)\n`);
      if (global.gc) global.gc();
    }
  }
  const totalMs = Date.now() - startedAt;

  const summary = {};
  for (const engine of ENGINE_NAMES) {
    summary[engine] = {};
    for (const [workloadKey] of WORKLOADS) {
      const values = runs[engine]
        .filter((record) => !record.error && record[workloadKey])
        .map((record) => opsPerSec(record[workloadKey]));
      const sizes = runs[engine]
        .filter((record) => !record.error && record.diskBytes)
        .map((record) => record.diskBytes);
      summary[engine][workloadKey] = {
        opsPerSec: values.length ? median(values) : 0,
        runs: values,
        medianMs: values.length
          ? median(
              runs[engine]
                .filter((record) => !record.error && record[workloadKey])
                .map((record) => record[workloadKey].ms),
            )
          : 0,
        diskBytes: sizes.length ? median(sizes) : 0,
      };
    }
  }

  const nodeVersion = process.version;
  const sqliteVersion = (() => {
    try {
      const db = new sqlite.DatabaseSync(':memory:');
      const row = db.prepare('SELECT sqlite_version() AS v').get();
      db.close();
      return row.v;
    } catch {
      return 'unknown';
    }
  })();
  const lmdbVersion = (() => {
    try {
      const entry = require.resolve('lmdb');
      const pkgPath = path.join(path.dirname(entry), '..', 'package.json');
      const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
      const lib = require('lmdb').version;
      return `${pkg.version} (LMDB ${lib.versionString})`;
    } catch {
      return 'unknown';
    }
  })();

  console.log('\n=== Environment ===');
  console.log(`Node ${nodeVersion} | ${process.platform} ${process.arch} | ${os.cpus().length} logical cores`);
  console.log(`spectre.db ${spectreVersion} (native=${hasNativeEngine}) -> ${require.resolve(SPECTRE_PACKAGE)}`);
  console.log(`SQLite ${sqliteVersion} (node:sqlite, WAL, synchronous=${DURABLE ? 'FULL' : 'NORMAL'})`);
  console.log(`lmdb ${lmdbVersion} (sync per write, compression=false)`);
  console.log(
    `Config: N=${fmt(N)}, reads=${fmt(READS)}, mixed=${fmt(MIXED)}, batch=${fmt(BATCH_SIZE)}, runs=${RUNS}, value≈${workload.valueBytes.toFixed(0)} B, mode=${DURABLE ? 'durable' : 'process'}`,
  );
  console.log(`Total wall time: ${(totalMs / 1000).toFixed(1)}s`);

  console.log('\n=== Throughput (median ops/s) ===');
  const throughputRows = WORKLOADS.map(([workloadKey, label]) => {
    const cells = ENGINE_NAMES.map((engine) => {
      const value = summary[engine][workloadKey].opsPerSec;
      return value ? fmt(value) : '—';
    });
    const best = Math.max(...ENGINE_NAMES.map((engine) => summary[engine][workloadKey].opsPerSec));
    const winner = ENGINE_NAMES.filter((engine) => summary[engine][workloadKey].opsPerSec === best);
    return [label, ...cells, winner.join(' / ') || '—'];
  });
  console.log(renderTable(['Workload', ...ENGINE_NAMES, 'fastest'], throughputRows));

  console.log('\n=== Median time per workload (ms) ===');
  const timeRows = WORKLOADS.map(([workloadKey, label]) => [
    label,
    ...ENGINE_NAMES.map((engine) => {
      const value = summary[engine][workloadKey].medianMs;
      return value ? value.toFixed(1) : '—';
    }),
  ]);
  console.log(renderTable(['Workload', ...ENGINE_NAMES], timeRows));

  console.log('\n=== Relative to fastest (x slower) ===');
  const relativeRows = WORKLOADS.map(([workloadKey, label]) => {
    const values = ENGINE_NAMES.map((engine) => summary[engine][workloadKey].opsPerSec);
    const best = Math.max(...values);
    return [
      label,
      ...values.map((value) => (value ? (best / value).toFixed(2) + 'x' : '—')),
    ];
  });
  console.log(renderTable(['Workload', ...ENGINE_NAMES], relativeRows));

  console.log('\n=== On-disk size after write (MB) ===');
  const sizeRows = WORKLOADS.filter(([workloadKey]) => workloadKey === 'write' || workloadKey === 'writeBatched').map(
    ([workloadKey, label]) => [
      label,
      ...ENGINE_NAMES.map((engine) => {
        const key = workloadKey === 'write' ? 'diskBytes' : 'diskBytesBatched';
        const sizes = runs[engine]
          .filter((record) => !record.error && record[key])
          .map((record) => record[key]);
        return sizes.length ? mb(median(sizes)) : '—';
      }),
    ],
  );
  console.log(renderTable(['Workload', ...ENGINE_NAMES], sizeRows));

  const failures = [];
  for (const engine of ENGINE_NAMES) {
    for (const record of runs[engine]) {
      if (record.error) failures.push(`${engine} run ${record.run + 1}: ${record.error}`);
      else if (!record.verified) failures.push(`${engine} run ${record.run + 1}: verification failed`);
    }
  }
  if (failures.length) {
    console.log('\n=== Failures ===');
    for (const failure of failures) console.log('- ' + failure);
  } else {
    console.log('\nAll runs verified (write/read/scan/mixed/delete round-trips consistent).');
  }

  const payload = {
    generatedAt: new Date().toISOString(),
    environment: {
      node: nodeVersion,
      platform: process.platform,
      arch: process.arch,
      cpus: os.cpus().length,
      cpuModel: os.cpus()[0] && os.cpus()[0].model,
      totalMemory: os.totalmem(),
      spectre: spectreVersion,
      spectreNative: hasNativeEngine,
      spectrePackage: SPECTRE_PACKAGE,
      sqlite: sqliteVersion,
      lmdb: lmdbVersion,
    },
    config: {
      n: N,
      reads: READS,
      mixed: MIXED,
      batchSize: BATCH_SIZE,
      runs: RUNS,
      valueBytes: workload.valueBytes,
      durable: DURABLE,
      sqlitePragmas: { journal_mode: 'WAL', synchronous: DURABLE ? 'FULL' : 'NORMAL', temp_store: 'MEMORY' },
      spectreOptions: { autoSave: 0, backup: false, cache: true, durability: DURABLE ? 'durable' : 'process' },
      lmdbOptions: { compression: false },
    },
    summary,
    runs,
    failures,
    totalWallTimeMs: totalMs,
  };

  fs.mkdirSync(RESULTS_DIR, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const prefix = DURABLE ? 'bench-durable-' : 'bench-';
  const file = path.join(RESULTS_DIR, `${prefix}${stamp}.json`);
  fs.writeFileSync(file, JSON.stringify(payload, null, 2));
  fs.writeFileSync(path.join(RESULTS_DIR, `${prefix}latest.json`), JSON.stringify(payload, null, 2));
  console.log(`\nResults written to ${path.relative(ROOT, file)}`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
