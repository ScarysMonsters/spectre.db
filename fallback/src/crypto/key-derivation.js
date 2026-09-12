'use strict';

const crypto = require('crypto');
const { ErrorCodes, createError } = require('../utils/error-codes');

/**
 * Derive a 256-bit key from a raw key using scrypt
 * @param {string|Buffer} raw - Raw key
 * @param {string} [salt] - Salt for key derivation
 * @returns {Promise<Buffer>} Derived 256-bit key
 */
async function deriveKey(raw, salt = 'spectre-db-kdf-v1') {
  if (Buffer.isBuffer(raw) && raw.length === 32) {
    return raw;
  }

  return new Promise((resolve, reject) => {
    crypto.scrypt(String(raw), salt, 32, (err, derivedKey) => {
      if (err) {
        reject(createError(ErrorCodes.KEY_DERIVATION_FAILED, `Key derivation failed: ${err.message}`));
      } else {
        resolve(derivedKey);
      }
    });
  });
}

/**
 * Validate an encryption key
 * @param {Buffer} key - Key to validate
 * @throws {Error} If key is invalid
 */
function validateKey(key) {
  if (!Buffer.isBuffer(key) || key.length !== 32) {
    throw createError(ErrorCodes.INVALID_KEY, 'Invalid encryption key: must be a 32-byte Buffer');
  }
}

/**
 * Zero out a key in memory (security best practice)
 * @param {Buffer} key - Key to zero out
 */
function zeroKey(key) {
  if (Buffer.isBuffer(key)) {
    key.fill(0);
  }
}

module.exports = {
  deriveKey,
  validateKey,
  zeroKey,
};