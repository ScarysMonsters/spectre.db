use crate::crypto;
use crate::error::{codes, Result, SpectreError};
use crate::formats::{self, Compression, Manifest, ManifestEntry, Op, WalReplay};
use crate::index::SecondaryIndex;
use crate::json_compat;
use crate::lock::FileLock;
use crate::pathnorm::DbPaths;
use crate::store::Store;
use crate::validator;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatMode {
    V2,
    Json,
}

impl FormatMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            FormatMode::V2 => "v2",
            FormatMode::Json => "json",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Durability {
    #[default]
    Process,
    Durable,
}

impl Durability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Durability::Process => "process",
            Durability::Durable => "durable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompactMode {
    #[default]
    Auto,
    Legacy,
    Segments,
}

#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub compress: bool,
    pub compression: Compression,
    pub encryption_key: Option<Vec<u8>>,
    pub encrypt_backups: bool,

    pub encrypt_snapshot: bool,

    pub scrypt_log_n: u8,
    pub scrypt_r: u32,
    pub scrypt_p: u32,
    pub backup_count: u32,

    pub format: String,
    pub lock_timeout_ms: u64,
    pub wal_buffering: bool,

    pub sync_wal: bool,
    pub durability: Durability,
    pub compact_mode: CompactMode,


    pub legacy_threshold: u64,

    pub segment_merge_every: u32,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            compress: false,
            compression: Compression::None,
            encryption_key: None,
            encrypt_backups: false,
            encrypt_snapshot: false,
            scrypt_log_n: 14,
            scrypt_r: 8,
            scrypt_p: 1,
            backup_count: 3,
            format: "auto".to_string(),
            lock_timeout_ms: crate::lock::DEFAULT_LOCK_TIMEOUT_MS,
            wal_buffering: false,
            sync_wal: false,
            durability: Durability::Process,
            compact_mode: CompactMode::Auto,
            legacy_threshold: 100_000,
            segment_merge_every: 8,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InitEvent {
    pub event: String,

    pub payload: String,
}

pub struct Engine {
    store: Store,
    opts: EngineOptions,
    paths: DbPaths,
    mode: FormatMode,
    wal_file: Option<File>,
    wal_buf: Vec<u8>,
    wal_ops: usize,
    wal_bytes: u64,
    closed: bool,
    enc_key: Option<[u8; crypto::KEY_LEN]>,
    lock: Option<FileLock>,
    init_events: Vec<InitEvent>,


    dirty: BTreeSet<Vec<u8>>,

    dirty_all: bool,

    manifest: Manifest,
    next_seg_id: u64,

    wal_gen: u64,
    base_lsn: u64,
    last_lsn: u64,

    recovery_count: u64,
    last_compaction_ms: Option<u64>,
    cache_hits: u64,
    cache_misses: u64,

    indexes: BTreeMap<String, SecondaryIndex>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("mode", &self.mode)
            .field("entries", &self.store.len())
            .field("wal_ops", &self.wal_ops)
            .field("closed", &self.closed)
            .finish()
    }
}

impl Engine {


    pub fn open(path: &std::path::Path, opts: EngineOptions) -> Result<Self> {
        let paths = DbPaths::resolve(path)
            .map_err(|e| SpectreError::io(e, "Invalid database path"))?;
        std::fs::create_dir_all(&paths.dir)
            .map_err(|e| SpectreError::io(e, "Failed to create directory"))?;

        let mut lock = FileLock::new(paths.lock_path());
        lock.acquire(opts.lock_timeout_ms)?;


        let mut opts = opts;
        if opts.sync_wal {
            opts.durability = Durability::Durable;
        }

        if opts.compress && opts.compression == Compression::None {
            opts.compression = Compression::Gzip;
        }

        let enc_key = match &opts.encryption_key {
            Some(raw) => Some(crypto::derive_key_params(raw, opts.scrypt_log_n, opts.scrypt_r, opts.scrypt_p)?),
            None => None,
        };

        let exists = |p: &PathBuf| p.exists();
        let has_v2 = exists(&paths.spdb_path())
            || exists(&paths.spwal_path())
            || exists(&paths.manifest_path());
        let has_json = exists(&paths.snapshot_path()) || exists(&paths.wal_path());

        let mode = match opts.format.as_str() {
            "v2" => FormatMode::V2,
            "json" => FormatMode::Json,
            _ => {
                if has_v2 {
                    FormatMode::V2
                } else if has_json {
                    FormatMode::Json
                } else {
                    FormatMode::V2
                }
            }
        };

        let mut engine = Self {
            store: Store::new(),
            opts,
            paths,
            mode,
            wal_file: None,
            wal_buf: Vec::with_capacity(64 * 1024),
            wal_ops: 0,
            wal_bytes: 0,
            closed: false,
            enc_key,
            lock: Some(lock),
            init_events: Vec::new(),
            dirty: BTreeSet::new(),
            dirty_all: false,
            manifest: Manifest::default(),
            next_seg_id: 1,
            wal_gen: 0,
            base_lsn: 0,
            last_lsn: 0,
            recovery_count: 0,
            last_compaction_ms: None,
            cache_hits: 0,
            cache_misses: 0,
            indexes: BTreeMap::new(),
        };

        engine.load_state()?;
        engine.open_wal()?;
        Ok(engine)
    }

    fn load_state(&mut self) -> Result<()> {
        if self.mode == FormatMode::V2 {

            if self.paths.manifest_path().exists() {
                match std::fs::read(self.paths.manifest_path()) {
                    Ok(bytes) => match formats::decode_manifest(&bytes) {
                        Ok(m) => {
                            for seg in &m.segments {
                                let p = self.paths.dir.join(&seg.name);
                                if let Ok(sb) = std::fs::read(&p) {
                                    let crc = crc32fast::hash(&sb);
                                    if crc != seg.file_crc {
                                        self.push_warn(format!(
                                            "Segment {} CRC mismatch — skipped",
                                            seg.name
                                        ));
                                        continue;
                                    }
                                    if let Ok(part) = formats::load_snapshot(&sb, self.enc_key.as_ref()) {
                                        for (k, v) in part.iter() {
                                            if v == b"\x00DEL" {
                                                self.store.delete(k);
                                            } else {
                                                self.store.raw_insert(k, v.to_vec().into_boxed_slice());
                                            }
                                        }
                                    } else {
                                        self.push_warn(format!("Segment {} unreadable — skipped", seg.name));
                                    }
                                } else {
                                    self.push_warn(format!("Segment {} missing — skipped", seg.name));
                                }
                            }
                            self.next_seg_id = m.segments.iter().map(|s| s.id + 1).max().unwrap_or(1);
                            self.manifest = m;
                        }
                        Err(_) => {
                            self.push_warn("Manifest corrupted — ignoring segments".into());
                        }
                    },
                    Err(_) => self.push_warn("Manifest unreadable — ignoring segments".into()),
                }
            }
        }


        let snap_path = match self.mode {
            FormatMode::V2 => self.paths.spdb_path(),
            FormatMode::Json => self.paths.snapshot_path(),
        };
        if snap_path.exists() {
            match self.read_snapshot_file(&snap_path) {
                Ok(base) => {

                    for (k, v) in base.iter() {
                        self.store.raw_insert(k, v.to_vec().into_boxed_slice());
                    }
                }


                Err(err) if err.code == codes::DECRYPTION_FAILED => return Err(err),
                Err(_) => {
                    self.push_warn("Snapshot corrupted, attempting backup restore".into());
                    self.restore_from_backup()?;
                }
            }
        }


        let wal_path = match self.mode {
            FormatMode::V2 => self.paths.spwal_path(),
            FormatMode::Json => self.paths.wal_path(),
        };
        if wal_path.exists() {
            if let Ok(bytes) = std::fs::read(&wal_path) {
                let replay: WalReplay = match self.mode {
                    FormatMode::V2 => formats::replay_wal(&bytes),
                    FormatMode::Json => {
                        let (ops, warns) = json_compat::decode_json_wal(&bytes);
                        WalReplay { ops, warns, header: None, last_lsn: 0 }
                    }
                };
                if let Some(h) = &replay.header {
                    self.wal_gen = h.generation;
                    self.base_lsn = h.base_lsn;
                }
                formats::apply_ops(&mut self.store, &replay.ops);
                self.last_lsn = self.base_lsn + replay.last_lsn;
                self.wal_ops = replay.ops.len();
                if replay.ops.len() > 0 {
                    self.recovery_count += 1;
                }
                if replay.warns > 0 {
                    self.push_warn(format!("{} invalid WAL entries skipped", replay.warns));
                }
            }
        }


        if self.mode == FormatMode::V2 {
            let paths: Vec<String> = self.manifest.indexes.clone();
            for p in paths {
                let entries: Vec<(Vec<u8>, Vec<u8>)> = self
                    .store
                    .iter()
                    .filter(|(k, _)| !crate::store::is_raw_key(k))
                    .map(|(k, v)| (k.to_vec(), v.to_vec()))
                    .collect();
                self.indexes.insert(p.clone(), SecondaryIndex::build(&p, entries.into_iter()));
            }
        }
        Ok(())
    }

    fn snap_key(&self) -> Option<&[u8; crypto::KEY_LEN]> {
        if self.opts.encrypt_snapshot {
            self.enc_key.as_ref()
        } else {
            None
        }
    }

    fn push_warn(&mut self, msg: String) {
        self.init_events.push(InitEvent {
            event: "warn".into(),
            payload: serde_json::to_string(&msg).unwrap_or_default(),
        });
    }

    fn read_snapshot_file(&self, path: &std::path::Path) -> Result<Store> {
        let bytes = std::fs::read(path)
            .map_err(|e| SpectreError::snapshot_corrupted(format!("Read failed: {}", e)))?;
        match self.mode {
            FormatMode::V2 => formats::load_snapshot(&bytes, self.enc_key.as_ref()),
            FormatMode::Json => json_compat::load_json_snapshot(&bytes, self.opts.compress),
        }
    }

    fn restore_from_backup(&mut self) -> Result<()> {
        for gen in 1..=self.opts.backup_count.max(0) {
            let p = self.paths.backup_path(gen, self.mode == FormatMode::V2);
            if !p.exists() {
                continue;
            }
            match self.read_backup_file(&p) {
                Ok(store) => {
                    self.store = store;
                    self.init_events.push(InitEvent {
                        event: "restore".into(),
                        payload: serde_json::to_string(&serde_json::json!({
                            "generation": gen,
                            "path": p.to_string_lossy(),
                        }))
                        .unwrap_or_default(),
                    });
                    return Ok(());
                }
                Err(err) => {
                    self.init_events.push(InitEvent {
                        event: "warn".into(),
                        payload: serde_json::to_string(&format!(
                            "Failed to restore backup {}: {}",
                            gen, err.message
                        ))
                        .unwrap_or_default(),
                    });
                }
            }
        }
        self.store = Store::new();
        self.init_events.push(InitEvent { event: "reset".into(), payload: String::new() });
        Ok(())
    }

    fn read_backup_file(&self, path: &std::path::Path) -> Result<Store> {
        let mut bytes = std::fs::read(path)
            .map_err(|e| SpectreError::new(codes::BACKUP_CORRUPTED, format!("Read failed: {}", e)))?;
        match self.mode {
            FormatMode::V2 => {
                if self.opts.encrypt_backups {
                    let key = self.enc_key.as_ref().ok_or_else(|| {
                        SpectreError::new(codes::DECRYPTION_FAILED, "Failed to decrypt backup: no key")
                    })?;
                    bytes = crypto::decrypt_backup(&bytes, key)?;
                }
                formats::load_snapshot(&bytes, self.enc_key.as_ref())
            }
            FormatMode::Json => {
                if self.opts.encrypt_backups {


                    if self.opts.compress {
                        let mut dec = flate2::read::GzDecoder::new(&bytes[..]);
                        let mut raw = Vec::new();
                        std::io::Read::read_to_end(&mut dec, &mut raw).map_err(|e| {
                            SpectreError::new(codes::BACKUP_CORRUPTED, format!("Gzip failed: {}", e))
                        })?;
                        bytes = raw;
                    }
                    let key = self.enc_key.as_ref().ok_or_else(|| {
                        SpectreError::new(codes::DECRYPTION_FAILED, "Failed to decrypt backup: no key")
                    })?;
                    bytes = crypto::decrypt_backup(&bytes, key)?;
                } else if self.opts.compress {
                    let mut dec = flate2::read::GzDecoder::new(&bytes[..]);
                    let mut raw = Vec::new();
                    std::io::Read::read_to_end(&mut dec, &mut raw).map_err(|e| {
                        SpectreError::new(codes::BACKUP_CORRUPTED, format!("Gzip failed: {}", e))
                    })?;
                    bytes = raw;
                }
                json_compat::load_json_snapshot(&bytes, false)
            }
        }
    }

    fn open_wal(&mut self) -> Result<()> {
        let wal_path = match self.mode {
            FormatMode::V2 => self.paths.spwal_path(),
            FormatMode::Json => self.paths.wal_path(),
        };
        let needs_header = self.mode == FormatMode::V2
            && (!wal_path.exists() || std::fs::metadata(&wal_path).map(|m| m.len() == 0).unwrap_or(true));
        let f = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&wal_path)
            .map_err(|e| SpectreError::write_failed(format!("Failed to open WAL: {}", e)))?;
        self.wal_file = Some(f);
        if needs_header {

            let header = formats::encode_wal3_header(self.wal_gen, self.last_lsn);
            if let Some(f) = self.wal_file.as_mut() {
                let _ = f.write_all(&header);
                if self.opts.durability == Durability::Durable {
                    let _ = f.sync_data();
                }
            }
        }
        Ok(())
    }


    pub fn drain_init_events(&mut self) -> Vec<InitEvent> {
        std::mem::take(&mut self.init_events)
    }


    pub fn get(&mut self, key: &str) -> Result<Option<Vec<u8>>> {
        self.check_open()?;
        validator::validate_key(key)?;
        let bytes = match self.store.get(key.as_bytes()) {
            Some(v) => {
                self.cache_hits += 1;
                v
            }
            None => {
                self.cache_misses += 1;
                return Ok(None);
            }
        };
        if let Some(k) = &self.enc_key {
            if crypto::looks_encrypted(&bytes) {
                return Ok(Some(crypto::decrypt_value(&bytes, k)?));
            }
        }
        Ok(Some(bytes))
    }

    pub fn has(&self, key: &str) -> Result<bool> {
        self.check_open()?;
        validator::validate_key(key)?;
        Ok(self.store.contains(key.as_bytes()))
    }

    pub fn set(&mut self, key: &str, value_json: &[u8]) -> Result<()> {
        self.check_open()?;
        validator::validate_key(key)?;
        validator::validate_value_size(value_json.len())?;
        serde_json::from_slice::<serde::de::IgnoredAny>(value_json)
            .map_err(|_| SpectreError::new(codes::OPERATION_FAILED, "Value is not valid JSON"))?;

        let stored: Vec<u8> = if self.enc_key.is_some()
            && validator::is_sensitive_key(key)
            && value_json != b"null"
        {
            crypto::encrypt_value(value_json, self.enc_key.as_ref().unwrap())?
        } else {
            value_json.to_vec()
        };

        self.store.set(key.as_bytes(), stored.clone().into_boxed_slice());
        self.dirty.insert(key.as_bytes().to_vec());
        self.index_touch(key.as_bytes());
        self.wal_append(&Op::Set { key: key.as_bytes().to_vec(), value: stored.into_boxed_slice() })?;
        Ok(())
    }

    pub fn delete(&mut self, key: &str) -> Result<bool> {
        self.check_open()?;
        validator::validate_key(key)?;
        if self.store.delete(key.as_bytes()) {
            self.dirty.insert(key.as_bytes().to_vec());
            self.index_touch(key.as_bytes());
            self.wal_append(&Op::Del { key: key.as_bytes().to_vec() })?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn clear(&mut self) -> Result<()> {
        self.check_open()?;
        self.store.clear();
        self.dirty.clear();
        self.dirty_all = true;
        for idx in self.indexes.values_mut() {
            idx.tree.clear();
        }
        self.wal_append(&Op::Clear)?;
        Ok(())
    }


    pub fn batch(&mut self, ops: Vec<Op>) -> Result<Vec<bool>> {
        self.check_open()?;
        for op in &ops {
            if let Op::Set { key, value } = op {
                let k = std::str::from_utf8(key)
                    .map_err(|_| SpectreError::invalid_key("Non-UTF-8 key in batch"))?;
                validator::validate_key(k)?;
                validator::validate_value_size(value.len())?;
            }
        }

        let mut flags = Vec::with_capacity(ops.len());
        for op in &ops {
            match op {
                Op::Set { .. } => flags.push(true),
                Op::Del { key } => flags.push(self.store.contains(key)),
                _ => flags.push(false),
            }
        }


        self.wal_append_batch(&ops)?;
        let mut touched: Vec<Vec<u8>> = Vec::with_capacity(ops.len());
        for op in ops {
            match op {
                Op::Set { key, value } => {
                    self.store.set(&key, value);
                    self.dirty.insert(key.clone());
                    touched.push(key);
                }
                Op::Del { key } => {
                    self.store.delete(&key);
                    self.dirty.insert(key.clone());
                    touched.push(key);
                }
                Op::Clear => {
                    self.store.clear();
                    self.dirty.clear();
                    self.dirty_all = true;
                }
                Op::Batch(sub) => {
                    for s in sub {
                        match s {
                            Op::Set { key, value } => {
                                self.store.set(&key, value);
                                self.dirty.insert(key.clone());
                                touched.push(key);
                            }
                            Op::Del { key } => {
                                self.store.delete(&key);
                                self.dirty.insert(key.clone());
                                touched.push(key);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        for k in &touched {
            self.index_touch(k);
        }
        Ok(flags)
    }


    pub fn all(&self, prefix: Option<&str>) -> Result<Vec<(String, Vec<u8>)>> {
        self.check_open()?;
        let entries = self.store.all(prefix.map(|p| p.as_bytes()));
        let mut out = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            let value = match &self.enc_key {
                Some(key) if crypto::looks_encrypted(&v) => crypto::decrypt_value(&v, key)?,
                _ => v,
            };
            out.push((k, value));
        }
        Ok(out)
    }

    pub fn count(&self, prefix: Option<&str>) -> Result<usize> {
        self.check_open()?;
        Ok(self.store.count(prefix.map(|p| p.as_bytes())))
    }


    pub fn all_json(&self, prefix: Option<&str>) -> Result<String> {
        self.check_open()?;
        let mut out = String::with_capacity(64 + (self.store.value_bytes() as usize) * 2);
        out.push('[');
        let mut first = true;
        {
            let mut push_entry = |key_bytes: &[u8], value: &[u8]| -> Result<()> {
                if first {
                    out.push('[');
                    first = false;
                } else {
                    out.push_str(",[");
                }
                push_json_key(&mut out, key_bytes);
                out.push(',');

                let decrypted;
                let value: &[u8] = match &self.enc_key {
                    Some(key) if crypto::looks_encrypted(value) => {
                        decrypted = crypto::decrypt_value(value, key)?;
                        &decrypted
                    }
                    _ => value,
                };
                out.push_str(std::str::from_utf8(value).map_err(|_| {
                    SpectreError::operation_failed("non-UTF-8 value bytes")
                })?);
                out.push(']');
                Ok(())
            };

            let prefix_b = prefix.map(|p| p.as_bytes());
            self.store.scan(prefix_b, &mut push_entry)?;
        }
        out.push(']');
        Ok(out)
    }


    fn crash_point(&self, name: &str) {
        crash_point_named(name);
    }

    fn wal_append(&mut self, op: &Op) -> Result<()> {
        if self.mode == FormatMode::V2 {
            let start = self.wal_buf.len();
            let lsn = self.last_lsn + 1;
            match op {
                Op::Set { key, value } => {
                    let payload_len = 4 + key.len() + value.len();
                    self.wal_buf.push(formats::OP_SET);
                    self.wal_buf.extend_from_slice(&(payload_len as u32).to_le_bytes());
                    self.wal_buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
                    self.wal_buf.extend_from_slice(key);
                    self.wal_buf.extend_from_slice(value);
                    formats::frame3_tail(&mut self.wal_buf, start, lsn);
                }
                Op::Del { key } => {
                    self.wal_buf.push(formats::OP_DEL);
                    self.wal_buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
                    self.wal_buf.extend_from_slice(key);
                    formats::frame3_tail(&mut self.wal_buf, start, lsn);
                }
                Op::Clear => {
                    self.wal_buf.push(formats::OP_CLEAR);
                    self.wal_buf.extend_from_slice(&0u32.to_le_bytes());
                    formats::frame3_tail(&mut self.wal_buf, start, lsn);
                }
                Op::Batch(_) => unreachable!("batches use wal_append_batch"),
            }
            self.last_lsn = lsn;
            self.wal_ops += 1;
        } else {
            json_compat::encode_json_wal_op(op, &mut self.wal_buf);
            self.wal_ops += 1;
        }
        if self.opts.wal_buffering {
            if self.wal_buf.len() >= 64 * 1024 {
                self.wal_flush()?;
            }
        } else {
            self.wal_flush()?;
        }
        Ok(())
    }


    fn wal_append_batch(&mut self, ops: &[Op]) -> Result<()> {
        if self.mode != FormatMode::V2 {

            let op = Op::Batch(ops.to_vec());
            json_compat::encode_json_wal_op(&op, &mut self.wal_buf);
            self.wal_ops += 1;
            return self.wal_flush_gate();
        }
        let start = self.wal_buf.len();
        self.wal_buf.push(formats::OP_BATCH_V3);
        let plen_pos = self.wal_buf.len();
        self.wal_buf.extend_from_slice(&0u32.to_le_bytes());
        formats::batch3_payload(ops, &mut self.wal_buf);
        let plen = (self.wal_buf.len() - plen_pos - 4) as u32;
        self.wal_buf[plen_pos..plen_pos + 4].copy_from_slice(&plen.to_le_bytes());
        let lsn = self.last_lsn + 1;
        formats::frame3_tail(&mut self.wal_buf, start, lsn);
        self.last_lsn = lsn;
        self.wal_ops += 1;
        self.wal_flush_gate()
    }


    fn wal_flush_gate(&mut self) -> Result<()> {
        if self.opts.wal_buffering {
            if self.wal_buf.len() >= 64 * 1024 {
                self.wal_flush()?;
            }
        } else {
            self.wal_flush()?;
        }
        Ok(())
    }

    fn wal_flush(&mut self) -> Result<()> {
        if self.wal_buf.is_empty() {
            return Ok(());
        }
        let durable = self.opts.durability == Durability::Durable;
        {
            let f = self
                .wal_file
                .as_mut()
                .ok_or_else(|| SpectreError::write_failed("WAL is not open"))?;
            f.write_all(&self.wal_buf)
                .map_err(|e| SpectreError::write_failed(format!("Failed to write to WAL: {}", e)))?;
        }
        self.crash_point("wal_after_write");
        if durable {
            let f = self.wal_file.as_mut().unwrap();
            f.sync_data()
                .map_err(|e| SpectreError::write_failed(format!("Failed to sync WAL: {}", e)))?;
            self.crash_point("wal_after_sync");
        }
        self.wal_bytes += self.wal_buf.len() as u64;
        self.wal_buf.clear();
        Ok(())
    }


    pub fn compact(&mut self) -> Result<()> {
        self.check_open()?;
        self.wal_flush()?;


        let use_segments = self.mode == FormatMode::V2
            && match self.opts.compact_mode {
                CompactMode::Legacy => false,
                CompactMode::Segments => true,
                CompactMode::Auto => self.store.count(None) as u64 > self.opts.legacy_threshold,
            };

        if !use_segments {
            return self.compact_legacy();
        }


        let full_merge = self.dirty_all
            || self.manifest.segments.is_empty() && self.dirty.len() >= self.store.count(None)
            || self.manifest.segments.len() >= self.opts.segment_merge_every as usize;

        if full_merge {
            self.compact_legacy()?;

            for seg in std::mem::take(&mut self.manifest.segments) {
                let _ = std::fs::remove_file(self.paths.dir.join(&seg.name));
            }
            self.manifest.generation += 1;
            self.manifest.indexes = self.indexes.keys().cloned().collect();
            self.write_manifest()?;
            self.reset_wal()?;
            self.last_compaction_ms = Some(unix_ms());
            return Ok(());
        }


        let mut scratch = Store::new();
        let dirty_keys: Vec<Vec<u8>> = self.dirty.iter().cloned().collect();
        for k in &dirty_keys {
            if let Some(v) = self.store.get(k) {
                scratch.raw_insert(k, v.into_boxed_slice());
            } else {


                scratch.raw_insert(k, b"\x00DEL".to_vec().into_boxed_slice());
            }
        }
        let seg_id = self.next_seg_id;
        let bytes = formats::write_snapshot_ex(&scratch, self.opts.compression, self.snap_key());
        let name = format!("{}.spseg-{}", self.paths.base, seg_id);
        let path = self.paths.dir.join(&name);
        let tmp = unique_tmp(&path);
        write_file_atomic(&tmp, &bytes, &path, None)?;
        self.crash_point("seg_written");
        if self.opts.durability == Durability::Durable {
            if let Ok(f) = File::open(&path) {
                let _ = f.sync_data();
            }
        }

        self.manifest.segments.push(ManifestEntry {
            id: seg_id,
            gen: self.manifest.generation + 1,
            entries: scratch.len() as u64,
            file_crc: crc32fast::hash(&bytes),
            name: name.clone(),
        });
        self.next_seg_id += 1;
        self.dirty.clear();
        self.dirty_all = false;
        self.write_manifest()?;
        self.crash_point("man_after_rename");


        self.last_compaction_ms = Some(unix_ms());
        Ok(())
    }


    fn compact_legacy(&mut self) -> Result<()> {
        self.rotate_backups()?;

        let bytes = match self.mode {
            FormatMode::V2 => formats::write_snapshot_ex(&self.store, self.opts.compression, self.snap_key()),
            FormatMode::Json => json_compat::write_json_snapshot(&self.store, self.opts.compress)?,
        };
        let snap_path = match self.mode {
            FormatMode::V2 => self.paths.spdb_path(),
            FormatMode::Json => self.paths.snapshot_path(),
        };
        let tmp = unique_tmp(&snap_path);
        write_file_atomic(&tmp, &bytes, &snap_path, Some("snap_before_rename"))?;
        self.crash_point("snap_after_rename");


        if self.opts.encrypt_backups {
            let gen1 = self.paths.backup_path(1, self.mode == FormatMode::V2);
            if gen1.exists() {
                if let Some(key) = &self.enc_key {
                    let content = std::fs::read(&gen1)
                        .map_err(|e| SpectreError::io(e, "Failed to read backup"))?;
                    let content = self.prepare_backup_content(&content)?;
                    let enc = crypto::encrypt_backup(&content, key)?;
                    std::fs::write(&gen1, enc)
                        .map_err(|e| SpectreError::io(e, "Failed to write backup"))?;
                }
            }
        }

        self.dirty.clear();
        self.dirty_all = false;
        self.last_compaction_ms = Some(unix_ms());


        self.reset_wal()?;
        Ok(())
    }


    fn reset_wal(&mut self) -> Result<()> {
        self.wal_file = None;
        self.wal_buf.clear();
        let wal_path = match self.mode {
            FormatMode::V2 => self.paths.spwal_path(),
            FormatMode::Json => self.paths.wal_path(),
        };
        let f = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&wal_path)
            .map_err(|e| SpectreError::write_failed(format!("Failed to truncate WAL: {}", e)))?;
        self.wal_file = Some(f);
        if self.mode == FormatMode::V2 {
            self.base_lsn = self.last_lsn;
            let header = formats::encode_wal3_header(self.wal_gen, self.base_lsn);
            self.wal_file.as_mut().unwrap().write_all(&header)
                .map_err(|e| SpectreError::write_failed(format!("Failed to stamp WAL header: {}", e)))?;
        }
        self.wal_ops = 0;
        self.wal_bytes = 0;
        Ok(())
    }


    fn write_manifest(&mut self) -> Result<()> {
        self.manifest.indexes = self.indexes.keys().cloned().collect();
        let bytes = formats::encode_manifest(&self.manifest);
        let path = self.paths.manifest_path();
        let tmp = unique_tmp(&path);
        write_file_atomic(&tmp, &bytes, &path, Some("man_before_rename"))?;
        Ok(())
    }


    pub fn prepare_compact(&mut self) -> Result<crate::segments::CompactJob> {
        self.check_open()?;
        self.wal_flush()?;
        let use_segments = self.mode == FormatMode::V2
            && match self.opts.compact_mode {
                CompactMode::Legacy => false,
                CompactMode::Segments => true,
                CompactMode::Auto => self.store.count(None) as u64 > self.opts.legacy_threshold,
            };
        if !use_segments {

            return Ok(crate::segments::CompactJob::legacy());
        }
        let full_merge = self.dirty_all
            || self.manifest.segments.is_empty() && self.dirty.len() >= self.store.count(None)
            || self.manifest.segments.len() >= self.opts.segment_merge_every as usize;
        let compression = self.opts.compression;
        let enc_key = self.snap_key().copied();
        let generation = self.manifest.generation + 1;
        let seg_id = self.next_seg_id;
        let base = self.paths.base.clone();
        let job = if full_merge {
            let mut scratch = Store::new();
            for (k, v) in self.store.iter() {
                scratch.raw_insert(k, v.to_vec().into_boxed_slice());
            }
            self.dirty.clear();
            self.dirty_all = false;
            crate::segments::CompactJob::full(scratch, compression, enc_key, generation, base)
        } else {
            let mut scratch = Store::new();
            let dirty_keys: Vec<Vec<u8>> = self.dirty.iter().cloned().collect();
            for k in &dirty_keys {
                if let Some(v) = self.store.get(k) {
                    scratch.raw_insert(k, v.into_boxed_slice());
                } else {
                    scratch.raw_insert(k, b"\x00DEL".to_vec().into_boxed_slice());
                }
            }
            self.dirty.clear();
            self.dirty_all = false;
            crate::segments::CompactJob::delta(scratch, compression, enc_key, generation, seg_id, base)
        };
        Ok(job)
    }


    pub fn finish_compact(&mut self, done: crate::segments::CompactJobDone) -> Result<&'static str> {
        self.check_open()?;
        if done.legacy {
            return Ok("legacy");
        }
        if done.generation != self.manifest.generation + 1 {
            return Ok("stale");
        }
        let path = self.paths.dir.join(&done.name);
        let tmp = unique_tmp(&path);
        write_file_atomic(&tmp, &done.bytes, &path, None)?;
        if self.opts.durability == Durability::Durable {
            if let Ok(f) = File::open(&path) {
                let _ = f.sync_data();
            }
        }
        if done.full {
            for seg in std::mem::take(&mut self.manifest.segments) {
                let _ = std::fs::remove_file(self.paths.dir.join(&seg.name));
            }
        }
        self.manifest.segments.push(ManifestEntry {
            id: done.seg_id,
            gen: done.generation,
            entries: done.entries,
            file_crc: done.crc,
            name: done.name.clone(),
        });
        self.manifest.generation = done.generation;
        self.next_seg_id = self.next_seg_id.max(done.seg_id + 1);
        self.write_manifest()?;
        self.last_compaction_ms = Some(unix_ms());
        Ok("committed")
    }


    fn prepare_backup_content(&self, raw: &[u8]) -> Result<Vec<u8>> {
        if self.mode != FormatMode::Json || !self.opts.compress {
            return Ok(raw.to_vec());
        }
        let mut dec = flate2::read::GzDecoder::new(raw);
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut dec, &mut out)
            .map_err(|e| SpectreError::new(codes::BACKUP_CORRUPTED, format!("Gzip failed: {}", e)))?;
        Ok(out)
    }

    fn rotate_backups(&mut self) -> Result<()> {
        let v2 = self.mode == FormatMode::V2;
        let snapshot = if v2 { self.paths.spdb_path() } else { self.paths.snapshot_path() };
        for gen in (1..=self.opts.backup_count.max(0)).rev() {
            let src = if gen == 1 { snapshot.clone() } else { self.paths.backup_path(gen - 1, v2) };
            let dst = self.paths.backup_path(gen, v2);
            if src.exists() {
                let _ = std::fs::rename(&src, &dst);
            }
        }
        Ok(())
    }


    pub fn migrate(&mut self, target: FormatMode) -> Result<()> {
        self.check_open()?;
        self.wal_flush()?;
        if target == self.mode {
            return Ok(());
        }
        let (old_snap, old_wal) = match self.mode {
            FormatMode::V2 => (self.paths.spdb_path(), self.paths.spwal_path()),
            FormatMode::Json => (self.paths.snapshot_path(), self.paths.wal_path()),
        };
        self.mode = target;


        self.compact_legacy()?;
        if self.mode == FormatMode::Json {
            let _ = std::fs::remove_file(self.paths.manifest_path());
        }
        for seg in std::mem::take(&mut self.manifest.segments) {
            let _ = std::fs::remove_file(self.paths.dir.join(&seg.name));
        }
        let _ = std::fs::remove_file(self.paths.manifest_path());
        self.manifest.generation = 0;
        self.reset_wal()?;
        let _ = std::fs::remove_file(&old_snap);
        let _ = std::fs::remove_file(&old_wal);
        Ok(())
    }

    pub fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.wal_flush()?;
        if self.wal_ops > 0 || !self.dirty.is_empty() || self.dirty_all {
            self.compact()?;
        }
        self.closed = true;
        self.wal_file = None;
        if let Some(mut l) = self.lock.take() {
            l.release();
        }
        Ok(())
    }


    pub fn stats(&self) -> Result<Stats> {
        self.check_open()?;
        let snap_path = match self.mode {
            FormatMode::V2 => self.paths.spdb_path(),
            FormatMode::Json => self.paths.snapshot_path(),
        };
        let mut on_disk = std::fs::metadata(&snap_path).map(|m| m.len()).unwrap_or(0);
        for seg in &self.manifest.segments {
            on_disk += std::fs::metadata(self.paths.dir.join(&seg.name)).map(|m| m.len()).unwrap_or(0);
        }
        let store_bytes = self.store.value_bytes();
        Ok(Stats {
            entries: self.store.count(None),
            raw_entries: self.store.raw_count(),
            store_value_bytes: store_bytes,
            file_size: on_disk,
            wal_ops: self.wal_ops,
            wal_bytes: self.wal_bytes + self.wal_buf.len() as u64,
            format: self.mode.as_str().to_string(),
            compress: self.opts.compress || self.opts.compression != Compression::None,
            compression: self.opts.compression.as_str().to_string(),
            encrypted: self.enc_key.is_some(),
            snapshot_encrypted: self.opts.encrypt_snapshot && self.enc_key.is_some(),
            durability: self.opts.durability.as_str().to_string(),
            snapshot_path: snap_path.to_string_lossy().into_owned(),
            wal_path: match self.mode {
                FormatMode::V2 => self.paths.spwal_path(),
                FormatMode::Json => self.paths.wal_path(),
            }
            .to_string_lossy()
            .into_owned(),

            pending_writes: self.dirty.len() as u64,
            cache_hits: self.cache_hits,
            cache_misses: self.cache_misses,
            compression_ratio: if on_disk > 0 { store_bytes as f64 / on_disk as f64 } else { 0.0 },
            segment_count: self.manifest.segments.len() as u64,
            generation: self.manifest.generation,
            last_compaction_ms: self.last_compaction_ms,
            last_lsn: self.last_lsn,
            recovery_count: self.recovery_count,
            index_count: self.indexes.len() as u64,
        })
    }

    pub fn mode(&self) -> FormatMode {
        self.mode
    }

    fn check_open(&self) -> Result<()> {
        if self.closed {
            Err(SpectreError::closed())
        } else {
            Ok(())
        }
    }
}


impl Engine {


    pub fn set_raw(&mut self, key: &str, bytes: &[u8]) -> Result<()> {
        self.check_open()?;
        validator::validate_key(key)?;
        validator::validate_value_size(bytes.len())?;
        self.store.set_raw(key.as_bytes(), bytes.to_vec().into_boxed_slice());
        self.dirty.insert(key.as_bytes().to_vec());
        self.wal_append(&Op::Set {
            key: [b"\x00".as_slice(), key.as_bytes()].concat(),
            value: bytes.to_vec().into_boxed_slice(),
        })?;
        Ok(())
    }

    pub fn get_raw(&mut self, key: &str) -> Result<Option<Vec<u8>>> {
        self.check_open()?;
        validator::validate_key(key)?;
        match self.store.get_raw(key.as_bytes()) {
            Some(v) => {
                self.cache_hits += 1;
                Ok(Some(v))
            }
            None => {
                self.cache_misses += 1;
                Ok(None)
            }
        }
    }

    pub fn delete_raw(&mut self, key: &str) -> Result<bool> {
        self.check_open()?;
        validator::validate_key(key)?;
        if self.store.delete_raw(key.as_bytes()) {
            self.dirty.insert(key.as_bytes().to_vec());
            self.wal_append(&Op::Del {
                key: [b"\x00".as_slice(), key.as_bytes()].concat(),
            })?;
            Ok(true)
        } else {
            Ok(false)
        }
    }


    pub fn scan_page(
        &mut self,
        prefix: Option<&str>,
        after: &str,
        limit: usize,
    ) -> Result<(Vec<(String, Vec<u8>)>, String)> {
        self.check_open()?;


        let (rows, cursor) = self.store.page(
            prefix.map(|p| p.as_bytes()),
            after.as_bytes(),
            limit.max(1),
        );
        let mut out = Vec::with_capacity(rows.len());
        for (k, v) in rows {
            let value = match &self.enc_key {
                Some(key) if crypto::looks_encrypted(&v) => crypto::decrypt_value(&v, key)?,
                _ => v,
            };
            out.push((String::from_utf8_lossy(&k).into_owned(), value));
        }
        Ok((out, String::from_utf8_lossy(&cursor).into_owned()))
    }


    pub fn create_index(&mut self, path: &str) -> Result<u64> {
        self.check_open()?;
        if path.is_empty() {
            return Err(SpectreError::invalid_key("Index path is empty"));
        }
        if !self.indexes.contains_key(path) {
            let entries: Vec<(Vec<u8>, Vec<u8>)> = self
                .store
                .iter()
                .filter(|(k, _)| !crate::store::is_raw_key(k))
                .map(|(k, v)| (k.to_vec(), v.to_vec()))
                .collect();
            let idx = SecondaryIndex::build(path, entries.into_iter());
            self.indexes.insert(path.to_string(), idx);
            if self.mode == FormatMode::V2 {
                self.write_manifest()?;
            }
        }
        Ok(self.indexes.get(path).map(|i| i.len()).unwrap_or(0) as u64)
    }

    pub fn drop_index(&mut self, path: &str) -> Result<bool> {
        self.check_open()?;
        let removed = self.indexes.remove(path).is_some();
        if removed && self.mode == FormatMode::V2 {
            self.write_manifest()?;
        }
        Ok(removed)
    }

    pub fn index_list(&self) -> Vec<String> {
        self.indexes.keys().cloned().collect()
    }


    pub fn index_lookup(&mut self, path: &str, value_json: &[u8]) -> Result<Vec<String>> {
        self.check_open()?;
        self.ensure_index(path)?;
        Ok(self
            .indexes
            .get(path)
            .map(|i| i.lookup(value_json).into_iter().map(|k| String::from_utf8_lossy(&k).into_owned()).collect())
            .unwrap_or_default())
    }


    pub fn index_range(
        &mut self,
        path: &str,
        gte: Option<&[u8]>,
        lt: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<String>> {
        self.check_open()?;
        self.ensure_index(path)?;
        Ok(self
            .indexes
            .get(path)
            .map(|i| {
                i.range(gte, lt, limit)
                    .into_iter()
                    .map(|k| String::from_utf8_lossy(&k).into_owned())
                    .collect()
            })
            .unwrap_or_default())
    }

    fn ensure_index(&mut self, path: &str) -> Result<()> {
        if !self.indexes.contains_key(path) {
            self.create_index(path)?;
        }
        Ok(())
    }


    fn index_touch(&mut self, key: &[u8]) {
        if self.indexes.is_empty() {
            return;
        }
        let leaf = if self.store.contains(key) {
            Some(key.to_vec())
        } else {
            (0..key.len())
                .find(|&i| key[i] == b'.' && self.store.contains(&key[..i]))
                .map(|i| key[..i].to_vec())
        };
        match leaf {
            Some(lk) => {
                let val = self.store.get(&lk).unwrap_or_default();
                for idx in self.indexes.values_mut() {
                    idx.insert_key(&lk, &val);
                }
            }
            None => {
                for idx in self.indexes.values_mut() {
                    idx.remove_key(key);
                }
            }
        }
    }


    pub fn durability(&self) -> Durability {
        self.opts.durability
    }

    pub fn compression(&self) -> Compression {
        self.opts.compression
    }

    pub fn compact_mode(&self) -> CompactMode {
        self.opts.compact_mode
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct Stats {
    pub entries: usize,
    pub raw_entries: usize,
    pub store_value_bytes: u64,
    pub file_size: u64,
    pub wal_ops: usize,
    pub wal_bytes: u64,
    pub format: String,
    pub compress: bool,
    pub compression: String,
    pub encrypted: bool,
    pub snapshot_encrypted: bool,
    pub durability: String,
    pub snapshot_path: String,
    pub wal_path: String,

    pub pending_writes: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub compression_ratio: f64,
    pub segment_count: u64,
    pub generation: u64,
    pub last_compaction_ms: Option<u64>,
    pub last_lsn: u64,
    pub recovery_count: u64,
    pub index_count: u64,
}


fn push_json_key(out: &mut String, key: &[u8]) {
    let needs_escape = key
        .iter()
        .any(|&b| b == b'"' || b == b'\\' || b < 0x20);
    if !needs_escape {
        out.push('"');
        out.push_str(std::str::from_utf8(key).unwrap_or(""));
        out.push('"');
    } else {
        let s = String::from_utf8_lossy(key);
        let quoted = serde_json::to_string(s.as_ref()).unwrap_or_else(|_| "\"\"".into());
        out.push_str(&quoted);
    }
}

fn unique_tmp(base: &std::path::Path) -> PathBuf {
    let mut rand = [0u8; 6];
    let _ = getrandom::getrandom(&mut rand);
    let hex: String = rand.iter().map(|b| format!("{:02x}", b)).collect();
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut s = base.as_os_str().to_os_string();
    s.push(format!(".{}.{}.{}.tmp", std::process::id(), ms, hex));
    PathBuf::from(s)
}

fn write_file_atomic(
    tmp: &std::path::Path,
    bytes: &[u8],
    target: &std::path::Path,
    crash_before_rename: Option<&str>,
) -> Result<()> {
    let write = |p: &std::path::Path| -> Result<()> {
        let mut f = File::create(p).map_err(|e| SpectreError::io(e, "Failed to write snapshot"))?;
        f.write_all(bytes)
            .map_err(|e| SpectreError::io(e, "Failed to write snapshot"))?;
        f.sync_all()
            .map_err(|e| SpectreError::io(e, "Failed to sync snapshot"))?;
        Ok(())
    };
    match write(tmp) {
        Ok(()) => {
            crash_point_named("snap_tmp_written");
            if let Some(name) = crash_before_rename {
                crash_point_named(name);
            }
            std::fs::rename(tmp, target)
                .map_err(|e| SpectreError::io(e, "Failed to rename snapshot"))?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(tmp);
            Err(e)
        }
    }
}

fn crash_point_named(name: &str) {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    static CFG: OnceLock<Option<(String, u64)>> = OnceLock::new();
    static HITS: AtomicU64 = AtomicU64::new(0);
    let cfg = CFG.get_or_init(|| {
        std::env::var("SPECTRE_CRASH_AT").ok().map(|v| {
            match v.split_once(':') {
                Some((n, skip)) => (n.to_string(), skip.parse().unwrap_or(0)),
                None => (v, 0),
            }
        })
    });
    if let Some((target, skip)) = cfg {
        if target == name {
            let n = HITS.fetch_add(1, Ordering::SeqCst);
            if n >= *skip {
                std::process::abort();
            }
        }
    }
}
