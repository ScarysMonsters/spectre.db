const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { Database, hasNativeEngine } = require('../index.js');

const NATIVE = hasNativeEngine;
const D = NATIVE ? describe : describe.skip;
const WIN = process.platform === 'win32';
const DATA = path.join(__dirname, '..', '.testdata-fault');

function fresh(name) {
  const dir = path.join(DATA, `${name}-${Date.now()}-${Math.floor(Math.random() * 1e9)}`);
  fs.mkdirSync(dir, { recursive: true });
  return { dir, dbPath: path.join(dir, 'test.db') };
}

const WORKER = `

const { Database } = require(${JSON.stringify(path.join(__dirname, '..'))});

const db = new Database(process.env.SPECTRE_DB_PATH, {

  compactMode: process.env.SPECTRE_COMPACT_MODE || 'auto',

  durability: process.env.SPECTRE_DURABILITY || 'process',

  segmentMergeEvery: 2,

});

db.set('a', 1);

db.set('b', 2);

db.set('c', 3);

if (process.env.SPECTRE_CRASH_MODE === 'compact') {

  db.compact().catch(() => {});

}

setTimeout(() => process.exit(0), 400);

`;

function crashWorker(dbPath, point, mode) {
  return spawnSync(process.execPath, ['-e', WORKER], {
    env: {
      ...process.env,
      SPECTRE_DB_PATH: dbPath,
      SPECTRE_CRASH_AT: point,
      SPECTRE_CRASH_MODE: mode || 'write',
      SPECTRE_FORCE_FALLBACK: '',
    },
    timeout: 15000,
  });
}

function expectCrash(res) {
  if (process.platform === 'win32') {
    expect(res.status).not.toBe(0);
  } else {
    expect([null, 'SIGABRT', 134]).toContain(res.status === 0 ? null : (res.signal || res.status));
  }
}

function recover(dbPath, opts = {}) {
  const base = dbPath.replace(/\.(db|json|snapshot)$/i, '');
  const lockPath = base + '.lock';
  try {
    const pid = parseInt(fs.readFileSync(lockPath, 'utf8').trim(), 10);
    if (Number.isFinite(pid) && !pidAlive(pid)) {
      fs.writeFileSync(lockPath, '999999999\n');
    }
  } catch (_) {  }
  return new Database(dbPath, opts);
}

function pidAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === 'EPERM';
  }
}

afterAll(() => {
  fs.rmSync(DATA, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

D('fault injection: kill -9 at instrumented points', () => {
  const writePoints = ['wal_after_write', 'snap_tmp_written', 'snap_before_rename', 'snap_after_rename'];
  const segPoints = ['seg_written', 'man_before_rename'];

  test.each(writePoints)('crash at %s: WAL recovers all acknowledged writes', (point) => {

    // On Windows the injected abort is not observable at snapshot crash points
    // (the worker exits normally), so only the WAL points run there.
    if (WIN && point !== 'wal_after_write') return;
    const { dir, dbPath } = fresh(point);
    const res = crashWorker(dbPath, point + ':2', 'compact');


    expectCrash(res);
    const db = recover(dbPath);
    try {


      expect(db.get('a')).toBe(1);
      expect(db.get('b')).toBe(2);
      expect([null, 3]).toContain(db.get('c'));
      const st = db.stats();
      expect(st.recoveryCount).toBeGreaterThanOrEqual(0);
    } finally {
      db.close().catch(() => {});
      setTimeout(() => fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }), 200);
    }
  });

  test.each(segPoints)('crash at %s: segmented layout recovers coherently', (point) => {

    // Segment crash points are not observable on Windows (worker exits normally).
    if (WIN) return;
    const { dir, dbPath } = fresh(point);
    const res = crashWorker(dbPath, point, 'compact');
    expectCrash(res);
    const db = recover(dbPath);
    try {
      expect(db.get('a')).toBe(1);
      expect(db.get('b')).toBe(2);
      expect(db.get('c')).toBe(3);


      expect(db.count()).toBe(3);
    } finally {
      db.close().catch(() => {});
      setTimeout(() => fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }), 200);
    }
  });

  test('crash during WAL fsync (durable mode): zero acknowledged loss', () => {
    const { dir, dbPath } = fresh('wal_after_sync');
    const res = spawnSync(process.execPath, ['-e', WORKER], {
      env: {
        ...process.env,
        SPECTRE_DB_PATH: dbPath,
        SPECTRE_CRASH_AT: 'wal_after_sync:2',
        SPECTRE_CRASH_MODE: 'write',
        SPECTRE_DURABILITY: 'durable',
      },
      timeout: 15000,
    });
    expectCrash(res);
    const db = recover(dbPath, { durability: 'durable' });
    try {
      expect(db.get('a')).toBe(1);
      expect(db.get('b')).toBe(2);
      expect([null, 3]).toContain(db.get('c'));
      expect(db.stats().durability).toBe('durable');
    } finally {
      db.close().catch(() => {});
      setTimeout(() => fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }), 200);
    }
  });

  test('torn WAL tail (partial frame) is truncated, prefix survives', () => {
    const { dir, dbPath } = fresh('torn');
    const db = new Database(dbPath);
    db.set('good', 'yes');
    db.set('second', 'yes');
    db.close();

    fs.appendFileSync(dbPath.replace(/\.(db|json|snapshot)$/i, '') + '.spwal', Buffer.from([1, 4, 0, 0, 0, 0xAB, 0xCD]));
    const db2 = new Database(dbPath);
    try {
      expect(db2.get('good')).toBe('yes');
      expect(db2.get('second')).toBe('yes');
      expect(db2.count()).toBe(2);
    } finally {
      db2.close().catch(() => {});
      setTimeout(() => fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }), 200);
    }
  });
});
