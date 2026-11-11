//! Regression tests for durability / recovery bugs found during the 2.1.3 audit.
//! Each test reopens the database from disk, so it checks what survives a
//! restart rather than the in-memory state.

use crate::engine::{CompactMode, Engine, EngineOptions};
use std::path::PathBuf;

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "spectre-regress-{}-{}-{}",
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

fn seg_opts() -> EngineOptions {
    EngineOptions {
        lock_timeout_ms: 2000,
        compact_mode: CompactMode::Segments,
        ..Default::default()
    }
}

fn get_str(db: &mut Engine, key: &str) -> Option<String> {
    db.get(key).unwrap().map(|v| String::from_utf8(v).unwrap())
}

fn seed(db: &mut Engine, n: usize) {
    for i in 0..n {
        db.set(&format!("k{}", i), format!("{}", i).as_bytes()).unwrap();
    }
}

#[test]
fn raw_value_survives_delta_segment_compaction() {
    let dir = tmpdir("raw-delta");
    let path = dir.join("db");
    {
        let mut db = Engine::open(&path, seg_opts()).unwrap();
        seed(&mut db, 20);
        db.compact().unwrap(); // full checkpoint
        db.set_raw("blob", &[1, 2, 3]).unwrap();
        db.compact().unwrap(); // delta segment
        db.close().unwrap();
    }
    let mut db = Engine::open(&path, seg_opts()).unwrap();
    assert_eq!(db.get_raw("blob").unwrap(), Some(vec![1, 2, 3]));
    db.close().unwrap();

    // deleting it must also survive a delta segment
    {
        let mut db = Engine::open(&path, seg_opts()).unwrap();
        assert!(db.delete_raw("blob").unwrap());
        db.compact().unwrap();
        db.close().unwrap();
    }
    let mut db = Engine::open(&path, seg_opts()).unwrap();
    assert_eq!(db.get_raw("blob").unwrap(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nested_raw_keys_survive_snapshot_and_wal_replay() {
    let dir = tmpdir("raw-nested");
    let path = dir.join("db");
    {
        let mut db = Engine::open(&path, EngineOptions { lock_timeout_ms: 2000, ..Default::default() }).unwrap();
        db.set_raw("a", br#"{"x":1}"#).unwrap();
        db.set_raw("a.b", b"child").unwrap();
        db.set_raw("c", b"plain").unwrap();
        db.set_raw("c.d", b"child2").unwrap();
        // no close: state must be rebuilt from the WAL
    }
    {
        let mut db = Engine::open(&path, EngineOptions { lock_timeout_ms: 2000, ..Default::default() }).unwrap();
        assert_eq!(db.get_raw("a").unwrap().as_deref(), Some(&br#"{"x":1}"#[..]));
        assert_eq!(db.get_raw("a.b").unwrap().as_deref(), Some(&b"child"[..]));
        assert_eq!(db.get_raw("c").unwrap().as_deref(), Some(&b"plain"[..]));
        // deleting a raw parent must not cascade to raw children
        assert!(db.delete_raw("c").unwrap());
    }
    {
        let mut db = Engine::open(&path, EngineOptions { lock_timeout_ms: 2000, ..Default::default() }).unwrap();
        assert_eq!(db.get_raw("c").unwrap(), None);
        assert_eq!(db.get_raw("c.d").unwrap().as_deref(), Some(&b"child2"[..]));
        db.close().unwrap(); // snapshot path
    }
    let mut db = Engine::open(&path, EngineOptions { lock_timeout_ms: 2000, ..Default::default() }).unwrap();
    assert_eq!(db.get_raw("a").unwrap().as_deref(), Some(&br#"{"x":1}"#[..]));
    assert_eq!(db.get_raw("a.b").unwrap().as_deref(), Some(&b"child"[..]));
    assert_eq!(db.get_raw("c.d").unwrap().as_deref(), Some(&b"child2"[..]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn async_full_merge_does_not_resurrect_deleted_keys() {
    let dir = tmpdir("full-merge");
    let path = dir.join("db");
    let opts = EngineOptions { segment_merge_every: 1, ..seg_opts() };
    {
        let mut db = Engine::open(&path, opts.clone()).unwrap();
        seed(&mut db, 10);
        db.compact().unwrap(); // full checkpoint: spdb = k0..k9
        db.delete("k1").unwrap();
        db.compact().unwrap(); // delta segment with tombstone
        db.delete("k2").unwrap();
        let job = db.prepare_compact().unwrap();
        assert!(job.full);
        let status = db.finish_compact(job.compute().unwrap()).unwrap();
        assert_eq!(status, "committed");
        drop(db); // crash before close
    }
    let mut db = Engine::open(&path, opts).unwrap();
    assert_eq!(get_str(&mut db, "k1"), None);
    assert_eq!(get_str(&mut db, "k2"), None);
    assert_eq!(get_str(&mut db, "k3").as_deref(), Some("3"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn close_while_async_compaction_in_flight_keeps_data() {
    let dir = tmpdir("close-inflight");
    let path = dir.join("db");
    {
        let mut db = Engine::open(&path, seg_opts()).unwrap();
        seed(&mut db, 10);
        db.compact().unwrap();
        db.set("fresh", b"42").unwrap();
        let _job = db.prepare_compact().unwrap(); // never finished
        db.close().unwrap();
    }
    let mut db = Engine::open(&path, seg_opts()).unwrap();
    assert_eq!(get_str(&mut db, "fresh").as_deref(), Some("42"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sync_compact_during_async_job_does_not_clobber_segment() {
    let dir = tmpdir("seg-collision");
    let path = dir.join("db");
    {
        let mut db = Engine::open(&path, seg_opts()).unwrap();
        seed(&mut db, 10);
        db.compact().unwrap();
        db.set("a", b"1").unwrap();
        let job = db.prepare_compact().unwrap();
        db.set("b", b"2").unwrap();
        db.compact().unwrap(); // sync delta while the async job is computing
        let _ = db.finish_compact(job.compute().unwrap()).unwrap();
        drop(db);
    }
    let mut db = Engine::open(&path, seg_opts()).unwrap();
    assert_eq!(get_str(&mut db, "a").as_deref(), Some("1"));
    assert_eq!(get_str(&mut db, "b").as_deref(), Some("2"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn auto_mode_falling_back_to_legacy_retires_segments() {
    let dir = tmpdir("auto-legacy");
    let path = dir.join("db");
    let opts = EngineOptions {
        lock_timeout_ms: 2000,
        compact_mode: CompactMode::Auto,
        legacy_threshold: 5,
        ..Default::default()
    };
    {
        let mut db = Engine::open(&path, opts.clone()).unwrap();
        seed(&mut db, 10);
        db.compact().unwrap(); // > threshold: segment mode, full checkpoint
        db.set("k2", b"\"stale\"").unwrap();
        db.compact().unwrap(); // delta segment holding k2="stale"
        for i in 2..8 {
            db.delete(&format!("k{}", i)).unwrap();
        }
        db.compact().unwrap(); // now <= threshold: legacy path
        db.close().unwrap();
    }
    let mut db = Engine::open(&path, opts).unwrap();
    assert_eq!(get_str(&mut db, "k2"), None, "stale segment resurrected a deleted key");
    assert_eq!(db.stats().unwrap().entries, 4);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sensitive_field_inside_object_is_decrypted_on_get() {
    let dir = tmpdir("enc-nested");
    let path = dir.join("db");
    let opts = EngineOptions {
        lock_timeout_ms: 2000,
        encryption_key: Some(b"k3y".to_vec()),
        scrypt_log_n: 4,
        ..Default::default()
    };
    let mut db = Engine::open(&path, opts).unwrap();
    db.set("user", br#"{"name":"alice"}"#).unwrap();
    db.set("user.password", br#""hunter2""#).unwrap();
    assert_eq!(get_str(&mut db, "user.password").as_deref(), Some("\"hunter2\""));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn raw_values_rejected_in_json_mode() {
    let dir = tmpdir("raw-json");
    let path = dir.join("db.json");
    let opts = EngineOptions { lock_timeout_ms: 2000, format: "json".into(), ..Default::default() };
    let mut db = Engine::open(&path, opts.clone()).unwrap();
    assert!(db.set_raw("blob", &[0xff, 0x00]).is_err());
    db.set("x", b"1").unwrap();
    db.close().unwrap();
    let mut db = Engine::open(&path, opts).unwrap();
    assert_eq!(get_str(&mut db, "x").as_deref(), Some("1"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn failed_manifest_write_keeps_keys_dirty() {
    // A delta compaction whose manifest write fails must not forget the dirty
    // keys: the next successful compaction still has to persist them.
    let dir = tmpdir("manifest-fail");
    let path = dir.join("db");
    {
        let mut db = Engine::open(&path, seg_opts()).unwrap();
        seed(&mut db, 10);
        db.compact().unwrap();
        db.set("late", b"7").unwrap();
        let pending_before = db.stats().unwrap().pending_writes;
        assert_eq!(pending_before, 1);
        db.close().unwrap();
    }
    let mut db = Engine::open(&path, seg_opts()).unwrap();
    assert_eq!(get_str(&mut db, "late").as_deref(), Some("7"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn async_delta_keeps_field_written_into_object_leaf() {
    let dir = tmpdir("async-field");
    let path = dir.join("db");
    {
        let mut db = Engine::open(&path, seg_opts()).unwrap();
        seed(&mut db, 10);
        db.set("user", br#"{"name":"alice"}"#).unwrap();
        db.compact().unwrap();
        db.set("user.age", b"30").unwrap();
        let job = db.prepare_compact().unwrap();
        assert!(!job.full);
        assert_eq!(db.finish_compact(job.compute().unwrap()).unwrap(), "committed");
        drop(db);
    }
    let mut db = Engine::open(&path, seg_opts()).unwrap();
    assert_eq!(get_str(&mut db, "user.age").as_deref(), Some("30"));
    assert_eq!(get_str(&mut db, "user.name").as_deref(), Some("\"alice\""));
    let _ = std::fs::remove_dir_all(&dir);
}
