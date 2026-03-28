'use strict';

const { toNullProto, toPlainObject } = require('../utils/json-safe');
const { ErrorCodes, createError } = require('../utils/error-codes');

/**
 * Transaction for atomic operations
 * Provides rollback on error
 */
class Transaction {
  constructor(engine) {
    this._engine = engine;
    this._snapshot = null;
    this._ops = [];
    this._isActive = false;
  }

  /**
   * Begin a transaction
   * @throws {Error} If transaction is already active
   */
  async begin() {
    if (this._isActive) {
      throw createError(ErrorCodes.TRANSACTION_ACTIVE, 'Transaction already active');
    }

    this._isActive = true;
    this._snapshot = toNullProto(toPlainObject(this._engine._store));
  }

  /**
   * Commit the transaction
   * @returns {Promise<any>} Result of the transaction
   * @throws {Error} If transaction is not active or commit fails
   */
  async commit() {
    if (!this._isActive) {
      throw createError(ErrorCodes.TRANSACTION_NOT_ACTIVE, 'Transaction not active');
    }

    this._isActive = false;

    try {
      if (this._ops.length > 0) {
        await this._engine._wal.append({ op: 'batch', ops: this._ops });
        this._engine._walOps++;
      }

      this._engine.emit('transaction', { ops: this._ops });

      // Release snapshot
      this._snapshot = null;
      this._ops = [];

      return;
    } catch (err) {
      // Rollback on commit failure
      await this.rollback();
      throw createError(
        ErrorCodes.TRANSACTION_COMMIT_FAILED,
        `Transaction commit failed: ${err.message}`
      );
    }
  }

  /**
   * Rollback the transaction
   * @throws {Error} If transaction is not active
   */
  async rollback() {
    if (!this._isActive) {
      throw createError(ErrorCodes.TRANSACTION_NOT_ACTIVE, 'Transaction not active');
    }

    this._isActive = false;

    // Restore snapshot
    this._engine._store = toNullProto(this._snapshot);
    this._engine._cache.clear();
    this._engine.emit('rollback', { error: new Error('Transaction rolled back') });

    // Release snapshot
    this._snapshot = null;
    this._ops = [];
  }

  /**
   * Check if transaction is active
   * @returns {boolean} True if active
   */
  isActive() {
    return this._isActive;
  }

  /**
   * Get the operations in this transaction
   * @returns {Array} Array of operations
   */
  getOps() {
    return [...this._ops];
  }

  /**
   * Destroy the transaction and free resources
   */
  destroy() {
    this._snapshot = null;
    this._ops = [];
    this._isActive = false;
  }
}

module.exports = { Transaction };