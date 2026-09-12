'use strict';

/**
 * spectre.db v1.1.0 JavaScript engine — frozen fallback.
 *
 * v2 changes, all bug fixes required for correct behavior on modern Node.js:
 *   - a guard refusing to open databases written in the v2 binary format
 *     (.spdb / .spwal), with a clear migration message;
 *   - `ready` now awaits `_init()` (v1.1.0 resolved it early — see
 *     docs/FORMATS.md §8);
 *   - single-flight WAL open (Node 26 turns FileHandles collected by the GC
 *     into hard errors).
 */

const fs = require('fs');
const path = require('path');
const { Engine, Table } = require('./src/core/engine');

function normalizePath(filePath) {
  return path
    .resolve(filePath)
    .replace(/\.json\.gz$/i, '')
    .replace(/\.json$/i, '')
    .replace(/\.gz$/i, '');
}

function mapLegacyOptions(opts) {
  const mapped = {};

  if (opts.maxCacheSize !== undefined) mapped.maxCacheSize = opts.maxCacheSize;
  if (opts.cacheTTL !== undefined) mapped.cacheTTL = opts.cacheTTL;
  if (opts.compress !== undefined) mapped.compress = opts.compress;
  if (opts.encryptionKey !== undefined) mapped.encryptionKey = opts.encryptionKey;
  if (opts.backupCount !== undefined) mapped.backupCount = opts.backupCount;
  if (opts.encryptBackups !== undefined) mapped.encryptBackups = opts.encryptBackups;

  if (opts.autoSave !== undefined) {
    mapped.compactInterval = opts.autoSave > 0 ? opts.autoSave : 0;
    mapped.compactThreshold = opts.autoSave > 0 ? 50 : Infinity;
  }

  if (opts.backup === false) mapped.backupCount = 0;
  if (opts.cache === false) mapped.maxCacheSize = 0;

  if (opts.compactThreshold !== undefined) mapped.compactThreshold = opts.compactThreshold;
  if (opts.compactInterval !== undefined) mapped.compactInterval = opts.compactInterval;

  return mapped;
}

class Database extends Engine {
  constructor(filePath = './database.json', options = {}) {
    const cleanPath = normalizePath(filePath);
    const guardBase = cleanPath.replace(/\.(json|db|snapshot)$/i, '');

    // ---- v2 guard -------------------------------------------------------
    for (const marker of [`${guardBase}.spdb`, `${guardBase}.spwal`]) {
      if (fs.existsSync(marker)) {
        const err = new Error(
          'This database uses the spectre.db v2 binary format (.spdb/.spwal). ' +
            'The JavaScript fallback engine cannot open it. Use the native engine ' +
            '(rebuild with `npm run build`) or migrate the database back to JSON ' +
            'format with the native engine: `await db.migrate("json")`.'
        );
        err.code = 3000;
        err.name = 'SNAPSHOT_CORRUPTED';
        throw err;
      }
    }
    // ----------------------------------------------------------------------

    const opts = mapLegacyOptions(options);

    super(cleanPath, opts);

    this._legacyWarmKeys = Array.isArray(options.warmKeys) ? options.warmKeys : [];

    // Chain warmKeys loading after ready
    this._readyPromise = this._readyPromise.then(() => {
      for (const key of this._legacyWarmKeys) {
        try {
          this.get(key);
        } catch (_) {}
      }
    });
  }

  get ready() {
    return this._readyPromise;
  }

  transaction(arg) {
    if (typeof arg === 'function') {
      return super.transaction(arg);
    }

    if (Array.isArray(arg)) {
      return super.transaction((tx) => {
        const results = [];
        for (const op of arg) {
          switch (op.type) {
            case 'set':
              results.push(tx.set(op.key, op.value));
              break;
            case 'delete':
              results.push(tx.delete(op.key));
              break;
            case 'add':
              results.push(tx.add(op.key, op.value));
              break;
            case 'sub':
              results.push(tx.sub(op.key, op.value));
              break;
            case 'push':
              results.push(tx.push(op.key, op.value));
              break;
            case 'pull':
              results.push(tx.pull(op.key, op.value));
              break;
            default:
              throw new Error(`[spectre.db] Unknown operation type: "${op.type}"`);
          }
        }
        return results;
      });
    }

    throw new TypeError('[spectre.db] transaction() expects an array of operations or an async function');
  }
}

module.exports = { Database, Table };
