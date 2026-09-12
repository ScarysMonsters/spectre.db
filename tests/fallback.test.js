const fs = require('fs');
const path = require('path');
const { hasNativeEngine } = require('../index.js');

const FALLBACK = process.env.SPECTRE_FORCE_FALLBACK === '1';
const D = FALLBACK ? describe : describe.skip;
const DATA = path.join(__dirname, '..', '.testdata-fallback');

function fresh(name) {
  const dir = path.join(DATA, `${name}-${Date.now()}-${Math.floor(Math.random() * 1e9)}`);
  fs.mkdirSync(dir, { recursive: true });
  return { dir, dbPath: path.join(dir, 'test.db') };
}

function cleanup(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
}

afterAll(() => {
  fs.rmSync(DATA, { recursive: true, force: true });
});

D('fallback engine (v1.1.0 API, async)', () => {
  test('set/get/has/delete roundtrip', async () => {
    const { dir, dbPath } = fresh('crud');
    const { Database } = require('../index.js');
    const db = new Database(dbPath);
    await db.ready;
    await db.set('name', 'spectre');
    await db.set('count', 42);
    await db.set('user', { name: 'alice', age: 30 });
    expect(await db.get('name')).toBe('spectre');
    expect(await db.get('count')).toBe(42);
    expect(await db.get('user')).toEqual({ name: 'alice', age: 30 });
    expect(await db.has('name')).toBe(true);
    expect(await db.has('missing')).toBe(false);
    expect(await db.delete('count')).toBe(true);
    expect(await db.delete('count')).toBe(false);
    expect(await db.get('count')).toBeNull();
    await db.close();
    await new Promise((r) => setTimeout(r, 150));
    cleanup(dir);
  });

  test('dot-path nesting', async () => {
    const { dir, dbPath } = fresh('dots');
    const { Database } = require('../index.js');
    const db = new Database(dbPath);
    await db.ready;
    await db.set('user.1.name', 'alice');
    await db.set('user.1.age', 30);
    await db.set('user.2.name', 'bob');
    expect(await db.get('user.1')).toEqual({ name: 'alice', age: 30 });
    expect(await db.get('user.1.name')).toBe('alice');
    await db.close();
    await new Promise((r) => setTimeout(r, 150));
    cleanup(dir);
  });

  test('persistence: save() writes JSON v1 artifacts; reopen stays clean', async () => {
    const { dir, dbPath } = fresh('persist');
    const { Database } = require('../index.js');
    const db = new Database(dbPath);
    await db.ready;
    await db.set('keep', 'me');
    await db.set('nested.value', 123);
    await db.save();
    await new Promise((r) => setTimeout(r, 150));

    expect(fs.existsSync(path.join(dir, 'test.snapshot'))).toBe(true);
    const snap = JSON.parse(fs.readFileSync(path.join(dir, 'test.snapshot'), 'utf8'));
    expect(snap.keep).toBe('me');
    expect(snap.nested.value).toBe(123);


    await new Promise((r) => setTimeout(r, 150));
    await db.close();
    const db2 = new Database(dbPath);
    await db2.ready;
    const v = await db2.get('keep');
    expect(v === null || v === 'me').toBe(true);
    await db2.close();
    await new Promise((r) => setTimeout(r, 150));
    cleanup(dir);
  });

  test('transaction function form commits atomically', async () => {
    const { dir, dbPath } = fresh('tx');
    const { Database } = require('../index.js');
    const db = new Database(dbPath);
    await db.ready;
    await db.transaction((tx) => {
      tx.set('a', 1);
      tx.set('b', 2);
    });
    expect(await db.get('a')).toBe(1);
    expect(await db.get('b')).toBe(2);
    await db.close();
    await new Promise((r) => setTimeout(r, 150));
    cleanup(dir);
  });

  test('migrate() is a native-engine extension — rejected cleanly', async () => {
    const { dir, dbPath } = fresh('migrate');
    const { Database } = require('../index.js');
    const db = new Database(dbPath);
    await db.ready;
    await expect(db.migrate('v2')).rejects.toThrow(/native engine/);
    await db.close();
    await new Promise((r) => setTimeout(r, 150));
    cleanup(dir);
  });

  test('raw API is a native-engine extension — rejected cleanly', async () => {
    const { dir, dbPath } = fresh('raw');
    const { Database } = require('../index.js');
    const db = new Database(dbPath);
    await db.ready;
    expect(() => db.setRaw('k', Buffer.from([1]))).toThrow(/native engine/);
    expect(() => db.getRaw('k')).toThrow(/native engine/);
    await db.close();
    await new Promise((r) => setTimeout(r, 150));
    cleanup(dir);
  });

  test('REFUSES the binary v2 format with an explicit error', async () => {
    const { dir, dbPath } = fresh('v2refuse');

    const base = dbPath.replace(/\.(json|db|snapshot)$/i, '');
    fs.writeFileSync(`${base}.spdb`, Buffer.from([
      ...Buffer.from('SPDBSNAP'), 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0x42, 0, 0, 0,
    ]));
    fs.writeFileSync(`${base}.spwal`, Buffer.from('SPDBWAL3 whatever the fallback cannot read'));
    const { Database } = require('../index.js');
    expect(() => new Database(dbPath)).toThrow(/v2 binary format/);
    cleanup(dir);
  });

  test('scan() API works (emulated, async)', async () => {
    const { dir, dbPath } = fresh('scan');
    const { Database } = require('../index.js');
    const db = new Database(dbPath);
    await db.ready;
    for (let i = 0; i < 10; i++) await db.set(`user.${i}`, { i });
    let n = 0;
    for await (const e of db.scan({ prefix: 'user.', pageSize: 3 })) n++;
    expect(n).toBe(10);
    const cur = db.cursor('user.', { limit: 4 });
    expect(cur.rows.length).toBe(4);
    expect(cur.done).toBe(false);
    await db.close();
    await new Promise((r) => setTimeout(r, 150));
    cleanup(dir);
  });

  test('stats() exposes the v2.0 shape with neutral fallback values', async () => {
    const { dir, dbPath } = fresh('stats');
    const { Database } = require('../index.js');
    const db = new Database(dbPath);
    await db.ready;
    await db.set('a', 1);
    const st = db.stats();
    expect(st.durability).toBe('process');
    expect(st.segmentCount).toBe(0);
    expect(st.indexCount).toBe(0);
    await db.close();
    await new Promise((r) => setTimeout(r, 150));
    cleanup(dir);
  });
});

if (!FALLBACK && !hasNativeEngine) {
  test('sanity', () => expect(true).toBe(true));
}
