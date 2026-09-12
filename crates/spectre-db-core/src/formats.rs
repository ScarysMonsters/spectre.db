use crate::crypto;
use crate::error::{codes, Result, SpectreError};
use crate::store::Store;
use crc32fast::Hasher as Crc;
use std::io::Write;

pub const SNAPSHOT_MAGIC: &[u8; 8] = b"SPDBSNAP";
pub const SNAPSHOT_VERSION: u16 = 2;
pub const FLAG_GZIP: u16 = 0b0001;
pub const FLAG_ZSTD: u16 = 0b0010;
pub const FLAG_ENCRYPTED: u16 = 0b0100;

pub const OP_SET: u8 = 1;
pub const OP_DEL: u8 = 2;
pub const OP_CLEAR: u8 = 3;
pub const OP_BATCH: u8 = 4;
pub const OP_BATCH_V3: u8 = 5;

pub const WAL3_MAGIC: &[u8; 8] = b"SPDBWAL3";
pub const WAL3_VERSION: u16 = 3;
pub const WAL3_HEADER_LEN: usize = 32;

pub const MANIFEST_MAGIC: &[u8; 8] = b"SPDBMAN\0";
pub const MANIFEST_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    #[default]
    None,
    Gzip,
    Zstd,
}

impl Compression {
    pub fn as_str(&self) -> &'static str {
        match self {
            Compression::None => "none",
            Compression::Gzip => "gzip",
            Compression::Zstd => "zstd",
        }
    }
    pub fn flags(&self) -> u16 {
        match self {
            Compression::None => 0,
            Compression::Gzip => FLAG_GZIP,
            Compression::Zstd => FLAG_ZSTD,
        }
    }
    pub fn from_flags(flags: u16) -> Self {
        if flags & FLAG_ZSTD != 0 {
            Compression::Zstd
        } else if flags & FLAG_GZIP != 0 {
            Compression::Gzip
        } else {
            Compression::None
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManifestEntry {
    pub id: u64,
    pub gen: u64,
    pub entries: u64,
    pub name: String,
    pub file_crc: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub generation: u64,
    pub indexes: Vec<String>,
    pub segments: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct WalHeader {
    pub version: u16,
    pub generation: u64,
    pub base_lsn: u64,
}

#[derive(Debug, Clone, Default)]
pub struct WalReplay {
    pub ops: Vec<Op>,
    pub warns: usize,

    pub header: Option<WalHeader>,
    pub last_lsn: u64,
}

#[derive(Debug, Clone)]
pub enum Op {
    Set { key: Vec<u8>, value: Box<[u8]> },
    Del { key: Vec<u8> },
    Clear,
    Batch(Vec<Op>),
}


pub fn write_snapshot(store: &Store, compress: bool) -> Vec<u8> {
    write_snapshot_ex(store, if compress { Compression::Gzip } else { Compression::None }, None)
}

pub fn write_snapshot_ex(store: &Store, algo: Compression, key: Option<&[u8; crypto::KEY_LEN]>) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::with_capacity(16 + (store.len() * 48) + store.value_bytes() as usize);
    body.extend_from_slice(&store.len().to_le_bytes());
    for (k, v) in store.iter() {
        body.extend_from_slice(&(k.len() as u32).to_le_bytes());
        body.extend_from_slice(k);
        body.extend_from_slice(&(v.len() as u32).to_le_bytes());
        body.extend_from_slice(v);
    }

    let mut flags = algo.flags();
    let payload = match algo {
        Compression::None => body,
        Compression::Gzip => {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            let _ = enc.write_all(&body);
            enc.finish().unwrap_or(body)
        }
        Compression::Zstd => {

            zstd::encode_all(&body[..], 3).unwrap_or(body)
        }
    };
    let payload = match key {
        Some(k) => {
            flags |= FLAG_ENCRYPTED;
            crypto::encrypt_snapshot(&payload, k).unwrap_or_else(|_| payload)
        }
        None => payload,
    };

    let mut out = Vec::with_capacity(22 + payload.len() + 4);
    out.extend_from_slice(SNAPSHOT_MAGIC);
    out.extend_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&payload);
    let mut crc = Crc::new();
    crc.update(&out);
    out.extend_from_slice(&crc.finalize().to_le_bytes());
    out
}

pub fn load_snapshot(bytes: &[u8], key: Option<&[u8; crypto::KEY_LEN]>) -> Result<Store> {
    const HEADER: usize = 16;
    if bytes.len() < HEADER + 8 + 4 || &bytes[..8] != SNAPSHOT_MAGIC {
        return Err(SpectreError::snapshot_corrupted("Invalid v2 snapshot header"));
    }
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != SNAPSHOT_VERSION {
        return Err(SpectreError::snapshot_corrupted(format!(
            "Unsupported snapshot version: {}",
            version
        )));
    }
    let flags = u16::from_le_bytes([bytes[10], bytes[11]]);
    let algo = Compression::from_flags(flags);

    let (payload, stored_crc) = bytes[HEADER..].split_at(bytes.len() - HEADER - 4);
    let mut crc = Crc::new();
    crc.update(&bytes[..bytes.len() - 4]);
    if crc.finalize() != u32::from_le_bytes([stored_crc[0], stored_crc[1], stored_crc[2], stored_crc[3]]) {
        return Err(SpectreError::snapshot_corrupted("Snapshot CRC mismatch"));
    }

    let mut payload = payload.to_vec();
    if flags & FLAG_ENCRYPTED != 0 {
        let k = key.ok_or_else(|| {
            SpectreError::new(codes::DECRYPTION_FAILED, "Snapshot is encrypted: encryption_key required")
        })?;
        payload = crypto::decrypt_snapshot(&payload, k)?;
    }

    let body: Vec<u8> = match algo {
        Compression::None => payload,
        Compression::Gzip => {
            let mut dec = flate2::read::GzDecoder::new(&payload[..]);
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut dec, &mut buf).map_err(|e| {
                SpectreError::snapshot_corrupted(format!("Gzip decompression failed: {}", e))
            })?;
            buf
        }
        Compression::Zstd => zstd::decode_all(&payload[..]).map_err(|e| {
            SpectreError::snapshot_corrupted(format!("Zstd decompression failed: {}", e))
        })?,
    };

    let count = u64::from_le_bytes(body[0..8].try_into().unwrap()) as usize;
    let mut store = Store::new();
    let mut off = 8usize;
    for _ in 0..count {
        if off + 4 > body.len() {
            return Err(SpectreError::snapshot_corrupted("Truncated snapshot entry"));
        }
        let klen = u32::from_le_bytes(body[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if off + klen + 4 > body.len() {
            return Err(SpectreError::snapshot_corrupted("Truncated snapshot entry"));
        }
        let key = body[off..off + klen].to_vec();
        off += klen;
        let vlen = u32::from_le_bytes(body[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if off + vlen > body.len() {
            return Err(SpectreError::snapshot_corrupted("Truncated snapshot value"));
        }
        let val = body[off..off + vlen].to_vec().into_boxed_slice();
        off += vlen;
        store.set(&key, val);
    }
    Ok(store)
}


fn frame(op: u8, payload: &[u8], out: &mut Vec<u8>) {
    let start = out.len();
    out.push(op);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    let mut crc = Crc::new();
    crc.update(&out[start..]);
    out.extend_from_slice(&crc.finalize().to_le_bytes());
}

fn set_payload(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(4 + key.len() + value.len());
    p.extend_from_slice(&(key.len() as u32).to_le_bytes());
    p.extend_from_slice(key);
    p.extend_from_slice(value);
    p
}

pub fn encode_set(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(13 + key.len() + value.len());
    frame(OP_SET, &set_payload(key, value), &mut out);
    out
}

pub fn encode_del(key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(13 + key.len());
    frame(OP_DEL, key, &mut out);
    out
}

pub fn encode_clear() -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    frame(OP_CLEAR, &[], &mut out);
    out
}

pub fn encode_batch(ops: &[Op]) -> Vec<u8> {
    let mut sub = Vec::new();
    sub.extend_from_slice(&(ops.len() as u32).to_le_bytes());
    for op in ops {
        match op {
            Op::Set { key, value } => frame(OP_SET, &set_payload(key, value), &mut sub),
            Op::Del { key } => frame(OP_DEL, key, &mut sub),
            _ => {}
        }
    }
    let mut out = Vec::with_capacity(9 + sub.len());
    frame(OP_BATCH, &sub, &mut out);
    out
}

pub fn decode_wal(bytes: &[u8]) -> (Vec<Op>, usize) {
    let mut ops = Vec::new();
    let mut warns = 0usize;
    let mut off = 0usize;
    while off + 9 <= bytes.len() {
        let op = bytes[off];
        let plen = u32::from_le_bytes(bytes[off + 1..off + 5].try_into().unwrap()) as usize;
        let end = off + 5 + plen + 4;
        if end > bytes.len() {
            warns += 1;
            break;
        }
        let payload = &bytes[off + 5..off + 5 + plen];
        let mut crc = Crc::new();
        crc.update(&bytes[off..off + 5 + plen]);
        if crc.finalize() != u32::from_le_bytes(bytes[off + 5 + plen..end].try_into().unwrap()) {
            warns += 1;
            break;
        }
        match decode_payload(op, payload) {
            Some(parsed) => ops.push(parsed),
            None => warns += 1,
        }
        off = end;
    }
    if off < bytes.len() && warns == 0 {
        warns += 1;
    }
    (ops, warns)
}

fn decode_payload(op: u8, payload: &[u8]) -> Option<Op> {
    match op {
        OP_SET => {
            if payload.len() < 4 {
                return None;
            }
            let klen = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
            if payload.len() < 4 + klen {
                return None;
            }
            let key = payload[4..4 + klen].to_vec();
            let value = payload[4 + klen..].to_vec().into_boxed_slice();
            Some(Op::Set { key, value })
        }
        OP_DEL => Some(Op::Del { key: payload.to_vec() }),
        OP_CLEAR => Some(Op::Clear),
        OP_BATCH => {
            if payload.len() < 4 {
                return None;
            }
            let count = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
            let mut ops = Vec::with_capacity(count);
            let mut off = 4usize;
            for _ in 0..count {
                if off + 9 > payload.len() {
                    return None;
                }
                let sop = payload[off];
                let plen = u32::from_le_bytes(payload[off + 1..off + 5].try_into().unwrap()) as usize;
                let end = off + 5 + plen + 4;
                if end > payload.len() {
                    return None;
                }
                let sub_payload = &payload[off + 5..off + 5 + plen];
                let mut crc = Crc::new();
                crc.update(&payload[off..off + 5 + plen]);
                if crc.finalize()
                    != u32::from_le_bytes(payload[off + 5 + plen..end].try_into().unwrap())
                {
                    return None;
                }
                ops.push(decode_payload(sop, sub_payload)?);
                off = end;
            }
            Some(Op::Batch(ops))
        }
        _ => None,
    }
}

pub fn apply_ops(store: &mut Store, ops: &[Op]) -> usize {
    let mut n = 0;
    for op in ops {
        match op {
            Op::Set { key, value } => {
                store.set(key, value.clone());
                n += 1;
            }
            Op::Del { key } => {
                store.delete(key);
                n += 1;
            }
            Op::Clear => {
                store.clear();
                n += 1;
            }
            Op::Batch(sub) => n += apply_ops(store, sub),
        }
    }
    n
}

pub fn validate_json_bytes(bytes: &[u8]) -> Result<()> {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .map(|_| ())
        .map_err(|e| SpectreError::new(codes::INVALID_KEY, format!("Invalid JSON value: {}", e)))
}


pub fn encode_wal3_header(generation: u64, base_lsn: u64) -> Vec<u8> {
    let mut h = Vec::with_capacity(WAL3_HEADER_LEN);
    h.extend_from_slice(WAL3_MAGIC);
    h.extend_from_slice(&WAL3_VERSION.to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    h.extend_from_slice(&generation.to_le_bytes());
    h.extend_from_slice(&base_lsn.to_le_bytes());
    let mut crc = Crc::new();
    crc.update(&h);
    h.extend_from_slice(&crc.finalize().to_le_bytes());
    h
}

pub fn frame3(op: u8, payload: &[u8], lsn: u64, out: &mut Vec<u8>) {
    let start = out.len();
    out.push(op);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    let lsn_pos = out.len();
    out.extend_from_slice(&lsn.to_le_bytes());
    let mut crc = Crc::new();
    crc.update(&out[start..]);
    out.extend_from_slice(&crc.finalize().to_le_bytes());
    let _ = lsn_pos;
}

pub fn frame3_tail(out: &mut Vec<u8>, start: usize, lsn: u64) {
    out.extend_from_slice(&lsn.to_le_bytes());
    let mut crc = Crc::new();
    crc.update(&out[start..]);
    out.extend_from_slice(&crc.finalize().to_le_bytes());
}

pub fn batch3_payload(ops: &[Op], out: &mut Vec<u8>) {
    out.extend_from_slice(&(ops.len() as u32).to_le_bytes());
    for op in ops {
        match op {
            Op::Set { key, value } => {
                out.push(OP_SET);
                out.extend_from_slice(&(key.len() as u32).to_le_bytes());
                out.extend_from_slice(key);
                out.extend_from_slice(&(value.len() as u32).to_le_bytes());
                out.extend_from_slice(value);
            }
            Op::Del { key } => {
                out.push(OP_DEL);
                out.extend_from_slice(&(key.len() as u32).to_le_bytes());
                out.extend_from_slice(key);
            }
            Op::Clear => {
                out.push(OP_CLEAR);
            }
            Op::Batch(_) => {}
        }
    }
}

pub fn replay_wal(bytes: &[u8]) -> WalReplay {
    if bytes.len() >= WAL3_HEADER_LEN && &bytes[..8] == WAL3_MAGIC {
        let mut crc = Crc::new();
        crc.update(&bytes[..28]);
        let stored = u32::from_le_bytes(bytes[28..32].try_into().unwrap());
        if crc.finalize() != stored {
            return WalReplay { warns: 1, ..Default::default() };
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != WAL3_VERSION {
            return WalReplay {
                warns: 1,
                header: Some(WalHeader { version, generation: 0, base_lsn: 0 }),
                ..Default::default()
            };
        }
        let generation = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
        let base_lsn = u64::from_le_bytes(bytes[20..28].try_into().unwrap());
        let mut out = WalReplay {
            header: Some(WalHeader { version, generation, base_lsn }),
            ..Default::default()
        };
        let mut off = WAL3_HEADER_LEN;
        while off + 17 <= bytes.len() {
            let op = bytes[off];
            let plen = u32::from_le_bytes(bytes[off + 1..off + 5].try_into().unwrap()) as usize;
            let end = off + 5 + plen + 8 + 4;
            if end > bytes.len() {
                out.warns += 1;
                break;
            }
            let payload = &bytes[off + 5..off + 5 + plen];
            let lsn = u64::from_le_bytes(bytes[off + 5 + plen..off + 5 + plen + 8].try_into().unwrap());
            let mut crc = Crc::new();
            crc.update(&bytes[off..off + 5 + plen + 8]);
            if crc.finalize() != u32::from_le_bytes(bytes[end - 4..end].try_into().unwrap()) {
                out.warns += 1;
                break;
            }
            match op {
                OP_SET | OP_DEL | OP_CLEAR => match decode_payload(op, payload) {
                    Some(parsed) => {
                        out.last_lsn = out.last_lsn.max(lsn);
                        out.ops.push(parsed);
                    }
                    None => {
                        out.warns += 1;
                        break;
                    }
                },
                OP_BATCH_V3 => match decode_batch3(payload) {
                    Some(subs) => {
                        out.last_lsn = out.last_lsn.max(lsn);
                        out.ops.push(Op::Batch(subs));
                    }
                    None => {
                        out.warns += 1;
                        break;
                    }
                },
                _ => {
                    out.warns += 1;
                    break;
                }
            }
            off = end;
        }
        if off < bytes.len() && out.warns == 0 {
            out.warns += 1;
        }
        out
    } else {
        let (ops, warns) = decode_wal(bytes);
        WalReplay { ops, warns, header: None, last_lsn: 0 }
    }
}

fn decode_batch3(payload: &[u8]) -> Option<Vec<Op>> {
    if payload.len() < 4 {
        return None;
    }
    let count = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
    let mut ops = Vec::with_capacity(count);
    let mut off = 4usize;
    for _ in 0..count {
        if off + 1 > payload.len() {
            return None;
        }
        let t = payload[off];
        off += 1;
        match t {
            OP_SET => {
                if off + 4 > payload.len() {
                    return None;
                }
                let klen = u32::from_le_bytes(payload[off..off + 4].try_into().unwrap()) as usize;
                off += 4;
                if off + klen + 4 > payload.len() {
                    return None;
                }
                let key = payload[off..off + klen].to_vec();
                off += klen;
                let vlen = u32::from_le_bytes(payload[off..off + 4].try_into().unwrap()) as usize;
                off += 4;
                if off + vlen > payload.len() {
                    return None;
                }
                ops.push(Op::Set {
                    key,
                    value: payload[off..off + vlen].to_vec().into_boxed_slice(),
                });
                off += vlen;
            }
            OP_DEL => {
                if off + 4 > payload.len() {
                    return None;
                }
                let klen = u32::from_le_bytes(payload[off..off + 4].try_into().unwrap()) as usize;
                off += 4;
                if off + klen > payload.len() {
                    return None;
                }
                ops.push(Op::Del { key: payload[off..off + klen].to_vec() });
                off += klen;
            }
            OP_CLEAR => ops.push(Op::Clear),
            _ => return None,
        }
    }
    Some(ops)
}


pub fn encode_manifest(m: &Manifest) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + m.segments.len() * 48);
    out.extend_from_slice(MANIFEST_MAGIC);
    out.extend_from_slice(&MANIFEST_VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&m.generation.to_le_bytes());
    out.extend_from_slice(&(m.indexes.len() as u32).to_le_bytes());
    for idx in &m.indexes {
        out.extend_from_slice(&(idx.len() as u16).to_le_bytes());
        out.extend_from_slice(idx.as_bytes());
    }
    out.extend_from_slice(&(m.segments.len() as u32).to_le_bytes());
    for s in &m.segments {
        out.extend_from_slice(&s.id.to_le_bytes());
        out.extend_from_slice(&s.gen.to_le_bytes());
        out.extend_from_slice(&s.entries.to_le_bytes());
        out.extend_from_slice(&(s.name.len() as u16).to_le_bytes());
        out.extend_from_slice(s.name.as_bytes());
        out.extend_from_slice(&s.file_crc.to_le_bytes());
    }
    let mut crc = Crc::new();
    crc.update(&out);
    out.extend_from_slice(&crc.finalize().to_le_bytes());
    out
}

pub fn decode_manifest(bytes: &[u8]) -> Result<Manifest> {
    if bytes.len() < 20 || &bytes[..8] != MANIFEST_MAGIC {
        return Err(SpectreError::snapshot_corrupted("Invalid manifest header"));
    }
    let mut crc = Crc::new();
    crc.update(&bytes[..bytes.len() - 4]);
    if crc.finalize() != u32::from_le_bytes(bytes[bytes.len() - 4..].try_into().unwrap()) {
        return Err(SpectreError::snapshot_corrupted("Manifest CRC mismatch"));
    }
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != MANIFEST_VERSION {
        return Err(SpectreError::snapshot_corrupted(format!(
            "Unsupported manifest version: {}",
            version
        )));
    }
    let mut m = Manifest { generation: u64::from_le_bytes(bytes[12..20].try_into().unwrap()), ..Default::default() };
    let mut off = 20usize;
    let rd_u32 = |off: &mut usize| -> u32 {
        let v = u32::from_le_bytes(bytes[*off..*off + 4].try_into().unwrap());
        *off += 4;
        v
    };
    let rd_u64 = |off: &mut usize| -> u64 {
        let v = u64::from_le_bytes(bytes[*off..*off + 8].try_into().unwrap());
        *off += 8;
        v
    };
    let idx_count = rd_u32(&mut off) as usize;
    for _ in 0..idx_count {
        let l = u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap()) as usize;
        off += 2;
        if off + l > bytes.len() {
            return Err(SpectreError::snapshot_corrupted("Truncated manifest index"));
        }
        let s = String::from_utf8_lossy(&bytes[off..off + l]).into_owned();
        off += l;
        m.indexes.push(s);
    }
    let seg_count = rd_u32(&mut off) as usize;
    for _ in 0..seg_count {
        let id = rd_u64(&mut off);
        let gen = rd_u64(&mut off);
        let entries = rd_u64(&mut off);
        let name_len = u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap()) as usize;
        off += 2;
        if off + name_len + 4 > bytes.len() {
            return Err(SpectreError::snapshot_corrupted("Truncated manifest segment"));
        }
        let name = String::from_utf8_lossy(&bytes[off..off + name_len]).into_owned();
        off += name_len;
        let file_crc = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
        off += 4;
        m.segments.push(ManifestEntry { id, gen, entries, name, file_crc });
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrip() {
        let mut s = Store::new();
        s.set(b"user.1", br#"{"name":"alice","age":30}"#.to_vec().into_boxed_slice());
        s.set(b"user.2", br#"[1,2,3]"#.to_vec().into_boxed_slice());
        s.set(b"plain", b"42".to_vec().into_boxed_slice());
        let bytes = write_snapshot(&s, false);
        assert_eq!(&bytes[..8], SNAPSHOT_MAGIC);
        let mut s2 = load_snapshot(&bytes, None).unwrap();
        assert_eq!(s2.len(), 3);
        assert_eq!(s2.get(b"user.1"), Some(br#"{"name":"alice","age":30}"#.to_vec()));
    }

    #[test]
    fn snapshot_roundtrip_gzip() {
        let mut s = Store::new();
        for i in 0..1000 {
            s.set(format!("k{}", i).as_bytes(), br#"{"v":"xxxxxxxxxxxxxxxxxxxxxxxx"}"#.to_vec().into_boxed_slice());
        }
        let bytes = write_snapshot(&s, true);
        let s2 = load_snapshot(&bytes, None).unwrap();
        assert_eq!(s2.len(), 1000);
        assert!(bytes.len() < write_snapshot(&s, false).len());
    }

    #[test]
    fn snapshot_crc_corruption() {
        let mut s = Store::new();
        s.set(b"a", b"1".to_vec().into_boxed_slice());
        let mut bytes = write_snapshot(&s, false);
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xFF;
        assert!(load_snapshot(&bytes, None).is_err());
    }

    #[test]
    fn wal_roundtrip() {
        let _ops = vec![
            Op::Set { key: b"a.b".to_vec(), value: br#"{"x":1}"#.to_vec().into_boxed_slice() },
            Op::Del { key: b"c".to_vec() },
            Op::Clear,
            Op::Batch(vec![
                Op::Set { key: b"b".to_vec(), value: b"2".to_vec().into_boxed_slice() },
                Op::Del { key: b"d".to_vec() },
            ]),
        ];
        let mut buf = Vec::new();
        buf.extend_from_slice(&encode_set(b"a.b", br#"{"x":1}"#));
        buf.extend_from_slice(&encode_del(b"c"));
        buf.extend_from_slice(&encode_clear());
        buf.extend_from_slice(&encode_batch(&[
            Op::Set { key: b"b".to_vec(), value: b"2".to_vec().into_boxed_slice() },
            Op::Del { key: b"d".to_vec() },
        ]));

        let (decoded, warns) = decode_wal(&buf);
        assert_eq!(warns, 0);
        assert_eq!(decoded.len(), 4);
        let mut store = Store::new();
        assert_eq!(apply_ops(&mut store, &decoded), 5);

        assert_eq!(store.len(), 1);
        assert_eq!(store.get(b"b"), Some(b"2".to_vec()));
    }

    #[test]
    fn wal_torn_write() {
        let mut buf = encode_set(b"a", b"1");
        buf.extend_from_slice(&encode_set(b"b", b"2"));
        buf.truncate(buf.len() - 3);
        let (ops, _warns) = decode_wal(&buf);
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn wal_crc_corruption() {
        let mut buf = encode_set(b"a", b"1");
        buf.extend_from_slice(&encode_set(b"b", b"2"));
        buf[2] ^= 0xFF;
        let (ops, warns) = decode_wal(&buf);
        assert!(ops.is_empty());
        assert_eq!(warns, 1);
    }
}
