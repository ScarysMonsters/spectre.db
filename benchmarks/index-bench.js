'use strict';

const fs = require('fs');
const path = require('path');

const packagePath = process.argv[2];
const dataDir = process.argv[3];

if (!packagePath || !dataDir) {
  console.error('usage: node index-bench.js <package-path> <data-dir>');
  process.exit(1);
}

const { Database, version, hasNativeEngine } = require(packagePath);
const N = Number(process.env.INDEX_N || 100000);
const BATCH = 1000;
const UPDATES = Number(process.env.INDEX_UPDATES || 10000);

function makeValue(i) {
  return {
    status: 's' + (i % 10),
    group: 'g' + (i % 1000),
    region: 'r' + (i % 10),
    tier: 't' + (i % 5),
    score: i % 1000,
    payload: 'x'.repeat(120),
  };
}

const hr = () => process.hrtime.bigint();
const ms = (t) => Number(process.hrtime.bigint() - t) / 1e6;
const keys = Array.from({ length: N }, (_, i) => 'user.' + i);
const values = Array.from({ length: N }, (_, i) => makeValue(i));

function readDirSize(dir) {
  let total = 0;
  try {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) total += readDirSize(full);
      else if (entry.isFile()) total += fs.statSync(full).size;
    }
  } catch {}
  return total;
}

function cleanup(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
}

async function open(name) {
  const dir = path.join(dataDir, name);
  cleanup(dir);
  fs.mkdirSync(dir, { recursive: true });
  const db = new Database(path.join(dir, 'bench'), { autoSave: 0, backup: false });
  await db.ready;
  return { db, dir };
}

async function insertAll(db) {
  const start = hr();
  for (let offset = 0; offset < N; offset += BATCH) {
    const pairs = [];
    const end = Math.min(offset + BATCH, N);
    for (let i = offset; i < end; i++) pairs.push([keys[i], values[i]]);
    db.setMany(pairs);
  }
  return ms(start);
}

(async () => {
  fs.mkdirSync(dataDir, { recursive: true });

  const plain = await open('plain');
  const insertPlainMs = await insertAll(plain.db);
  const plainStats = plain.db.getStats();
  const rssAfterPlain = process.memoryUsage().rss;
  await plain.db.close();
  const plainBytes = readDirSize(plain.dir);
  cleanup(plain.dir);

  const indexed = await open('indexed');
  const indexPaths = ['status', 'group', 'region', 'tier', 'score'];
  for (const p of indexPaths) indexed.db.index(p);
  const insertIndexedMs = await insertAll(indexed.db);
  const indexedStats = indexed.db.getStats();
  const rssAfterIndexed = process.memoryUsage().rss;

  const latencies = new Float64Array(UPDATES);
  const updateStart = hr();
  for (let i = 0; i < UPDATES; i++) {
    const key = keys[i];
    const value = makeValue(i + 1);
    const opStart = process.hrtime.bigint();
    indexed.db.set(key, value);
    latencies[i] = Number(process.hrtime.bigint() - opStart) / 1e3;
  }
  const updateMs = ms(updateStart);
  const rssAfterUpdates = process.memoryUsage().rss;
  const sorted = Array.from(latencies).sort((a, b) => a - b);
  const percentile = (p) => sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))] : 0;

  const lookupStart = hr();
  const lookup = indexed.db.index('status').keys('s3');
  const lookupMs = ms(lookupStart);

  const rangeStart = hr();
  const range = indexed.db.range('score', { gte: 100, lt: 200, limit: 1000 });
  const rangeMs = ms(rangeStart);

  await indexed.db.close();
  const indexedBytes = readDirSize(indexed.dir);
  cleanup(indexed.dir);
  cleanup(dataDir);

  const result = {
    package: packagePath,
    version,
    native: hasNativeEngine,
    n: N,
    updates: UPDATES,
    indexPaths,
    insertPlainMs,
    insertPlainOps: Math.round((N / insertPlainMs) * 1000),
    insertIndexedMs,
    insertIndexedOps: Math.round((N / insertIndexedMs) * 1000),
    updateMs,
    updateOps: Math.round((UPDATES / updateMs) * 1000),
    updateP50Us: percentile(50),
    updateP95Us: percentile(95),
    updateP99Us: percentile(99),
    updateMaxUs: sorted.length ? sorted[sorted.length - 1] : 0,
    rssAfterPlain,
    rssAfterIndexed,
    rssAfterUpdates,
    lookupCount: lookup.length,
    lookupMs,
    rangeCount: range.length,
    rangeMs,
    plainBytes,
    indexedBytes,
    plainEntries: plainStats.entries,
    indexedEntries: indexedStats.entries,
  };
  console.log(JSON.stringify(result, null, 2));
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
