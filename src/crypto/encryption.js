'use strict';

const crypto = require('crypto');
const { ErrorCodes, createError } = require('../utils/error-codes');

// IV pool size to prevent IV reuse
const IV_POOL_SIZE = 1000;
let ivPool = [];

/**
 * Generate a unique initialization vector (IV)
 * @returns {Buffer} 12-byte IV for AES-256-GCM
 */
function generateIV() {
  if (ivPool.length === 0) {
    // Generate a pool of IVs
    for (let i = 0; i < IV_POOL_SIZE; i++) {
      ivPool.push(crypto.randomBytes(12));
    }
  }
  return ivPool.pop();
}

/**
 * Refill the IV pool
 */
function refillIVPool() {
  ivPool = [];
  for (let i = 0; i < IV_POOL_SIZE; i++) {
    ivPool.push(crypto.randomBytes(12));
  }
}

/**
 * Encrypt a value using AES-256-GCM
 * @param {*} value - Value to encrypt
 * @param {Buffer} keyBuf - 256-bit encryption key
 * @returns {Object} Encrypted envelope
 * @throws {Error} If encryption fails
 */
function encryptValue(value, keyBuf) {
  try {
    const iv = generateIV();
    const cipher = crypto.createCipheriv('aes-256-gcm', keyBuf, iv);
    const plain = JSON.stringify(value);
    const encrypted = Buffer.concat([cipher.update(plain, 'utf8'), cipher.final()]);
    const tag = cipher.getAuthTag();

    return {
      __enc: 1,
      iv: iv.toString('base64'),
      ct: encrypted.toString('base64'),
      tag: tag.toString('base64'),
    };
  } catch (err) {
    throw createError(ErrorCodes.ENCRYPTION_FAILED, `Encryption failed: ${err.message}`);
  }
}

/**
 * Decrypt a value using AES-256-GCM
 * @param {Object} envelope - Encrypted envelope
 * @param {Buffer} keyBuf - 256-bit encryption key
 * @returns {*} Decrypted value
 * @throws {Error} If decryption fails
 */
function decryptValue(envelope, keyBuf) {
  try {
    const iv = Buffer.from(envelope.iv, 'base64');
    const ct = Buffer.from(envelope.ct, 'base64');
    const tag = Buffer.from(envelope.tag, 'base64');

    const decipher = crypto.createDecipheriv('aes-256-gcm', keyBuf, iv);
    decipher.setAuthTag(tag);
    const plain = Buffer.concat([decipher.update(ct), decipher.final()]);

    return JSON.parse(plain.toString('utf8'));
  } catch (err) {
    throw createError(ErrorCodes.DECRYPTION_FAILED, `Decryption failed: ${err.message}`);
  }
}

/**
 * Check if a value is encrypted
 * @param {*} value - Value to check
 * @returns {boolean} True if value is encrypted
 */
function isEncrypted(value) {
  return value !== null && typeof value === 'object' && value.__enc === 1;
}

module.exports = {
  generateIV,
  refillIVPool,
  encryptValue,
  decryptValue,
  isEncrypted,
};