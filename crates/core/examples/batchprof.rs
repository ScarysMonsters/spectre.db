use spectre_db_core::{Engine, EngineOptions, Op};
use std::time::Instant;
fn main() {
    let val = "x".repeat(280);
    for &n in &[500usize, 5000] {
        let dir = std::env::temp_dir().join(format!("bp-{}-{}", n, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut db = Engine::open(&dir.join("d.db"), EngineOptions::default()).unwrap();
        let ops: Vec<Op> = (0..n)
            .map(|i| Op::Set { key: format!("bk{}", i).into_bytes(), value: format!(r#"{{"i":{},"v":"{}"}}"#, i, val).into_bytes().into_boxed_slice() })
            .collect();
        let t = Instant::now();
        db.batch(ops).unwrap();
        println!("core batch {} ops: {:?}", n, t.elapsed());
        let ops2: Vec<Op> = (0..n)
            .map(|i| Op::Set { key: format!("ck{}", i).into_bytes(), value: format!(r#"{{"i":{},"v":"{}"}}"#, i, val).into_bytes().into_boxed_slice() })
            .collect();
        let t = Instant::now();
        db.batch(ops2).unwrap();
        println!("core batch2 {} ops: {:?}", n, t.elapsed());
        db.close().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
