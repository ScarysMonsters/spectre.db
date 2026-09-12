'use strict';

const { Database, hasNativeEngine } = require('spectredb');

async function main() {
  if (!hasNativeEngine) {
    console.log('migrate() requires the native engine — aborting.');
    return;
  }

  const db = new Database('./data/legacy', {
    format: 'json',
  });

  await db.ready;

  db.set('user.1.name', 'Alice');
  db.set('user.1.coins', 100);
  await db.save();

  console.log('--- JSON (v1.1.0) database ---');
  console.log('stats:', db.stats());

  console.log('--- Migrating to v2 binary format ---');
  await db.migrate('v2');
  console.log('after migrate:', db.stats().format);

  db.set('user.2.name', 'Bob');
  await db.save();

  console.log('--- Migrating back to JSON ---');
  await db.migrate('json');
  console.log('after migrate back:', db.stats().format);
  console.log('user.1 still there:', db.get('user.1.name'));

  await db.close();
  console.log('done.');
}

main().catch(console.error);
