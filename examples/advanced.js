'use strict';

const { Database } = require('spectre.db');

async function encryptionExample() {
  console.log('--- Encryption ---');

  const db = new Database('./data/secure', {
    encryptionKey: 'my-super-secret-passphrase',
  });

  await db.ready;

  db.set('user.token',    'discord-bot-token-abc123');
  db.set('user.password', 'hunter2');
  db.set('user.name',     'Alice');

  console.log('token  (decrypted via get):', db.get('user.token'));
  console.log('name   (plaintext):', db.get('user.name'));

  await db.close();
  console.log('');
}

async function tableExample() {
  console.log('--- Tables ---');

  const db = new Database('./data/tables');
  await db.ready;

  const guilds = db.table('guilds');
  const users  = db.table('users');

  guilds.set('123.prefix', '!');
  guilds.set('123.lang',   'en');
  guilds.set('456.prefix', '?');

  users.set('99.xp',    0);
  users.set('99.level', 1);

  await guilds.transaction(async (tx) => {
    tx.set('123.prefix', '$');
    tx.set('123.modRole', 'moderator');
  });

  console.log('guild prefix:', guilds.get('123.prefix'));
  console.log('guild count:', guilds.count());
  console.log('user xp:', users.get('99.xp'));

  await db.close();
  console.log('');
}

async function compactionExample() {
  console.log('--- Compaction ---');

  const db = new Database('./data/compact', {
    compactThreshold: 5,
    compactInterval:  1000,
  });

  await db.ready;

  db.on('save', (stats) => {
    console.log('compacted — walOps reset, fileSize:', stats.fileSize, 'bytes');
  });

  for (let i = 0; i < 10; i++) {
    db.set(`counter.${i}`, i * 10);
  }

  await db.save();

  console.log('stats after manual save:', db.getStats());
  await db.close();
  console.log('');
}

async function main() {
  await encryptionExample();
  await tableExample();
  await compactionExample();
}

main().catch(console.error);