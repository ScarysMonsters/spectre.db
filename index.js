'use strict';


const fs = require('fs');
const path = require('path');
const { EventEmitter } = require('events');
const crypto = require('crypto');


function loadNative() {
  const candidates = [
    path.join(__dirname, 'build', 'Release', 'spectre.db-rs.node'),
    path.join(__dirname, 'prebuilds', `spectre.db-rs-${process.platform}-${process.arch}.node`),
  ];
  for (const c of candidates) {
    try {
      if (fs.existsSync(c)) return require(c);
    } catch (_) {

    }
  }

  const libc = process.platform === 'linux'
    ? (process.report && typeof process.report.getReport === 'function'
        && process.report.getReport().header && process.report.getReport().header.glibcVersionRuntime
      ? ['gnu', 'musl']
      : ['musl', 'gnu'])
    : [''];
  const pkgNames = [];
  for (const lc of libc) {
    const triplet = lc ? `${process.platform}-${process.arch}-${lc}` : `${process.platform}-${process.arch}`;
    pkgNames.push(`spectre-db-${triplet}`);
    pkgNames.push(`spectre.db-rs-${triplet}`);
  }
  if (process.platform === 'win32') {
    pkgNames.unshift(`spectre-db-win32-${process.arch}-msvc`);
  }
  for (const name of pkgNames) {
    try {
      return require(name);
    } catch (_) {

    }
  }
  return null;
}

const nativeBinding =
  process.env.SPECTRE_FORCE_FALLBACK === '1' ? null : loadNative();


function normalizePath(filePath) {
  return path
    .resolve(filePath)
    .replace(/\.json\.gz$/i, '')
    .replace(/\.json$/i, '')
    .replace(/\.gz$/i, '');
}

function mapLegacyOptions(opts = {}) {
  const mapped = {
    maxCacheSize: 1000,
    cacheTTL: 0,
    compactThreshold: 500,
    compactInterval: 5 * 60 * 1000,
    compress: false,
    compression: 'none',
    encryptionKey: null,
    encryptBackups: false,
    encryptSnapshot: false,
    scryptLogN: 14,
    scryptR: 8,
    scryptP: 1,
    backupCount: 3,
    format: 'auto',
    lockTimeout: 30000,
    walBuffering: false,
    syncWal: false,
    durability: 'process',
    compactMode: 'auto',
    legacyThreshold: 100000,
    segmentMergeEvery: 8,
  };

  if (opts.maxCacheSize !== undefined) mapped.maxCacheSize = opts.maxCacheSize;
  if (opts.cacheTTL !== undefined) mapped.cacheTTL = opts.cacheTTL;
  if (opts.compress !== undefined) mapped.compress = opts.compress;
  if (opts.compression !== undefined) mapped.compression = opts.compression;
  if (opts.encryptionKey !== undefined) mapped.encryptionKey = opts.encryptionKey;
  if (opts.backupCount !== undefined) mapped.backupCount = opts.backupCount;
  if (opts.encryptBackups !== undefined) mapped.encryptBackups = opts.encryptBackups;
  if (opts.encryptSnapshot !== undefined) mapped.encryptSnapshot = opts.encryptSnapshot;
  if (opts.scryptLogN !== undefined) mapped.scryptLogN = opts.scryptLogN;
  if (opts.scryptR !== undefined) mapped.scryptR = opts.scryptR;
  if (opts.scryptP !== undefined) mapped.scryptP = opts.scryptP;
  if (opts.compactThreshold !== undefined) mapped.compactThreshold = opts.compactThreshold;
  if (opts.compactInterval !== undefined) mapped.compactInterval = opts.compactInterval;
  if (opts.format !== undefined) mapped.format = opts.format;
  if (opts.lockTimeout !== undefined) mapped.lockTimeout = opts.lockTimeout;
  if (opts.walBuffering !== undefined) mapped.walBuffering = opts.walBuffering;
  if (opts.syncWal !== undefined) mapped.syncWal = opts.syncWal;
  if (opts.durability !== undefined) mapped.durability = opts.durability;
  if (opts.compactMode !== undefined) mapped.compactMode = opts.compactMode;
  if (opts.legacyThreshold !== undefined) mapped.legacyThreshold = opts.legacyThreshold;
  if (opts.segmentMergeEvery !== undefined) mapped.segmentMergeEvery = opts.segmentMergeEvery;

  if (opts.autoSave !== undefined) {
    mapped.compactInterval = opts.autoSave > 0 ? opts.autoSave : 0;
    mapped.compactThreshold = opts.autoSave > 0 ? 50 : Infinity;
  }
  if (opts.backup === false) mapped.backupCount = 0;
  if (opts.cache === false) mapped.maxCacheSize = 0;

  return mapped;
}


function decodeNativeError(err) {
  const m = typeof err?.message === 'string' ? err.message.match(/^(\d+)\|([\s\S]*)$/) : null;
  if (!m) return err;
  const decoded = new Error(m[2]);
  decoded.code = Number(m[1]);
  return decoded;
}

function ncall(engine, method, ...args) {
  try {
    return engine[method](...args);
  } catch (err) {
    throw decodeNativeError(err);
  }
}


class Database extends EventEmitter {
  constructor(filePath = './database.json', options = {}) {
    super();
    const cleanPath = normalizePath(filePath);
    const opts = mapLegacyOptions(options);

    this._opts = opts;
    this._path = cleanPath;
    this._closed = false;

    if (nativeBinding) {

      const engineOptions = {
        compress: opts.compress,
        compression: opts.compression,
        encryptBackups: opts.encryptBackups,
        encryptSnapshot: opts.encryptSnapshot,
        backupCount: opts.backupCount,
        format: opts.format,
        lockTimeout: opts.lockTimeout,
        walBuffering: opts.walBuffering,
        durability: opts.durability,
        compactMode: opts.compactMode,
        legacyThreshold: opts.legacyThreshold,
        segmentMergeEvery: opts.segmentMergeEvery,
      };
      if (opts.scryptLogN !== 14) engineOptions.scryptLogN = opts.scryptLogN;
      if (opts.scryptR !== 8) engineOptions.scryptR = opts.scryptR;
      if (opts.scryptP !== 1) engineOptions.scryptP = opts.scryptP;
      if (opts.encryptionKey != null) engineOptions.encryptionKey = String(opts.encryptionKey);
      if (opts.syncWal) {
        engineOptions.syncWal = true;
        this.emit('warn', 'syncWal is deprecated since 2.0.0 — use durability: "durable"');
      }
      this._engine = new nativeBinding.SpectreEngine(cleanPath, engineOptions);
      this._engineKind = 'native';

      for (const ev of ncall(this._engine, 'drainInitEvents')) {
        const payload = ev.payload ? JSON.parse(ev.payload) : undefined;
        if (payload !== undefined) this.emit(ev.event, payload);
        else this.emit(ev.event);
      }
    } else {
      const { Database: FallbackDatabase } = require('./fallback');
      this._engine = new FallbackDatabase(cleanPath, opts);
      this._engineKind = 'fallback';


      for (const ev of ['change', 'clear', 'save', 'transaction', 'warn', 'restore', 'reset', 'error']) {
        this._engine.on(ev, (...args) => this.emit(ev === 'error' ? 'warn' : ev, ...args));
      }
    }


    this._readyPromise = this._engineKind === 'fallback'
      ? Promise.resolve(this._engine.ready).then(() => this)
      : Promise.resolve(this);


    if (this._engineKind === 'native' && opts.compactInterval > 0) {
      this._compactTimer = setInterval(() => {
        try {
          const st = ncall(this._engine, 'stats');
          if (st.walOps >= opts.compactThreshold) {
            this.compact().catch(() => {});
          }
        } catch (_) {

        }
      }, opts.compactInterval);
      this._compactTimer.unref?.();
    }


    const warmKeys = options.warmKeys;
    if (Array.isArray(warmKeys)) {
      this._readyPromise = this._readyPromise.then(() => {
        for (const key of warmKeys) {
          try {
            this.get(key);
          } catch (_) {

          }
        }
        return this;
      });
    }
  }

  get ready() {
    return this._readyPromise;
  }


  get(key) {
    this._assertOpen();
    if (this._engineKind === 'fallback') return this._engine.get(key);
    const raw = ncall(this._engine, 'getJson', key);
    return raw == null ? null : JSON.parse(raw);
  }

  set(key, value) {
    this._assertOpen();
    if (this._engineKind === 'fallback') return this._engine.set(key, value);
    if (value === undefined) {
      throw new TypeError('[spectre.db] value must be JSON-serializable: undefined is not supported (v1.1.0 dropped it silently at compaction)');
    }
    let json;
    try {
      json = JSON.stringify(value);
    } catch (err) {
      if (/circular/i.test(err.message)) {
        const e = new Error('Circular reference detected');
        e.code = 1102;
        throw e;
      }
      if (/BigInt/i.test(err.message)) {
        const e = new TypeError('BigInt not supported');
        e.code = 1103;
        throw e;
      }
      throw err;
    }
    ncall(this._engine, 'setJson', key, json);
    this.emit('change', { type: 'set', key, value });
    return value;
  }

  has(key) {
    return this.get(key) !== null;
  }

  delete(key) {
    this._assertOpen();
    if (this._engineKind === 'fallback') return this._engine.delete(key);
    const deleted = ncall(this._engine, 'del', key);
    if (deleted) this.emit('change', { type: 'delete', key });
    return deleted;
  }

  add(key, n) {
    if (typeof n !== 'number' || !Number.isFinite(n)) throw new TypeError('add() requires a finite number');
    const cur = this.get(key) ?? 0;
    if (typeof cur !== 'number') throw new TypeError(`Value at "${key}" is not a number`);
    return this.set(key, cur + n);
  }

  sub(key, n) {
    return this.add(key, -n);
  }

  push(key, value) {
    const arr = this.get(key) ?? [];
    if (!Array.isArray(arr)) throw new TypeError(`Value at "${key}" is not an array`);
    arr.push(value);
    this.set(key, arr);
    return arr.length;
  }

  pull(key, predicate) {
    const arr = this.get(key) ?? [];
    if (!Array.isArray(arr)) throw new TypeError(`Value at "${key}" is not an array`);
    const test = typeof predicate === 'function' ? predicate : (v) => JSON.stringify(v) === JSON.stringify(predicate);
    const index = arr.findIndex(test);
    if (index === -1) return false;
    arr.splice(index, 1);
    this.set(key, arr);
    return true;
  }


  all(prefix) {
    this._assertOpen();
    if (this._engineKind === 'fallback') {


      const out = [];
      const split = (ID, data) => {
        if (data !== null && typeof data === 'object' && !Array.isArray(data)
          && data.__enc !== 1) {
          for (const [field, v] of Object.entries(data)) {
            split(`${ID}.${field}`, v);
          }
          return;
        }
        out.push({ ID, data });
      };
      for (const { ID, data } of this._engine.all()) {
        if (prefix && !String(ID).startsWith(prefix)) continue;
        split(ID, data);
      }
      return out;
    }
    const json = ncall(this._engine, 'allJson', prefix ?? undefined);
    return JSON.parse(json).map(([ID, data]) => ({ ID, data }));
  }

  filter(predicate) {
    this._assertOpen();
    return this.all().filter(({ ID, data }) => predicate(data, ID));
  }

  startsWith(prefix) {
    this._assertOpen();
    return this.all(prefix);
  }

  count(prefix) {
    this._assertOpen();
    if (this._engineKind === 'fallback') return this.all(prefix).length;
    return ncall(this._engine, 'count', prefix ?? undefined);
  }

  paginate(prefix, page = 1, limit = 10, sortBy = 'data', sortDesc = true) {
    const data = this.startsWith(prefix).sort((a, b) => {
      const vA = a[sortBy];
      const vB = b[sortBy];
      if (vA === vB) return 0;
      return sortDesc ? (vA < vB ? 1 : -1) : vA > vB ? 1 : -1;
    });
    const total = data.length;
    const pages = Math.ceil(total / limit) || 1;
    const start = (page - 1) * limit;
    return {
      data: data.slice(start, start + limit),
      pagination: { page, limit, total, pages, hasNext: page < pages, hasPrev: page > 1 },
    };
  }


  scan(opts = {}) {
    const self = this;
    const pageSize = Math.max(1, opts.pageSize ?? 500);
    let prefix = opts.prefix;
    let after = opts.after ?? '';
    let budget = opts.limit ?? Infinity;
    let buffer = [];
    let done = false;
    return {
      [Symbol.asyncIterator]() { return this; },
      async next() {
        if (buffer.length === 0) {
          if (done || budget <= 0 || self._closed) return { done: true, value: undefined };
          const page = self._scanPage(prefix, after, Math.min(pageSize, budget));
          buffer = page.rows;
          after = page.cursor;
          budget -= buffer.length;
          if (page.cursor == null) done = true;
          if (buffer.length === 0 && done) return { done: true, value: undefined };
        }
        const [ID, data] = buffer.shift();
        return { done: false, value: { ID, data } };
      },
    };
  }

  iterate(opts) {
    return this.scan(opts);
  }


  cursor(prefix, opts = {}) {
    const page = this._scanPage(prefix, opts.after ?? '', opts.limit ?? 500);
    return {
      rows: page.rows.map(([ID, data]) => ({ ID, data })),
      cursor: page.cursor,
      done: page.cursor == null,
    };
  }

  _scanPage(prefix, after, limit) {
    if (this._engineKind === 'fallback') {


      const entries = this.all(prefix)
        .filter(({ ID }) => !after || ID > after)
        .slice(0, limit)
        .map(({ ID, data }) => [ID, data]);
      const isEnd = entries.length < limit;
      return {
        rows: entries,
        cursor: isEnd ? null : entries.length ? entries[entries.length - 1][0] : null,
      };
    }
    const doc = JSON.parse(ncall(this._engine, 'scanJson', prefix ?? undefined, after || undefined, limit));
    return { rows: doc.rows, cursor: doc.cursor };
  }


  setRaw(key, data) {
    if (this._engineKind === 'fallback') {
      throw new Error('[spectre.db] setRaw() requires the native engine (JS fallback stores JSON only)');
    }
    this._assertOpen();
    const buf = Buffer.isBuffer(data) ? data : Buffer.from(data);
    ncall(this._engine, 'setRaw', key, buf);
    this.emit('change', { type: 'setRaw', key, size: buf.length });
    return data;
  }

  getRaw(key) {
    if (this._engineKind === 'fallback') {
      throw new Error('[spectre.db] getRaw() requires the native engine (JS fallback stores JSON only)');
    }
    this._assertOpen();
    return ncall(this._engine, 'getRaw', key) ?? null;
  }

  deleteRaw(key) {
    if (this._engineKind === 'fallback') return false;
    this._assertOpen();
    const deleted = ncall(this._engine, 'deleteRaw', key);
    if (deleted) this.emit('change', { type: 'deleteRaw', key });
    return deleted;
  }


  index(path) {
    this._assertOpen();
    if (this._engineKind === 'fallback') {
      throw new Error('[spectre.db] index() requires the native engine');
    }
    ncall(this._engine, 'createIndex', path);
    const self = this;
    return {
      path,
      get(value) {
        return ncall(self._engine, 'indexLookup', path, JSON.stringify(value)).map((k) => self.get(k));
      },
      keys(value) {
        return ncall(self._engine, 'indexLookup', path, JSON.stringify(value));
      },
      range(opts = {}) {
        return ncall(self._engine, 'indexRange', path,
          opts.gte !== undefined ? JSON.stringify(opts.gte) : undefined,
          opts.lt !== undefined ? JSON.stringify(opts.lt) : undefined,
          opts.limit ?? 0);
      },
      drop() {
        return ncall(self._engine, 'dropIndex', path);
      },
    };
  }

  listIndexes() {
    if (this._engineKind === 'fallback') return [];
    return ncall(this._engine, 'indexList');
  }


  find(predicate) {
    if (predicate !== null && typeof predicate === 'object' && !Array.isArray(predicate)) {
      const keys = this._matchKeys(predicate);
      for (const k of keys) {
        const data = this.get(k);
        if (data !== null) return { ID: k, data };
      }
      return null;
    }
    return this.all().find(({ ID, data }) => predicate(data, ID)) ?? null;
  }

  _matchKeys(matchObj) {
    if (this._engineKind === 'fallback') {
      return this.all()
        .filter(({ data }) => Object.entries(matchObj).every(([p, v]) => {
          let cur = data;
          for (const part of p.split('.')) {
            if (cur == null || typeof cur !== 'object') return false;
            cur = cur[part];
          }
          return JSON.stringify(cur) === JSON.stringify(v);
        }))
        .map(({ ID }) => ID);
    }


    const entries = Object.entries(matchObj);
    const indexed = entries.find(([p]) => this.listIndexes().includes(p));
    if (indexed) {
      const candidates = ncall(this._engine, 'indexLookup', indexed[0], JSON.stringify(indexed[1]));
      return candidates.filter((k) => {
        const data = this.get(k);
        if (data === null) return false;
        return entries.every(([p, v]) => {
          let cur = data;
          for (const part of p.split('.')) {
            if (cur == null || typeof cur !== 'object') return false;
            cur = cur[part];
          }
          return JSON.stringify(cur) === JSON.stringify(v);
        });
      });
    }

    return this.all()
      .filter(({ data }) => entries.every(([p, v]) => {
        let cur = data;
        for (const part of p.split('.')) {
          if (cur == null || typeof cur !== 'object') return false;
          cur = cur[part];
        }
        return JSON.stringify(cur) === JSON.stringify(v);
      }))
      .map(({ ID }) => ID);
  }


  range(path, opts = {}) {
    this._assertOpen();
    if (this._engineKind === 'fallback') {

      return this.all()
        .map(({ ID, data }) => {
          let cur = data;
          for (const part of path.split('.')) {
            if (cur == null || typeof cur !== 'object') return null;
            cur = cur[part];
          }
          return { ID, value: cur };
        })
        .filter((e) => {
          if (!e || e.value === undefined) return false;
          if (opts.gte !== undefined && !(e.value >= opts.gte)) return false;
          if (opts.lt !== undefined && !(e.value < opts.lt)) return false;
          return true;
        })
        .slice(0, opts.limit ?? Infinity)
        .map(({ ID }) => ({ ID, data: this.get(ID) }));
    }
    if (!this.listIndexes().includes(path)) {
      ncall(this._engine, 'createIndex', path);
    }
    return ncall(this._engine, 'indexRange', path,
      opts.gte !== undefined ? JSON.stringify(opts.gte) : undefined,
      opts.lt !== undefined ? JSON.stringify(opts.lt) : undefined,
      opts.limit ?? 0)
      .map((k) => ({ ID: k, data: this.get(k) }));
  }


  transaction(arg) {
    if (this._engineKind === 'fallback') return this._engine.transaction(arg);

    if (Array.isArray(arg)) {
      const ops = [];
      const results = [];
      for (const op of arg) {
        switch (op.type) {
          case 'set':
            ops.push({ type: 'set', key: op.key, json: JSON.stringify(op.value) });
            results.push(op.value);
            break;
          case 'delete':
            ops.push({ type: 'delete', key: op.key });
            results.push(true);
            break;
          case 'add':
          case 'sub':
          case 'push':
          case 'pull':
            throw new TypeError(`[spectre.db] ${op.type} is not supported in array transactions; precompute it as "set"`);
          default:
            throw new Error(`[spectre.db] Unknown operation type: "${op.type}"`);
        }
      }
      if (ops.length > 0) {
        let flags;
        if (this._engineKind === 'native' && ops.every((o) => o.type === 'set')) {

          const parts = new Array(ops.length);
          for (let i = 0; i < ops.length; i++) {
            parts[i] = '[' + JSON.stringify(ops[i].key) + ',' + ops[i].json + ']';
          }
          flags = ncall(this._engine, 'setManyJson', '[' + parts.join(',') + ']');
        } else {
          flags = ncall(this._engine, 'batch', ops);
        }
        for (let i = 0; i < results.length; i++) {
          if (arg[i].type === 'delete') results[i] = flags[i] !== false;
        }
        this.emit('transaction', { ops });
      }
      return Promise.resolve(results);
    }

    if (typeof arg === 'function') {
      const staged = new Map();
      const resolveJson = (key) => {
        if (staged.has(key)) {
          const s = staged.get(key);
          return s.deleted ? undefined : s.json;
        }
        return ncall(this._engine, 'getJson', key);
      };
      const tx = {
        get: (key) => {
          const raw = resolveJson(key);
          return raw == null ? null : JSON.parse(raw);
        },
        set: (key, value) => {
          const json = JSON.stringify(value);
          staged.set(key, { json });
          return value;
        },
        delete: (key) => {
          const existed = resolveJson(key) != null;
          staged.set(key, { deleted: true });
          return existed;
        },
        add: (key, n) => {
          const cur = tx.get(key) ?? 0;
          if (typeof cur !== 'number') throw new TypeError(`Value at "${key}" is not a number`);
          return tx.set(key, cur + n);
        },
        sub: (key, n) => tx.add(key, -n),
        push: (key, value) => {
          const arr = tx.get(key) ?? [];
          if (!Array.isArray(arr)) throw new TypeError(`Value at "${key}" is not an array`);
          arr.push(value);
          return tx.set(key, arr);
        },
        pull: (key, pred) => {
          const arr = tx.get(key) ?? [];
          if (!Array.isArray(arr)) throw new TypeError(`Value at "${key}" is not an array`);
          const test = typeof pred === 'function' ? pred : (v) => JSON.stringify(v) === JSON.stringify(pred);
          const i = arr.findIndex(test);
          if (i === -1) return false;
          arr.splice(i, 1);
          return tx.set(key, arr);
        },
      };
      let result;
      try {
        result = arg(tx);
        if (result && typeof result.then === 'function') {


          throw new TypeError('[spectre.db] async transaction functions are not supported by the native engine; use the array form');
        }
      } catch (err) {

        return Promise.reject(err);
      }
      const ops = [];
      for (const [key, s] of staged) {
        if (s.deleted) ops.push({ type: 'delete', key });
        else ops.push({ type: 'set', key, json: s.json });
      }
      if (ops.length > 0) {
        if (this._engineKind === 'native' && ops.every((o) => o.type === 'set')) {
          const parts = new Array(ops.length);
          for (let i = 0; i < ops.length; i++) {
            parts[i] = '[' + JSON.stringify(ops[i].key) + ',' + ops[i].json + ']';
          }
          ncall(this._engine, 'setManyJson', '[' + parts.join(',') + ']');
        } else {
          ncall(this._engine, 'batch', ops);
        }
        this.emit('transaction', { ops });
      }
      return Promise.resolve(result);
    }

    throw new TypeError('[spectre.db] transaction() expects an array of operations or a function');
  }


  async compact() {
    if (this._engineKind === 'fallback') return this._engine.compact();
    this._assertOpen();
    ncall(this._engine, 'compact');
    this.emit('save', this.getStats());
  }


  async compactAsync() {
    if (this._engineKind === 'fallback') return this._engine.compact();
    this._assertOpen();
    const status = await ncall(this._engine, 'compactAsync');
    this.emit('save', this.getStats());
    return status;
  }

  async save() {
    return this.compact();
  }

  async clear() {
    if (this._engineKind === 'fallback') return this._engine.clear();
    this._assertOpen();
    ncall(this._engine, 'clear');
    this.emit('clear');
    return this;
  }

  async close() {
    if (this._closed) return;
    this._closed = true;
    if (this._compactTimer) {
      clearInterval(this._compactTimer);
      this._compactTimer = null;
    }
    if (this._engineKind === 'fallback') {
      await this._engine.close();
    } else {
      ncall(this._engine, 'close');
    }
  }


  async migrate(format) {
    this._assertOpen();
    if (this._engineKind === 'fallback') {
      throw new Error('[spectre.db] migrate() requires the native engine');
    }
    ncall(this._engine, 'migrate', format);
    return this;
  }

  getStats() {
    if (this._engineKind === 'fallback') return this._engine.getStats();
    const st = ncall(this._engine, 'stats');
    return {
      driver: 'spectre.db',
      engine: st.engine,
      format: st.format,
      compress: st.compress,
      encrypted: st.encrypted,
      entries: st.entries,
      cacheSize: st.entries,
      maxCacheSize: this._opts.maxCacheSize,
      fileSize: st.fileSize,
      storeBytes: st.storeBytes,
      walOps: st.walOps,
      walBytes: st.walBytes,
      compactThreshold: this._opts.compactThreshold,
      shards: 0,
      snapshotPath: st.snapshotPath,
      walPath: st.walPath,
    };
  }


  stats() {
    if (this._engineKind === 'fallback') {
      const s = this._engine.getStats();
      return { ...s, durability: 'process', compression: this._opts.compression, pendingWrites: 0,
        cacheHits: 0, cacheMisses: 0, compressionRatio: 0, segmentCount: 0, generation: 0,
        lastCompactionMs: null, lastLsn: 0, recoveryCount: 0, indexCount: 0, rawEntries: 0 };
    }
    const st = ncall(this._engine, 'stats');
    return {
      driver: 'spectre.db',
      engine: st.engine,
      format: st.format,
      durability: st.durability,
      compression: st.compression,
      compress: st.compress,
      encrypted: st.encrypted,
      snapshotEncrypted: st.snapshotEncrypted,
      entries: st.entries,
      rawEntries: st.rawEntries,
      storeBytes: st.storeBytes,
      fileSize: st.fileSize,
      walOps: st.walOps,
      walBytes: st.walBytes,
      snapshotPath: st.snapshotPath,
      walPath: st.walPath,
      pendingWrites: st.pendingWrites,
      cacheHits: st.cacheHits,
      cacheMisses: st.cacheMisses,
      compressionRatio: st.compressionRatio,
      segmentCount: st.segmentCount,
      generation: st.generation,
      lastCompactionMs: st.lastCompactionMs ?? null,
      lastLsn: st.lastLsn,
      recoveryCount: st.recoveryCount,
      indexCount: st.indexCount,
    };
  }

  _assertOpen() {
    if (this._closed) {
      const err = new Error('Database is closed');
      err.code = 8000;
      throw err;
    }
  }

  table(name) {
    if (typeof name !== 'string' || name.length === 0) throw new TypeError('Table name must be a non-empty string');
    return new Table(this, name);
  }
}


class Table {
  constructor(db, name) {
    this._db = db;
    this._name = name;
  }

  _k(key) {
    return `${this._name}.${key}`;
  }

  get(key) {
    return this._db.get(this._k(key));
  }
  set(key, value) {
    return this._db.set(this._k(key), value);
  }
  has(key) {
    return this._db.has(this._k(key));
  }
  delete(key) {
    return this._db.delete(this._k(key));
  }
  add(key, n) {
    return this._db.add(this._k(key), n);
  }
  sub(key, n) {
    return this._db.sub(this._k(key), n);
  }
  push(key, value) {
    return this._db.push(this._k(key), value);
  }
  pull(key, pred) {
    return this._db.pull(this._k(key), pred);
  }
  all() {
    return this._db.startsWith(`${this._name}.`);
  }
  count() {
    return this.all().length;
  }

  async clear() {
    return this._db.transaction((tx) => {
      for (const { ID } of this.all()) tx.delete(ID);
    });
  }

  async transaction(fn) {
    return this._db.transaction((tx) => {
      const scoped = {
        set: (key, value) => tx.set(this._k(key), value),
        delete: (key) => tx.delete(this._k(key)),
        add: (key, n) => tx.add(this._k(key), n),
        sub: (key, n) => tx.sub(this._k(key), n),
        push: (key, value) => tx.push(this._k(key), value),
        pull: (key, pred) => tx.pull(this._k(key), pred),
        get: (key) => tx.get(this._k(key)),
      };
      return fn(scoped);
    });
  }
}

module.exports = { Database, Table, hasNativeEngine: nativeBinding != null, version: '2.0.0' };
