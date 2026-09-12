pub mod crypto;
pub mod engine;
pub mod error;
pub mod formats;
pub mod index;
pub mod json_compat;
pub mod lock;
pub mod pathnorm;
pub mod segments;
pub mod store;
pub mod validator;

pub use engine::{CompactMode, Durability, Engine, EngineOptions, FormatMode, InitEvent, Stats};
pub use error::{Result, SpectreError};
pub use formats::Op;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "spectre-core-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn opts() -> EngineOptions {
        EngineOptions { lock_timeout_ms: 2000, ..Default::default() }
    }

    #[test]
    fn set_get_delete_roundtrip() {
        let dir = tmpdir("basic");
        let path = dir.join("data.json");
        {
            let mut db = Engine::open(&path, opts()).unwrap();
            assert_eq!(db.mode(), FormatMode::V2);
            db.set("user.1", br#"{"name":"alice"}"#).unwrap();
            db.set("user.2", b"42").unwrap();
            assert_eq!(db.get("user.1").unwrap().unwrap(), br#"{"name":"alice"}"#.to_vec());
            assert_eq!(db.get("user.2").unwrap().unwrap(), b"42".to_vec());
            assert_eq!(db.get("user.3").unwrap(), None);
            assert!(db.has("user.1").unwrap());
            assert!(!db.has("user.3").unwrap());
            assert!(db.delete("user.2").unwrap());
            assert!(!db.delete("user.2").unwrap());
            assert_eq!(db.get("user.2").unwrap(), None);
        }

        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(db.get("user.1").unwrap().unwrap(), br#"{"name":"alice"}"#.to_vec());
        assert_eq!(db.get("user.2").unwrap(), None);
        assert_eq!(db.stats().unwrap().entries, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_survives_crash_without_compact() {


        let dir = tmpdir("crash");
        let path = dir.join("data.db");
        {
            let mut db = Engine::open(&path, opts()).unwrap();
            db.set("a", b"1").unwrap();
            db.set("b", b"2").unwrap();
            db.delete("a").unwrap();
            db.clear().unwrap();
            db.set("c", b"3").unwrap();
            std::mem::forget(db);
        }
        let lock_path = dir.join("data.lock");
        std::fs::write(&lock_path, "999999999\n").unwrap();
        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(db.get("c").unwrap().unwrap(), b"3");
        assert_eq!(db.get("a").unwrap(), None);
        assert_eq!(db.get("b").unwrap(), None);
        assert_eq!(db.stats().unwrap().entries, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compact_resets_wal() {
        let dir = tmpdir("compact");
        let path = dir.join("data.db");
        let mut db = Engine::open(&path, opts()).unwrap();
        for i in 0..100 {
            db.set(&format!("k{}", i), b"1").unwrap();
        }
        assert_eq!(db.stats().unwrap().wal_ops, 100);
        db.compact().unwrap();
        let st = db.stats().unwrap();
        assert_eq!(st.wal_ops, 0);
        assert_eq!(st.entries, 100);
        assert!(st.file_size > 0);
        drop(db);
        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(db.stats().unwrap().entries, 100);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_mode_compat_and_detection() {
        let dir = tmpdir("jsonmode");
        let path = dir.join("legacy.json");

        let o = EngineOptions { format: "json".into(), ..opts() };
        {
            let mut db = Engine::open(&path, o.clone()).unwrap();
            assert_eq!(db.mode(), FormatMode::Json);
            db.set("a.b", br#"{"x":1}"#).unwrap();
            db.set("a.c", b"true").unwrap();
            db.close().unwrap();
        }

        let raw = std::fs::read(dir.join("legacy.snapshot")).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["a"]["b"]["x"], 1);
        assert_eq!(v["a"]["c"], true);


        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(db.mode(), FormatMode::Json);
        assert_eq!(db.get("a.b").unwrap().unwrap(), br#"{"x":1}"#.to_vec());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_json_to_v2_and_back() {
        let dir = tmpdir("migrate");
        let path = dir.join("m.json");
        let o = EngineOptions { format: "json".into(), ..opts() };
        let mut db = Engine::open(&path, o).unwrap();
        db.set("x", b"1").unwrap();
        db.set("y.z", b"2").unwrap();
        db.migrate(FormatMode::V2).unwrap();
        assert_eq!(db.mode(), FormatMode::V2);
        assert!(!dir.join("m.snapshot").exists());
        assert!(!dir.join("m.wal").exists());
        assert!(dir.join("m.spdb").exists());
        db.close().unwrap();

        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(db.mode(), FormatMode::V2);
        assert_eq!(db.get("x").unwrap().unwrap(), b"1");
        assert_eq!(db.get("y.z").unwrap().unwrap(), b"2");
        db.close().unwrap();


        let mut db = Engine::open(&path, opts()).unwrap();
        db.migrate(FormatMode::Json).unwrap();
        db.close().unwrap();
        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(db.mode(), FormatMode::Json);
        assert_eq!(db.get("x").unwrap().unwrap(), b"1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn encryption_sensitive_keys() {
        let dir = tmpdir("enc");
        let path = dir.join("enc.json");
        let o = EngineOptions { encryption_key: Some(b"my-secret".to_vec()), ..opts() };
        {
            let mut db = Engine::open(&path, o.clone()).unwrap();
            db.set("password", br#"{"pw":"hunter2"}"#).unwrap();
            db.set("profile.name", br#""alice""#).unwrap();

            assert_eq!(db.get("password").unwrap().unwrap(), br#"{"pw":"hunter2"}"#.to_vec());
        }

        let mut db = Engine::open(&path, o).unwrap();
        db.close().unwrap();
        let raw = std::fs::read(dir.join("enc.spdb")).unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("\"__enc\":1"));
        assert!(!text.contains("hunter2"));

        let mut db = Engine::open(&path, opts()).unwrap();
        let raw_val = db.get("password").unwrap().unwrap();
        assert!(String::from_utf8_lossy(&raw_val).contains("__enc"));
        db.close().unwrap();

        let o = EngineOptions { encryption_key: Some(b"my-secret".to_vec()), ..opts() };
        let mut db = Engine::open(&path, o).unwrap();
        let all = db.all(None).unwrap();
        let pw = all.iter().find(|(k, _)| k == "password").unwrap();
        assert_eq!(pw.1, br#"{"pw":"hunter2"}"#.to_vec());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lock_blocks_second_process_semantics() {
        let dir = tmpdir("lock");
        let path = dir.join("l.json");
        let _db = Engine::open(&path, opts()).unwrap();

        let err = Engine::open(&path, opts()).unwrap_err();
        assert_eq!(err.code, error::codes::LOCK_TIMEOUT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_events_drained() {
        let dir = tmpdir("events");
        let path = dir.join("e.json");
        let mut db = Engine::open(&path, opts()).unwrap();
        let events = db.drain_init_events();
        assert!(events.is_empty());
        db.set("a", b"1").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch_atomic_record() {
        let dir = tmpdir("batch");
        let path = dir.join("b.db");
        let mut db = Engine::open(&path, opts()).unwrap();
        db.batch(vec![
            Op::Set { key: b"x".to_vec(), value: b"1".to_vec().into_boxed_slice() },
            Op::Set { key: b"y".to_vec(), value: b"2".to_vec().into_boxed_slice() },
            Op::Del { key: b"z".to_vec() },
        ])
        .unwrap();
        assert_eq!(db.stats().unwrap().wal_ops, 1);
        assert_eq!(db.get("x").unwrap().unwrap(), b"1");
        drop(db);
        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(db.get("x").unwrap().unwrap(), b"1");
        assert_eq!(db.get("y").unwrap().unwrap(), b"2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn all_with_prefix_and_scan() {
        let dir = tmpdir("scan");
        let path = dir.join("s.db");
        let mut db = Engine::open(&path, opts()).unwrap();
        for i in 0..10 {
            db.set(&format!("user.{}", i), format!("{{\"i\":{}}}", i).as_bytes()).unwrap();
            db.set(&format!("post.{}", i), b"1").unwrap();
        }
        let users = db.all(Some("user")).unwrap();
        assert_eq!(users.len(), 10);
        let all = db.all(None).unwrap();
        assert_eq!(all.len(), 20);
        assert_eq!(db.count(Some("user")).unwrap(), 10);
        assert_eq!(db.count(None).unwrap(), 20);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_wal_interop_with_v11_layout() {

        let dir = tmpdir("interop");
        let path = dir.join("v11.json");
        std::fs::write(
            dir.join("v11.wal"),
            "{\"op\":\"set\",\"k\":\"a.b\",\"v\":{\"deep\":{\"x\":[1,2,{\"z\":null}]}}}\n\
             {\"op\":\"set\",\"k\":\"t\",\"v\":\"txt\"}\n\
             {\"op\":\"del\",\"k\":\"t\"}\n",
        )
        .unwrap();
        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(
            db.get("a.b").unwrap().unwrap(),
            br#"{"deep":{"x":[1,2,{"z":null}]}}"#.to_vec()
        );
        assert_eq!(db.get("t").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_snapshot_restores_from_backup() {
        let dir = tmpdir("backup");
        let path = dir.join("bk.db");
        let mut db = Engine::open(&path, opts()).unwrap();
        db.set("good", b"1").unwrap();
        db.compact().unwrap();
        db.set("second", b"2").unwrap();
        db.compact().unwrap();
        drop(db);

        let snap = dir.join("bk.spdb");
        let mut raw = std::fs::read(&snap).unwrap();
        raw[20] ^= 0xFF;
        std::fs::write(&snap, &raw).unwrap();
        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(db.get("good").unwrap().unwrap(), b"1");
        assert_eq!(db.get("second").unwrap(), None);
        let ev = db.drain_init_events();
        assert!(ev.iter().any(|e| e.event == "warn"));
        assert!(ev.iter().any(|e| e.event == "restore"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_validation_errors() {
        let dir = tmpdir("keys");
        let path = dir.join("k.db");
        let mut db = Engine::open(&path, opts()).unwrap();
        assert!(db.set("", b"1").is_err());
        assert!(db.set("__proto__.x", b"1").is_err());
        assert!(db.get("a..b").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn operations_after_close_fail() {
        let dir = tmpdir("closed");
        let path = dir.join("c.db");
        let mut db = Engine::open(&path, opts()).unwrap();
        db.set("a", b"1").unwrap();
        db.close().unwrap();
        assert_eq!(db.get("a").unwrap_err().code, error::codes::DATABASE_CLOSED);
        db.close().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gzip_v2_snapshot_roundtrip() {
        let dir = tmpdir("gz");
        let path = dir.join("g.db");
        let o = EngineOptions { compress: true, ..opts() };
        let mut db = Engine::open(&path, o.clone()).unwrap();
        for i in 0..500 {
            db.set(&format!("k{}", i), b"{\"pad\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}").unwrap();
        }
        db.compact().unwrap();
        let size = db.stats().unwrap().file_size;
        drop(db);
        let db = Engine::open(&path, o).unwrap();
        assert_eq!(db.count(None).unwrap(), 500);
        assert!(size < 10_000, "gzip snapshot should be small, got {}", size);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod v2_tests {
    use super::*;
    use crate::store::Store;
    use formats::{Compression, Manifest, ManifestEntry};

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "spectre-v2-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn opts() -> EngineOptions {
        EngineOptions { lock_timeout_ms: 2000, ..Default::default() }
    }


    #[test]
    fn scan_page_cursor_paging() {
        let dir = tmpdir("scanpage");
        let mut db = Engine::open(&dir.join("s.db"), opts()).unwrap();
        for i in 0..50 {
            db.set(&format!("user.{}", i), br#"{"i":1}"#).unwrap();
            db.set(&format!("post.{}", i), b"1").unwrap();
        }
        let mut cursor = String::new();
        let mut seen = 0usize;
        let mut pages = 0usize;
        loop {
            let (rows, next) = db.scan_page(Some("user"), &cursor, 7).unwrap();
            seen += rows.len();
            pages += 1;
            if next.is_empty() {
                assert!(rows.len() <= 7);
                break;
            }
            assert_eq!(rows.len(), 7);

            assert!(rows.last().unwrap().0.as_bytes() <= next.as_bytes());
            cursor = next;
        }
        assert_eq!(seen, 50);
        assert!(pages >= 8);

        db.set_raw("blob", &[0u8, 159, 146, 150]).unwrap();
        let (rows, _) = db.scan_page(None, "", 1000).unwrap();
        assert_eq!(rows.len(), 100);
        let _ = std::fs::remove_dir_all(&dir);
    }


    #[test]
    fn raw_values_isolated_from_json() {
        let dir = tmpdir("raw");
        let path = dir.join("r.db");
        let mut db = Engine::open(&path, opts()).unwrap();
        db.set_raw("img.1", &[0xFF, 0xD8, 0xFF, 0x00, 0x42]).unwrap();
        db.set("user.1", br#"{"name":"a"}"#).unwrap();

        assert_eq!(db.get("img.1").unwrap(), None);
        assert!(!db.has("img.1").unwrap());
        assert_eq!(db.count(None).unwrap(), 1);
        assert_eq!(db.all(None).unwrap().len(), 1);

        assert_eq!(db.get_raw("img.1").unwrap().unwrap(), vec![0xFF, 0xD8, 0xFF, 0x00, 0x42]);
        assert_eq!(db.get_raw("user.1").unwrap(), None);

        std::mem::forget(db);
        std::fs::write(dir.join("r.lock"), "999999999\n").unwrap();
        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(db.get_raw("img.1").unwrap().unwrap()[0], 0xFF);
        assert_eq!(db.get("user.1").unwrap().unwrap(), br#"{"name":"a"}"#.to_vec());
        assert!(db.delete_raw("img.1").unwrap());
        assert_eq!(db.get_raw("img.1").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }


    #[test]
    fn segmented_delta_compaction_and_tombstones() {
        let dir = tmpdir("segments");
        let path = dir.join("seg.db");

        let o = EngineOptions {
            compact_mode: CompactMode::Segments,
            segment_merge_every: 3,
            lock_timeout_ms: 2000,
            ..opts()
        };
        let mut db = Engine::open(&path, o.clone()).unwrap();
        for i in 0..100 {
            db.set(&format!("k{}", i), br#"{"v":1}"#).unwrap();
        }
        db.compact().unwrap();
        assert!(dir.join("seg.spdb").exists());
        assert!(dir.join("seg.spman").exists());


        db.set("k5", br#"{"v":2}"#).unwrap();
        db.delete("k6").unwrap();
        db.compact().unwrap();
        let st = db.stats().unwrap();
        assert_eq!(st.segment_count, 1);
        assert_eq!(st.pending_writes, 0);


        db.set("k7", br#"{"v":3}"#).unwrap();
        db.compact().unwrap();
        db.set("k8", br#"{"v":4}"#).unwrap();
        db.compact().unwrap();
        db.set("k9", br#"{"v":5}"#).unwrap();
        db.compact().unwrap();
        let st = db.stats().unwrap();
        assert!(st.segment_count <= 1, "full merge should reset segments: {}", st.segment_count);

        drop(db);
        let mut db = Engine::open(&path, o).unwrap();
        assert_eq!(db.get("k5").unwrap().unwrap(), br#"{"v":2}"#.to_vec());
        assert_eq!(db.get("k6").unwrap(), None, "deleted key must not resurrect");
        assert_eq!(db.get("k9").unwrap().unwrap(), br#"{"v":5}"#.to_vec());
        assert_eq!(db.count(None).unwrap(), 99);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_roundtrip_and_bad_segment_crc() {
        let m = Manifest {
            generation: 12,
            indexes: vec!["a.b".into(), "status".into()],
            segments: vec![
                ManifestEntry { id: 1, gen: 12, entries: 10, name: "db.spseg-1".into(), file_crc: 0xDEADBEEF },
                ManifestEntry { id: 2, gen: 13, entries: 3, name: "db.spseg-2".into(), file_crc: 42 },
            ],
        };
        let bytes = formats::encode_manifest(&m);
        let back = formats::decode_manifest(&bytes).unwrap();
        assert_eq!(back.generation, 12);
        assert_eq!(back.indexes, m.indexes);
        assert_eq!(back.segments.len(), 2);
        assert_eq!(back.segments[1].name, "db.spseg-2");
        assert_eq!(back.segments[1].file_crc, 42);

        let mut bad = bytes.clone();
        bad[15] ^= 0xFF;
        assert!(formats::decode_manifest(&bad).is_err());
    }


    #[test]
    fn durability_alias_and_stats_surface() {
        let dir = tmpdir("durability");
        let o = EngineOptions { durability: Durability::Durable, lock_timeout_ms: 2000, ..opts() };
        let mut db = Engine::open(&dir.join("d.db"), o).unwrap();
        assert_eq!(db.durability(), Durability::Durable);
        db.set("a", b"1").unwrap();
        db.compact().unwrap();
        let st = db.stats().unwrap();
        assert_eq!(st.durability, "durable");
        assert!(st.last_compaction_ms.is_some());
        assert!(st.last_lsn >= 1);
        assert_eq!(st.compression, "none");
        drop(db);

        let o = EngineOptions { sync_wal: true, lock_timeout_ms: 2000, ..opts() };
        let db = Engine::open(&dir.join("d.db"), o).unwrap();
        assert_eq!(db.durability(), Durability::Durable);
        let _ = std::fs::remove_dir_all(&dir);
    }


    #[test]
    fn wal_v3_header_and_lsn_monotonic() {
        let dir = tmpdir("lsn");
        let mut db = Engine::open(&dir.join("l.db"), opts()).unwrap();
        for i in 0..10 {
            db.set(&format!("k{}", i), b"1").unwrap();
        }
        assert_eq!(db.stats().unwrap().last_lsn, 10);
        drop(db);
        let raw = std::fs::read(dir.join("l.spwal")).unwrap();
        assert_eq!(&raw[..8], b"SPDBWAL3");
        let replay = formats::replay_wal(&raw);
        assert_eq!(replay.header.as_ref().unwrap().version, 3);
        assert_eq!(replay.last_lsn, 10);

        let mut db = Engine::open(&dir.join("l.db"), opts()).unwrap();
        db.set("after", b"1").unwrap();
        assert_eq!(db.stats().unwrap().last_lsn, 11);

        let legacy = formats::encode_set(b"old", b"1");
        let r = formats::replay_wal(&legacy);
        assert!(r.header.is_none());
        assert_eq!(r.ops.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }


    #[test]
    fn zstd_snapshot_smaller_and_roundtrip() {
        let dir = tmpdir("zstd");
        let mut s = Store::new();
        for i in 0..500 {
            s.set(format!("k{}", i).as_bytes(), br#"{"pad":"aaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.to_vec().into_boxed_slice());
        }
        let none = formats::write_snapshot_ex(&s, Compression::None, None);
        let zstd = formats::write_snapshot_ex(&s, Compression::Zstd, None);
        let gzip = formats::write_snapshot_ex(&s, Compression::Gzip, None);
        assert!(zstd.len() < none.len());
        let mut back = formats::load_snapshot(&zstd, None).unwrap();
        assert_eq!(back.len(), 500);
        assert_eq!(back.get(b"k7"), Some(br#"{"pad":"aaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.to_vec()));
        let _ = gzip;

        let o = EngineOptions { compression: Compression::Zstd, lock_timeout_ms: 2000, ..opts() };
        let mut db = Engine::open(&dir.join("z.db"), o.clone()).unwrap();
        for i in 0..200 {
            db.set(&format!("k{}", i), br#"{"pad":"aaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#).unwrap();
        }
        db.compact().unwrap();
        assert_eq!(db.stats().unwrap().compression, "zstd");
        drop(db);
        let db = Engine::open(&dir.join("z.db"), o).unwrap();
        assert_eq!(db.count(None).unwrap(), 200);
        let _ = std::fs::remove_dir_all(&dir);
    }


    #[test]
    fn encrypted_snapshot_requires_key_and_roundtrips() {
        let dir = tmpdir("encsnap");
        let path = dir.join("e.db");
        let o = EngineOptions {
            encryption_key: Some(b"k".to_vec()),
            encrypt_snapshot: true,
            lock_timeout_ms: 2000,
            ..opts()
        };
        let mut db = Engine::open(&path, o.clone()).unwrap();
        db.set("password", br#"{"pw":"hunter2"}"#).unwrap();
        db.set("note", br#"{"txt":"public"}"#).unwrap();
        db.compact().unwrap();
        drop(db);
        let raw = std::fs::read(dir.join("e.spdb")).unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains("hunter2"));
        assert!(!text.contains("public"), "whole snapshot must be ciphertext");


        let err = Engine::open(&path, opts()).unwrap_err();
        assert_eq!(err.code, error::codes::DECRYPTION_FAILED);

        let mut db = Engine::open(&path, o).unwrap();
        assert_eq!(db.get("password").unwrap().unwrap(), br#"{"pw":"hunter2"}"#.to_vec());
        assert_eq!(db.get("note").unwrap().unwrap(), br#"{"txt":"public"}"#.to_vec());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn custom_scrypt_parameters_derive_differently() {
        let a = crypto::derive_key_params(b"secret", 14, 8, 1).unwrap();
        let b = crypto::derive_key_params(b"secret", 15, 8, 1).unwrap();
        let c = crypto::derive_key_params(b"secret", 14, 8, 1).unwrap();
        assert_ne!(a, b);
        assert_eq!(a, c);

        let raw = [7u8; 32];
        assert_eq!(crypto::derive_key_params(&raw, 10, 4, 1).unwrap(), raw);
    }


    #[test]
    fn secondary_index_lifecycle_and_range() {
        let dir = tmpdir("index");
        let path = dir.join("i.db");
        let mut db = Engine::open(&path, opts()).unwrap();
        for i in 0..20 {
            db.set(&format!("user.{}", i), format!(r#"{{"age":{},"status":"{}"}}"#, 20 + i, if i % 2 == 0 { "active" } else { "idle" }).as_bytes()).unwrap();
        }
        assert_eq!(db.create_index("age").unwrap(), 20);
        assert_eq!(db.create_index("status").unwrap(), 20);
        assert_eq!(db.index_list().len(), 2);

        let hits = db.index_lookup("status", br#""active""#).unwrap();
        assert_eq!(hits.len(), 10);
        let mid = db.index_range("age", Some(b"25"), Some(b"28"), 0).unwrap();
        assert_eq!(mid, vec!["user.5".to_string(), "user.6".to_string(), "user.7".to_string()]);
        let limited = db.index_range("age", None, Some(b"100"), 3).unwrap();
        assert_eq!(limited.len(), 3);


        db.set("user.5", br#"{"age":99,"status":"active"}"#).unwrap();
        assert_eq!(db.index_lookup("age", b"25").unwrap().len(), 0);
        assert!(db.index_lookup("age", b"99").unwrap().contains(&"user.5".to_string()));
        db.delete("user.6").unwrap();

        assert_eq!(db.index_lookup("status", br#""active""#).unwrap().len(), 10);


        let st = db.stats().unwrap();
        assert_eq!(st.index_count, 2);


        drop(db);
        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(db.index_list(), vec!["age".to_string(), "status".to_string()]);
        assert_eq!(db.index_lookup("age", b"99").unwrap(), vec!["user.5".to_string()]);
        assert!(db.drop_index("age").unwrap());
        assert_eq!(db.index_list(), vec!["status".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }


    #[test]
    fn stats_observability_fields() {
        let dir = tmpdir("stats");
        let mut db = Engine::open(&dir.join("s.db"), opts()).unwrap();
        let _ = db.get("miss").unwrap();
        db.set("hit", b"1").unwrap();
        let _ = db.get("hit").unwrap();
        db.set("p2", b"2").unwrap();
        let st = db.stats().unwrap();
        assert_eq!(st.cache_hits, 1);
        assert_eq!(st.cache_misses, 1);
        assert_eq!(st.pending_writes, 2);
        assert_eq!(st.last_lsn, 2);
        assert_eq!(st.generation, 0);
        assert_eq!(st.recovery_count, 0);
        drop(db);
        let db = Engine::open(&dir.join("s.db"), opts()).unwrap();
        assert_eq!(db.stats().unwrap().recovery_count, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }


    #[test]
    fn async_compaction_phases_commit_and_recover() {
        let dir = tmpdir("asynccompact");
        let path = dir.join("a.db");
        let o = EngineOptions {
            compact_mode: CompactMode::Segments,
            lock_timeout_ms: 2000,
            ..opts()
        };
        let mut db = Engine::open(&path, o.clone()).unwrap();
        for i in 0..30 {
            db.set(&format!("k{}", i), br#"{"v":1}"#).unwrap();
        }
        db.compact().unwrap();

        let job = db.prepare_compact().unwrap();
        assert!(!job.legacy);
        let done = job.compute();

        db.set("k0", br#"{"v":100}"#).unwrap();
        db.set("new", b"1").unwrap();

        assert_eq!(db.finish_compact(done).unwrap(), "committed");
        drop(db);
        let mut db = Engine::open(&path, o).unwrap();
        assert_eq!(db.get("k0").unwrap().unwrap(), br#"{"v":100}"#.to_vec());
        assert_eq!(db.get("new").unwrap().unwrap(), b"1");
        assert_eq!(db.count(None).unwrap(), 31);
        let _ = std::fs::remove_dir_all(&dir);
    }


    #[test]
    fn batch_survives_crash_and_single_record() {
        let dir = tmpdir("batchfast");
        let path = dir.join("b.db");
        let mut db = Engine::open(&path, opts()).unwrap();
        db.batch(vec![
            Op::Set { key: b"x".to_vec(), value: b"1".to_vec().into_boxed_slice() },
            Op::Del { key: b"x".to_vec() },
            Op::Set { key: b"y".to_vec(), value: b"2".to_vec().into_boxed_slice() },
        ])
        .unwrap();
        assert_eq!(db.stats().unwrap().wal_ops, 1);
        std::mem::forget(db);
        std::fs::write(dir.join("b.lock"), "999999999\n").unwrap();
        let mut db = Engine::open(&path, opts()).unwrap();
        assert_eq!(db.get("x").unwrap(), None);
        assert_eq!(db.get("y").unwrap().unwrap(), b"2");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
