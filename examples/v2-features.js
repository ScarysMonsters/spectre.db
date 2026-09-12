'use strict';

const { Database, hasNativeEngine } = require('@sexfy/spectre.db');

async function main() {
  console.log('native engine:', hasNativeEngine);

  const db = new Database('./data/v2-features', {
    format: 'v2',
    compression: 'zstd',
    durability: 'durable',
  });

  await db.ready;

  for (let i = 0; i < 1000; i++) {
    db.set(`user.${i}`, { name: `User ${i}`, status: i % 2 ? 'active' : 'idle', age: 18 + (i % 50) });
  }

  console.log('--- Lazy iteration (scan) ---');
  let scanned = 0;
  for await (const { ID, data } of db.scan({ prefix: 'user.', pageSize: 100, limit: 250 })) {
    scanned++;
    if (scanned === 1) console.log('first row:', ID, data);
  }
  console.log('scanned rows:', scanned);

  console.log('--- Manual cursor ---');
  const page = db.cursor('user.', { limit: 5 });
  console.log('cursor page:', page.rows.length, 'next:', page.cursor, 'done:', page.done);

  if (hasNativeEngine) {
    console.log('--- Secondary index + find + range ---');
    db.index('user.status');
    const active = db.find({ status: 'active' });
    console.log('first active:', active && active.ID);

    const adults = db.range('user.age', { gte: 30, lt: 40, limit: 5 });
    console.log('range results:', adults.length);

    console.log('--- Raw binary values ---');
    db.setRaw('blob:1', Buffer.from('binary payload'));
    console.log('raw:', db.getRaw('blob:1').toString());

    console.log('--- Async compaction ---');
    const status = await db.compactAsync();
    console.log('compactAsync:', status);
  } else {
    console.log('(index/range/raw/compactAsync require the native engine)');
  }

  console.log('--- Stats ---');
  console.log(db.stats());

  await db.close();
  console.log('done.');
}

main().catch(console.error);
