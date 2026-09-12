'use strict';

const { EventEmitter } = require('events');

/**
 * Safe event emitter with listener limits
 * Prevents memory leaks from too many listeners
 */
class SafeEventEmitter extends EventEmitter {
  constructor(maxListeners = 100) {
    super();
    this.setMaxListeners(maxListeners);
  }

  /**
   * Add an event listener
   * @param {string} event - Event name
   * @param {Function} listener - Listener function
   * @returns {SafeEventEmitter} This emitter
   * @throws {Error} If too many listeners
   */
  on(event, listener) {
    // Check listener limit
    if (this.listenerCount(event) >= this.getMaxListeners()) {
      throw new Error(`Too many listeners for event: ${event} (max: ${this.getMaxListeners()})`);
    }
    return super.on(event, listener);
  }

  /**
   * Add a one-time event listener
   * @param {string} event - Event name
   * @param {Function} listener - Listener function
   * @returns {SafeEventEmitter} This emitter
   * @throws {Error} If too many listeners
   */
  once(event, listener) {
    // Check listener limit
    if (this.listenerCount(event) >= this.getMaxListeners()) {
      throw new Error(`Too many listeners for event: ${event} (max: ${this.getMaxListeners()})`);
    }
    return super.once(event, listener);
  }

  /**
   * Remove all listeners for an event or all events
   * @param {string} [event] - Event name (optional)
   */
  removeAllListeners(event) {
    super.removeAllListeners(event);
  }

  /**
   * Get the number of listeners for an event
   * @param {string} event - Event name
   * @returns {number} Number of listeners
   */
  listenerCount(event) {
    return super.listenerCount(event);
  }

  /**
   * Get the maximum number of listeners
   * @returns {number} Maximum listeners
   */
  getMaxListeners() {
    return super.getMaxListeners();
  }

  /**
   * Set the maximum number of listeners
   * @param {number} n - Maximum listeners
   */
  setMaxListeners(n) {
    super.setMaxListeners(n);
  }
}

module.exports = { SafeEventEmitter };