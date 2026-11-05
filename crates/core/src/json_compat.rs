use crate::error::{Result, SpectreError};
use crate::formats::Op;
use crate::store::Store;
use serde_json::{Map, Value};


pub fn flatten_json(root: &Value) -> Vec<(Vec<u8>, Box<[u8]>)> {
    let mut out = Vec::new();
    let mut path = String::new();
    walk(root, &mut path, &mut out);
    out
}

fn is_envelope(v: &Value) -> bool {
    match v {
        Value::Object(m) => m
            .get("__enc")
            .and_then(|x| x.as_i64())
            .map(|n| n == 1)
            .unwrap_or(false),
        _ => false,
    }
}

fn walk(node: &Value, path: &mut String, out: &mut Vec<(Vec<u8>, Box<[u8]>)>) {
    if let Value::Object(map) = node {
        if !is_envelope(node) && !map.is_empty() {
            let base_len = path.len();
            for (k, v) in map {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(k);
                walk(v, path, out);
                path.truncate(base_len);
            }
            return;
        }
    }
    if !path.is_empty() || !matches!(node, Value::Object(_)) {
        if let Ok(bytes) = serde_json::to_vec(node) {
            out.push((path.as_bytes().to_vec(), bytes.into_boxed_slice()));
        }
    }
}

pub fn load_json_snapshot(bytes: &[u8], compress: bool) -> Result<Store> {
    let raw: Vec<u8> = if compress {
        let mut dec = flate2::read::GzDecoder::new(bytes);
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut dec, &mut buf).map_err(|e| {
            SpectreError::new(crate::error::codes::SNAPSHOT_CORRUPTED, format!("Gzip failed: {}", e))
        })?;
        buf
    } else {
        bytes.to_vec()
    };
    let value: Value = serde_json::from_slice(&raw)
        .map_err(|e| SpectreError::snapshot_corrupted(format!("JSON parse failed: {}", e)))?;
    let mut store = Store::new();
    for (k, v) in flatten_json(&value) {
        store.set(&k, v);
    }
    Ok(store)
}

pub fn write_json_snapshot(store: &Store, compress: bool) -> Result<Vec<u8>> {
    let mut root = Map::new();
    for (k, v) in store.iter() {
        let key = std::str::from_utf8(k)
            .map_err(|_| SpectreError::snapshot_corrupted("Non-UTF-8 key in store"))?;
        let val: Value = serde_json::from_slice(v)
            .map_err(|e| SpectreError::snapshot_corrupted(format!("Stored value is not JSON: {}", e)))?;
        insert_path(&mut root, key, val);
    }
    let doc = Value::Object(root);
    let bytes = serde_json::to_vec(&doc)
        .map_err(|e| SpectreError::snapshot_corrupted(format!("Serialize failed: {}", e)))?;
    if !compress {
        return Ok(bytes);
    }
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut enc, &bytes)
        .and_then(|_| enc.finish())
        .map_err(|e| SpectreError::snapshot_corrupted(format!("Gzip failed: {}", e)))
}

fn insert_path(root: &mut Map<String, Value>, path: &str, val: Value) {
    let mut parts = path.split('.');
    let last = parts.next_back().unwrap_or(path);
    let mut node = root;
    for part in parts {
        let entry = node
            .entry(part.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        node = entry.as_object_mut().unwrap();
    }
    node.insert(last.to_string(), val);
}


pub fn encode_json_wal_op(op: &Op, out: &mut Vec<u8>) {
    match op {
        Op::Set { key, value } => {
            out.push(b'{');
            out.extend_from_slice(b"\"op\":\"set\",\"k\":");
            out.extend_from_slice(&quote_key(key));
            out.extend_from_slice(b",\"v\":");
            out.extend_from_slice(value);
            out.extend_from_slice(b"}\n");
        }
        Op::Del { key } => {
            out.push(b'{');
            out.extend_from_slice(b"\"op\":\"del\",\"k\":");
            out.extend_from_slice(&quote_key(key));
            out.extend_from_slice(b"}\n");
        }
        Op::Clear => {
            out.extend_from_slice(b"{\"op\":\"clear\"}\n");
        }
        Op::Batch(ops) => {
            out.push(b'{');
            out.extend_from_slice(b"\"op\":\"batch\",\"ops\":[");
            for (i, sub) in ops.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }

                match sub {
                    Op::Set { key, value } => {
                        out.push(b'{');
                        out.extend_from_slice(b"\"op\":\"set\",\"k\":");
                        out.extend_from_slice(&quote_key(key));
                        out.extend_from_slice(b",\"v\":");
                        out.extend_from_slice(value);
                        out.push(b'}');
                    }
                    Op::Del { key } => {
                        out.push(b'{');
                        out.extend_from_slice(b"\"op\":\"del\",\"k\":");
                        out.extend_from_slice(&quote_key(key));
                        out.push(b'}');
                    }
                    _ => {}
                }
            }
            out.extend_from_slice(b"]}\n");
        }
    }
}

fn quote_key(key: &[u8]) -> Vec<u8> {
    let s = std::str::from_utf8(key).unwrap_or("");
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string()).into_bytes()
}

pub fn decode_json_wal(bytes: &[u8]) -> (Vec<Op>, usize) {
    let mut ops = Vec::new();
    let mut warns = 0usize;
    for line in bytes.split(|&b| b == b'\n') {
        if line.iter().all(|&b| b.is_ascii_whitespace()) {
            continue;
        }
        match serde_json::from_slice::<Value>(line) {
            Ok(v) => match parse_entry(&v) {
                Some(op) => ops.push(op),
                None => warns += 1,
            },
            Err(_) => warns += 1,
        }
    }
    (ops, warns)
}

fn parse_entry(v: &Value) -> Option<Op> {
    let op = v.get("op")?.as_str()?;
    match op {
        "set" => {
            let key = v.get("k")?.as_str()?.as_bytes().to_vec();
            let val = serde_json::to_vec(v.get("v")?).ok()?;
            Some(Op::Set { key, value: val.into_boxed_slice() })
        }
        "del" => {
            let key = v.get("k")?.as_str()?.as_bytes().to_vec();
            Some(Op::Del { key })
        }
        "clear" => Some(Op::Clear),
        "batch" => {
            let arr = v.get("ops")?.as_array()?;
            let mut ops = Vec::with_capacity(arr.len());
            for item in arr {
                ops.push(parse_entry(item)?);
            }
            Some(Op::Batch(ops))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_snapshot_roundtrip() {
        let mut s = Store::new();
        s.set(b"user.1.name", br#""alice""#.to_vec().into_boxed_slice());
        s.set(b"user.1.age", b"30".to_vec().into_boxed_slice());
        s.set(b"user.2", br#"{"score":99}"#.to_vec().into_boxed_slice());
        s.set(b"flags", b"[true,false]".to_vec().into_boxed_slice());

        let bytes = write_json_snapshot(&s, false).unwrap();
        let parsed: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["user"]["1"]["name"], "alice");
        assert_eq!(parsed["user"]["2"]["score"], 99);
        assert_eq!(parsed["flags"][0], true);

        let mut s2 = load_json_snapshot(&bytes, false).unwrap();
        assert_eq!(s2.len(), 4);
        assert_eq!(s2.get(b"user.1.name"), Some(br#""alice""#.to_vec()));
    }

    #[test]
    fn json_snapshot_gzip_roundtrip() {
        let mut s = Store::new();
        s.set(b"k", br#"{"pad":"aaaaaaaaaaaaaaaaaaaa"}"#.to_vec().into_boxed_slice());
        let gz = write_json_snapshot(&s, true).unwrap();
        let mut s2 = load_json_snapshot(&gz, true).unwrap();
        assert_eq!(s2.get(b"k"), Some(b"{\"pad\":\"aaaaaaaaaaaaaaaaaaaa\"}".to_vec()));
    }

    #[test]
    fn json_wal_matches_v1_layout() {
        let mut buf = Vec::new();
        encode_json_wal_op(&Op::Set { key: b"a.b".to_vec(), value: br#"{"x":1}"#.to_vec().into_boxed_slice() }, &mut buf);
        encode_json_wal_op(&Op::Del { key: b"c".to_vec() }, &mut buf);
        encode_json_wal_op(&Op::Clear, &mut buf);
        let text = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], r#"{"op":"set","k":"a.b","v":{"x":1}}"#);
        assert_eq!(lines[1], r#"{"op":"del","k":"c"}"#);
        assert_eq!(lines[2], r#"{"op":"clear"}"#);

        let (ops, warns) = decode_json_wal(text.as_bytes());
        assert_eq!(warns, 0);
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn json_wal_batch() {
        let mut buf = Vec::new();
        encode_json_wal_op(&Op::Batch(vec![
            Op::Set { key: b"x".to_vec(), value: b"1".to_vec().into_boxed_slice() },
            Op::Del { key: b"y".to_vec() },
        ]), &mut buf);
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(text.trim_end(), r#"{"op":"batch","ops":[{"op":"set","k":"x","v":1},{"op":"del","k":"y"}]}"#);
        let (ops, warns) = decode_json_wal(text.as_bytes());
        assert_eq!(warns, 0);
        match &ops[0] {
            Op::Batch(sub) => assert_eq!(sub.len(), 2),
            _ => panic!("expected batch"),
        }
    }

    #[test]
    fn bad_lines_skipped() {
        let text = "{\"op\":\"set\",\"k\":\"a\",\"v\":1}\nnot json\n{\"op\":\"clear\"}\n";
        let (ops, warns) = decode_json_wal(text.as_bytes());
        assert_eq!(ops.len(), 2);
        assert_eq!(warns, 1);
    }

    #[test]
    fn envelope_is_leaf() {
        let env = br#"{"__enc":1,"iv":"AAA","ct":"BBB","tag":"CCC"}"#;
        let v: Value = serde_json::from_slice(env).unwrap();
        let flat = flatten_json(&v);
        assert_eq!(flat.len(), 0);
        let doc: Value = serde_json::from_slice(br#"{"user":{"token":{"__enc":1,"iv":"a","ct":"b","tag":"c"}}}"#).unwrap();
        let flat = flatten_json(&doc);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].0, b"user.token");
    }
}
