# spectre.db - Usage Guide

## Table of Contents

1. [Installation](#installation)
2. [Quick Start](#quick-start)
3. [Basic Operations](#basic-operations)
4. [Advanced Queries](#advanced-queries)
5. [Transactions](#transactions)
6. [Tables](#tables)
7. [Encryption](#encryption)
8. [Multi-Process](#multi-process)
9. [Performance](#performance)
10. [Best Practices](#best-practices)

---

## Installation

```bash
npm install spectre.db@latest
```

**Required:** Node.js 18.0.0 or newer

---

## Quick Start

```javascript
const { Database } = require('spectre.db');

// Create a database
const db = new Database('./data/mydb', {
  cache: true,
  autoSave: 5000,
  backup: true,
});

// Wait for database to be ready
await db.ready;

// Write data
db.set('users.1.name', 'Alice');
db.set('users.1.coins', 100);

// Read data
const name = db.get('users.1.name'); // 'Alice'
const coins = db.get('users.1.coins'); // 100

// Close properly
await db.close();
```

---

## Basic Operations

### set(key, value)

Sets a value for a key.

```javascript
db.set('user.name', 'Alice');
db.set('user.age', 30);
db.set('config.debug', true);
db.set('data.items', [1, 2, 3]);
```

### get(key)

Retrieves a value by key.

```javascript
const name = db.get('user.name'); // 'Alice'
const age = db.get('user.age'); // 30
const missing = db.get('user.nonexistent'); // null
```

### delete(key)

Deletes a value.

```javascript
db.set('temp.data', 'value');
db.delete('temp.data');
db.get('temp.data'); // null
```

### has(key)

Checks if a key exists.

```javascript
db.set('user.name', 'Alice');
db.has('user.name'); // true
db.has('user.age'); // false
```

### add(key, number)

Adds a number to an existing value.

```javascript
db.set('counter', 10);
db.add('counter', 5); // 15
db.add('counter', -3); // 12
```

### sub(key, number)

Subtracts a number from an existing value.

```javascript
db.set('counter', 10);
db.sub('counter', 5); // 5
```

### push(key, value)

Adds a value to an array.

```javascript
db.set('items', [1, 2, 3]);
db.push('items', 4); // [1, 2, 3, 4]
```

### pull(key, predicate)

Removes a value from an array.

```javascript
db.set('items', [1, 2, 3, 4]);
db.pull('items', 3); // true
db.get('items'); // [1, 2, 4]

// With predicate function
db.pull('items', (item) => item > 2);
```

---

## Advanced Queries

### all()

Retrieves all entries.

```javascript
db.set('user.1.name', 'Alice');
db.set('user.1.age', 30);
db.set('user.2.name', 'Bob');

const all = db.all();
// [
//   { ID: 'user.1.name', data: 'Alice' },
//   { ID: 'user.1.age', data: 30 },
//   { ID: 'user.2.name', data: 'Bob' }
// ]
```

### startsWith(prefix)

Retrieves entries starting with a prefix.

```javascript
const users = db.startsWith('user.');
// [
//   { ID: 'user.1.name', data: 'Alice' },
//   { ID: 'user.1.age', data: 30 },
//   { ID: 'user.2.name', data: 'Bob' }
// ]
```

### filter(predicate)

Filters entries with a function.

```javascript
const richUsers = db.filter((data, id) => {
  return id.endsWith('.coins') && data > 100;
});
```

### find(predicate)

Finds a specific entry.

```javascript
const user = db.find((data, id) => {
  return id === 'user.1.name' && data === 'Alice';
});
```

### paginate(prefix, page, limit, sortBy, sortDesc)

Paginates results.

```javascript
const page1 = db.paginate('user.', 1, 10, 'data', true);
// {
//   data: [...],
//   pagination: {
//     page: 1,
//     limit: 10,
//     total: 25,
//     pages: 3,
//     hasNext: true,
//     hasPrev: false
//   }
// }
```

---

## Transactions

Transactions guarantee atomicity of operations.

### Function-based transaction

```javascript
await db.transaction(async (tx) => {
  const balance = tx.get('user.balance') ?? 0;
  if (balance < amount) {
    throw new Error('Insufficient balance');
  }
  tx.set('user.balance', balance - amount);
  tx.set('user.lastTransaction', Date.now());
});
```

### Array-based transaction (legacy)

```javascript
await db.transaction([
  { type: 'set', key: 'config.debug', value: true },
  { type: 'add', key: 'stats.logins', value: 1 },
  { type: 'delete', key: 'cache.tmp' },
]);
```

### Automatic rollback

```javascript
try {
  await db.transaction(async (tx) => {
    tx.set('user.balance', 100);
    throw new Error('Simulated error');
  });
} catch (err) {
  // Transaction is automatically rolled back
  db.get('user.balance'); // null or previous value
}
```

---

## Tables

Tables allow you to create namespaces.

### Create a table

```javascript
const users = db.table('users');
const config = db.table('config');
```

### Table operations

```javascript
// Keys are automatically prefixed
users.set('1.name', 'Alice'); // stored as "users.1.name"
users.get('1.name'); // 'Alice'
users.has('1.name'); // true
users.delete('1.name'); // true
```

### Count entries

```javascript
users.set('1.name', 'Alice');
users.set('2.name', 'Bob');
users.count(); // 2
```

### Clear table

```javascript
await users.clear();
users.count(); // 0
```

### Table transactions

```javascript
await users.transaction(async (tx) => {
  tx.add('1.coins', 100);
  tx.set('1.lastDaily', Date.now());
});
```

---

## Encryption

spectre.db automatically encrypts sensitive keys.

### Sensitive keys

Keys containing these words are automatically encrypted:
- `password`
- `secret`
- `token`
- `apikey`
- `api_key`
- `private`

```javascript
const db = new Database('./data/secure', {
  encryptionKey: 'your-32-byte-encryption-key-here',
});

// These keys are automatically encrypted
db.set('user.password', 'secret123');
db.set('api.token', 'abc123');
db.set('auth.secret', 'xyz789');

// These keys are not encrypted
db.set('user.name', 'Alice');
db.set('config.debug', true);
```

### Backup encryption

```javascript
const db = new Database('./data/secure', {
  encryptionKey: 'your-32-byte-encryption-key-here',
  encryptBackups: true, // Encrypt backup files
  backup: true,
  backupCount: 3,
});

await db.save(); // Backups will be encrypted
```

---

## Multi-Process

spectre.db supports multi-process access with automatic file locking.

### Concurrent access

```javascript
// Process 1
const db1 = new Database('./shared.db');
await db1.ready;
db1.set('counter', 1);

// Process 2 (will wait for lock)
const db2 = new Database('./shared.db');
await db2.ready; // Waits for process 1 to release lock
```

### Lock error handling

```javascript
const db = new Database('./shared.db');

try {
  await db.ready;
} catch (err) {
  if (err.code === 7001) { // LOCK_TIMEOUT
    console.error('Failed to acquire lock');
  }
}
```

---

## Performance

### Optimization for Discord bots

```javascript
const db = new Database('./data/discord', {
  cache: true,
  maxCacheSize: 10000, // Larger cache
  cacheTTL: 60000, // 1 minute TTL
  autoSave: 10000, // Save every 10 seconds
  compactThreshold: 1000, // Compact after 1000 operations
});
```

### Performance test

```javascript
const db = new Database('./data/perf', {
  cache: true,
  maxCacheSize: 10000,
});

// Write 100k entries
console.time('set');
for (let i = 0; i < 100000; i++) {
  db.set(`key${i}`, i);
}
console.timeEnd('set'); // < 10 seconds

// Read 100k entries
console.time('get');
for (let i = 0; i < 100000; i++) {
  db.get(`key${i}`);
}
console.timeEnd('get'); // < 10 seconds
```

### Pagination for large datasets

```javascript
// Instead of loading everything
const all = db.all(); // Can be slow with 100k+ entries

// Use pagination
const page1 = db.paginate('user.', 1, 100);
const page2 = db.paginate('user.', 2, 100);
```

---

## Best Practices

### 1. Always close the database

```javascript
process.on('SIGINT', async () => {
  await db.close();
  process.exit(0);
});
```

### 2. Use transactions for complex operations

```javascript
// ❌ Bad
db.set('user.balance', balance - amount);
db.set('user.lastTransaction', Date.now());

// ✅ Good
await db.transaction(async (tx) => {
  tx.set('user.balance', balance - amount);
  tx.set('user.lastTransaction', Date.now());
});
```

### 3. Handle errors

```javascript
db.on('error', (err) => {
  console.error('Database error:', err);
});

db.on('rollback', ({ error }) => {
  console.error('Transaction rolled back:', error);
});
```

### 4. Use cache efficiently

```javascript
// Preload frequently used keys
const db = new Database('./data/db', {
  cache: true,
  warmKeys: ['config.settings', 'user.1.profile'],
});
```

### 5. Validate input

```javascript
function setUserData(userId, data) {
  // Validate data
  if (!data.name || typeof data.name !== 'string') {
    throw new Error('Invalid name');
  }

  db.set(`user.${userId}.name`, data.name);
  db.set(`user.${userId}.age`, data.age);
}
```

### 6. Use tables for organization

```javascript
const users = db.table('users');
const config = db.table('config');
const cache = db.table('cache');

// Clearer than:
// db.set('users.1.name', 'Alice');
// db.set('config.debug', true);
// db.set('cache.tmp', 'value');
```

---

## Complete Examples

### Discord bot with economy

```javascript
const { Client, GatewayIntentBits } = require('discord.js');
const { Database } = require('spectre.db');

const client = new Client({
  intents: [GatewayIntentBits.Guilds, GatewayIntentBits.GuildMessages],
});

const db = new Database('./data/discord', {
  cache: true,
  autoSave: 10000,
  backup: true,
  encryptionKey: process.env.DB_KEY,
});

client.on('messageCreate', async (message) => {
  if (message.content === '!daily') {
    const userId = message.author.id;
    const lastKey = `users.${userId}.lastDaily`;
    const last = db.get(lastKey) ?? 0;
    const now = Date.now();
    const cooldown = 24 * 60 * 60 * 1000;

    if (now - last < cooldown) {
      const remaining = Math.ceil((cooldown - (now - last)) / 3600000);
      return message.reply(`Come back in ${remaining}h for your daily reward!`);
    }

    await db.transaction(async (tx) => {
      const coins = (tx.get(`users.${userId}.coins`) ?? 0) + 100;
      tx.set(`users.${userId}.coins`, coins);
      tx.set(lastKey, now);
    });

    message.reply('You received 100 coins!');
  }
});

process.on('SIGINT', async () => {
  await db.close();
  process.exit(0);
});

client.login(process.env.TOKEN);
```

### Backend application with cache

```javascript
const express = require('express');
const { Database } = require('spectre.db');

const app = express();
const db = new Database('./data/backend', {
  cache: true,
  maxCacheSize: 10000,
  cacheTTL: 300000, // 5 minutes
});

app.get('/api/users/:id', async (req, res) => {
  const userId = req.params.id;
  const user = db.get(`users.${userId}`);

  if (!user) {
    return res.status(404).json({ error: 'User not found' });
  }

  res.json(user);
});

app.post('/api/users/:id', async (req, res) => {
  const userId = req.params.id;
  const data = req.body;

  await db.transaction(async (tx) => {
    tx.set(`users.${userId}`, data);
    tx.set(`users.${userId}.updatedAt`, Date.now());
  });

  res.json({ success: true });
});

process.on('SIGINT', async () => {
  await db.close();
  process.exit(0);
});

app.listen(3000);
```

---

## Troubleshooting

### Error: "Lock acquisition timeout"

The file lock could not be acquired. Check that no other process is using the database.

### Error: "Value too large"

The value you're trying to store exceeds 10MB. Split your data into multiple keys.

### Error: "Circular reference detected"

You're trying to store an object with circular references. Use a data structure without cycles.

### Slow performance

- Increase `maxCacheSize`
- Use `warmKeys` to preload frequent keys
- Use pagination instead of `all()`

---

## Support

For more help, check:
- [GitHub Issues](https://github.com/ScarysMonsters/spectre.db/issues)
- [Main documentation](./README.md)
