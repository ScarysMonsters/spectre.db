'use strict';

const path = require('path');
const fs = require('fs');

const { Database, hasNativeEngine, version } = require('../index.js');

async function main() {
  const root = path.join(__dirname, '..', '.testdata');
  fs.mkdirSync(root, { recursive: true });
  const dir = fs.mkdtempSync(path.join(root, 'smoke-'));
  const dbPath = path.join(dir, 'test.db');

  console.log('@sexfy/spectre.db', version, '— engine:', hasNativeEngine ? 'native' : 'fallback');

  const db = new Database(dbPath, { format: 'v2' });
  await db.ready;

  db.set('user:1', { name: 'Alice', age: 30, tags: ['a', 'b'] });
  db.set('user:2', { name: 'Bob', age: 25 });
  db.set('config.debug', true);

  console.log('get user:1 ->', JSON.stringify(db.get('user:1')));
  console.log('has user:2 ->', db.has('user:2'));
  console.log('get missing ->', db.get('nope'));
  console.log('get branch config ->', JSON.stringify(db.get('config')));
  console.log('get leaf-field config.debug ->', db.get('config.debug'));

  const all = db.all();
  console.log('all() entries:', all.length);
  console.log('all(user:) entries:', db.startsWith('user:').length);

  await db.transaction([
    { type: 'set', key: 'batch.a', value: 1 },
    { type: 'set', key: 'batch.b', value: { x: 2 } },
    { type: 'delete', key: 'user:2' },
  ]);
  console.log('after batch, user:2 ->', db.get('user:2'));
  console.log('batch.a ->', db.get('batch.a'));

  const page = db.cursor('user:', { limit: 10 });
  console.log('cursor rows:', page.rows.length, 'done:', page.done);

  let scanned = 0;
  for await (const entry of db.scan({ prefix: 'batch.' })) {
    if (entry) scanned++;
  }
  console.log('scan entries:', scanned);

  if (hasNativeEngine) {
    db.setRaw('raw:1', Buffer.from([0, 1, 2, 3]));
    console.log('raw roundtrip:', db.getRaw('raw:1').length, 'bytes');
  }

  console.log('stats:', JSON.stringify(db.stats()).slice(0, 160) + '...');

  await db.close();

  const db2 = new Database(dbPath, {});
  await db2.ready;
  console.log('reopened, files:', fs.readdirSync(dir).join(', '));
  console.log('persisted user:1 ->', JSON.stringify(db2.get('user:1')));
  await db2.close();

  const db3 = new Database(path.join(dir, 'protect.db'), {});
  await db3.ready;
  try {
    db3.set('__proto__.x', '1');
    console.log('prototype pollution: NOT blocked');
  } catch (err) {
    console.log('prototype pollution blocked:', err.message);
  }
  await db3.close();

  fs.rmSync(dir, { recursive: true, force: true });
  console.log('SMOKE OK');
}

main().catch((err) => {
  console.error('SMOKE FAILED:', err);
  process.exit(1);
});
