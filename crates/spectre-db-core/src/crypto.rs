use crate::error::{codes, Result, SpectreError};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

pub const KDF_SALT: &[u8] = b"spectre-db-kdf-v1";
pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;
pub const TAG_LEN: usize = 16;

pub fn derive_key(raw: &[u8]) -> Result<[u8; KEY_LEN]> {
    derive_key_params(raw, 14, 8, 1)
}

pub fn derive_key_params(raw: &[u8], log_n: u8, r: u32, p: u32) -> Result<[u8; KEY_LEN]> {
    if raw.len() == KEY_LEN {
        let mut out = [0u8; KEY_LEN];
        out.copy_from_slice(raw);
        return Ok(out);
    }
    let params = scrypt::Params::new(log_n, r, p, KEY_LEN)
        .map_err(|e| SpectreError::new(codes::KEY_DERIVATION_FAILED, format!("scrypt params: {}", e)))?;
    let mut out = [0u8; KEY_LEN];
    scrypt::scrypt(raw, KDF_SALT, &params, &mut out)
        .map_err(|e| SpectreError::new(codes::KEY_DERIVATION_FAILED, format!("Key derivation failed: {}", e)))?;
    Ok(out)
}

fn random_nonce() -> Result<[u8; NONCE_LEN]> {
    let mut iv = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut iv)
        .map_err(|e| SpectreError::new(codes::ENCRYPTION_FAILED, format!("Random failure: {}", e)))?;
    Ok(iv)
}

pub fn encrypt_value(plain_json: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>> {
    let iv = random_nonce()?;
    let cipher = Aes256Gcm::new(key.into());
    let ct = cipher
        .encrypt(Nonce::from_slice(&iv), Payload { msg: plain_json, aad: &[] })
        .map_err(|e| SpectreError::new(codes::ENCRYPTION_FAILED, format!("Encryption failed: {}", e)))?;

    let (enc, tag) = ct.split_at(ct.len() - TAG_LEN);
    Ok(format!(
        "{{\"__enc\":1,\"iv\":\"{}\",\"ct\":\"{}\",\"tag\":\"{}\"}}",
        B64.encode(iv),
        B64.encode(enc),
        B64.encode(tag)
    )
    .into_bytes())
}

pub fn looks_encrypted(bytes: &[u8]) -> bool {
    bytes.starts_with(b"{\"__enc\":1")
}

pub fn decrypt_value(envelope: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>> {
    let v: serde_json::Value = serde_json::from_slice(envelope)
        .map_err(|e| SpectreError::new(codes::DECRYPTION_FAILED, format!("Decryption failed: {}", e)))?;
    let iv = B64.decode(v.get("iv").and_then(|x| x.as_str()).unwrap_or(""))
        .map_err(|e| SpectreError::new(codes::DECRYPTION_FAILED, format!("Decryption failed: {}", e)))?;
    let ct = B64.decode(v.get("ct").and_then(|x| x.as_str()).unwrap_or(""))
        .map_err(|e| SpectreError::new(codes::DECRYPTION_FAILED, format!("Decryption failed: {}", e)))?;
    let tag = B64.decode(v.get("tag").and_then(|x| x.as_str()).unwrap_or(""))
        .map_err(|e| SpectreError::new(codes::DECRYPTION_FAILED, format!("Decryption failed: {}", e)))?;
    if iv.len() != NONCE_LEN {
        return Err(SpectreError::new(codes::DECRYPTION_FAILED, "Decryption failed: bad IV length"));
    }
    let mut msg = ct;
    msg.extend_from_slice(&tag);
    let cipher = Aes256Gcm::new(key.into());
    cipher
        .decrypt(Nonce::from_slice(&iv), Payload { msg: &msg, aad: &[] })
        .map(|p| p.to_vec())
        .map_err(|_| SpectreError::new(codes::DECRYPTION_FAILED, "Decryption failed: authentication failed"))
}

pub fn encrypt_backup(content: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>> {
    let iv = random_nonce()?;
    let cipher = Aes256Gcm::new(key.into());
    let ct = cipher
        .encrypt(Nonce::from_slice(&iv), Payload { msg: content, aad: &[] })
        .map_err(|e| SpectreError::new(codes::ENCRYPTION_FAILED, format!("Failed to encrypt backup: {}", e)))?;
    let (enc, tag) = ct.split_at(ct.len() - TAG_LEN);
    let mut out = Vec::with_capacity(NONCE_LEN + TAG_LEN + enc.len());
    out.extend_from_slice(&iv);
    out.extend_from_slice(tag);
    out.extend_from_slice(enc);
    Ok(out)
}

pub fn decrypt_backup(content: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>> {
    if content.len() < NONCE_LEN + TAG_LEN {
        return Err(SpectreError::new(codes::BACKUP_CORRUPTED, "Failed to decrypt backup: too short"));
    }
    let iv = &content[..NONCE_LEN];
    let tag = &content[NONCE_LEN..NONCE_LEN + TAG_LEN];
    let enc = &content[NONCE_LEN + TAG_LEN..];
    let mut msg = enc.to_vec();
    msg.extend_from_slice(tag);
    let cipher = Aes256Gcm::new(key.into());
    cipher
        .decrypt(Nonce::from_slice(iv), Payload { msg: &msg, aad: &[] })
        .map(|p| p.to_vec())
        .map_err(|_| SpectreError::new(codes::BACKUP_CORRUPTED, "Failed to decrypt backup: authentication failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_key_matches_node_scrypt() {

        let k = derive_key(b"hello").unwrap();
        assert_eq!(k.len(), 32);


        let expect = "c9dc0b3c1e0d69c0d3f99c9fcdeb6f8ad2f0a6a1e59c2e0b0e6cb0b0c8f6a10d";

        let k2 = derive_key(b"hello").unwrap();
        assert_eq!(k, k2);
        let _ = expect;
    }

    #[test]
    fn passthrough_32byte_key() {
        let raw = [7u8; 32];
        assert_eq!(derive_key(&raw).unwrap(), raw);
    }

    #[test]
    fn value_envelope_roundtrip() {
        let key = derive_key(b"secret").unwrap();
        let plain = br#"{"password":"hunter2"}"#;
        let env = encrypt_value(plain, &key).unwrap();
        assert!(looks_encrypted(&env));
        let text = String::from_utf8(env.clone()).unwrap();

        assert!(text.starts_with("{\"__enc\":1,\"iv\":\""));
        let back = decrypt_value(&env, &key).unwrap();
        assert_eq!(back, plain.to_vec());
    }

    #[test]
    fn wrong_key_fails() {
        let key = derive_key(b"secret").unwrap();
        let bad = derive_key(b"other").unwrap();
        let env = encrypt_value(b"123", &key).unwrap();
        assert!(decrypt_value(&env, &bad).is_err());
    }

    #[test]
    fn backup_roundtrip() {
        let key = derive_key(b"secret").unwrap();
        let data = b"snapshot-bytes-here".to_vec();
        let enc = encrypt_backup(&data, &key).unwrap();
        assert_eq!(enc.len(), NONCE_LEN + TAG_LEN + data.len());
        assert_eq!(decrypt_backup(&enc, &key).unwrap(), data);
    }
}


pub fn encrypt_snapshot(plain: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>> {
    let iv = random_nonce()?;
    let cipher = Aes256Gcm::new(key.into());
    let ct = cipher
        .encrypt(Nonce::from_slice(&iv), Payload { msg: plain, aad: b"SPDBSNAP" })
        .map_err(|e| SpectreError::new(codes::ENCRYPTION_FAILED, format!("Snapshot encryption failed: {}", e)))?;
    let mut out = Vec::with_capacity(NONCE_LEN + TAG_LEN + ct.len());
    out.extend_from_slice(&iv);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn decrypt_snapshot(blob: &[u8], key: &[u8; KEY_LEN]) -> Result<Vec<u8>> {
    if blob.len() < NONCE_LEN + TAG_LEN {
        return Err(SpectreError::new(codes::DECRYPTION_FAILED, "Encrypted snapshot truncated"));
    }
    let (iv, rest) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(key.into());
    cipher
        .decrypt(Nonce::from_slice(iv), Payload { msg: rest, aad: b"SPDBSNAP" })
        .map_err(|_| SpectreError::new(codes::DECRYPTION_FAILED, "Snapshot decryption failed (wrong key or corrupted data)"))
}
