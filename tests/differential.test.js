const fs = require('fs');
const path = require('path');
const { Database, hasNativeEngine } = require('../index.js');
const { Database: FallbackDatabase } = require('../fallback');

const D = hasNativeEngine ? describe : describe.skip;
const DATA = path.join(__dirname, '..', '.testdata-differential');

function fresh(name) {
  const dir = path.join(DATA, `${name}-${Date.now()}-${Math.floor(Math.random() * 1e9)}`);
  fs.mkdirSync(dir, { recursive: true });
  return { dir, native: path.join(dir, 'native.db'), fallback: path.join(dir, 'fallback.json') };
}

afterAll(() => {
  fs.rmSync(DATA, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

D('native/fallback differential', () => {
  test('core CRUD and bulk helpers agree', async () => {
    const { dir, native, fallback } = fresh('crud');
    const a = new Database(native);
    const b = new FallbackDatabase(fallback);
    await a.ready;
    await b.ready;
    try {
      const writes = [
        ['name', 'spectre'],
        ['count', 42],
        ['user.1', { name: 'alice', age: 30 }],
        ['user.2', { name: 'bob', age: 25 }],
        ['list', [1, 2, 3]],
      ];
      for (const [k, v] of writes) {
        await a.set(k, v);
        await b.set(k, v);
      }
      for (const [k] of writes) {
        expect(await a.get(k)).toEqual(await b.get(k));
      }
      expect(await a.has('missing')).toBe(await b.has('missing'));

      const many = [
        ['bulk.1', { n: 1 }],
        ['bulk.2', { n: 2 }],
      ];
      await a.setMany(many);
      for (const [k, v] of many) await b.set(k, v);
      expect(await a.getMany(['bulk.1', 'bulk.2', 'missing'])).toEqual(
        await Promise.all(['bulk.1', 'bulk.2', 'missing'].map((k) => b.get(k))),
      );

      expect(await a.deleteMany(['bulk.1', 'missing'])).toEqual(
        await Promise.all(['bulk.1', 'missing'].map((k) => b.delete(k))),
      );
      expect(await a.get('bulk.1')).toEqual(await b.get('bulk.1'));

      const av = (await a.all()).map((e) => [e.ID, e.data]).sort((x, y) => x[0].localeCompare(y[0]));
      const bv = (await b.all()).map((e) => [e.ID, e.data]).sort((x, y) => x[0].localeCompare(y[0]));
      expect(av).toEqual(bv);
      expect(await a.count()).toBe(bv.length);
      expect(await a.delete('user.1')).toBe(await b.delete('user.1'));
      expect(await a.get('user.1')).toEqual(await b.get('user.1'));
    } finally {
      await a.close();
      await b.close();
      fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    }
  });

  test('seeded nested mutation sequence agrees', async () => {
    const { dir, native, fallback } = fresh('fuzz');
    const a = new Database(native);
    const b = new FallbackDatabase(fallback);
    await a.ready;
    await b.ready;
    try {
      let seed = 123456789;
      const rnd = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
      const keys = ['a', 'b', 'user.1', 'user.2', 'obj.x', 'obj.y.deep', 'obj.y.other', 'list.0', 'list.1'];
      for (let i = 0; i < 200; i++) {
        const key = keys[Math.floor(rnd() * keys.length)];
        if (rnd() < 0.75) {
          const value = rnd() < 0.3
            ? Math.floor(rnd() * 100)
            : (rnd() < 0.5 ? { n: Math.floor(rnd() * 10) } : [Math.floor(rnd() * 10)]);
          await a.set(key, value);
          await b.set(key, value);
        } else {
          await a.delete(key);
          await b.delete(key);
        }
        const av = (await a.all()).map((e) => [e.ID, e.data]).sort((x, y) => x[0].localeCompare(y[0]));
        const bv = (await b.all()).map((e) => [e.ID, e.data]).sort((x, y) => x[0].localeCompare(y[0]));
        expect(av).toEqual(bv);
      }
    } finally {
      await a.close();
      await b.close();
      fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    }
  });
});
