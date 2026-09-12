'use strict';

const { ErrorCodes, createError } = require('./error-codes');

/**
 * Safely stringify a value to JSON
 * @param {*} value - Value to stringify
 * @param {number} [indent] - Indentation level
 * @returns {string} JSON string
 * @throws {Error} If value cannot be stringified
 */
function safeStringify(value, indent) {
  try {
    return JSON.stringify(value, null, indent);
  } catch (err) {
    if (err.message.includes('circular')) {
      throw createError(ErrorCodes.CIRCULAR_REFERENCE, 'Circular reference detected');
    }
    if (err.message.includes('BigInt')) {
      throw createError(ErrorCodes.UNSUPPORTED_TYPE, 'BigInt not supported');
    }
    throw createError(ErrorCodes.INVALID_VALUE, `Failed to stringify value: ${err.message}`);
  }
}

/**
 * Safely parse a JSON string
 * @param {string} str - JSON string to parse
 * @returns {*} Parsed value
 * @throws {Error} If string cannot be parsed
 */
function safeParse(str) {
  try {
    return JSON.parse(str);
  } catch (err) {
    throw createError(ErrorCodes.INVALID_VALUE, `Failed to parse JSON: ${err.message}`);
  }
}

/**
 * Detect circular references in an object
 * @param {*} value - Value to check
 * @returns {boolean} True if circular reference is detected
 */
function detectCircular(value) {
  const seen = new WeakSet();
  const stack = [value];

  while (stack.length > 0) {
    const current = stack.pop();

    if (typeof current === 'object' && current !== null) {
      if (seen.has(current)) {
        return true;
      }
      seen.add(current);

      for (const key of Object.keys(current)) {
        stack.push(current[key]);
      }
    }
  }

  return false;
}

/**
 * Convert an object to null-prototype object (prevents prototype pollution)
 * @param {*} value - Value to convert
 * @returns {*} Converted value
 */
function toNullProto(value) {
  if (value === null || value === undefined || typeof value !== 'object') {
    return value;
  }

  if (Array.isArray(value)) {
    return value.map(toNullProto);
  }

  const result = Object.create(null);
  for (const k of Object.keys(value)) {
    // Skip dangerous keys
    if (k === '__proto__' || k === 'constructor' || k === 'prototype') {
      continue;
    }
    result[k] = toNullProto(value[k]);
  }

  return result;
}

/**
 * Convert a null-prototype object to plain object
 * @param {*} value - Value to convert
 * @returns {*} Converted value
 */
function toPlainObject(value) {
  if (value === null || value === undefined || typeof value !== 'object') {
    return value;
  }

  if (Array.isArray(value)) {
    return value.map(toPlainObject);
  }

  const result = {};
  for (const k of Object.keys(value)) {
    result[k] = toPlainObject(value[k]);
  }

  return result;
}

/**
 * Deep clone an object
 * @param {*} value - Value to clone
 * @returns {*} Cloned value
 */
function deepClone(value) {
  return toNullProto(toPlainObject(value));
}

module.exports = {
  safeStringify,
  safeParse,
  detectCircular,
  toNullProto,
  toPlainObject,
  deepClone,
};