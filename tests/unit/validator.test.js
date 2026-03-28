'use strict';

const {
  validateKey,
  validateValue,
  validateKeyLength,
  isSensitiveKey,
  MAX_KEY_LENGTH,
  MAX_VALUE_SIZE,
} = require('../../src/utils/validator');
const { ErrorCodes } = require('../../src/utils/error-codes');

describe('Validator', () => {
  describe('validateKey', () => {
    it('should accept valid keys', () => {
      expect(() => validateKey('user.name')).not.toThrow();
      expect(() => validateKey('data.123.value')).not.toThrow();
      expect(() => validateKey('a')).not.toThrow();
    });

    it('should reject empty keys', () => {
      expect(() => validateKey('')).toThrow();
    });

    it('should reject non-string keys', () => {
      expect(() => validateKey(123)).toThrow();
      expect(() => validateKey(null)).toThrow();
      expect(() => validateKey(undefined)).toThrow();
    });

    it('should reject keys with empty segments', () => {
      expect(() => validateKey('user..name')).toThrow();
      expect(() => validateKey('.user')).toThrow();
      expect(() => validateKey('user.')).toThrow();
    });

    it('should reject forbidden segments', () => {
      expect(() => validateKey('__proto__.polluted')).toThrow();
      expect(() => validateKey('constructor.test')).toThrow();
      expect(() => validateKey('prototype.test')).toThrow();
      expect(() => validateKey('user.__proto__.test')).toThrow();
    });

    it('should reject keys with control characters', () => {
      expect(() => validateKey('user\x00name')).toThrow();
      expect(() => validateKey('user\nname')).toThrow();
      expect(() => validateKey('user\rname')).toThrow();
    });

    it('should reject keys with invisible characters', () => {
      expect(() => validateKey('user\u200Bname')).toThrow();
      expect(() => validateKey('user\u200Cname')).toThrow();
      expect(() => validateKey('user\u200Dname')).toThrow();
    });

    it('should reject keys that are too long', () => {
      const longKey = 'a'.repeat(MAX_KEY_LENGTH + 1);
      expect(() => validateKey(longKey)).toThrow();
    });

    it('should normalize Unicode keys', () => {
      const key1 = 'café';
      const key2 = 'cafe\u0301'; // Same as café but decomposed
      const parts1 = validateKey(key1);
      const parts2 = validateKey(key2);
      expect(parts1).toEqual(parts2);
    });
  });

  describe('validateValue', () => {
    it('should accept valid values', () => {
      expect(() => validateValue('test')).not.toThrow();
      expect(() => validateValue(123)).not.toThrow();
      expect(() => validateValue({ a: 1 })).not.toThrow();
      expect(() => validateValue([1, 2, 3])).not.toThrow();
      expect(() => validateValue(null)).not.toThrow();
      expect(() => validateValue(undefined)).not.toThrow();
      expect(() => validateValue(true)).not.toThrow();
      expect(() => validateValue(false)).not.toThrow();
    });

    it('should reject values that are too large', () => {
      const largeValue = 'x'.repeat(MAX_VALUE_SIZE + 1);
      expect(() => validateValue(largeValue)).toThrow();
    });

    it('should reject circular references', () => {
      const obj = { a: 1 };
      obj.self = obj;
      expect(() => validateValue(obj)).toThrow();
    });

    it('should reject BigInt values', () => {
      const bigIntValue = 999999999999999999999n;
      expect(() => validateValue(bigIntValue)).toThrow();
    });

    it('should accept values within size limit', () => {
      const value = 'x'.repeat(MAX_VALUE_SIZE);
      expect(() => validateValue(value)).not.toThrow();
    });
  });

  describe('validateKeyLength', () => {
    it('should accept keys within limit', () => {
      expect(() => validateKeyLength('a'.repeat(MAX_KEY_LENGTH))).not.toThrow();
      expect(() => validateKeyLength('a'.repeat(100))).not.toThrow();
    });

    it('should reject keys that are too long', () => {
      expect(() => validateKeyLength('a'.repeat(MAX_KEY_LENGTH + 1))).toThrow();
    });
  });

  describe('isSensitiveKey', () => {
    it('should identify sensitive keys', () => {
      expect(isSensitiveKey('password')).toBe(true);
      expect(isSensitiveKey('user.password')).toBe(true);
      expect(isSensitiveKey('password_hash')).toBe(true);
      expect(isSensitiveKey('secret')).toBe(true);
      expect(isSensitiveKey('api_secret')).toBe(true);
      expect(isSensitiveKey('token')).toBe(true);
      expect(isSensitiveKey('auth_token')).toBe(true);
      expect(isSensitiveKey('apikey')).toBe(true);
      expect(isSensitiveKey('api_key')).toBe(true);
      expect(isSensitiveKey('private')).toBe(true);
      expect(isSensitiveKey('private_key')).toBe(true);
    });

    it('should not identify non-sensitive keys', () => {
      expect(isSensitiveKey('username')).toBe(false);
      expect(isSensitiveKey('email')).toBe(false);
      expect(isSensitiveKey('name')).toBe(false);
      expect(isSensitiveKey('password_backup')).toBe(false); // Not at start
      expect(isSensitiveKey('mypassword')).toBe(false); // No separator
    });
  });

  describe('Constants', () => {
    it('should have correct MAX_KEY_LENGTH', () => {
      expect(MAX_KEY_LENGTH).toBe(1000);
    });

    it('should have correct MAX_VALUE_SIZE', () => {
      expect(MAX_VALUE_SIZE).toBe(10 * 1024 * 1024); // 10MB
    });
  });
});
