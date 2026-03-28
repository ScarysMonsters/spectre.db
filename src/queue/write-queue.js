'use strict';

/**
 * Write queue for serializing async operations
 * Ensures operations are executed in order and propagates errors
 */
class WriteQueue {
  constructor() {
    this._chain = Promise.resolve();
    this._errorHandler = null;
    this._pending = new Set();
  }

  /**
   * Set the error handler
   * @param {Function} handler - Error handler function
   */
  setErrorHandler(handler) {
    this._errorHandler = handler;
  }

  /**
   * Push an operation to the queue
   * @param {Function} fn - Async function to execute
   * @returns {Promise} Promise that resolves when operation completes
   */
  push(fn) {
    const promise = this._chain.then(() => fn());
    this._chain = promise.catch((err) => {
      // Propagate error to handler
      if (this._errorHandler) {
        this._errorHandler(err);
      }
      // Don't suppress the error
      throw err;
    });

    // Clean up resolved promises
    promise.finally(() => {
      this._pending.delete(promise);
    });

    this._pending.add(promise);
    return promise;
  }

  /**
   * Flush all pending operations
   * @returns {Promise} Promise that resolves when all operations complete
   */
  async flush() {
    await this._chain;
  }

  /**
   * Clear the queue and cancel pending operations
   */
  clear() {
    this._pending.clear();
    this._chain = Promise.resolve();
  }

  /**
   * Get the number of pending operations
   * @returns {number} Number of pending operations
   */
  get pendingCount() {
    return this._pending.size;
  }
}

module.exports = { WriteQueue };