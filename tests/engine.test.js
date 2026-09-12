'use strict';

const path = require('path');
const fs = require('fs');
const { Database, Table, hasNativeEngine } = require('..');

const DATA = path.join(__dirname, '..', '.testdata');

function fresh(name) {
  const dir = path.join(DATA, `${name}-${Date.now()}-${Math.floor(Math.random() * 1e9)}`);
  fs.mkdirSync(dir, { recursive: true });
  return { dir, dbPath: path.join(dir, 'test.db') };
}

function cleanup(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
}

const NATIVE = hasNativeEngine;
const D = NATIVE ? describe : describe.skip;
const T = NATIVE ? test : test.skip;

afterAll(() => {
  fs.rmSync(DATA, { recursive: true, force: true });
});


describe('basic CRUD (both engines)', () => {
  test('set/get/has/delete roundtrip', async () => {
    const { dir, dbPath } = fresh('crud');
    const db = new Database(dbPath);
    await db.ready;

    await db.set('name', 'spectre');
    await db.set('count', 42);
    await db.set('items', ['a', 'b']);
    await db.set('user', { name: 'alice', age: 30 });

    expect(await db.get('name')).toBe('spectre');
    expect(await db.get('count')).toBe(42);
    expect(await db.get('items')).toEqual(['a', 'b']);
    expect(await db.get('user')).toEqual({ name: 'alice', age: 30 });
    expect(await db.get('missing')).toBeNull();
    expect(await db.has('name')).toBe(true);
    expect(await db.has('missing')).toBe(false);

    expect(await db.delete('count')).toBe(true);
    expect(await db.delete('count')).toBe(false);
    expect(await db.get('count')).toBeNull();
    await db.close();
    cleanup(dir);
  });

  test('dot-path nesting and branch semantics', async () => {
    const { dir, dbPath } = fresh('dots');
    const db = new Database(dbPath);
    await db.ready;

    await db.set('user.1', { name: 'alice', age: 30 });

    expect(await db.get('user.1')).toEqual({ name: 'alice', age: 30 });

    expect(await db.get('user.1.name')).toBe('alice');
    expect(await db.get('user.1.age')).toBe(30);
    expect(await db.get('user.1.missing')).toBeNull();


    await db.set('user.1.email', 'a@x.io');
    expect(await db.get('user.1')).toEqual({ name: 'alice', age: 30, email: 'a@x.io' });


    await db.set('config', { deep: { a: 1, b: 2 } });
    await db.set('config', 'plain');
    expect(await db.get('config')).toBe('plain');
    expect(await db.get('config.deep')).toBeNull();


    await db.set('tree.x', 1);
    await db.set('tree.y', 2);
    expect(await db.get('tree')).toEqual({ x: 1, y: 2 });
    await db.close();
    cleanup(dir);
  });

  test('add/sub/push/pull', async () => {
    const { dir, dbPath } = fresh('ops');
    const db = new Database(dbPath);
    await db.ready;
    expect(await db.add('counter', 5)).toBe(5);
    expect(await db.add('counter', 2)).toBe(7);
    expect(await db.sub('counter', 3)).toBe(4);
    expect(await db.push('list', 'a')).toBe(1);
    expect(await db.push('list', 'b')).toBe(2);
    expect(await db.get('list')).toEqual(['a', 'b']);
    expect(await db.pull('list', 'a')).toBe(true);
    expect(await db.pull('list', 'zzz')).toBe(false);
    expect(() => db.add('counter', NaN)).toThrow();
    await db.close();
    cleanup(dir);
  });

  test('key validation errors', async () => {
    const { dir, dbPath } = fresh('keys');
    const db = new Database(dbPath);
    await db.ready;
    const expectCode = (fn, code) => {
      try {
        fn();
        throw new Error('expected throw');
      } catch (e) {
        if (NATIVE) expect(e.code).toBe(code);
        else expect(e).toBeTruthy();
      }
    };
    expectCode(() => db.set('', 'x'), 1000);
    expectCode(() => db.set('__proto__.x', 'x'), 1003);
    expectCode(() => db.get('a..b'), 1002);
    await db.close();
    cleanup(dir);
  });
});


describe('scans (both engines)', () => {
  test('all/filter/find/startsWith/paginate', async () => {
    const { dir, dbPath } = fresh('scan');
    const db = new Database(dbPath);
    await db.ready;
    await db.set('user.1', { name: 'alice', age: 30 });
    await db.set('user.2', { name: 'bob', age: 25 });
    await db.set('post.1', { title: 'hello' });

    const all = await db.all();
    expect(all.map((e) => e.ID).sort()).toEqual(
      ['post.1.title', 'user.1.age', 'user.1.name', 'user.2.age', 'user.2.name'].sort()
    );

    const users = await db.startsWith('user.');
    expect(users.length).toBe(4);

    expect(await db.filter((d) => d === 'bob')).toEqual([{ ID: 'user.2.name', data: 'bob' }]);
    expect(await db.find((d) => d === 'alice')).toEqual({ ID: 'user.1.name', data: 'alice' });

    const pg = await db.paginate('user.', 1, 2);
    expect(pg.pagination.total).toBe(4);
    expect(pg.pagination.pages).toBe(2);
    expect(pg.data.length).toBe(2);
    await db.close();
    cleanup(dir);
  });

  test('table API', async () => {
    const { dir, dbPath } = fresh('table');
    const db = new Database(dbPath);
    await db.ready;
    const users = db.table('users');
    await users.set('1', { name: 'alice' });
    await users.set('2', { name: 'bob' });
    expect(await users.count()).toBe(2);
    expect(await users.get('1')).toEqual({ name: 'alice' });
    expect(await db.get('users.1.name')).toBe('alice');
    expect(await users.all().every((e) => e.ID.startsWith('users.'))).toBe(true);
    await users.clear();
    expect(await users.count()).toBe(0);
    await db.close();
    cleanup(dir);
  });
});


describe('transactions (both engines)', () => {
  test('array form is atomic and returns results', async () => {
    const { dir, dbPath } = fresh('txarr');
    const db = new Database(dbPath);
    await db.ready;
    await db.set('keep', 1);
    const results = await db.transaction([
      { type: 'set', key: 'a', value: 1 },
      { type: 'set', key: 'b', value: { x: 2 } },
      { type: 'delete', key: 'keep' },
    ]);
    expect(results[0]).toBe(1);
    expect(results[2]).toBe(true);
    expect(await db.get('a')).toBe(1);
    expect(await db.get('b')).toEqual({ x: 2 });
    expect(await db.get('keep')).toBeNull();
    await db.close();
    cleanup(dir);
  });

  test('function form stages then commits', async () => {
    const { dir, dbPath } = fresh('txfn');
    const db = new Database(dbPath);
    await db.ready;
    await db.set('n', 1);
    const out = await db.transaction((tx) => {
      tx.set('a', 'x');
      tx.set('b', 2);
      tx.delete('n');
      return 'done';
    });
    expect(out).toBe('done');
    expect(await db.get('a')).toBe('x');
    expect(await db.get('n')).toBeNull();
    await db.close();
    cleanup(dir);
  });

  test('rollback on throw leaves store untouched', async () => {
    const { dir, dbPath } = fresh('txrb');
    const db = new Database(dbPath);
    await db.ready;
    await db.set('untouched', 'yes');
    await expect(
      db.transaction((tx) => {
        tx.set('temp', 'value');
        throw new Error('boom');
      })
    ).rejects.toThrow('boom');
    expect(await db.get('temp')).toBeNull();
    expect(await db.get('untouched')).toBe('yes');
    await db.close();
    cleanup(dir);
  });

  test('async transaction functions rejected by native engine', async () => {
    if (!NATIVE) return;
    const { dir, dbPath } = fresh('txasync');
    const db = new Database(dbPath);
    await db.ready;
    await expect(
      db.transaction(async (tx) => {
        tx.set('a', 1);
      })
    ).rejects.toThrow(/async transaction/);
    expect(await db.get('a')).toBeNull();
    await db.close();
    cleanup(dir);
  });
});


describe('persistence', () => {
  T('survives crash without compaction (WAL replay)', async () => {
    const { dir, dbPath } = fresh('crash');
    {
      const db = new Database(dbPath);
      await db.ready;
      await db.set('a', 1);
      await db.set('b', { deep: true });
      await db.delete('a');
      await db.compact();
      await db.set('c', 3);

      fs.writeFileSync(path.join(dir, 'test.lock'), '999999999\n');
    }
    const db2 = new Database(dbPath);
    await db2.ready;
    expect(db2.get('a')).toBeNull();
    expect(db2.get('b')).toEqual({ deep: true });
    expect(db2.get('c')).toBe(3);
    await db2.close();
    cleanup(dir);
  });

  T('close() auto-compacts and WAL resets', async () => {
    const { dir, dbPath } = fresh('closec');
    let db = new Database(dbPath);
    await db.ready;
    await db.set('x', 1);
    await db.set('y', 2);
    const stBefore = await db.getStats();
    expect(stBefore.walOps).toBe(2);
    await db.close();

    db = new Database(dbPath);
    await db.ready;
    const st = await db.getStats();
    expect(st.walOps).toBe(0);
    expect(st.fileSize).toBeGreaterThan(0);
    expect(await db.get('x')).toBe(1);
    await db.close();
    cleanup(dir);
  });

  T('compact() writes binary snapshot (v2 default)', async () => {
    const { dir, dbPath } = fresh('compact');
    const db = new Database(dbPath);
    await db.ready;
    for (let i = 0; i < 100; i++) db.set(`k${i}`, { i, pad: 'x'.repeat(20) });
    await db.compact();
    const st = await db.getStats();
    expect(st.walOps).toBe(0);
    expect(st.format).toBe('v2');
    expect(fs.existsSync(path.join(dir, 'test.spdb'))).toBe(true);

    const head = fs.readFileSync(path.join(dir, 'test.spdb')).subarray(0, 8).toString('ascii');
    expect(head).toBe('SPDBSNAP');
    await db.close();
    cleanup(dir);
  });
});


describe('formats', () => {
  T('auto-detects existing JSON v1 databases', async () => {
    const { dir, dbPath } = fresh('detect');

    fs.writeFileSync(path.join(dir, 'test.snapshot'), JSON.stringify({ user: { name: 'legacy' } }));
    fs.writeFileSync(
      path.join(dir, 'test.wal'),
      '{"op":"set","k":"post.1","v":{"title":"from wal"}}\n'
    );
    const db = new Database(dbPath);
    await db.ready;
    expect(await db.getStats().format).toBe('json');
    expect(await db.get('user.name')).toBe('legacy');
    expect(await db.get('post.1.title')).toBe('from wal');
    await db.close();
    cleanup(dir);
  });

  T('migrates json <-> v2 without data loss', async () => {
    const { dir, dbPath } = fresh('migrate');
    let db = new Database(dbPath, { format: 'json' });
    await db.ready;
    await db.set('a', 1);
    await db.set('nested.path', { ok: true });
    await db.close();

    db = new Database(dbPath);
    await db.migrate('v2');
    await db.close();
    expect(fs.existsSync(path.join(dir, 'test.snapshot'))).toBe(false);
    expect(fs.existsSync(path.join(dir, 'test.spdb'))).toBe(true);

    db = new Database(dbPath);
    expect(await db.getStats().format).toBe('v2');
    expect(await db.get('a')).toBe(1);
    expect(await db.get('nested.path')).toEqual({ ok: true });
    await db.migrate('json');
    await db.close();
    expect(fs.existsSync(path.join(dir, 'test.snapshot'))).toBe(true);

    db = new Database(dbPath);
    expect(await db.get('nested.path')).toEqual({ ok: true });
    await db.close();
    cleanup(dir);
  });

  (NATIVE ? test : test.skip)('fallback refuses v2 files', async () => {
    const { dir, dbPath } = fresh('guard');
    const db = new Database(dbPath);
    await db.ready;
    await db.set('x', 1);
    await db.compact();
    await db.close();
    const { Database: FallbackDatabase } = require('../fallback');
    expect(() => new FallbackDatabase(dbPath, {})).toThrow(/v2 binary format/);
    cleanup(dir);
  });

  test('interops with v1.1.0 files (native reads fallback JSON)', async () => {
    const { dir, dbPath } = fresh('interop');

    const { Database: FallbackDatabase } = require('../fallback');
    const fdb = new FallbackDatabase(dbPath, {});
    await fdb.ready;
    fdb.set('from', 'fallback');
    fdb.set('row', { a: 1 });
    await fdb.compact();
    await fdb.close();


    if (NATIVE) {
      const db = new Database(dbPath);
      await db.ready;
      expect(await db.getStats().format).toBe('json');
      expect(await db.get('from')).toBe('fallback');
      expect(await db.get('row')).toEqual({ a: 1 });
      await db.close();
    }
    cleanup(dir);
  });
});


describe('security & encryption', () => {
  T('encrypts sensitive keys at rest', async () => {
    const { dir, dbPath } = fresh('enc');
    const db = new Database(dbPath, { encryptionKey: 'my-secret' });
    await db.ready;
    await db.set('password', { pw: 'hunter2' });
    await db.set('profile.name', 'alice');
    expect(await db.get('password')).toEqual({ pw: 'hunter2' });
    await db.compact();
    await db.close();

    const raw = fs.readFileSync(path.join(dir, 'test.spdb'), 'utf8');
    expect(raw).toContain('"__enc":1');
    expect(raw).not.toContain('hunter2');

    const db2 = new Database(dbPath, { encryptionKey: 'my-secret' });
    await db2.ready;
    expect(db2.get('password')).toEqual({ pw: 'hunter2' });
    const all = db2.all();
    const pw = all.find((e) => e.ID === 'password');
    expect(pw.data).toEqual({ pw: 'hunter2' });
    await db2.close();
    cleanup(dir);
  });

  test('rejects control characters in keys', async () => {
    const { dir, dbPath } = fresh('ctrl');
    const db = new Database(dbPath);
    await db.ready;
    expect(() => db.set('a\u0000b', 1)).toThrow();
    await db.close();
    cleanup(dir);
  });
});


describe('locking', () => {
  T('blocks concurrent open (live lock)', async () => {
    const { dir, dbPath } = fresh('lock');
    const db = new Database(dbPath, { lockTimeout: 300 });
    await db.ready;
    expect(() => new Database(dbPath, { lockTimeout: 300 })).toThrow();
    await db.close();
    cleanup(dir);
  });

  T('takes over stale locks (dead pid)', async () => {
    const { dir, dbPath } = fresh('stale');
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'test.lock'), '999999999\n');
    const db = new Database(dbPath, { lockTimeout: 2000 });
    await db.ready;
    await db.set('ok', true);
    await db.close();
    cleanup(dir);
  });
});


describe('events', () => {
  test('change / clear / save events fire', async () => {
    const { dir, dbPath } = fresh('events');
    const db = new Database(dbPath);
    await db.ready;
    const seen = { change: 0, clear: 0, save: 0 };
    db.on('change', () => seen.change++);
    db.on('clear', () => seen.clear++);
    db.on('save', () => seen.save++);
    await db.set('a', 1);
    await db.delete('a');
    await db.clear();
    await db.compact();
    expect(seen.change).toBe(2);
    expect(seen.clear).toBe(1);
    expect(seen.save).toBe(1);
    await db.close();
    cleanup(dir);
  });

  T('transaction event fires once per commit', async () => {
    const { dir, dbPath } = fresh('txevent');
    const db = new Database(dbPath);
    await db.ready;
    let n = 0;
    db.on('transaction', () => n++);
    await db.transaction([{ type: 'set', key: 'a', value: 1 }]);
    await db.transaction((tx) => tx.set('b', 2));
    expect(n).toBe(2);
    await db.close();
    cleanup(dir);
  });
});


describe('stats & warmKeys', () => {
  T('stats exposes engine metadata', async () => {
    const { dir, dbPath } = fresh('stats');
    const db = new Database(dbPath);
    await db.ready;
    await db.set('user', { a: 1, b: 2 });
    const st = await db.getStats();
    expect(st.driver).toBe('spectre.db');
    expect(st.entries).toBe(2);
    expect(['v2', 'json']).toContain(st.format);
    if (NATIVE) expect(st.engine).toMatch(/rust/);
    await db.close();
    cleanup(dir);
  });

  test('warmKeys option accepted', async () => {
    const { dir, dbPath } = fresh('warm');
    const db = new Database(dbPath, { warmKeys: ['a'] });
    await db.ready;
    await db.set('a', 1);
    expect(await db.get('a')).toBe(1);
    await db.close();
    cleanup(dir);
  });

  test('operations after close throw', async () => {
    const { dir, dbPath } = fresh('closed');
    const db = new Database(dbPath);
    await db.ready;
    await db.close();
    await db.close();
    expect(() => db.set('x', 1)).toThrow();
    cleanup(dir);
  });
});
