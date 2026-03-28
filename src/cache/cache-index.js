'use strict';

/**
 * Cache index for prefix-based invalidation
 * Tracks which keys belong to which prefixes
 */
class CacheIndex {
  constructor() {
    this._index = new Map();
    this._keyCount = new Map();
  }

  /**
   * Add a key to the index
   * @param {string} key - Key to add
   */
  add(key) {
    const parts = key.split('.');
    for (let i = 1; i <= parts.length; i++) {
      const prefix = parts.slice(0, i).join('.');

      if (!this._index.has(prefix)) {
        this._index.set(prefix, new Set());
        this._keyCount.set(prefix, 0);
      }

      const set = this._index.get(prefix);
      if (!set.has(key)) {
        set.add(key);
        this._keyCount.set(prefix, this._keyCount.get(prefix) + 1);
      }
    }
  }

  /**
   * Remove a key from the index
   * @param {string} key - Key to remove
   */
  remove(key) {
    const parts = key.split('.');
    for (let i = 1; i <= parts.length; i++) {
      const prefix = parts.slice(0, i).join('.');
      const set = this._index.get(prefix);

      if (set && set.has(key)) {
        set.delete(key);
        const count = this._keyCount.get(prefix) - 1;

        if (count <= 0) {
          this._index.delete(prefix);
          this._keyCount.delete(prefix);
        } else {
          this._keyCount.set(prefix, count);
        }
      }
    }
  }

  /**
   * Get all keys for a prefix
   * @param {string} prefix - Prefix to look up
   * @returns {Set<string>|undefined} Set of keys
   */
  get(prefix) {
    return this._index.get(prefix);
  }

  /**
   * Check if a prefix exists in the index
   * @param {string} prefix - Prefix to check
   * @returns {boolean} True if prefix exists
   */
  has(prefix) {
    return this._index.has(prefix);
  }

  /**
   * Clear the entire index
   */
  clear() {
    this._index.clear();
    this._keyCount.clear();
  }

  /**
   * Get the size of the index
   * @returns {number} Number of prefixes
   */
  get size() {
    return this._index.size;
  }

  /**
   * Get the total number of keys tracked
   * @returns {number} Total key count
   */
  getTotalKeyCount() {
    let total = 0;
    for (const count of this._keyCount.values()) {
      total += count;
    }
    return total;
  }
}

module.exports = { CacheIndex };