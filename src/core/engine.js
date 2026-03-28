'use strict';

const fs = require('fs');
const fsp = fs.promises;
const path = require('path');
const zlib = require('zlib');
const crypto = require('crypto');
const { promisify } = require('util');

const asyncGzip = promisify(zlib.gzip);
const asyncGunzip = promisify(zlib.gunzip);

// Import refactored modules
const { SafeEventEmitter } = require('../events/event-emitter');
const { LRUCache } = require('../cache/lru-cache');
const { WriteQueue } = require('../queue/write-queue');
const { WALWriter, validateWALEntry } = require('../storage/wal');
const { FileLock } = require('../storage/lock');
const { BackupManager } = require('../storage/backup');
const { Transaction } = require('../transaction/transaction');
const { validateKey, validateValue, isSensitiveKey } = require('../utils/validator');
const { sanitizePath, getPathComponents } = require('../utils/path-normalizer');
const { toNullProto, toPlainObject, safeStringify } = require('../utils/json-safe');
const { deriveKey, zeroKey } = require('../crypto/key-derivation');
const { encryptValue, decryptValue, isEncrypted, refillIVPool } = require('../crypto/encryption');
const { ErrorCodes, createError } = require('../utils/error-codes');

/**
 * Generate a unique temporary file path
 * @param {string} base - Base path
 * @returns {string} Temporary file path
 */
function uniqueTmp(base) {
  const rand = crypto.randomBytes(6).toString('hex');
  return `${base}.${process.pid}.${Date.now()}.${rand}.tmp`;
}

/**
 * Engine class - Core database engine
 * Orchestrates all modules for database operations
 */
class Engine extends SafeEventEmitter {
  constructor(dbPath, options = {}) {
    super(100); // Max 100 listeners

    this._opts = {
      maxCacheSize: 1000,
      cacheTTL: 0,
      compactThreshold: 500,
      compactInterval: 5 * 60 * 1000,
      compress: false,
      encryptionKey: null,
      encryptBackups: false,
      backupCount: 3,
      ...options,
    };

    this._encKey = null;
    this._ready = false;
    this._readyPromise = Promise.resolve();
    this._closed = false;
    this._destroyed = false;
    this._lockAcquired = false;
    this._timerCreated = false;

    // Initialize components
    this._readyPromise = this._initComponents(dbPath);
  }

  /**
   * Initialize all components
   * @param {string} dbPath - Database path
   */
  async _initComponents(dbPath) {
    // Sanitize path
    const sanitizedPath = sanitizePath(dbPath);
    const { dir, base } = getPathComponents(sanitizedPath);

    this._dir = dir;
    this._base = base;
    this._snapshotPath = path.join(dir, `${base}.snapshot`);
    this._walPath = path.join(dir, `${base}.wal`);

    // Initialize encryption key
    if (this._opts.encryptionKey) {
      this._encKey = await deriveKey(this._opts.encryptionKey);
    }

    // Initialize storage components
    this._lock = new FileLock(sanitizedPath);
    this._wal = new WALWriter(this._walPath);
    this._backupManager = new BackupManager(this._snapshotPath, {
      backupCount: this._opts.backupCount,
      compress: this._opts.compress,
      encrypt: this._opts.encryptBackups,
      encryptionKey: this._encKey,
    });

    // Initialize cache
    this._cache = new LRUCache(this._opts.maxCacheSize, {
      compress: this._opts.compress,
      compressionThreshold: 1024,
    });

    // Initialize queue
    this._queue = new WriteQueue();
    this._queue.setErrorHandler((err) => {
      this.emit('error', err);
    });

    // Initialize state
    this._store = Object.create(null);
    this._walOps = 0;
    this._compactTimer = null;
    
    // Start initialization
    this._readyPromise = this._init();
    this._readyPromise.then(() => {
      this._ready = true;
    });
    }
    
    /**
     * Check if the database is ready and not closed
     * @throws {Error} If database is closed or not ready
     */
    _checkState() {
      if (this._closed) {
        throw createError(ErrorCodes.DATABASE_CLOSED, 'Database is closed');
      }
      if (!this._ready) {
        throw createError(ErrorCodes.DATABASE_NOT_READY, 'Database is not ready');
      }
    }

  /**
   * Initialize the database
   * @returns {Promise<void>}
   */
  async _init() {
    try {
      // Create directory
      await fsp.mkdir(this._dir, { recursive: true });
      
      // Acquire lock
      await this._lock.acquire();
      this._lockAcquired = true;
      
      // Load snapshot
      await this._loadSnapshot();
      
      // Replay WAL
      this._walOps = await this._replayWAL();
      
      // Open WAL
      await this._wal.open();
      
      // Setup compaction timer
      if (this._opts.compactInterval > 0) {
        this._compactTimer = setInterval(() => {
          if (this._walOps >= this._opts.compactThreshold) {
            this._queue.push(() => this._compact());
          }
        }, this._opts.compactInterval);
        this._compactTimer.unref?.();
        this._timerCreated = true;
      }
    } catch (err) {
      // Release lock on error
      if (this._lockAcquired) {
        await this._lock.release();
        this._lockAcquired = false;
      }
      throw err;
    }
  }

  /**
   * Ensure the database is ready
   * @returns {Promise<void>}
   */
  async _ensureReady() {
    if (!this._ready) {
      await this._readyPromise;
    }
  }

  /**
   * Load snapshot from disk
   * @returns {Promise<void>}
   */
  async _loadSnapshot() {
    const exists = await fsp
      .access(this._snapshotPath)
      .then(() => true)
      .catch(() => false);
    if (!exists) return;

    try {
      let buf = await fsp.readFile(this._snapshotPath);
      if (this._opts.compress) buf = await asyncGunzip(buf);
      this._store = toNullProto(JSON.parse(buf.toString('utf8')));
    } catch (err) {
      this.emit('warn', 'Snapshot corrupted, attempting backup restore');
      await this._restoreFromBackup();
    }
  }

  /**
   * Restore from backup
   * @returns {Promise<void>}
   */
  async _restoreFromBackup() {
    for (let gen = 1; gen <= this._opts.backupCount; gen++) {
      const p = this._backupManager.getBackupPath(gen);
      const exists = await this._backupManager.backupExists(gen);
      if (!exists) continue;

      try {
        let buf = await this._backupManager.decryptBackup(p);
        if (this._opts.compress) buf = await asyncGunzip(buf);
        this._store = toNullProto(JSON.parse(buf.toString('utf8')));
        this.emit('restore', { generation: gen, path: p });
        return;
      } catch (err) {
        this.emit('warn', `Failed to restore backup ${gen}: ${err.message}`);
      }
    }

    this._store = Object.create(null);
    this.emit('reset');
  }

  /**
   * Replay WAL entries
   * @returns {Promise<number>} Number of replayed entries
   */
  async _replayWAL() {
    const exists = await fsp
      .access(this._walPath)
      .then(() => true)
      .catch(() => false);
    if (!exists) return 0;

    let text;
    try {
      text = await fsp.readFile(this._walPath, 'utf8');
    } catch (err) {
      return 0;
    }

    let count = 0;
    for (const line of text.split('\n')) {
      const trimmed = line.trim();
      if (!trimmed) continue;

      try {
        const entry = JSON.parse(trimmed);
        validateWALEntry(entry);
        this._applyEntry(entry);
        count++;
      } catch (err) {
        this.emit('warn', `Failed to replay WAL entry: ${err.message}`);
      }
    }

    return count;
  }

  /**
   * Apply a WAL entry to the store
   * @param {Object} entry - WAL entry
   */
  _applyEntry(entry) {
    switch (entry.op) {
      case 'set':
        this._applySet(entry.k.split('.'), entry.v);
        break;
      case 'del':
        this._applyDelete(entry.k.split('.'));
        break;
      case 'clear':
        this._store = Object.create(null);
        break;
      case 'batch':
        for (const op of entry.ops) {
          this._applyEntry(op);
        }
        break;
    }
  }

  /**
   * Apply a set operation
   * @param {Array<string>} parts - Key parts
   * @param {*} value - Value to set
   */
  _applySet(parts, value) {
    let node = this._store;
    for (let i = 0; i < parts.length - 1; i++) {
      const part = parts[i];
      if (
        node[part] === null ||
        node[part] === undefined ||
        typeof node[part] !== 'object' ||
        Array.isArray(node[part])
      ) {
        node[part] = Object.create(null);
      }
      node = node[part];
    }
    node[parts[parts.length - 1]] = toNullProto(value);
  }

  /**
   * Apply a delete operation
   * @param {Array<string>} parts - Key parts
   * @returns {boolean} True if deleted
   */
  _applyDelete(parts) {
    let node = this._store;
    for (let i = 0; i < parts.length - 1; i++) {
      if (!node || typeof node[parts[i]] !== 'object' || Array.isArray(node[parts[i]])) {
        return false;
      }
      node = node[parts[i]];
    }
    const last = parts[parts.length - 1];
    if (!node || !(last in node)) return false;
    delete node[last];
    return true;
  }

  /**
   * Read a raw value from the store
   * @param {string} key - Key to read
   * @returns {*} Value or undefined
   */
  _readRaw(key) {
    const parts = key.split('.');
    let node = this._store;
    for (const part of parts) {
      if (node === null || node === undefined || typeof node !== 'object') return undefined;
      node = node[part];
    }
    return node;
  }

  /**
   * Unwrap an encrypted value
   * @param {*} value - Value to unwrap
   * @returns {*} Unwrapped value
   */
  _unwrap(value) {
    if (this._encKey && isEncrypted(value)) {
      return decryptValue(value, this._encKey);
    }
    return value;
  }

  /**
   * Wrap a value for encryption if needed
   * @param {string} key - Key
   * @param {*} value - Value to wrap
   * @returns {*} Wrapped value
   */
  _wrap(key, value) {
    if (this._encKey && isSensitiveKey(key) && value !== null && value !== undefined) {
      return encryptValue(value, this._encKey);
    }
    return value;
  }

  /**
   * Get a value from the database
   * @param {string} key - Key to get
   * @returns {*} Value or null
   */
  get(key) {
    validateKey(key);

    const cached = this._cache.get(key);
    if (cached !== undefined) return cached;

    const raw = this._readRaw(key);
    if (raw === undefined) return null;

    const value = this._unwrap(raw);
    if (value !== null && value !== undefined) {
      this._cache.set(key, value, this._opts.cacheTTL);
    }

    return value ?? null;
  }

  /**
   * Set a value in the database
   * @param {string} key - Key to set
   * @param {*} value - Value to set
   * @returns {*} Set value
   */
  set(key, value) {
    validateKey(key);
    validateValue(value);

    const stored = this._wrap(key, value);

    // Update store
    this._applySet(key.split('.'), stored);

    // Invalidate cache
    this._cache.invalidate(key);

    // Increment WAL ops counter
    this._walOps++;

    // Queue WAL write
    this._queue.push(() =>
      this._wal.append({ op: 'set', k: key, v: stored }).catch((err) => {
        this._walOps--;
        throw err;
      })
    );

    this.emit('change', { type: 'set', key, value });
    return value;
  }

  /**
   * Delete a value from the database
   * @param {string} key - Key to delete
   * @returns {boolean} True if deleted
   */
  async delete(key) {
    await this._ensureReady();
    validateKey(key);

    const deleted = this._applyDelete(key.split('.'));
    if (!deleted) return false;

    // Invalidate cache
    this._cache.invalidate(key);

    // Increment WAL ops counter
    this._walOps++;

    // Queue WAL write
    this._queue.push(() =>
      this._wal.append({ op: 'del', k: key }).catch((err) => {
        this._walOps--;
        throw err;
      })
    );

    this.emit('change', { type: 'delete', key });
    return true;
  }

  /**
   * Check if a key exists
   * @param {string} key - Key to check
   * @returns {boolean} True if exists
   */
  async has(key) {
    return (await this.get(key)) !== null;
  }

  /**
   * Add a number to a value
   * @param {string} key - Key
   * @param {number} n - Number to add
   * @returns {number} New value
   */
  async add(key, n) {
    if (typeof n !== 'number' || !isFinite(n)) {
      throw new TypeError('add() requires a finite number');
    }
    const cur = (await this.get(key)) ?? 0;
    if (typeof cur !== 'number') {
      throw new TypeError(`Value at "${key}" is not a number`);
    }
    return this.set(key, cur + n);
  }

  /**
   * Subtract a number from a value
   * @param {string} key - Key
   * @param {number} n - Number to subtract
   * @returns {number} New value
   */
  async sub(key, n) {
    return this.add(key, -n);
  }

  /**
   * Push a value to an array
   * @param {string} key - Key
   * @param {*} value - Value to push
   * @returns {number} New array length
   */
  async push(key, value) {
    const arr = (await this.get(key)) ?? [];
    if (!Array.isArray(arr)) {
      throw new TypeError(`Value at "${key}" is not an array`);
    }
    arr.push(value);
    return this.set(key, arr);
  }

  /**
   * Pull a value from an array
   * @param {string} key - Key
   * @param {*} predicate - Predicate or value to pull
   * @returns {boolean} True if pulled
   */
  async pull(key, predicate) {
    const arr = (await this.get(key)) ?? [];
    if (!Array.isArray(arr)) {
      throw new TypeError(`Value at "${key}" is not an array`);
    }
    const test =
      typeof predicate === 'function'
        ? predicate
        : (v) => safeStringify(v) === safeStringify(predicate);
    const index = arr.findIndex(test);
    if (index === -1) return false;
    arr.splice(index, 1);
    return this.set(key, arr);
  }

  /**
   * Execute a transaction
   * @param {Function} fn - Transaction function
   * @returns {Promise<any>} Transaction result
   */
  async transaction(fn) {
    await this._ensureReady();

    return this._queue.push(async () => {
      const tx = new Transaction(this);
      await tx.begin();

      const ops = [];
      const scopedTx = {
        set: (key, value) => {
          validateKey(key);
          validateValue(value);
          const stored = this._wrap(key, value);
          this._applySet(key.split('.'), stored);
          this._cache.invalidate(key);
          ops.push({ op: 'set', k: key, v: stored });
          return value;
        },
        delete: (key) => {
          validateKey(key);
          const ok = this._applyDelete(key.split('.'));
          if (ok) {
            this._cache.invalidate(key);
            ops.push({ op: 'del', k: key });
          }
          return ok;
        },
        get: (key) => this.get(key),
        add: async (key, n) => {
          const cur = (await scopedTx.get(key)) ?? 0;
          if (typeof cur !== 'number') {
            throw new TypeError(`Value at "${key}" is not a number`);
          }
          return scopedTx.set(key, cur + n);
        },
        sub: (key, n) => scopedTx.add(key, -n),
        push: async (key, value) => {
          const arr = (await scopedTx.get(key)) ?? [];
          if (!Array.isArray(arr)) {
            throw new TypeError(`Value at "${key}" is not an array`);
          }
          arr.push(value);
          return scopedTx.set(key, arr);
        },
        pull: async (key, pred) => {
          const arr = (await scopedTx.get(key)) ?? [];
          if (!Array.isArray(arr)) {
            throw new TypeError(`Value at "${key}" is not an array`);
          }
          const test =
            typeof pred === 'function'
              ? pred
              : (v) => safeStringify(v) === safeStringify(pred);
          const i = arr.findIndex(test);
          if (i === -1) return false;
          arr.splice(i, 1);
          return scopedTx.set(key, arr);
        },
      };

      try {
        const result = await fn(scopedTx);
        await tx.commit();
        return result;
      } catch (err) {
        await tx.rollback();
        throw err;
      } finally {
        tx.destroy();
      }
    });
  }

  /**
   * Get all entries
   * @param {Object} [options] - Options
   * @returns {Array<{ID: string, data: *}>} All entries
   */
  all(options = {}) {
    const { limit = Infinity, offset = 0 } = options;
    const result = [];
    let count = 0;
    let skipped = 0;

    const traverse = (obj, prefix) => {
      if (count >= limit) return;

      for (const key of Object.keys(obj)) {
        if (count >= limit) return;

        const fullKey = prefix ? `${prefix}.${key}` : key;
        const val = obj[key];

        if (val !== null && typeof val === 'object' && !Array.isArray(val) && !isEncrypted(val)) {
          traverse(val, fullKey);
        } else {
          if (skipped < offset) {
            skipped++;
            continue;
          }

          result.push({ ID: fullKey, data: this._unwrap(val) });
          count++;
        }
      }
    };

    traverse(this._store, '');
    return result;
  }

  /**
   * Filter entries
   * @param {Function} predicate - Predicate function
   * @returns {Array<{ID: string, data: *}>} Filtered entries
   */
  filter(predicate) {
    return this.all().filter(({ ID, data }) => predicate(data, ID));
  }

  /**
   * Find an entry
   * @param {Function} predicate - Predicate function
   * @returns {{ID: string, data: *}|null} Found entry or null
   */
  find(predicate) {
    return this.all().find(({ ID, data }) => predicate(data, ID)) ?? null;
  }

  /**
   * Get entries starting with a prefix
   * @param {string} prefix - Prefix
   * @returns {Array<{ID: string, data: *}>} Matching entries
   */
  startsWith(prefix) {
    return this.all().filter((e) => e.ID.startsWith(prefix));
  }

  /**
   * Paginate entries
   * @param {string} prefix - Prefix
   * @param {number} page - Page number
   * @param {number} limit - Page size
   * @param {string} sortBy - Sort field
   * @param {boolean} sortDesc - Sort descending
   * @returns {{data: Array, pagination: Object}} Paginated result
   */
  paginate(prefix, page = 1, limit = 10, sortBy = 'data', sortDesc = true) {
    const offset = (page - 1) * limit;
    const data = this
      .startsWith(prefix)
      .sort((a, b) => {
        const vA = a[sortBy],
          vB = b[sortBy];
        if (vA === vB) return 0;
        return sortDesc ? (vA < vB ? 1 : -1) : (vA > vB ? 1 : -1);
      });

    const total = data.length;
    const pages = Math.ceil(total / limit) || 1;
    const paginatedData = data.slice(offset, offset + limit);

    return {
      data: paginatedData,
      pagination: {
        page,
        limit,
        total,
        pages,
        hasNext: page < pages,
        hasPrev: page > 1,
      },
    };
  }

  /**
   * Create a table
   * @param {string} name - Table name
   * @returns {Table} Table instance
   */
  table(name) {
    if (typeof name !== 'string' || name.length === 0) {
      throw new TypeError('Table name must be a non-empty string');
    }
    validateKey(name);
    return new Table(this, name);
  }

  /**
   * Compact the database
   * @returns {Promise<void>}
   */
  async compact() {
    return this._queue.push(() => this._compact());
  }

  /**
   * Internal compact implementation
   * @returns {Promise<void>}
   */
  async _compact() {
    if (this._closed) return;

    // Rotate backups
    await this._backupManager.rotateBackups();

    // Serialize store
    const plain = toPlainObject(this._store);
    let content = Buffer.from(safeStringify(plain), 'utf8');

    // Compress if enabled
    if (this._opts.compress) {
      content = await asyncGzip(content, { level: 1 });
    }

    // Write to temp file
    const tmp = uniqueTmp(this._snapshotPath);
    try {
      await fsp.writeFile(tmp, content);
      await fsp.rename(tmp, this._snapshotPath);

      // Encrypt backups if enabled
      for (let gen = 1; gen <= this._opts.backupCount; gen++) {
        const backupPath = this._backupManager.getBackupPath(gen);
        try {
          await this._backupManager.encryptBackup(backupPath);
        } catch (err) {
          this.emit('warn', `Failed to encrypt backup ${gen}: ${err.message}`);
        }
      }

      // Truncate WAL
      await this._wal.truncateAndReopen();
      this._walOps = 0;

      this.emit('save', await this.getStats());
    } catch (err) {
      await fsp.unlink(tmp).catch(() => {});
      throw err;
    }
  }

  /**
   * Save the database
   * @returns {Promise<void>}
   */
  async save() {
    return this._queue.push(() => this._compact());
  }

  /**
   * Clear the database
   * @returns {Promise<Engine>} This engine
   */
  async clear() {
    this._store = Object.create(null);
    this._cache.clear();

    this._walOps++;
    this._queue.push(() =>
      this._wal.append({ op: 'clear' }).catch((err) => {
        this._walOps--;
        throw err;
      })
    );

    this.emit('clear');
    return this;
  }

  /**
   * Close the database
   * @returns {Promise<void>}
   */
  async close() {
    if (this._closed || this._destroyed) return;
    this._closed = true;

    // Clear timer
    if (this._compactTimer) {
      clearInterval(this._compactTimer);
      this._compactTimer = null;
    }

    // Flush queue
    await this._queue.flush();

    // Close WAL
    await this._wal.close();

    // Remove all listeners
    this.removeAllListeners();

    // Destroy cache
    this._cache.destroy();

    // Clear queue
    this._queue.clear();

    // Release lock
    if (this._lockAcquired) {
      await this._lock.release();
      this._lockAcquired = false;
    }

    // Zero encryption key
    if (this._encKey) {
      zeroKey(this._encKey);
      this._encKey = null;
    }

    this._destroyed = true;
  }

  /**
   * Get database statistics
   * @returns {Promise<Object>} Statistics
   */
  async getStats() {
    const fileSize = await fsp
      .access(this._snapshotPath)
      .then(() => fsp.stat(this._snapshotPath).then((stats) => stats.size), () => 0);

    return {
      driver: 'spectre.db',
      compress: this._opts.compress,
      encrypted: this._encKey !== null,
      entries: this.all().length,
      cacheSize: this._cache.size,
      maxCacheSize: this._opts.maxCacheSize,
      fileSize,
      walOps: this._walOps,
      compactThreshold: this._opts.compactThreshold,
      shards: 0,
      snapshotPath: this._snapshotPath,
      walPath: this._walPath,
    };
  }

  /**
   * Check if database is ready
   * @returns {boolean} True if ready
   */
  isReady() {
    return this._ready;
  }

  /**
   * Check if database is closed
   * @returns {boolean} True if closed
   */
  isClosed() {
    return this._closed;
  }
}

/**
 * Table class - Scoped key namespacing
 */
class Table {
  constructor(db, name) {
    this._db = db;
    this._name = name;
  }

  /**
   * Get full key for table
   * @param {string} key - Key
   * @returns {string} Full key
   */
  _k(key) {
    return `${this._name}.${key}`;
  }

  /**
   * Get a value
   * @param {string} key - Key
   * @returns {Promise<*>} Value
   */
  get(key) {
    return this._db.get(this._k(key));
  }

  /**
   * Set a value
   * @param {string} key - Key
   * @param {*} value - Value
   * @returns {Promise<*>} Set value
   */
  set(key, value) {
    return this._db.set(this._k(key), value);
  }

  /**
   * Check if key exists
   * @param {string} key - Key
   * @returns {Promise<boolean>} True if exists
   */
  has(key) {
    return this._db.has(this._k(key));
  }

  /**
   * Delete a value
   * @param {string} key - Key
   * @returns {Promise<boolean>} True if deleted
   */
  delete(key) {
    return this._db.delete(this._k(key));
  }

  /**
   * Add to a number
   * @param {string} key - Key
   * @param {number} n - Number to add
   * @returns {Promise<number>} New value
   */
  add(key, n) {
    return this._db.add(this._k(key), n);
  }

  /**
   * Subtract from a number
   * @param {string} key - Key
   * @param {number} n - Number to subtract
   * @returns {Promise<number>} New value
   */
  sub(key, n) {
    return this._db.sub(this._k(key), n);
  }

  /**
   * Push to an array
   * @param {string} key - Key
   * @param {*} value - Value to push
   * @returns {Promise<number>} New array length
   */
  push(key, value) {
    return this._db.push(this._k(key), value);
  }

  /**
   * Pull from an array
   * @param {string} key - Key
   * @param {*} predicate - Predicate or value
   * @returns {Promise<boolean>} True if pulled
   */
  pull(key, predicate) {
    return this._db.pull(this._k(key), predicate);
  }

  /**
   * Get all entries in table
   * @returns {Array<{ID: string, data: *}>} All entries
   */
  all() {
    return this._db.startsWith(`${this._name}.`);
  }

  /**
   * Count entries in table
   * @returns {number} Number of entries
   */
  count() {
    return this.all().length;
  }

  /**
   * Clear all entries in table
   * @returns {Promise<void>}
   */
  async clear() {
    return this._db.transaction((tx) => {
      for (const { ID } of this.all()) {
        tx.delete(ID);
      }
    });
  }

  /**
   * Execute a transaction
   * @param {Function} fn - Transaction function
   * @returns {Promise<any>} Transaction result
   */
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

module.exports = { Engine, Table };
