'use strict';

const fsp = require('fs').promises;
const { ErrorCodes, createError } = require('../utils/error-codes');

/**
 * WAL (Write-Ahead Log) writer
 * Handles append-only writes to the WAL file
 */
class WALWriter {
  constructor(walPath) {
    this._path = walPath;
    this._fd = null;
    this._isOpen = false;
  }

  /**
   * Open the WAL file for appending
   * @throws {Error} If file cannot be opened
   */
  async open() {
    if (this._isOpen) return;

    try {
      this._fd = await fsp.open(this._path, 'a');
      this._isOpen = true;
    } catch (err) {
      throw createError(ErrorCodes.WRITE_FAILED, `Failed to open WAL: ${err.message}`);
    }
  }

  /**
   * Append an entry to the WAL
   * @param {Object} entry - Entry to append
   * @throws {Error} If write fails
   */
  async append(entry) {
    // Ensure WAL is open
    if (!this._isOpen || !this._fd) {
      await this.open();
    }

    // Validate entry structure
    validateWALEntry(entry);

    try {
      await this._fd.write(JSON.stringify(entry) + '\n');
    } catch (err) {
      throw createError(ErrorCodes.WRITE_FAILED, `Failed to write to WAL: ${err.message}`);
    }
  }

  /**
   * Truncate the WAL and reopen it
   * @throws {Error} If operation fails
   */
  async truncateAndReopen() {
    if (this._isOpen && this._fd) {
      await this._fd.close();
      this._fd = null;
      this._isOpen = false;
    }

    try {
      await fsp.writeFile(this._path, '');
      this._fd = await fsp.open(this._path, 'a');
      this._isOpen = true;
    } catch (err) {
      throw createError(ErrorCodes.WRITE_FAILED, `Failed to truncate WAL: ${err.message}`);
    }
  }

  /**
   * Close the WAL file
   */
  async close() {
    if (this._fd) { await this._fd.close(); this._fd = null; }
    this._isOpen = false;
  }

  /**
   * Check if WAL is open
   * @returns {boolean} True if open
   */
  isOpen() {
    return this._isOpen;
  }
}

/**
 * Validate a WAL entry structure
 * @param {Object} entry - Entry to validate
 * @throws {Error} If entry is invalid
 */
function validateWALEntry(entry) {
  if (!entry || typeof entry !== 'object') {
    throw createError(ErrorCodes.WAL_CORRUPTED, 'Invalid WAL entry: not an object');
  }

  if (!entry.op || typeof entry.op !== 'string') {
    throw createError(ErrorCodes.WAL_CORRUPTED, 'Invalid WAL entry: missing or invalid op');
  }

  const validOps = ['set', 'del', 'clear', 'batch'];
  if (!validOps.includes(entry.op)) {
    throw createError(ErrorCodes.WAL_CORRUPTED, `Invalid WAL operation: ${entry.op}`);
  }

  if (entry.op === 'set') {
    if (entry.k === undefined || entry.v === undefined) {
      throw createError(ErrorCodes.WAL_CORRUPTED, 'Invalid WAL set entry: missing key or value');
    }
  }

  if (entry.op === 'del') {
    if (entry.k === undefined) {
      throw createError(ErrorCodes.WAL_CORRUPTED, 'Invalid WAL delete entry: missing key');
    }
  }

  if (entry.op === 'batch') {
    if (!Array.isArray(entry.ops)) {
      throw createError(ErrorCodes.WAL_CORRUPTED, 'Invalid WAL batch entry: ops is not an array');
    }

    // Validate each operation in the batch
    for (const op of entry.ops) {
      validateWALEntry(op);
    }
  }
}

module.exports = { WALWriter, validateWALEntry };