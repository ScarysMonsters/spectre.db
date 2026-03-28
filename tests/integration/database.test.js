'use strict';

const { Database } = require('../../index');
const fsp = require('fs').promises;
const path = require('path');

describe('Database Integration', () => {
  const dbPath = path.join(__dirname, 'test.db');

  beforeEach(async () => {
    // Clean up test files
    try {
      await fsp.unlink(dbPath + '.snapshot');
      await fsp.unlink(dbPath + '.wal');
      await fsp.unlink(dbPath + '.lock');
      for (let i = 1; i <= 3; i++) {
        await fsp.unlink(`${dbPath}.snapshot.${i}.bak`).catch(() => {});
      }
    } catch (err) {
      // Ignore errors
    }
  });

  afterEach(async () => {
    // Clean up after tests
    try {
      await fsp.unlink(dbPath + '.snapshot');
      await fsp.unlink(dbPath + '.wal');
      await fsp.unlink(dbPath + '.lock');
      for (let i = 1; i <= 3; i++) {
        await fsp.unlink(`${dbPath}.snapshot.${i}.bak`).catch(() => {});
      }
    } catch (err) {
      // Ignore errors
    }
  });

  describe('Basic Operations', () => {
    it('should set and get values', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('user.name', 'Alice');
      expect(db.get('user.name')).toBe('Alice');

      await db.close();
    });

    it('should persist data across restarts', async () => {
      const db1 = new Database(dbPath);
      await db1.ready;

      db1.set('user.name', 'Alice');
      await db1.close();

      const db2 = new Database(dbPath);
      await db2.ready;

      expect(db2.get('user.name')).toBe('Alice');
      await db2.close();
    });

    it('should handle transactions', async () => {
      const db = new Database(dbPath);
      await db.ready;

      await db.transaction(async (tx) => {
        tx.set('user.name', 'Alice');
        tx.set('user.age', 30);
      });

      expect(db.get('user.name')).toBe('Alice');
      expect(db.get('user.age')).toBe(30);

      await db.close();
    });

    it('should rollback on transaction error', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('user.name', 'Bob');

      try {
        await db.transaction(async (tx) => {
          tx.set('user.name', 'Alice');
          throw new Error('Test error');
        });
      } catch (err) {
        // Expected
      }

      expect(db.get('user.name')).toBe('Bob');

      await db.close();
    });

    it('should handle delete operations', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('user.name', 'Alice');
      expect(db.get('user.name')).toBe('Alice');

      const deleted = db.delete('user.name');
      expect(deleted).toBe(true);
      expect(db.get('user.name')).toBe(null);

      await db.close();
    });

    it('should handle has operations', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('user.name', 'Alice');
      expect(db.has('user.name')).toBe(true);
      expect(db.has('user.age')).toBe(false);

      await db.close();
    });

    it('should handle add operations', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('counter', 10);
      const result = db.add('counter', 5);
      expect(result).toBe(15);
      expect(db.get('counter')).toBe(15);

      await db.close();
    });

    it('should handle sub operations', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('counter', 10);
      const result = db.sub('counter', 5);
      expect(result).toBe(5);
      expect(db.get('counter')).toBe(5);

      await db.close();
    });

    it('should handle push operations', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('items', [1, 2, 3]);
      const result = db.push('items', 4);
      expect(result).toBe(4);
      expect(db.get('items')).toEqual([1, 2, 3, 4]);

      await db.close();
    });

    it('should handle pull operations', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('items', [1, 2, 3, 4]);
      const result = db.pull('items', 3);
      expect(result).toBe(true);
      expect(db.get('items')).toEqual([1, 2, 4]);

      await db.close();
    });
  });

  describe('Query Methods', () => {
    it('should get all entries', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('user.1.name', 'Alice');
      db.set('user.1.age', 30);
      db.set('user.2.name', 'Bob');

      const all = db.all();
      expect(all.length).toBe(3);

      await db.close();
    });

    it('should filter entries', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('user.1.coins', 100);
      db.set('user.2.coins', 200);
      db.set('user.3.coins', 300);

      const filtered = db.filter((data, id) => data > 100);
      expect(filtered.length).toBe(2);

      await db.close();
    });

    it('should find an entry', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('user.1.name', 'Alice');
      db.set('user.2.name', 'Bob');

      const found = db.find((data, id) => data === 'Bob');
      expect(found).not.toBeNull();
      expect(found.data).toBe('Bob');

      await db.close();
    });

    it('should get entries starting with prefix', async () => {
      const db = new Database(dbPath);
      await db.ready;

      db.set('user.1.name', 'Alice');
      db.set('user.2.name', 'Bob');
      db.set('config.debug', true);

      const users = db.startsWith('user.');
      expect(users.length).toBe(2);

      await db.close();
    });

    it('should paginate entries', async () => {
      const db = new Database(dbPath);
      await db.ready;

      for (let i = 1; i <= 20; i++) {
        db.set(`user.${i}.name`, `User${i}`);
      }

      const page1 = db.paginate('user.', 1, 5);
      expect(page1.data.length).toBe(5);
      expect(page1.pagination.total).toBe(20);
      expect(page1.pagination.pages).toBe(4);

      await db.close();
    });
  });

  describe('Table Operations', () => {
    it('should create and use tables', async () => {
      const db = new Database(dbPath);
      await db.ready;

      const users = db.table('users');
      users.set('1.name', 'Alice');
      users.set('2.name', 'Bob');

      expect(users.get('1.name')).toBe('Alice');
      expect(users.get('2.name')).toBe('Bob');
      expect(db.get('users.1.name')).toBe('Alice');

      await db.close();
    });

    it('should count table entries', async () => {
      const db = new Database(dbPath);
      await db.ready;

      const users = db.table('users');
      users.set('1.name', 'Alice');
      users.set('2.name', 'Bob');

      expect(users.count()).toBe(2);

      await db.close();
    });

    it('should clear table entries', async () => {
      const db = new Database(dbPath);
      await db.ready;

      const users = db.table('users');
      users.set('1.name', 'Alice');
      users.set('2.name', 'Bob');

      await users.clear();

      expect(users.count()).toBe(0);

      await db.close();
    });
  });

  describe('Multi-Process Support', () => {
    it('should prevent concurrent access', async () => {
      const db1 = new Database(dbPath);
      await db1.ready;

      const db2 = new Database(dbPath);

      await expect(db2.ready).rejects.toThrow();

      await db1.close();
    });
  });

  describe('Backup Encryption', () => {
    it('should encrypt backups when enabled', async () => {
      const db = new Database(dbPath, {
        encryptBackups: true,
        encryptionKey: 'test-key-32-bytes-long-1234567890',
      });
      await db.ready;

      db.set('user.name', 'Alice');
      await db.save();

      // Check that backup is encrypted
      const backupPath = dbPath + '.snapshot.1.bak';
      const content = await fsp.readFile(backupPath);

      // Content should not be plain JSON
      expect(content.toString('utf8')).not.toContain('Alice');

      await db.close();
    });
  });

  describe('Performance', () => {
    it('should handle 100k operations efficiently', async () => {
      const db = new Database(dbPath);
      await db.ready;

      const start = Date.now();

      // Set 100k entries
      for (let i = 0; i < 100000; i++) {
        db.set(`key${i}`, i);
      }

      const setTime = Date.now() - start;
      console.log(`Set 100k entries in ${setTime}ms`);

      // Get 100k entries
      const getStart = Date.now();
      for (let i = 0; i < 100000; i++) {
        db.get(`key${i}`);
      }
      const getTime = Date.now() - getStart;
      console.log(`Get 100k entries in ${getTime}ms`);

      // Target: < 100ms per operation for Discord bot
      // For 100k operations, this means < 10 seconds total
      expect(setTime).toBeLessThan(10000);
      expect(getTime).toBeLessThan(10000);

      await db.close();
    }, 30000);
  });
});
