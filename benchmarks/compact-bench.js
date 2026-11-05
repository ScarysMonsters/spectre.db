'use strict';

const fs = require('fs');
const path = require('path');

const packagePath = process.argv[2];
const dataDir = process.argv[3];
if (!packagePath || !dataDir) {
  console.error('usage: node compact-bench.js <package-path> <data-dir>');
  process.exit(1);
}

const { Database, version, hasNativeEngine } = require(packagePath);
const N = Number(process.env.COMPACT_N || 50000);
const VALUE = 'x'.repeat(Number(process.env.COMPACT_VALUE || 4096));
const BATCH = 1000;

function cleanup(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
}

function dirSize(dir) {
  let total = 0;
  try {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) total += dirSize(full);
      else if (entry.isFile()) total += fs.statSync(full).size;
    }
  } catch {}
  return total;
}

const hr = () => process.hrtime.bigint();
const ms = (t) => Number(process.hrtime.bigint() - t) / 1e6;

(async () => {
  cleanup(dataDir);
  fs.mkdirSync(dataDir, { recursive: true });
  const db = new Database(path.join(dataDir, 'bench'), {
    autoSave: 0,
    backup: false,
    legacyThreshold: 1,
  });
  await db.ready;

  const insertStart = hr();
  let batch = [];
  for (let i = 0; i < N; i++) {
    batch.push(['user.' + i, VALUE]);
    if (batch.length >= BATCH) {
      db.setMany(batch);
      batch = [];
    }
  }
  if (batch.length) db.setMany(batch);
  const insertMs = ms(insertStart);
  const rssBefore = process.memoryUsage().rss;
  const diskBefore = dirSize(dataDir);

  let peak = rssBefore;
  const timer = setInterval(() => {
    const rss = process.memoryUsage().rss;
    if (rss > peak) peak = rss;
  }, 10);
  const compactStart = hr();
  await db.compactAsync();
  const compactMs = ms(compactStart);
  clearInterval(timer);
  const rssAfter = process.memoryUsage().rss;
  const diskAfter = dirSize(dataDir);
  const stats = db.stats();

  await db.close();
  cleanup(dataDir);

  console.log(JSON.stringify({
    package: packagePath,
    version,
    native: hasNativeEngine,
    n: N,
    valueBytes: VALUE.length,
    insertMs,
    insertOps: Math.round((N / insertMs) * 1000),
    compactMs,
    rssBefore,
    rssAfter,
    rssPeak: peak,
    rssPeakDelta: peak - rssBefore,
    diskBefore,
    diskAfter,
    entries: stats.entries,
  }, null, 2));
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
