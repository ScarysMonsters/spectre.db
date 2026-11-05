'use strict';

const fs = require('fs');
const path = require('path');

const packagePath = process.argv[2];
const dataDir = process.argv[3];
if (!packagePath || !dataDir) {
  console.error('usage: node prefix-bench.js <package-path> <data-dir>');
  process.exit(1);
}

const { Database, version, hasNativeEngine } = require(packagePath);
const ZONES = 10;
const GROUPS = 100;
const PER_GROUP = Number(process.env.PREFIX_PER_GROUP || 1000);
const N = ZONES * GROUPS * PER_GROUP;
const BATCH = 5000;

function cleanup(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
}

const hr = () => process.hrtime.bigint();
const ms = (t) => Number(process.hrtime.bigint() - t) / 1e6;

function key(zone, group, i) {
  return `zone${zone}.group${group}.key${i}`;
}

(async () => {
  cleanup(dataDir);
  fs.mkdirSync(dataDir, { recursive: true });
  const db = new Database(path.join(dataDir, 'bench'), { autoSave: 0, backup: false });
  await db.ready;

  const start = hr();
  let batch = [];
  for (let z = 0; z < ZONES; z++) {
    for (let g = 0; g < GROUPS; g++) {
      for (let i = 0; i < PER_GROUP; i++) {
        batch.push([key(z, g, i), { z, g, i, v: 'x'.repeat(24) }]);
        if (batch.length >= BATCH) {
          db.setMany(batch);
          batch = [];
        }
      }
    }
  }
  if (batch.length) db.setMany(batch);
  const insertMs = ms(start);

  function scanPrefix(prefix) {
    const t = hr();
    let rows = 0;
    let after = '';
    for (;;) {
      const page = db.cursor(prefix, { after, limit: 2000 });
      rows += page.rows.length;
      if (page.done || !page.cursor) break;
      after = page.cursor;
    }
    return { rows, ms: ms(t) };
  }

  const prefixes = [
    ['zone9.', '10%'],
    ['zone9.group9', '1%'],
    ['zone9.group99.', '0.1%'],
  ];
  const results = {};
  for (const [prefix, selectivity] of prefixes) {
    const r = scanPrefix(prefix);
    results[selectivity] = { prefix, rows: r.rows, ms: r.ms, rowsPerSec: Math.round((r.rows / r.ms) * 1000) };
  }

  await db.close();
  cleanup(dataDir);

  console.log(JSON.stringify({
    package: packagePath,
    version,
    native: hasNativeEngine,
    n: N,
    zones: ZONES,
    groups: GROUPS,
    perGroup: PER_GROUP,
    insertMs,
    insertOps: Math.round((N / insertMs) * 1000),
    results,
  }, null, 2));
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
