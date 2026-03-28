'use strict';

const fsp = require('fs').promises;
const path = require('path');
const crypto = require('crypto');
const zlib = require('zlib');
const { promisify } = require('util');
const { ErrorCodes, createError } = require('../utils/error-codes');

const asyncGzip = promisify(zlib.gzip);
const asyncGunzip = promisify(zlib.gunzip);

/**
 * Backup manager for snapshot rotation and encryption
 */
class BackupManager {
  constructor(snapshotPath, options = {}) {
    this._snapshotPath = snapshotPath;
    this._backupCount = options.backupCount || 3;
    this._compress = options.compress || false;
    this._encrypt = options.encrypt || false;
    this._encryptionKey = options.encryptionKey || null;
  }

  /**
   * Rotate backups
   * Moves .1.bak to .2.bak, .2.bak to .3.bak, etc.
   * @throws {Error} If rotation fails
   */
  async rotateBackups() {
    for (let gen = this._backupCount; gen >= 1; gen--) {
      const src = gen === 1 ? this._snapshotPath : `${this._snapshotPath}.${gen - 1}.bak`;
      const dst = `${this._snapshotPath}.${gen}.bak`;

      const exists = await fsp
        .access(src)
        .then(() => true)
        .catch(() => false);
      if (!exists) continue;

      try {
        await fsp.rename(src, dst);
      } catch (err) {
        // Ignore rotation errors
      }
    }
  }

  /**
   * Encrypt a backup file
   * @param {string} backupPath - Path to backup file
   * @throws {Error} If encryption fails
   */
  async encryptBackup(backupPath) {
    if (!this._encrypt || !this._encryptionKey) {
      return;
    }

    try {
      let content = await fsp.readFile(backupPath);

      if (this._compress) {
        content = await asyncGunzip(content);
      }

      const iv = crypto.randomBytes(12);
      const cipher = crypto.createCipheriv('aes-256-gcm', this._encryptionKey, iv);
      const encrypted = Buffer.concat([cipher.update(content), cipher.final()]);
      const tag = cipher.getAuthTag();

      const encryptedContent = Buffer.concat([iv, tag, encrypted]);

      if (this._compress) {
        const compressed = await asyncGzip(encryptedContent, { level: 1 });
        await fsp.writeFile(backupPath, compressed);
      } else {
        await fsp.writeFile(backupPath, encryptedContent);
      }
    } catch (err) {
      throw createError(ErrorCodes.ENCRYPTION_FAILED, `Failed to encrypt backup: ${err.message}`);
    }
  }

  /**
   * Decrypt a backup file
   * @param {string} backupPath - Path to backup file
   * @returns {Promise<Buffer>} Decrypted content
   * @throws {Error} If decryption fails
   */
  async decryptBackup(backupPath) {
    if (!this._encrypt || !this._encryptionKey) {
      return await fsp.readFile(backupPath);
    }

    try {
      let content = await fsp.readFile(backupPath);

      if (this._compress) {
        content = await asyncGunzip(content);
      }

      const iv = content.slice(0, 12);
      const tag = content.slice(12, 28);
      const encrypted = content.slice(28);

      const decipher = crypto.createDecipheriv('aes-256-gcm', this._encryptionKey, iv);
      decipher.setAuthTag(tag);
      const decrypted = Buffer.concat([decipher.update(encrypted), decipher.final()]);

      if (this._compress) {
        return await asyncGunzip(decrypted);
      }

      return decrypted;
    } catch (err) {
      throw createError(ErrorCodes.DECRYPTION_FAILED, `Failed to decrypt backup: ${err.message}`);
    }
  }

  /**
   * Get backup file path for a generation
   * @param {number} generation - Backup generation (1-based)
   * @returns {string} Backup file path
   */
  getBackupPath(generation) {
    return `${this._snapshotPath}.${generation}.bak`;
  }

  /**
   * Check if a backup exists
   * @param {number} generation - Backup generation (1-based)
   * @returns {Promise<boolean>} True if backup exists
   */
  async backupExists(generation) {
    const backupPath = this.getBackupPath(generation);
    return fsp
      .access(backupPath)
      .then(() => true)
      .catch(() => false);
  }

  /**
   * Delete a backup
   * @param {number} generation - Backup generation (1-based)
   * @throws {Error} If deletion fails
   */
  async deleteBackup(generation) {
    const backupPath = this.getBackupPath(generation);
    try {
      await fsp.unlink(backupPath);
    } catch (err) {
      // Ignore deletion errors
    }
  }

  /**
   * Get the number of backup generations
   * @returns {number} Number of backup generations
   */
  getBackupCount() {
    return this._backupCount;
  }
}

module.exports = { BackupManager };