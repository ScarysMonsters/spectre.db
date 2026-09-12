'use strict';

const fsp = require('fs').promises;
const { ErrorCodes, createError } = require('../utils/error-codes');

/**
 * File lock for multi-process support
 * Prevents concurrent access to the database
 */
class FileLock {
  constructor(dbPath) {
    this._dbPath = dbPath;
    this._lockPath = `${dbPath}.lock`;
    this._lockFd = null;
    this._locked = false;
  }

  /**
   * Acquire the file lock
   * @param {number} [timeout=30000] - Timeout in milliseconds
   * @throws {Error} If lock cannot be acquired
   */
  async acquire(timeout = 30000) {
    if (this._locked) {
      throw createError(ErrorCodes.LOCK_ACQUISITION_FAILED, 'Lock already acquired');
    }

    const startTime = Date.now();

    while (true) {
      try {
        // Try to create the lock file exclusively
        this._lockFd = await fsp.open(this._lockPath, 'wx');
        this._locked = true;

        // Write the PID of the current process
        await this._lockFd.write(`${process.pid}\n`);
        await this._lockFd.sync();

        return;
      } catch (err) {
        if (err.code === 'EEXIST') {
          // Check if the lock is stale
          try {
            const content = await fsp.readFile(this._lockPath, 'utf8');
            const pid = parseInt(content.trim(), 10);

            // Check if the process exists
            try {
              process.kill(pid, 0);
            } catch (killErr) {
              // Process doesn't exist, we can take the lock
              await fsp.unlink(this._lockPath);
              continue;
            }
          } catch (readErr) {
            // Can't read the lock file, retry
          }

          // Check timeout
          if (Date.now() - startTime > timeout) {
            throw createError(
              ErrorCodes.LOCK_TIMEOUT,
              `Lock acquisition timeout after ${timeout}ms`
            );
          }

          // Wait a bit before retrying
          await new Promise((resolve) => setTimeout(resolve, 100));
        } else {
          throw createError(
            ErrorCodes.LOCK_ACQUISITION_FAILED,
            `Failed to acquire lock: ${err.message}`
          );
        }
      }
    }
  }

  /**
   * Release the file lock
   */
  async release() {
    if (!this._locked) {
      return;
    }

    try {
      if (this._lockFd) {
        await this._lockFd.close();
        this._lockFd = null;
      }

      await fsp.unlink(this._lockPath);
    } catch (err) {
      // Ignore errors during release
    } finally {
      this._locked = false;
    }
  }

  /**
   * Check if lock is acquired
   * @returns {boolean} True if locked
   */
  isLocked() {
    return this._locked;
  }

  /**
   * Get the lock file path
   * @returns {string} Lock file path
   */
  getLockPath() {
    return this._lockPath;
  }
}

module.exports = { FileLock };