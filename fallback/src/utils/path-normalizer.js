'use strict';

const path = require('path');
const { ErrorCodes, createError } = require('./error-codes');

/**
 * Normalize a file path
 * @param {string} filePath - Path to normalize
 * @returns {string} Normalized path
 */
function normalizePath(filePath) {
  const resolved = path.resolve(filePath);
  const normalized = path.normalize(resolved);

  // Remove common extensions
  return normalized
    .replace(/\.json\.gz$/i, '')
    .replace(/\.json$/i, '')
    .replace(/\.gz$/i, '');
}

/**
 * Sanitize a path to prevent path traversal attacks
 * @param {string} filePath - Path to sanitize
 * @param {string} [baseDir] - Base directory to check against (defaults to cwd)
 * @returns {string} Sanitized path
 * @throws {Error} If path traversal is detected
 */
function sanitizePath(filePath, baseDir = process.cwd()) {
  const resolved = path.resolve(filePath);
  const normalized = path.normalize(resolved);

  // Check if the path is outside the base directory
  const relative = path.relative(baseDir, normalized);

  if (relative.startsWith('..') || path.isAbsolute(relative) && !normalized.startsWith(baseDir)) {
    throw createError(ErrorCodes.PATH_TRAVERSAL, 'Path traversal detected');
  }

  return normalized;
}

/**
 * Validate a directory path
 * @param {string} dirPath - Directory path to validate
 * @returns {string} Validated directory path
 * @throws {Error} If directory is invalid
 */
async function validateDirectory(dirPath) {
  const fs = require('fs').promises;

  try {
    const stats = await fs.stat(dirPath);

    if (!stats.isDirectory()) {
      throw createError(ErrorCodes.INVALID_PATH, 'Path is not a directory');
    }

    // Check if it's a symlink
    const realPath = await fs.realpath(dirPath);
    if (realPath !== dirPath) {
      throw createError(ErrorCodes.SYMLINK_DETECTED, 'Symlink detected');
    }

    return realPath;
  } catch (err) {
    if (err.code === 'ENOENT') {
      throw createError(ErrorCodes.DIRECTORY_NOT_FOUND, 'Directory not found');
    }
    throw err;
  }
}

/**
 * Get the directory and base name from a path
 * @param {string} filePath - File path
 * @returns {{dir: string, base: string}} Directory and base name
 */
function getPathComponents(filePath) {
  const resolved = path.resolve(filePath);
  const dir = path.dirname(resolved);
  const base = path.basename(resolved).replace(/\.(json|db|snapshot)$/i, '');

  return { dir, base };
}

module.exports = {
  normalizePath,
  sanitizePath,
  validateDirectory,
  getPathComponents,
};