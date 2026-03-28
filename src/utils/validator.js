'use strict';

const { ErrorCodes, createError } = require('./error-codes');

// Forbidden key segments to prevent prototype pollution
const FORBIDDEN_SEGMENTS = new Set(['__proto__', 'constructor', 'prototype']);

// Maximum key length
const MAX_KEY_LENGTH = 1000;

// Maximum value size (10MB)
const MAX_VALUE_SIZE = 10 * 1024 * 1024;

// Control characters to reject
const CONTROL_CHARS = /[\x00-\x1F\x7F]/;

// Invisible characters to reject
const INVISIBLE_CHARS = /[\u200B-\u200D\uFEFF\u2028\u2029]/;

/**
 * Validate a key
 * @param {string} key - Key to validate
 * @returns {Array<string>} Array of key parts
 * @throws {Error} If key is invalid
 */
function validateKey(key) {
  if (typeof key !== 'string' || key.length === 0) {
    throw createError(ErrorCodes.INVALID_KEY, 'Key must be a non-empty string');
  }

  // Check key length
  if (key.length > MAX_KEY_LENGTH) {
    throw createError(
      ErrorCodes.KEY_TOO_LONG,
      `Key too long: ${key.length} chars (max: ${MAX_KEY_LENGTH})`
    );
  }

  // Normalize Unicode to NFC form
  const normalized = key.normalize('NFC');

  // Check for control characters
  if (CONTROL_CHARS.test(normalized)) {
    throw createError(ErrorCodes.KEY_CONTROL_CHARS, 'Key contains control characters');
  }

  // Check for invisible characters
  if (INVISIBLE_CHARS.test(normalized)) {
    throw createError(ErrorCodes.KEY_INVISIBLE_CHARS, 'Key contains invisible characters');
  }

  const parts = normalized.split('.');
  for (const part of parts) {
    if (part.length === 0) {
      throw createError(
        ErrorCodes.KEY_EMPTY_SEGMENT,
        `Key contains an empty segment: "${key}"`
      );
    }
    if (FORBIDDEN_SEGMENTS.has(part)) {
      throw createError(
        ErrorCodes.KEY_FORBIDDEN_SEGMENT,
        `Forbidden key segment "${part}" in key: "${key}"`
      );
    }
  }

  return parts;
}

/**
 * Validate a value
 * @param {*} value - Value to validate
 * @throws {Error} If value is invalid
 */
function validateValue(value) {
  // Check value size
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
    if (err.message.includes('BigInt')) {
      throw createError(ErrorCodes.UNSUPPORTED_TYPE, 'BigInt not supported');
    }
    throw err;
  }

  // Check for circular references
  const seen = new WeakSet();
  const stack = [value];

  while (stack.length > 0) {
    const current = stack.pop();

    if (typeof current === 'object' && current !== null) {
      if (seen.has(current)) {
        throw createError(ErrorCodes.CIRCULAR_REFERENCE, 'Circular reference detected');
      }
      seen.add(current);

      for (const key of Object.keys(current)) {
        stack.push(current[key]);
      }
    }
  }
}

/**
 * Validate key length
 * @param {string} key - Key to validate
 * @throws {Error} If key is too long
 */
function validateKeyLength(key) {
  if (key.length > MAX_KEY_LENGTH) {
    throw createError(
      ErrorCodes.KEY_TOO_LONG,
      `Key too long: ${key.length} chars (max: ${MAX_KEY_LENGTH})`
    );
  }
}

/**
 * Check if a key is sensitive (should be encrypted)
 * @param {string} key - Key to check
 * @returns {boolean} True if key is sensitive
 */
function isSensitiveKey(key) {
  // Updated regex to be more precise and avoid false positives
  const SENSITIVE_KEY_RE = /^(?:password|secret|token|apikey|api_key|private)(?:[._]|$)/i;
  return SENSITIVE_KEY_RE.test(key);
}

module.exports = {
  validateKey,
  validateValue,
  validateKeyLength,
  isSensitiveKey,
  FORBIDDEN_SEGMENTS,
  MAX_KEY_LENGTH,
  MAX_VALUE_SIZE,
};