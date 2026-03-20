'use strict';

const { Database } = require('spectre.db');

async function main() {
  const db = new Database('./data/mydb', {
    cache:    true,
    autoSave: 5000,
    backup:   true,
  });

  await db.ready;

  db.set('users.1.name',  'Alice');
  db.set('users.1.coins', 0);
  db.set('users.1.roles', ['member']);

  console.log(db.get('users.1.name'));
  console.log(db.has('users.1.coins'));

  db.add('users.1.coins', 150);
  db.sub('users.1.coins', 50);
  console.log('coins:', db.get('users.1.coins'));

  db.push('users.1.roles', 'admin');
  console.log('roles:', db.get('users.1.roles'));

  db.pull('users.1.roles', 'member');
  console.log('roles after pull:', db.get('users.1.roles'));

  db.set('users.2.name',  'Bob');
  db.set('users.2.coins', 500);

  const all = db.all();
  console.log('all entries:', all.length);

  const rich = db.filter((data, id) => id.endsWith('.coins') && data > 100);
  console.log('rich users:', rich);

  const { data, pagination } = db.paginate('users.', 1, 10, 'data', true);
  console.log('page:', data.length, 'total:', pagination.total);

  await db.transaction(async (tx) => {
    tx.set('users.3.name',  'Charlie');
    tx.set('users.3.coins', tx.get('users.1.coins') + 10);
  });
  console.log('tx result:', db.get('users.3.name'), db.get('users.3.coins'));

  try {
    await db.transaction(async (tx) => {
      tx.set('users.4.name', 'Dave');
      throw new Error('Something went wrong');
    });
  } catch {
    console.log('rollback ok — users.4 exists:', db.has('users.4.name'));
  }

  const users = db.table('users');
  console.log('table count:', users.count());
  console.log('table get 1.name:', users.get('1.name'));

  await db.transaction([
    { type: 'set',    key: 'config.debug',   value: true },
    { type: 'set',    key: 'config.version', value: 2    },
    { type: 'delete', key: 'users.3.name'               },
  ]);
  console.log('legacy tx config.debug:', db.get('config.debug'));
  console.log('legacy tx users.3.name (deleted):', db.get('users.3.name'));

  console.log('stats:', db.getStats());

  await db.close();
  console.log('closed.');
}

main().catch(console.error);