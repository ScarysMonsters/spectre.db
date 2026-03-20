'use strict';

const fs   = require('fs');
const fsp  = fs.promises;
const path = require('path');
const zlib = require('zlib');
const crypto = require('crypto');
const { EventEmitter } = require('events');
const { promisify } = require('util');

const asyncGzip   = promisify(zlib.gzip);
const asyncGunzip = promisify(zlib.gunzip);

const FORBIDDEN_SEGMENTS = new Set(['__proto__', 'constructor', 'prototype']);
const SENSITIVE_KEY_RE   = /password|secret|token|apikey|api_key|private/i;

function validateKey(key) {
  if (typeof key !== 'string' || key.length === 0) {
    throw new TypeError('Key must be a non-empty string');
  }
  const parts = key.split('.');
  for (const part of parts) {
    if (part.length === 0) throw new Error(`Key contains an empty segment: "${key}"`);
    if (FORBIDDEN_SEGMENTS.has(part)) throw new Error(`Forbidden key segment "${part}" in key: "${key}"`);
  }
  return parts;
}

function uniqueTmp(base) {
  const rand = crypto.randomBytes(6).toString('hex');
  return `${base}.${process.pid}.${Date.now()}.${rand}.tmp`;
}

function encryptValue(value, keyBuf) {
  const iv        = crypto.randomBytes(12);
  const cipher    = crypto.createCipheriv('aes-256-gcm', keyBuf, iv);
  const plain     = JSON.stringify(value);
  const encrypted = Buffer.concat([cipher.update(plain, 'utf8'), cipher.final()]);
  const tag       = cipher.getAuthTag();
  return { __enc: 1, iv: iv.toString('base64'), ct: encrypted.toString('base64'), tag: tag.toString('base64') };
}

function decryptValue(envelope, keyBuf) {
  const iv      = Buffer.from(envelope.iv, 'base64');
  const ct      = Buffer.from(envelope.ct, 'base64');
  const tag     = Buffer.from(envelope.tag, 'base64');
  const decipher = crypto.createDecipheriv('aes-256-gcm', keyBuf, iv);
  decipher.setAuthTag(tag);
  const plain = Buffer.concat([decipher.update(ct), decipher.final()]);
  return JSON.parse(plain.toString('utf8'));
}

function toNullProto(value) {
  if (value === null || value === undefined || typeof value !== 'object') return value;
  if (Array.isArray(value)) return value.map(toNullProto);
  const result = Object.create(null);
  for (const k of Object.keys(value)) result[k] = toNullProto(value[k]);
  return result;
}

function toPlainObject(value) {
  if (value === null || value === undefined || typeof value !== 'object') return value;
  if (Array.isArray(value)) return value.map(toPlainObject);
  const result = {};
  for (const k of Object.keys(value)) result[k] = toPlainObject(value[k]);
  return result;
}

class WriteQueue {
  constructor() {
    this._chain = Promise.resolve();
  }

  push(fn) {
    const result = this._chain.then(() => fn());
    this._chain  = result.catch(() => {});
    return result;
  }
}

class LRUNode {
  constructor(key, value, expiry) {
    this.key    = key;
    this.value  = value;
    this.expiry = expiry;
    this.prev   = null;
    this.next   = null;
  }
}

class LRUCache {
  constructor(maxSize) {
    this._max   = maxSize;
    this._map   = new Map();
    this._index = new Map();
    this._head  = new LRUNode(null, null, -1);
    this._tail  = new LRUNode(null, null, -1);
    this._head.next = this._tail;
    this._tail.prev = this._head;
  }

  _attach(node) {
    node.next           = this._head.next;
    node.prev           = this._head;
    this._head.next.prev = node;
    this._head.next     = node;
  }

  _detach(node) {
    node.prev.next = node.next;
    node.next.prev = node.prev;
    node.prev = null;
    node.next = null;
  }

  _evict(node) {
    this._detach(node);
    this._map.delete(node.key);
    this._removeFromIndex(node.key);
  }

  _removeFromIndex(key) {
    const parts = key.split('.');
    for (let i = 1; i <= parts.length; i++) {
      const prefix = parts.slice(0, i).join('.');
      const set    = this._index.get(prefix);
      if (!set) continue;
      set.delete(key);
      if (set.size === 0) this._index.delete(prefix);
    }
  }

  get(key) {
    const node = this._map.get(key);
    if (!node) return undefined;
    if (node.expiry > 0 && Date.now() > node.expiry) { this._evict(node); return undefined; }
    this._detach(node);
    this._attach(node);
    return node.value;
  }

  set(key, value, ttlMs = 0) {
    const expiry = ttlMs > 0 ? Date.now() + ttlMs : -1;
    if (this._map.has(key)) {
      const node  = this._map.get(key);
      node.value  = value;
      node.expiry = expiry;
      this._detach(node);
      this._attach(node);
      return;
    }
    const node = new LRUNode(key, value, expiry);
    this._map.set(key, node);
    this._attach(node);
    const parts = key.split('.');
    for (let i = 1; i <= parts.length; i++) {
      const prefix = parts.slice(0, i).join('.');
      if (!this._index.has(prefix)) this._index.set(prefix, new Set());
      this._index.get(prefix).add(key);
    }
    if (this._map.size > this._max) this._evict(this._tail.prev);
  }

  invalidate(key) {
    const affected = this._index.get(key);
    if (affected) for (const k of [...affected]) { const node = this._map.get(k); if (node) this._evict(node); }
    const parts = key.split('.');
    for (let i = 1; i < parts.length; i++) {
      const parent = parts.slice(0, i).join('.');
      const node   = this._map.get(parent);
      if (node) this._evict(node);
    }
  }

  clear() {
    this._map.clear();
    this._index.clear();
    this._head.next = this._tail;
    this._tail.prev = this._head;
  }

  get size() { return this._map.size; }
}

class WALWriter {
  constructor(walPath) {
    this._path = walPath;
    this._fd   = null;
  }

  async open() {
    this._fd = await fsp.open(this._path, 'a');
  }

  async append(entry) {
    await this._fd.write(JSON.stringify(entry) + '\n');
  }

  async truncateAndReopen() {
    if (this._fd) await this._fd.close();
    await fsp.writeFile(this._path, '');
    this._fd = await fsp.open(this._path, 'a');
  }

  async close() {
    if (this._fd) { await this._fd.close(); this._fd = null; }
  }
}

class Engine extends EventEmitter {
  constructor(dbPath, options = {}) {
    super();

    this._opts = {
      maxCacheSize:      1000,
      cacheTTL:          0,
      compactThreshold:  500,
      compactInterval:   5 * 60 * 1000,
      compress:          false,
      encryptionKey:     null,
      backupCount:       3,
      ...options,
    };

    this._encKey = null;
    if (this._opts.encryptionKey) {
      const raw    = this._opts.encryptionKey;
      this._encKey = Buffer.isBuffer(raw) && raw.length === 32
        ? raw
        : crypto.scryptSync(String(raw), 'spectre-db-kdf-v1', 32);
    }

    const resolved     = path.resolve(dbPath);
    this._dir          = path.dirname(resolved);
    this._base         = path.basename(resolved).replace(/\.(json|db|snapshot)$/, '');
    this._snapshotPath = path.join(this._dir, `${this._base}.snapshot`);
    this._walPath      = path.join(this._dir, `${this._base}.wal`);

    this._store        = Object.create(null);
    this._cache        = new LRUCache(this._opts.maxCacheSize);
    this._queue        = new WriteQueue();
    this._wal          = new WALWriter(this._walPath);
    this._walOps       = 0;
    this._closed       = false;
    this._compactTimer = null;

    this.ready = this._init();
  }

  async _init() {
    await fsp.mkdir(this._dir, { recursive: true });
    await this._loadSnapshot();
    this._walOps = await this._replayWAL();
    await this._wal.open();

    if (this._opts.compactInterval > 0) {
      this._compactTimer = setInterval(() => {
        if (this._walOps >= this._opts.compactThreshold) {
          this._queue.push(() => this._compact());
        }
      }, this._opts.compactInterval);
      this._compactTimer.unref?.();
    }
  }

  async _loadSnapshot() {
    const exists = await fsp.access(this._snapshotPath).then(() => true, () => false);
    if (!exists) return;
    try {
      let buf = await fsp.readFile(this._snapshotPath);
      if (this._opts.compress) buf = await asyncGunzip(buf);
      this._store = toNullProto(JSON.parse(buf.toString('utf8')));
    } catch {
      this.emit('warn', 'Snapshot corrupted, attempting backup restore');
      await this._restoreFromBackup();
    }
  }

  async _restoreFromBackup() {
    for (let gen = 1; gen <= this._opts.backupCount; gen++) {
      const p      = `${this._snapshotPath}.${gen}.bak`;
      const exists = await fsp.access(p).then(() => true, () => false);
      if (!exists) continue;
      try {
        let buf = await fsp.readFile(p);
        if (this._opts.compress) buf = await asyncGunzip(buf);
        this._store = toNullProto(JSON.parse(buf.toString('utf8')));
        this.emit('restore', { generation: gen, path: p });
        return;
      } catch {}
    }
    this._store = Object.create(null);
    this.emit('reset');
  }

  async _replayWAL() {
    const exists = await fsp.access(this._walPath).then(() => true, () => false);
    if (!exists) return 0;
    let text;
    try { text = await fsp.readFile(this._walPath, 'utf8'); } catch { return 0; }
    let count = 0;
    for (const line of text.split('\n')) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      try { this._applyEntry(JSON.parse(trimmed)); count++; } catch {}
    }
    return count;
  }

  _applyEntry(entry) {
    switch (entry.op) {
      case 'set':   this._applySet(entry.k.split('.'), entry.v); break;
      case 'del':   this._applyDelete(entry.k.split('.')); break;
      case 'clear': this._store = Object.create(null); break;
      case 'batch': for (const op of entry.ops) this._applyEntry(op); break;
    }
  }

  _applySet(parts, value) {
    let node = this._store;
    for (let i = 0; i < parts.length - 1; i++) {
      const part = parts[i];
      if (node[part] === null || node[part] === undefined || typeof node[part] !== 'object' || Array.isArray(node[part])) {
        node[part] = Object.create(null);
      }
      node = node[part];
    }
    node[parts[parts.length - 1]] = toNullProto(value);
  }

  _applyDelete(parts) {
    let node = this._store;
    for (let i = 0; i < parts.length - 1; i++) {
      if (!node || typeof node[parts[i]] !== 'object' || Array.isArray(node[parts[i]])) return false;
      node = node[parts[i]];
    }
    const last = parts[parts.length - 1];
    if (!node || !(last in node)) return false;
    delete node[last];
    return true;
  }

  _readRaw(key) {
    const parts = key.split('.');
    let node = this._store;
    for (const part of parts) {
      if (node === null || node === undefined || typeof node !== 'object') return undefined;
      node = node[part];
    }
    return node;
  }

  _unwrap(value) {
    if (this._encKey && value !== null && typeof value === 'object' && value.__enc === 1) {
      return decryptValue(value, this._encKey);
    }
    return value;
  }

  _wrap(key, value) {
    if (this._encKey && SENSITIVE_KEY_RE.test(key) && value !== null && value !== undefined) {
      return encryptValue(value, this._encKey);
    }
    return value;
  }

  get(key) {
    validateKey(key);
    const cached = this._cache.get(key);
    if (cached !== undefined) return cached;
    const raw = this._readRaw(key);
    if (raw === undefined) return null;
    const value = this._unwrap(raw);
    if (value !== null && value !== undefined) this._cache.set(key, value, this._opts.cacheTTL);
    return value ?? null;
  }

  set(key, value) {
    validateKey(key);
    const stored = this._wrap(key, value);
    this._applySet(key.split('.'), stored);
    this._cache.invalidate(key);
    this._queue.push(() => this._wal.append({ op: 'set', k: key, v: stored }).then(() => { this._walOps++; }));
    this.emit('change', { type: 'set', key, value });
    return value;
  }

  delete(key) {
    validateKey(key);
    const deleted = this._applyDelete(key.split('.'));
    if (!deleted) return false;
    this._cache.invalidate(key);
    this._queue.push(() => this._wal.append({ op: 'del', k: key }).then(() => { this._walOps++; }));
    this.emit('change', { type: 'delete', key });
    return true;
  }

  has(key)        { return this.get(key) !== null; }
  add(key, n)     { if (typeof n !== 'number' || !isFinite(n)) throw new TypeError('add() requires a finite number'); const cur = this.get(key) ?? 0; if (typeof cur !== 'number') throw new TypeError(`Value at "${key}" is not a number`); return this.set(key, cur + n); }
  sub(key, n)     { return this.add(key, -n); }

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
    const test  = typeof predicate === 'function' ? predicate : v => JSON.stringify(v) === JSON.stringify(predicate);
    const index = arr.findIndex(test);
    if (index === -1) return false;
    arr.splice(index, 1);
    this.set(key, arr);
    return true;
  }

  async transaction(fn) {
    return this._queue.push(async () => {
      const snapshot = toNullProto(toPlainObject(this._store));
      const ops = [];
      const tx  = {
        set: (key, value) => {
          validateKey(key);
          const stored = this._wrap(key, value);
          this._applySet(key.split('.'), stored);
          this._cache.invalidate(key);
          ops.push({ op: 'set', k: key, v: stored });
          return value;
        },
        delete: (key) => {
          validateKey(key);
          const ok = this._applyDelete(key.split('.'));
          if (ok) { this._cache.invalidate(key); ops.push({ op: 'del', k: key }); }
          return ok;
        },
        get:  (key) => this.get(key),
        add:  (key, n)     => { const cur = tx.get(key) ?? 0; if (typeof cur !== 'number') throw new TypeError(`Value at "${key}" is not a number`); return tx.set(key, cur + n); },
        sub:  (key, n)     => tx.add(key, -n),
        push: (key, value) => { const arr = tx.get(key) ?? []; if (!Array.isArray(arr)) throw new TypeError(`Value at "${key}" is not an array`); arr.push(value); return tx.set(key, arr); },
        pull: (key, pred)  => { const arr = tx.get(key) ?? []; if (!Array.isArray(arr)) throw new TypeError(`Value at "${key}" is not an array`); const test = typeof pred === 'function' ? pred : v => JSON.stringify(v) === JSON.stringify(pred); const i = arr.findIndex(test); if (i === -1) return false; arr.splice(i, 1); return tx.set(key, arr); },
      };
      try {
        const result = await fn(tx);
        if (ops.length > 0) { await this._wal.append({ op: 'batch', ops }); this._walOps++; }
        this.emit('transaction', { ops });
        return result;
      } catch (err) {
        this._store = toNullProto(snapshot);
        this._cache.clear();
        this.emit('rollback', { error: err });
        throw err;
      }
    });
  }

  all() {
    const result   = [];
    const traverse = (obj, prefix) => {
      for (const key of Object.keys(obj)) {
        const fullKey = prefix ? `${prefix}.${key}` : key;
        const val     = obj[key];
        if (val !== null && typeof val === 'object' && !Array.isArray(val) && val.__enc !== 1) {
          traverse(val, fullKey);
        } else {
          result.push({ ID: fullKey, data: this._unwrap(val) });
        }
      }
    };
    traverse(this._store, '');
    return result;
  }

  filter(predicate)  { return this.all().filter(({ ID, data }) => predicate(data, ID)); }
  find(predicate)    { return this.all().find(({ ID, data })   => predicate(data, ID)) ?? null; }
  startsWith(prefix) { return this.all().filter(e => e.ID.startsWith(prefix)); }

  paginate(prefix, page = 1, limit = 10, sortBy = 'data', sortDesc = true) {
    const data  = this.startsWith(prefix).sort((a, b) => {
      const vA = a[sortBy], vB = b[sortBy];
      if (vA === vB) return 0;
      return sortDesc ? (vA < vB ? 1 : -1) : (vA > vB ? 1 : -1);
    });
    const total = data.length;
    const pages = Math.ceil(total / limit) || 1;
    const start = (page - 1) * limit;
    return { data: data.slice(start, start + limit), pagination: { page, limit, total, pages, hasNext: page < pages, hasPrev: page > 1 } };
  }

  table(name) {
    if (typeof name !== 'string' || name.length === 0) throw new TypeError('Table name must be a non-empty string');
    validateKey(name);
    return new Table(this, name);
  }

  async compact() { return this._queue.push(() => this._compact()); }

  async _compact() {
    if (this._closed) return;
    await this._rotateBackups();
    const plain = toPlainObject(this._store);
    let content = Buffer.from(JSON.stringify(plain), 'utf8');
    if (this._opts.compress) content = await asyncGzip(content, { level: 1 });
    const tmp = uniqueTmp(this._snapshotPath);
    try {
      await fsp.writeFile(tmp, content);
      await fsp.rename(tmp, this._snapshotPath);
    } catch (err) {
      await fsp.unlink(tmp).catch(() => {});
      throw err;
    }
    await this._wal.truncateAndReopen();
    this._walOps = 0;
    this.emit('save', this.getStats());
  }

  async _rotateBackups() {
    for (let gen = this._opts.backupCount; gen >= 1; gen--) {
      const src    = gen === 1 ? this._snapshotPath : `${this._snapshotPath}.${gen - 1}.bak`;
      const dst    = `${this._snapshotPath}.${gen}.bak`;
      const exists = await fsp.access(src).then(() => true, () => false);
      if (exists) await fsp.rename(src, dst).catch(() => {});
    }
  }

  async save()  { return this._queue.push(() => this._compact()); }

  async clear() {
    this._store = Object.create(null);
    this._cache.clear();
    this._queue.push(() => this._wal.append({ op: 'clear' }).then(() => { this._walOps++; }));
    this.emit('clear');
    return this;
  }

  async close() {
    if (this._closed) return;
    this._closed = true;
    if (this._compactTimer) { clearInterval(this._compactTimer); this._compactTimer = null; }
    await this._queue.push(async () => { if (this._walOps > 0) await this._compact(); });
    await this._wal.close();
    this.removeAllListeners();
  }

  getStats() {
    const fileSize = fs.existsSync(this._snapshotPath) ? fs.statSync(this._snapshotPath).size : 0;
    return {
      driver:           'spectre.db',
      compress:         this._opts.compress,
      encrypted:        this._encKey !== null,
      entries:          this.all().length,
      cacheSize:        this._cache.size,
      maxCacheSize:     this._opts.maxCacheSize,
      fileSize,
      walOps:           this._walOps,
      compactThreshold: this._opts.compactThreshold,
      shards:           0,
      snapshotPath:     this._snapshotPath,
      walPath:          this._walPath,
    };
  }
}

class Table {
  constructor(db, name) {
    this._db   = db;
    this._name = name;
  }

  _k(key)          { return `${this._name}.${key}`; }
  get(key)         { return this._db.get(this._k(key)); }
  set(key, value)  { return this._db.set(this._k(key), value); }
  has(key)         { return this._db.has(this._k(key)); }
  delete(key)      { return this._db.delete(this._k(key)); }
  add(key, n)      { return this._db.add(this._k(key), n); }
  sub(key, n)      { return this._db.sub(this._k(key), n); }
  push(key, value) { return this._db.push(this._k(key), value); }
  pull(key, pred)  { return this._db.pull(this._k(key), pred); }
  all()            { return this._db.startsWith(`${this._name}.`); }
  count()          { return this.all().length; }

  async clear() {
    return this._db.transaction(tx => { for (const { ID } of this.all()) tx.delete(ID); });
  }

  async transaction(fn) {
    return this._db.transaction(tx => {
      const scoped = {
        set:    (key, value) => tx.set(this._k(key), value),
        delete: (key)        => tx.delete(this._k(key)),
        add:    (key, n)     => tx.add(this._k(key), n),
        sub:    (key, n)     => tx.sub(this._k(key), n),
        push:   (key, value) => tx.push(this._k(key), value),
        pull:   (key, pred)  => tx.pull(this._k(key), pred),
        get:    (key)        => tx.get(this._k(key)),
      };
      return fn(scoped);
    });
  }
}

module.exports = { Engine, Table };