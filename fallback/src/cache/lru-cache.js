'use strict';

const { CacheIndex } = require('./cache-index');
const { ErrorCodes, createError } = require('../utils/error-codes');

// Maximum value size (10MB)
const MAX_VALUE_SIZE = 10 * 1024 * 1024;

/**
 * LRU Cache node
 */
class LRUNode {
  constructor(key, value, expiry) {
    this.key = key;
    this.value = value;
    this.expiry = expiry;
    this.prev = null;
    this.next = null;
  }
}

/**
 * LRU Cache with TTL support and prefix-based invalidation
 */
class LRUCache {
  constructor(maxSize, options = {}) {
    this._max = maxSize;
    this._compress = options.compress || false;
    this._compressionThreshold = options.compressionThreshold || 1024; // 1KB

    this._map = new Map();
    this._index = new CacheIndex();

    // Create dummy head and tail nodes
    this._head = new LRUNode(null, null, -1);
    this._tail = new LRUNode(null, null, -1);
    this._head.next = this._tail;
    this._tail.prev = this._head;
  }

  /**
   * Attach a node to the head of the list (most recently used)
   * @param {LRUNode} node - Node to attach
   */
  _attach(node) {
    node.next = this._head.next;
    node.prev = this._head;
    this._head.next.prev = node;
    this._head.next = node;
  }

  /**
   * Detach a node from the list
   * @param {LRUNode} node - Node to detach
   */
  _detach(node) {
    node.prev.next = node.next;
    node.next.prev = node.prev;
    node.prev = null;
    node.next = null;
  }

  /**
   * Evict a node from the cache
   * @param {LRUNode} node - Node to evict
   */
  _evict(node) {
    this._detach(node);
    this._map.delete(node.key);
    this._index.remove(node.key);
  }

  /**
   * Get a value from the cache
   * @param {string} key - Key to get
   * @returns {*} Value or undefined if not found or expired
   */
  get(key) {
    const node = this._map.get(key);
    if (!node) return undefined;

    // Check expiry
    if (node.expiry > 0 && Date.now() > node.expiry) {
      this._evict(node);
      return undefined;
    }

    // Move to head (most recently used)
    this._detach(node);
    this._attach(node);

    // Decompress if necessary
    let value = node.value;
    if (value && value.__compressed) {
      try {
        const { decompress } = require('lz4');
        const decompressed = decompress(value.data);
        value = JSON.parse(decompressed.toString('utf8'));
      } catch (err) {
        // Fallback to compressed value
      }
    }

    return value;
  }

  /**
   * Set a value in the cache
   * @param {string} key - Key to set
   * @param {*} value - Value to set
   * @param {number} [ttlMs=0] - TTL in milliseconds (0 = no expiry)
   * @throws {Error} If value is too large
   */
  set(key, value, ttlMs = 0) {
    const expiry = ttlMs > 0 ? Date.now() + ttlMs : -1;

    // Validate value size
    try {
      const size = JSON.stringify(value).length;
      if (size > MAX_VALUE_SIZE) {
        throw createError(
          ErrorCodes.VALUE_TOO_LARGE,
          `Value too large: ${size} bytes (max: ${MAX_VALUE_SIZE})`
        );
      }
    } catch (err) {
      if (err.message.includes('circular')) {
        throw createError(ErrorCodes.CIRCULAR_REFERENCE, 'Circular reference detected');
      }
      throw err;
    }

    // Compress if enabled and value is large enough
    let cachedValue = value;
    if (this._compress && JSON.stringify(value).length > this._compressionThreshold) {
      try {
        const { compress } = require('lz4');
        const serialized = JSON.stringify(value);
        cachedValue = {
          __compressed: true,
          data: compress(Buffer.from(serialized)),
        };
      } catch (err) {
        // Fallback to uncompressed value
      }
    }

    if (this._map.has(key)) {
      const node = this._map.get(key);

      // Remove old index entry
      this._index.remove(key);

      node.value = cachedValue;
      node.expiry = expiry;
      this._detach(node);
      this._attach(node);

      // Add new index entry
      this._index.add(key);
      return;
    }

    const node = new LRUNode(key, cachedValue, expiry);
    this._map.set(key, node);
    this._attach(node);
    this._index.add(key);

    // Evict if over capacity
    if (this._map.size > this._max) {
      this._evict(this._tail.prev);
    }
  }

  /**
   * Invalidate a key and all its prefixes
   * @param {string} key - Key to invalidate
   */
  invalidate(key) {
    // Invalidate exact key
    const node = this._map.get(key);
    if (node) {
      this._evict(node);
    }

    // Invalidate all keys with this prefix
    const affected = this._index.get(key);
    if (affected) {
      for (const k of [...affected]) {
        const n = this._map.get(k);
        if (n) {
          this._evict(n);
        }
      }
    }

    // Invalidate parent prefixes
    const parts = key.split('.');
    for (let i = 1; i < parts.length; i++) {
      const parent = parts.slice(0, i).join('.');
      const n = this._map.get(parent);
      if (n) {
        this._evict(n);
      }
    }
  }

  /**
   * Clear the entire cache
   */
  clear() {
    this._map.clear();
    this._index.clear();
    this._head.next = this._tail;
    this._tail.prev = this._head;
  }

  /**
   * Destroy the cache and free resources
   */
  destroy() {
    this.clear();
    this._head = null;
    this._tail = null;
  }

  /**
   * Get the cache size
   * @returns {number} Number of entries
   */
  get size() {
    return this._map.size;
  }
}

module.exports = { LRUCache };