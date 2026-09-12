use napi::{bindgen_prelude::Buffer, Env, JsString};
use napi_derive::napi;
use spectre_db_core as core;
use std::sync::{Arc, Mutex, MutexGuard};

#[napi(object)]
pub struct EngineOptions {
    pub compress: Option<bool>,

    pub compression: Option<String>,
    pub encryption_key: Option<String>,
    pub encrypt_backups: Option<bool>,

    pub encrypt_snapshot: Option<bool>,

    pub scrypt_log_n: Option<u32>,
    pub scrypt_r: Option<u32>,
    pub scrypt_p: Option<u32>,
    pub backup_count: Option<u32>,

    pub format: Option<String>,
    pub lock_timeout: Option<u32>,
    pub wal_buffering: Option<bool>,

    pub sync_wal: Option<bool>,

    pub durability: Option<String>,

    pub compact_mode: Option<String>,

    pub legacy_threshold: Option<f64>,

    pub segment_merge_every: Option<u32>,
}

#[napi(object)]
pub struct InitEvent {
    pub event: String,

    pub payload: String,
}

#[napi(object)]
pub struct BatchOp {

    pub r#type: String,
    pub key: String,

    pub json: Option<String>,
}

#[napi(object)]
pub struct StatsObject {
    pub driver: String,
    pub engine: String,
    pub format: String,
    pub compress: bool,
    pub compression: String,
    pub encrypted: bool,
    pub snapshot_encrypted: bool,
    pub durability: String,
    pub entries: f64,
    pub raw_entries: f64,
    pub store_bytes: f64,
    pub file_size: f64,
    pub wal_ops: f64,
    pub wal_bytes: f64,
    pub snapshot_path: String,
    pub wal_path: String,
    pub backup_count: f64,

    pub pending_writes: f64,
    pub cache_hits: f64,
    pub cache_misses: f64,
    pub compression_ratio: f64,
    pub segment_count: f64,
    pub generation: f64,
    pub last_compaction_ms: Option<f64>,
    pub last_lsn: f64,
    pub recovery_count: f64,
    pub index_count: f64,
}

#[napi]
pub struct SpectreEngine {
    inner: Arc<Mutex<core::Engine>>,
}

fn lock(inner: &Mutex<core::Engine>) -> MutexGuard<'_, core::Engine> {
    inner.lock().unwrap_or_else(|p| p.into_inner())
}

fn to_core_options(o: Option<&EngineOptions>) -> core::EngineOptions {
    let d = core::EngineOptions::default();
    let Some(o) = o else { return d };
    core::EngineOptions {
        compress: o.compress.unwrap_or(false),
        compression: match o.compression.as_deref() {
            Some("gzip") => core::formats::Compression::Gzip,
            Some("zstd") => core::formats::Compression::Zstd,
            _ => core::formats::Compression::None,
        },
        encryption_key: o.encryption_key.as_ref().map(|s| s.as_bytes().to_vec()),
        encrypt_backups: o.encrypt_backups.unwrap_or(false),
        encrypt_snapshot: o.encrypt_snapshot.unwrap_or(false),
        scrypt_log_n: o.scrypt_log_n.map(|v| v as u8).unwrap_or(14),
        scrypt_r: o.scrypt_r.unwrap_or(8),
        scrypt_p: o.scrypt_p.unwrap_or(1),
        backup_count: o.backup_count.unwrap_or(3),
        format: o.format.clone().unwrap_or_else(|| "auto".to_string()),
        lock_timeout_ms: o.lock_timeout.map(|v| v as u64).unwrap_or(core::lock::DEFAULT_LOCK_TIMEOUT_MS),
        wal_buffering: o.wal_buffering.unwrap_or(false),
        sync_wal: o.sync_wal.unwrap_or(false),
        durability: match o.durability.as_deref() {
            Some("durable") => core::Durability::Durable,
            _ => core::Durability::Process,
        },
        compact_mode: match o.compact_mode.as_deref() {
            Some("legacy") => core::CompactMode::Legacy,
            Some("segments") => core::CompactMode::Segments,
            _ => core::CompactMode::Auto,
        },
        legacy_threshold: o.legacy_threshold.map(|v| v as u64).unwrap_or(100_000),
        segment_merge_every: o.segment_merge_every.unwrap_or(8),
    }
}

fn to_napi_err(e: core::SpectreError) -> napi::Error {

    napi::Error::new(napi::Status::GenericFailure, format!("{}|{}", e.code, e.message))
}

#[napi]
impl SpectreEngine {
    #[napi(constructor)]
    pub fn new(path: String, options: Option<EngineOptions>) -> napi::Result<Self> {
        let core_opts = to_core_options(options.as_ref());
        let engine = core::Engine::open(std::path::Path::new(&path), core_opts)
            .map_err(to_napi_err)?;
        Ok(Self { inner: Arc::new(Mutex::new(engine)) })
    }


    #[napi]
    pub fn get_json(&self, env: Env, key: String) -> napi::Result<Option<JsString>> {
        match lock(&self.inner).get(&key).map_err(to_napi_err)? {
            Some(bytes) => {
                let text = std::str::from_utf8(&bytes)
                    .map(|s| s.to_owned())
                    .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
                Ok(Some(env.create_string_from_std(text)?))
            }
            None => Ok(None),
        }
    }

    #[napi]
    pub fn has(&self, key: String) -> napi::Result<bool> {
        lock(&self.inner).has(&key).map_err(to_napi_err)
    }


    #[napi]
    pub fn set_json(&self, key: String, json: String) -> napi::Result<()> {
        lock(&self.inner).set(&key, json.as_bytes()).map_err(to_napi_err)
    }

    #[napi]
    pub fn del(&self, key: String) -> napi::Result<bool> {
        lock(&self.inner).delete(&key).map_err(to_napi_err)
    }

    #[napi]
    pub fn clear(&self) -> napi::Result<()> {
        lock(&self.inner).clear().map_err(to_napi_err)
    }


    #[napi]
    pub fn batch(&self, ops: Vec<BatchOp>) -> napi::Result<Vec<bool>> {
        let core_ops: Vec<core::Op> = ops
            .into_iter()
            .map(|op| match op.r#type.as_str() {
                "set" | "add" | "sub" | "push" | "pull" => {
                    let json = op.json.unwrap_or_else(|| "null".to_string());
                    core::Op::Set { key: op.key.into_bytes(), value: json.into_bytes().into_boxed_slice() }
                }
                "delete" => core::Op::Del { key: op.key.into_bytes() },
                other => core::Op::Set {
                    key: op.key.into_bytes(),
                    value: format!("\"unsupported:{}\"", other).into_bytes().into_boxed_slice(),
                },
            })
            .collect();
        lock(&self.inner).batch(core_ops).map_err(to_napi_err)
    }


    #[napi]
    pub fn set_many_json(&self, doc: String) -> napi::Result<Vec<bool>> {
        let pairs: Vec<(String, serde_json::Value)> = serde_json::from_str(&doc)
            .map_err(|e| napi::Error::new(napi::Status::GenericFailure, format!("Invalid batch document: {}", e)))?;
        let mut core_ops = Vec::with_capacity(pairs.len());
        for (key, value) in pairs {
            let json = serde_json::to_vec(&value)
                .map_err(|e| napi::Error::new(napi::Status::GenericFailure, format!("Value serialization failed: {}", e)))?;
            core_ops.push(core::Op::Set { key: key.into_bytes(), value: json.into_boxed_slice() });
        }
        lock(&self.inner).batch(core_ops).map_err(to_napi_err)
    }


    #[napi]
    pub fn all_json(&self, prefix: Option<String>) -> napi::Result<String> {
        lock(&self.inner).all_json(prefix.as_deref()).map_err(to_napi_err)
    }


    #[napi]
    pub fn scan_json(
        &self,
        env: Env,
        prefix: Option<String>,
        after: Option<String>,
        limit: Option<u32>,
    ) -> napi::Result<JsString> {
        let (rows, cursor) = lock(&self.inner)
            .scan_page(
                prefix.as_deref(),
                after.as_deref().unwrap_or(""),
                limit.unwrap_or(500) as usize,
            )
            .map_err(to_napi_err)?;
        let mut out = String::with_capacity(64 + rows.len() * 48);
        out.push_str("{\"rows\":[");
        let mut first = true;
        for (k, v) in &rows {
            if first {
                first = false;
            } else {
                out.push(',');
            }

            out.push_str("[");
            out.push_str(&serde_json::to_string(k).unwrap_or_else(|_| "\"\"".into()));
            out.push(',');
            out.push_str(&String::from_utf8_lossy(v));
            out.push(']');
        }
        out.push_str("],\"cursor\":");
        if cursor.is_empty() {
            out.push_str("null");
        } else {
            out.push_str(&serde_json::to_string(&cursor).unwrap_or_else(|_| "\"\"".into()));
        }
        out.push('}');
        env.create_string_from_std(out)
    }

    #[napi]
    pub fn count(&self, prefix: Option<String>) -> napi::Result<f64> {
        lock(&self.inner)
            .count(prefix.as_deref())
            .map(|c| c as f64)
            .map_err(to_napi_err)
    }


    #[napi]
    pub fn get_raw(&self, key: String) -> napi::Result<Option<Buffer>> {
        Ok(lock(&self.inner).get_raw(&key).map_err(to_napi_err)?.map(Buffer::from))
    }

    #[napi]
    pub fn set_raw(&self, key: String, data: Buffer) -> napi::Result<()> {
        lock(&self.inner).set_raw(&key, &data).map_err(to_napi_err)
    }

    #[napi]
    pub fn delete_raw(&self, key: String) -> napi::Result<bool> {
        lock(&self.inner).delete_raw(&key).map_err(to_napi_err)
    }


    #[napi]
    pub fn create_index(&self, path: String) -> napi::Result<f64> {
        lock(&self.inner)
            .create_index(&path)
            .map(|n| n as f64)
            .map_err(to_napi_err)
    }

    #[napi]
    pub fn drop_index(&self, path: String) -> napi::Result<bool> {
        lock(&self.inner).drop_index(&path).map_err(to_napi_err)
    }

    #[napi]
    pub fn index_list(&self) -> Vec<String> {
        lock(&self.inner).index_list()
    }


    #[napi]
    pub fn index_lookup(&self, path: String, value_json: String) -> napi::Result<Vec<String>> {
        lock(&self.inner)
            .index_lookup(&path, value_json.as_bytes())
            .map_err(to_napi_err)
    }


    #[napi]
    pub fn index_range(
        &self,
        path: String,
        gte: Option<String>,
        lt: Option<String>,
        limit: Option<u32>,
    ) -> napi::Result<Vec<String>> {
        lock(&self.inner)
            .index_range(
                &path,
                gte.as_deref().map(str::as_bytes),
                lt.as_deref().map(str::as_bytes),
                limit.unwrap_or(0) as usize,
            )
            .map_err(to_napi_err)
    }


    #[napi(getter)]
    pub fn durability(&self) -> String {
        lock(&self.inner).durability().as_str().to_string()
    }

    #[napi]
    pub fn stats(&self) -> napi::Result<StatsObject> {
        let st = lock(&self.inner).stats().map_err(to_napi_err)?;
        Ok(StatsObject {
            driver: "spectre.db".to_string(),
            engine: "spectre.db/2.0.0 (rust)".to_string(),
            format: st.format,
            compress: st.compress,
            compression: st.compression,
            encrypted: st.encrypted,
            snapshot_encrypted: st.snapshot_encrypted,
            durability: st.durability,
            entries: st.entries as f64,
            raw_entries: st.raw_entries as f64,
            store_bytes: st.store_value_bytes as f64,
            file_size: st.file_size as f64,
            wal_ops: st.wal_ops as f64,
            wal_bytes: st.wal_bytes as f64,
            snapshot_path: st.snapshot_path,
            wal_path: st.wal_path,
            backup_count: 3.0,
            pending_writes: st.pending_writes as f64,
            cache_hits: st.cache_hits as f64,
            cache_misses: st.cache_misses as f64,
            compression_ratio: st.compression_ratio,
            segment_count: st.segment_count as f64,
            generation: st.generation as f64,
            last_compaction_ms: st.last_compaction_ms.map(|v| v as f64),
            last_lsn: st.last_lsn as f64,
            recovery_count: st.recovery_count as f64,
            index_count: st.index_count as f64,
        })
    }

    #[napi]
    pub fn compact(&self) -> napi::Result<()> {
        lock(&self.inner).compact().map_err(to_napi_err)
    }

    #[napi]
    pub fn save(&self) -> napi::Result<()> {
        lock(&self.inner).compact().map_err(to_napi_err)
    }


    #[napi(ts_return_type = "Promise<\"committed\" | \"stale\" | \"legacy\">")]
    pub fn compact_async(&self) -> napi::Result<napi::bindgen_prelude::AsyncTask<CompactTask>> {
        let job = lock(&self.inner).prepare_compact().map_err(to_napi_err)?;
        Ok(napi::bindgen_prelude::AsyncTask::new(CompactTask {
            engine: Arc::clone(&self.inner),
            job: Some(job),
            done: None,
        }))
    }


    #[napi]
    pub fn migrate(&self, format: String) -> napi::Result<()> {
        let mode = match format.as_str() {
            "v2" => core::FormatMode::V2,
            "json" => core::FormatMode::Json,
            other => {
                return Err(napi::Error::new(
                    napi::Status::GenericFailure,
                    format!("Unknown format: {}", other),
                ))
            }
        };
        lock(&self.inner).migrate(mode).map_err(to_napi_err)
    }

    #[napi]
    pub fn close(&self) -> napi::Result<()> {
        lock(&self.inner).close().map_err(to_napi_err)
    }


    #[napi]
    pub fn drain_init_events(&self) -> napi::Result<Vec<InitEvent>> {
        let events = lock(&self.inner).drain_init_events();
        Ok(events
            .into_iter()
            .map(|e| InitEvent { event: e.event, payload: e.payload })
            .collect())
    }


    #[napi(getter)]
    pub fn format(&self) -> String {
        lock(&self.inner).mode().as_str().to_string()
    }
}

pub struct CompactTask {
    engine: Arc<Mutex<core::Engine>>,
    job: Option<core::segments::CompactJob>,
    done: Option<core::segments::CompactJobDone>,
}

impl<'task> napi::ScopedTask<'task> for CompactTask {
    type Output = String;
    type JsValue = JsString<'task>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let job = self
            .job
            .take()
            .ok_or_else(|| napi::Error::new(napi::Status::GenericFailure, "compact task already consumed"))?;
        if job.legacy {

            lock(&self.engine).compact().map_err(to_napi_err)?;
            return Ok("legacy".to_string());
        }
        self.done = Some(job.compute());
        Ok("computed".to_string())
    }

    fn resolve(&mut self, env: &'task Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        let status = if let Some(done) = self.done.take() {
            lock(&self.engine).finish_compact(done).map_err(to_napi_err)?
        } else {
            output.as_str()
        };
        env.create_string(status)
    }
}
