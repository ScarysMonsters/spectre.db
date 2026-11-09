use crate::crypto;
use crate::error::{codes, Result, SpectreError};
use crate::formats::{self, Compression, Manifest, ManifestEntry, Op, WalReplay};
use crate::index::SecondaryIndex;
use crate::json_compat;
use crate::lock::FileLock;
use crate::pathnorm::DbPaths;
use crate::store::Store;
use crate::validator;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
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
    pub wal_checkpoint_bytes: u64,
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
            wal_checkpoint_bytes: 64 * 1024 * 1024,
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


    snapshot_bytes: u64,
    checkpointing: bool,
    ckpt_pending: bool,
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
            snapshot_bytes: 0,
            checkpointing: false,
            ckpt_pending: false,
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
        } else {
            let backup = self.paths.backup_path(1, self.mode == FormatMode::V2);
            if backup.exists() {
                self.push_warn("Snapshot missing, restoring from backup".into());
                self.restore_from_backup()?;
            }
        }
        self.snapshot_bytes = std::fs::metadata(&snap_path).map(|m| m.len()).unwrap_or(0);

        for seg in &self.manifest.segments {
            let p = self.paths.dir.join(&seg.name);
            if let Ok(md) = std::fs::metadata(&p) {
                self.snapshot_bytes += md.len();
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
                        WalReplay { ops, warns, header: None, last_lsn: 0, valid_len: 0 }
                    }
                };
                if let Some(h) = &replay.header {
                    self.wal_gen = h.generation;
                    self.base_lsn = h.base_lsn;
                }
                formats::apply_ops(&mut self.store, &replay.ops);
                self.last_lsn = replay.last_lsn.max(self.base_lsn);
                self.wal_ops = replay.ops.len();
                if replay.ops.len() > 0 {
                    self.recovery_count += 1;
                }
                if replay.warns > 0 {
                    self.push_warn(format!("{} invalid WAL entries skipped", replay.warns));
                }
                if self.mode == FormatMode::V2 && replay.valid_len < bytes.len() {
                    let corrupt_path = wal_path.with_extension("spwal.corrupt");
                    let _ = std::fs::copy(&wal_path, &corrupt_path);
                    let file = OpenOptions::new()
                        .write(true)
                        .open(&wal_path)
                        .map_err(|e| SpectreError::write_failed(format!("Failed to open WAL for truncation: {}", e)))?;
                    file.set_len(replay.valid_len as u64)
                        .map_err(|e| SpectreError::write_failed(format!("Failed to truncate WAL: {}", e)))?;
                    if self.opts.durability == Durability::Durable {
                        let _ = file.sync_all();
                    }
                    drop(file);
                    sync_dir(&wal_path);
                    self.push_warn(format!(
                        "Truncated invalid WAL tail: kept {} of {} bytes (copy at {})",
                        replay.valid_len,
                        bytes.len(),
                        corrupt_path.display()
                    ));
                }
            }
        }


        if self.mode == FormatMode::V2 {
            let paths: Vec<String> = self.manifest.indexes.clone();
            for p in paths {
                let idx = SecondaryIndex::build(
                    &p,
                    self.store
                        .iter()
                        .filter(|(k, _)| !crate::store::is_raw_key(k)),
                );
                self.indexes.insert(p, idx);
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
            sync_dir(&wal_path);
        }
        Ok(())
    }


    pub fn drain_init_events(&mut self) -> Vec<InitEvent> {
        std::mem::take(&mut self.init_events)
    }


    pub fn get(&mut self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.get_cow(key)?.map(|value| value.into_owned()))
    }

    pub fn get_cow<'a>(&'a mut self, key: &str) -> Result<Option<Cow<'a, [u8]>>> {
        self.check_open()?;
        let hit = self
            .store
            .get_ref(key.as_bytes())
            .map(|bytes| (bytes.as_ptr(), bytes.len()));
        if let Some((pointer, length)) = hit {
            self.cache_hits += 1;
            // SAFETY: the store is not mutated after this lookup, so the value pointer
            // stays valid for the lifetime of the returned borrow.
            let bytes: &'a [u8] = unsafe { std::slice::from_raw_parts(pointer, length) };
            if let Some(k) = &self.enc_key {
                if crypto::looks_encrypted(bytes) {
                    return Ok(Some(Cow::Owned(crypto::decrypt_value(bytes, k)?)));
                }
            }
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        validator::validate_key_fast(key)?;
        match self.store.get(key.as_bytes()) {
            Some(bytes) => {
                self.cache_hits += 1;
                Ok(Some(Cow::Owned(bytes)))
            }
            None => {
                self.cache_misses += 1;
                Ok(None)
            }
        }
    }

    pub fn has(&self, key: &str) -> Result<bool> {
        self.check_open()?;
        if self.store.contains(key.as_bytes()) {
            return Ok(true);
        }
        validator::validate_key_fast(key)?;
        Ok(false)
    }

    pub fn get_many_json(&mut self, keys: &[String]) -> Result<String> {
        self.check_open()?;
        let mut out = String::with_capacity(keys.len() * 32 + 2);
        out.push('[');
        for (index, key) in keys.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            match self.get_cow(key)? {
                Some(bytes) => {
                    let text = std::str::from_utf8(bytes.as_ref()).map_err(|_| {
                        SpectreError::new(codes::OPERATION_FAILED, "Stored value is not valid UTF-8")
                    })?;
                    out.push_str(text);
                }
                None => out.push_str("null"),
            }
        }
        out.push(']');
        Ok(out)
    }

    pub fn set(&mut self, key: &str, value_json: &[u8]) -> Result<()> {
        self.check_open()?;
        self.checkpoint_gate()?;
        validator::validate_key_fast(key)?;
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

        let removed_descendants = if self.indexes.is_empty() {
            Vec::new()
        } else {
            self.store.descendant_keys(key.as_bytes())
        };
        self.wal_append_set(key, &stored)?;
        self.store.set_slice(key.as_bytes(), &stored);
        self.dirty.insert(key.as_bytes().to_vec());
        for descendant in &removed_descendants {
            self.index_remove_key(descendant);
        }
        self.index_touch(key.as_bytes());
        Ok(())
    }

    pub fn delete(&mut self, key: &str) -> Result<bool> {
        self.check_open()?;
        self.checkpoint_gate()?;
        validator::validate_key_fast(key)?;
        let removed_descendants = if self.indexes.is_empty() {
            Vec::new()
        } else {
            self.store.descendant_keys(key.as_bytes())
        };
        self.wal_append_del(key)?;
        if self.store.delete(key.as_bytes()) {
            self.dirty.insert(key.as_bytes().to_vec());
            for descendant in &removed_descendants {
                self.index_remove_key(descendant);
            }
            self.index_touch(key.as_bytes());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn delete_many(&mut self, keys: &[String]) -> Result<Vec<bool>> {
        self.check_open()?;
        self.checkpoint_gate()?;
        let ops = keys
            .iter()
            .map(|key| Op::Del { key: key.as_bytes().to_vec() })
            .collect();
        self.batch(ops)
    }

    pub fn clear(&mut self) -> Result<()> {
        self.check_open()?;
        self.checkpoint_gate()?;
        self.wal_append(&Op::Clear)?;
        self.store.clear();
        self.dirty.clear();
        self.dirty_all = true;
        for idx in self.indexes.values_mut() {
            idx.clear();
        }
        Ok(())
    }


    fn encrypt_batch_ops(&self, ops: Vec<Op>) -> Result<Vec<Op>> {
        let Some(enc_key) = &self.enc_key else {
            return Ok(ops);
        };
        let mut out = Vec::with_capacity(ops.len());
        for op in ops {
            match op {
                Op::Set { key, value } => {
                    let text = std::str::from_utf8(&key)
                        .map_err(|_| SpectreError::invalid_key("Non-UTF-8 key in batch"))?;
                    let value = if validator::is_sensitive_key(text) && value.as_ref() != b"null" {
                        crypto::encrypt_value(&value, enc_key)?.into_boxed_slice()
                    } else {
                        value
                    };
                    out.push(Op::Set { key, value });
                }
                Op::Batch(sub) => out.push(Op::Batch(self.encrypt_batch_ops(sub)?)),
                other => out.push(other),
            }
        }
        Ok(out)
    }

    pub fn batch(&mut self, ops: Vec<Op>) -> Result<Vec<bool>> {
        self.check_open()?;
        self.checkpoint_gate()?;
        validate_batch_ops(&ops)?;
        let ops = self.encrypt_batch_ops(ops)?;
        self.wal_append_batch(&ops)?;
        let mut flags = Vec::with_capacity(ops.len());
        let track_indexes = !self.indexes.is_empty();
        let mut touched: BTreeSet<Vec<u8>> = BTreeSet::new();
        let mut removed: BTreeSet<Vec<u8>> = BTreeSet::new();
        for op in ops {
            match op {
                Op::Set { key, value } => {
                    flags.push(true);
                    if track_indexes {
                        removed.extend(self.store.descendant_keys(&key));
                    }
                    self.store.set(&key, value);
                    self.dirty.insert(key.clone());
                    if track_indexes {
                        touched.insert(key);
                    }
                }
                Op::Del { key } => {
                    if track_indexes {
                        removed.insert(key.clone());
                        removed.extend(self.store.descendant_keys(&key));
                    }
                    let (exact, _) = self.store.delete_with_exact(&key);
                    flags.push(exact);
                    self.dirty.insert(key.clone());
                    if track_indexes {
                        touched.insert(key);
                    }
                }
                Op::Clear => {
                    flags.push(false);
                    self.store.clear();
                    self.dirty.clear();
                    self.dirty_all = true;
                    for idx in self.indexes.values_mut() {
                        idx.clear();
                    }
                }
                Op::Batch(sub) => {
                    flags.push(false);
                    for s in sub {
                        match s {
                            Op::Set { key, value } => {
                                if track_indexes {
                                    removed.extend(self.store.descendant_keys(&key));
                                }
                                self.store.set(&key, value);
                                self.dirty.insert(key.clone());
                                if track_indexes {
                                    touched.insert(key);
                                }
                            }
                            Op::Del { key } => {
                                if track_indexes {
                                    removed.insert(key.clone());
                                    removed.extend(self.store.descendant_keys(&key));
                                }
                                self.store.delete(&key);
                                self.dirty.insert(key.clone());
                                if track_indexes {
                                    touched.insert(key);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        if track_indexes && !self.indexes.is_empty() {
            for key in &removed {
                self.index_remove_key(key);
            }
            let mut leaves: BTreeSet<Arc<[u8]>> = BTreeSet::new();
            let mut missing: Vec<Vec<u8>> = Vec::new();
            for key in &touched {
                match self.resolve_index_leaf(key) {
                    Some(leaf) => {
                        leaves.insert(leaf);
                    }
                    None => missing.push(key.clone()),
                }
            }
            for key in &missing {
                self.index_remove_key(key);
            }
            for leaf in &leaves {
                self.index_update_leaf(leaf);
            }
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

    fn wal_append_set(&mut self, key: &str, value: &[u8]) -> Result<()> {
        if self.mode != FormatMode::V2 {
            return self.wal_append(&Op::Set {
                key: key.as_bytes().to_vec(),
                value: value.to_vec().into_boxed_slice(),
            });
        }
        self.wal_buf.reserve(key.len() + value.len() + 32);
        let start = self.wal_buf.len();
        let lsn = self.last_lsn + 1;
        let payload_len = 4 + key.len() + value.len();
        self.wal_buf.push(formats::OP_SET);
        self.wal_buf.extend_from_slice(&(payload_len as u32).to_le_bytes());
        self.wal_buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
        self.wal_buf.extend_from_slice(key.as_bytes());
        self.wal_buf.extend_from_slice(value);
        formats::frame3_tail(&mut self.wal_buf, start, lsn);
        self.last_lsn = lsn;
        self.wal_ops += 1;
        self.wal_flush_gate()
    }

    fn wal_append_del(&mut self, key: &str) -> Result<()> {
        if self.mode != FormatMode::V2 {
            return self.wal_append(&Op::Del { key: key.as_bytes().to_vec() });
        }
        self.wal_buf.reserve(key.len() + 24);
        let start = self.wal_buf.len();
        let lsn = self.last_lsn + 1;
        self.wal_buf.push(formats::OP_DEL);
        self.wal_buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
        self.wal_buf.extend_from_slice(key.as_bytes());
        formats::frame3_tail(&mut self.wal_buf, start, lsn);
        self.last_lsn = lsn;
        self.wal_ops += 1;
        self.wal_flush_gate()
    }

    fn wal_append(&mut self, op: &Op) -> Result<()> {
        if self.mode == FormatMode::V2 {
            let reserve = match op {
                Op::Set { key, value } => key.len() + value.len() + 32,
                Op::Del { key } => key.len() + 24,
                Op::Clear => 16,
                Op::Batch(_) => 16,
            };
            self.wal_buf.reserve(reserve);
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
        let mut reserve = 32;
        for op in ops {
            reserve += match op {
                Op::Set { key, value } => key.len() + value.len() + 32,
                Op::Del { key } => key.len() + 24,
                Op::Clear => 16,
                Op::Batch(sub) => sub.len() * 64,
            };
        }
        self.wal_buf.reserve(reserve);
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
        self.auto_checkpoint()
    }

    fn auto_checkpoint(&mut self) -> Result<()> {
        if self.opts.wal_checkpoint_bytes == 0 || self.checkpointing {
            return Ok(());
        }
        let floor = self.opts.wal_checkpoint_bytes.max(self.snapshot_bytes / 4);
        if self.wal_bytes >= floor {
            // Arm only: firing here would truncate the WAL while the op that
            // just appended is not yet applied to the store AND its frame is
            // about to be wiped. The trigger takes effect at the next mutative
            // op boundary (checkpoint_gate), where the entire store is already
            // durable in the WAL and the snapshot will legitimately contain it.
            self.ckpt_pending = true;
        }
        Ok(())
    }

    fn checkpoint_gate(&mut self) -> Result<()> {
        if self.ckpt_pending && !self.checkpointing {
            self.ckpt_pending = false;
            self.checkpoint_full()?;
        }
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
            self.checkpoint_full()?;
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


    /// Explicit full checkpoint: dump the whole store into the main snapshot
    /// file, retire segmented files, and truncate the WAL. Cost is one
    /// snapshot write; the store is already in sync with the WAL by
    /// construction (WAL-first ordering), so truncating afterwards is safe.
    fn checkpoint_full(&mut self) -> Result<()> {
        self.checkpointing = true;
        let res = self.checkpoint_full_inner();
        self.checkpointing = false;
        res
    }

    fn checkpoint_full_inner(&mut self) -> Result<()> {
        self.wal_flush()?;
        self.compact_legacy()?;
        if self.mode == FormatMode::V2 {
            for seg in std::mem::take(&mut self.manifest.segments) {
                let _ = std::fs::remove_file(self.paths.dir.join(&seg.name));
            }
            self.manifest.generation += 1;
            self.manifest.indexes = self.indexes.keys().cloned().collect();
            self.write_manifest()?;
        }
        Ok(())
    }

    /// Force a full checkpoint (snapshot write + WAL truncation).
    pub fn checkpoint(&mut self) -> Result<()> {
        self.check_open()?;
        self.checkpoint_full()?;
        self.last_compaction_ms = Some(unix_ms());
        Ok(())
    }

    /// Run an automatic checkpoint if the WAL has outgrown the configured
    /// budget (max of `wal_checkpoint_bytes` and a quarter of the last
    /// snapshot size). Returns whether a checkpoint was performed.
    pub fn maybe_checkpoint(&mut self) -> Result<bool> {
        self.check_open()?;
        if self.opts.wal_checkpoint_bytes == 0 || self.checkpointing {
            return Ok(false);
        }
        let floor = self.opts.wal_checkpoint_bytes.max(self.snapshot_bytes / 4);
        if self.wal_bytes < floor {
            return Ok(false);
        }
        self.checkpoint_full()?;
        self.last_compaction_ms = Some(unix_ms());
        Ok(true)
    }

    // Legacy single-file compaction: rewrite the whole store into the main
    // snapshot file, rotate `.bak*` copies, then truncate the WAL (all its
    // content ships inside the new snapshot by construction).
    fn compact_legacy(&mut self) -> Result<()> {
        let bytes = match self.mode {
            FormatMode::V2 => formats::write_snapshot_ex(&self.store, self.opts.compression, self.snap_key()),
            FormatMode::Json => json_compat::write_json_snapshot(&self.store, self.opts.compress)?,
        };
        let snap_path = match self.mode {
            FormatMode::V2 => self.paths.spdb_path(),
            FormatMode::Json => self.paths.snapshot_path(),
        };
        let tmp = unique_tmp(&snap_path);
        let write_tmp = |path: &std::path::Path| -> Result<()> {
            let mut file = File::create(path)
                .map_err(|e| SpectreError::io(e, "Failed to write snapshot"))?;
            file.write_all(&bytes)
                .map_err(|e| SpectreError::io(e, "Failed to write snapshot"))?;
            file.sync_all()
                .map_err(|e| SpectreError::io(e, "Failed to sync snapshot"))?;
            Ok(())
        };
        if let Err(error) = write_tmp(&tmp) {
            let _ = std::fs::remove_file(&tmp);
            return Err(error);
        }
        self.crash_point("snap_tmp_written");
        self.rotate_backups()?;
        self.crash_point("snap_before_rename");
        std::fs::rename(&tmp, &snap_path)
            .map_err(|e| SpectreError::io(e, "Failed to rename snapshot"))?;
        sync_dir(&snap_path);
        self.snapshot_bytes = bytes.len() as u64;
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
        if self.opts.durability == Durability::Durable {
            if let Some(file) = self.wal_file.as_mut() {
                let _ = file.sync_all();
            }
        }
        sync_dir(&wal_path);
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
            for (k, v) in self.store.iter_arc() {
                scratch.raw_insert_arc(k, Arc::clone(v));
            }
            self.dirty.clear();
            self.dirty_all = false;
            crate::segments::CompactJob::full(scratch, compression, enc_key, generation, base)
        } else {
            let mut scratch = Store::new();
            let dirty_keys: Vec<Vec<u8>> = self.dirty.iter().cloned().collect();
            for k in &dirty_keys {
                if let Some(v) = self.store.get_shared(k) {
                    scratch.raw_insert_arc(k, v);
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
                std::fs::copy(&src, &dst)
                    .map_err(|e| SpectreError::io(e, "Failed to rotate backup"))?;
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
        self.checkpoint_gate()?;
        validator::validate_key_fast(key)?;
        validator::validate_value_size(bytes.len())?;
        self.wal_append(&Op::Set {
            key: [b"\x00".as_slice(), key.as_bytes()].concat(),
            value: bytes.to_vec().into_boxed_slice(),
        })?;
        self.store.set_raw(key.as_bytes(), bytes.to_vec().into_boxed_slice());
        self.dirty.insert(key.as_bytes().to_vec());
        Ok(())
    }

    pub fn get_raw(&mut self, key: &str) -> Result<Option<Vec<u8>>> {
        self.check_open()?;
        if let Some(v) = self.store.get_raw(key.as_bytes()) {
            self.cache_hits += 1;
            return Ok(Some(v));
        }
        validator::validate_key_fast(key)?;
        self.cache_misses += 1;
        Ok(None)
    }

    pub fn delete_raw(&mut self, key: &str) -> Result<bool> {
        self.check_open()?;
        self.checkpoint_gate()?;
        validator::validate_key_fast(key)?;
        self.wal_append(&Op::Del {
            key: [b"\x00".as_slice(), key.as_bytes()].concat(),
        })?;
        if self.store.delete_raw(key.as_bytes()) {
            self.dirty.insert(key.as_bytes().to_vec());
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

    pub fn scan_json_page(
        &mut self,
        prefix: Option<&str>,
        after: &str,
        limit: usize,
    ) -> Result<String> {
        self.check_open()?;
        let (rows, cursor) = self.store.page_ref(
            prefix.map(|p| p.as_bytes()),
            after.as_bytes(),
            limit.max(1),
        );
        let mut capacity = 32;
        for &(key, value) in &rows {
            capacity += key.len() + value.len() + 16;
        }
        let mut out = String::with_capacity(capacity);
        out.push_str("{\"rows\":[");
        for (index, &(key, value)) in rows.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push('[');
            push_json_string(&mut out, key);
            out.push(',');
            let decrypted;
            let bytes: &[u8] = match &self.enc_key {
                Some(enc_key) if crypto::looks_encrypted(value) => {
                    decrypted = crypto::decrypt_value(value, enc_key)?;
                    &decrypted
                }
                _ => value,
            };
            match std::str::from_utf8(bytes) {
                Ok(text) => out.push_str(text),
                Err(_) => out.push_str(&String::from_utf8_lossy(bytes)),
            }
            out.push(']');
        }
        out.push_str("],\"cursor\":");
        if cursor.is_empty() {
            out.push_str("null");
        } else {
            push_json_string(&mut out, &cursor);
        }
        out.push('}');
        Ok(out)
    }


    pub fn create_index(&mut self, path: &str) -> Result<u64> {
        self.check_open()?;
        if path.is_empty() {
            return Err(SpectreError::invalid_key("Index path is empty"));
        }
        if !self.indexes.contains_key(path) {
            let idx = SecondaryIndex::build(
                path,
                self.store
                    .iter()
                    .filter(|(k, _)| !crate::store::is_raw_key(k)),
            );
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


    fn resolve_index_leaf(&self, key: &[u8]) -> Option<Arc<[u8]>> {
        if let Some(leaf) = self.store.get_key_arc(key) {
            return Some(leaf);
        }
        for i in 0..key.len() {
            if key[i] == b'.' {
                if let Some(leaf) = self.store.get_key_arc(&key[..i]) {
                    return Some(leaf);
                }
            }
        }
        None
    }

    fn index_remove_key(&mut self, key: &[u8]) {
        for idx in self.indexes.values_mut() {
            idx.remove_key(key);
        }
    }

    fn index_update_leaf(&mut self, leaf: &Arc<[u8]>) {
        let parsed: Option<serde_json::Value> = match self.store.get_ref(leaf) {
            Some(bytes) => {
                if let Some(enc_key) = &self.enc_key {
                    if crypto::looks_encrypted(bytes) {
                        match crypto::decrypt_value(bytes, enc_key) {
                            Ok(value) => serde_json::from_slice(&value).ok(),
                            Err(_) => None,
                        }
                    } else {
                        serde_json::from_slice(bytes).ok()
                    }
                } else {
                    serde_json::from_slice(bytes).ok()
                }
            }
            None => None,
        };
        for idx in self.indexes.values_mut() {
            idx.insert_parsed_key(leaf, parsed.as_ref());
        }
    }

    fn index_touch(&mut self, key: &[u8]) {
        if self.indexes.is_empty() {
            return;
        }
        match self.resolve_index_leaf(key) {
            Some(leaf) => self.index_update_leaf(&leaf),
            None => self.index_remove_key(key),
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
            sync_dir(target);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(tmp);
            Err(e)
        }
    }
}

#[cfg(unix)]
fn sync_dir(path: &std::path::Path) {
    if let Some(dir) = path.parent() {
        if let Ok(file) = File::open(dir) {
            let _ = file.sync_all();
        }
    }
}

#[cfg(not(unix))]
fn sync_dir(_path: &std::path::Path) {}

fn validate_batch_ops(ops: &[Op]) -> Result<()> {
    for op in ops {
        match op {
            Op::Set { key, value } => {
                let text = std::str::from_utf8(key)
                    .map_err(|_| SpectreError::invalid_key("Non-UTF-8 key in batch"))?;
                validator::validate_key_fast(text)?;
                validator::validate_value_size(value.len())?;
            }
            Op::Del { key } => {
                let text = std::str::from_utf8(key)
                    .map_err(|_| SpectreError::invalid_key("Non-UTF-8 key in batch"))?;
                validator::validate_key_fast(text)?;
            }
            Op::Batch(sub) => validate_batch_ops(sub)?,
            Op::Clear => {}
        }
    }
    Ok(())
}

fn push_json_string(out: &mut String, bytes: &[u8]) {
    out.push('"');
    let text = String::from_utf8_lossy(bytes);
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        let escaped = match ch {
            '"' => "\\\"",
            '\\' => "\\\\",
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' => "\\t",
            '\u{08}' => "\\b",
            '\u{0C}' => "\\f",
            c if (c as u32) < 0x20 => "",
            _ => continue,
        };
        out.push_str(&text[start..index]);
        if escaped.is_empty() {
            use std::fmt::Write;
            let _ = write!(out, "\\u{:04x}", ch as u32);
        } else {
            out.push_str(escaped);
        }
        start = index + ch.len_utf8();
    }
    out.push_str(&text[start..]);
    out.push('"');
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
