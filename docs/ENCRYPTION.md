# Encryption

spectre.db offers two independent encryption layers:

1. **Per-value encryption** — values stored under *sensitive* keys are
   encrypted automatically (AES-256-GCM).
2. **Whole-file encryption** — backups (`encryptBackups`) and/or the complete
   v2 snapshot (`encryptSnapshot`).

Both are enabled with a single `encryptionKey`.

```js
const db = new Database('./data/secure', {
  encryptionKey: process.env.DB_ENCRYPTION_KEY,
  encryptBackups: true,
  encryptSnapshot: true,
});
```

---

## Key derivation

The raw `encryptionKey` (string or Buffer) is stretched with **scrypt**:

```
scrypt(rawKey, "spectre-db-kdf-v1", N = 2^scryptLogN, r = scryptR, p = scryptP, 32 bytes)
```

| Option | Default | Description |
|---|---|---|
| `scryptLogN` | `14` | log2 of the CPU/memory cost (N = 16384 by default) |
| `scryptR` | `8` | Block size |
| `scryptP` | `1` | Parallelism |

A 32-byte raw key is used as-is (v1.1.0 parity). The derived key is zeroed in
memory when the database is closed.

The KDF output must be identical when reopening: **always use the same key and
the same scrypt parameters**, otherwise decryption fails (`DECRYPTION_FAILED`,
code 4001).

---

## Per-value encryption (sensitive keys)

Keys **starting with** one of these words, followed by `.`, `_` or the end of
the key, are encrypted automatically:

- `password`
- `secret`
- `token`
- `apikey`
- `api_key`
- `private`

```js
const db = new Database('./data/secure', {
  encryptionKey: 'my-passphrase',
});

db.set('password', 'hunter2');        // encrypted
db.set('token.discord', 'abc123');    // encrypted
db.set('api_key', 'sk-...');          // encrypted
db.set('user.password', 'hunter2');   // NOT encrypted (does not start with the word)
db.set('profile.name', 'Alice');      // plaintext
```

Encrypted values are stored as an envelope:

```json
{ "__enc": 1, "iv": "<base64>", "ct": "<base64>", "tag": "<base64>" }
```

`get()` decrypts transparently. Without the key, encrypted values are returned
as the raw envelope (and cannot be read).

> The same rule applies to the fallback engine, byte-for-byte compatible
> envelopes.

---

## Backup encryption

```js
const db = new Database('./data/secure', {
  encryptionKey: process.env.DB_ENCRYPTION_KEY,
  encryptBackups: true,
  backup: true,
  backupCount: 3,
});

await db.save(); // rotated backups are encrypted
```

- Backup layout: `iv(12) || tag(16) || ciphertext`.
- In JSON mode, the *uncompressed* content is encrypted first, then gzipped
  when `compress` is enabled.
- Restoring an encrypted backup requires the same `encryptionKey`.

---

## Whole-snapshot encryption

```js
const db = new Database('./data/secure', {
  encryptionKey: process.env.DB_ENCRYPTION_KEY,
  encryptSnapshot: true,
});
```

- The v2 snapshot payload is compressed first (if any), then encrypted:
  `iv(12) || tag(16) || ciphertext`, with `SPDBSNAP` as AAD.
- Opening an encrypted snapshot without a key fails with
  `DECRYPTION_FAILED` (4001) — there is **no silent plaintext fallback** and no
  silent backup restore in that case.

---

## Key management

- Read the key from the environment (`process.env.DB_ENCRYPTION_KEY`), never
  hardcode it in source.
- Keep the key outside backups: an encrypted backup is useless without it.
- Losing the key means losing the data — there is no recovery mechanism.
- Key rotation is not built in: to rotate, open with the old key, `migrate()`
  to a new database with the new key, and replace the old files.
- Per-value encryption does not protect key names, entry counts or timestamps —
  only the values. Use `encryptSnapshot` for full at-rest confidentiality.

## What is not encrypted

- Keys (the paths themselves) and structural metadata.
- Raw binary values (`setRaw`): they are stored as opaque bytes and are not
  automatically encrypted. Encrypt them yourself before storing if needed.
- The fallback engine's JSON WAL/snapshot when only per-value encryption is
  enabled (sensitive values are encrypted, everything else is plaintext).

## Example: encrypted database from scratch

```js
const { Database } = require('@sexfy/spectre.db');

const db = new Database('./data/secure', {
  encryptionKey: process.env.DB_ENCRYPTION_KEY,
  encryptBackups: true,
  encryptSnapshot: true,
  backup: true,
  backupCount: 3,
  scryptLogN: 15, // optional hardening
});

await db.ready;

db.set('password', 'hunter2');
db.set('session.token', 'abc123');
db.set('profile.name', 'Alice');

await db.save();
await db.close();
```
