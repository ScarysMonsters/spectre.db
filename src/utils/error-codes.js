'use strict';

/**
 * Error codes for spectre.db
 * Structured error codes for better error handling and debugging
 */

const ErrorCodes = {
  // Validation errors (1000-1099)
  INVALID_KEY: 1000,
  KEY_TOO_LONG: 1001,
  KEY_EMPTY_SEGMENT: 1002,
  KEY_FORBIDDEN_SEGMENT: 1003,
  KEY_CONTROL_CHARS: 1004,
  KEY_INVISIBLE_CHARS: 1005,

  INVALID_VALUE: 1100,
  VALUE_TOO_LARGE: 1101,
  CIRCULAR_REFERENCE: 1102,
  UNSUPPORTED_TYPE: 1103,

  // Path errors (2000-2099)
  PATH_TRAVERSAL: 2000,
  INVALID_PATH: 2001,
  SYMLINK_DETECTED: 2002,
  DIRECTORY_NOT_FOUND: 2003,

  // Storage errors (3000-3099)
  SNAPSHOT_CORRUPTED: 3000,
  WAL_CORRUPTED: 3001,
  BACKUP_CORRUPTED: 3002,
  WRITE_FAILED: 3003,
  READ_FAILED: 3004,
  FILE_LOCKED: 3005,

  // Encryption errors (4000-4099)
  ENCRYPTION_FAILED: 4000,
  DECRYPTION_FAILED: 4001,
  INVALID_KEY: 4002,
  KEY_DERIVATION_FAILED: 4003,

  // Transaction errors (5000-5099)
  TRANSACTION_ACTIVE: 5000,
  TRANSACTION_NOT_ACTIVE: 5001,
  TRANSACTION_ROLLED_BACK: 5002,
  TRANSACTION_COMMIT_FAILED: 5003,

  // Cache errors (6000-6099)
  CACHE_ERROR: 6000,
  CACHE_INVALIDATION_FAILED: 6001,

  // Lock errors (7000-7099)
  LOCK_ACQUISITION_FAILED: 7000,
  LOCK_TIMEOUT: 7001,
  LOCK_RELEASE_FAILED: 7002,

  // State errors (8000-8099)
  DATABASE_CLOSED: 8000,
  DATABASE_NOT_READY: 8001,
  STATE_CORRUPTED: 8002,

  // Operation errors (9000-9099)
  OPERATION_FAILED: 9000,
  OPERATION_TIMEOUT: 9001,
  UNKNOWN_OPERATION: 9002,
};

/**
 * Create a structured error object
 * @param {number} code - Error code
 * @param {string} message - Error message
 * @param {Error} [originalError] - Original error if any
 * @returns {Error} Error object with code property
 */
function createError(code, message, originalError) {
  const error = new Error(message);
  error.code = code;
  error.name = getErrorName(code);
  
  if (originalError) {
    error.originalError = originalError;
    error.stack = `${error.stack}\nCaused by: ${originalError.stack}`;
  }
  
  return error;
}

/**
 * Get error name from code
 * @param {number} code - Error code
 * @returns {string} Error name
 */
function getErrorName(code) {
  for (const [name, value] of Object.entries(ErrorCodes)) {
    if (value === code) {
      return name;
    }
  }
  return 'UNKNOWN_ERROR';
}

/**
 * Check if error is a specific type
 * @param {Error} error - Error to check
 * @param {number} code - Error code to check against
 * @returns {boolean} True if error matches code
 */
function isError(error, code) {
  return error && error.code === code;
}

module.exports = {
  ErrorCodes,
  createError,
  getErrorName,
  isError,
};