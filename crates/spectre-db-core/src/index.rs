use serde_json::Value;
use std::collections::BTreeMap;

pub fn order_key(v: &Value) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    match v {
        Value::Null => out.push(0x00),
        Value::Bool(false) => out.push(0x01),
        Value::Bool(true) => out.push(0x02),
        Value::Number(n) => {
            out.push(0x03);
            let f = n.as_f64().unwrap_or(0.0);
            let bits = f.to_bits();
            let sortable = if bits >> 63 == 1 { !bits } else { bits | (1 << 63) };
            out.extend_from_slice(&sortable.to_be_bytes());
        }
        Value::String(s) => {
            out.push(0x04);
            out.extend_from_slice(s.as_bytes());
        }
        other => {
            out.push(0x05);
            if let Ok(b) = serde_json::to_vec(other) {
                out.extend_from_slice(&b);
            }
        }
    }
    out
}

pub fn order_key_from_json(bytes: &[u8]) -> Option<Vec<u8>> {
    serde_json::from_slice::<Value>(bytes).ok().map(|v| order_key(&v))
}

pub fn extract_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for part in path.split('.') {
        match cur {
            Value::Object(map) => {
                cur = map.get(part)?;
            }
            Value::Array(arr) => {
                let idx: usize = part.parse().ok()?;
                cur = arr.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(cur)
}

#[derive(Debug, Default)]
pub struct SecondaryIndex {
    pub path: String,

    pub tree: BTreeMap<Vec<u8>, Vec<Vec<u8>>>,
}

impl SecondaryIndex {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), tree: BTreeMap::new() }
    }


    pub fn build(path: &str, entries: impl Iterator<Item = (Vec<u8>, Vec<u8>)>) -> Self {
        let mut idx = Self::new(path);
        for (k, v) in entries {
            idx.insert_key(&k, &v);
        }
        idx
    }


    pub fn insert_key(&mut self, key: &[u8], value_bytes: &[u8]) {
        let parsed: Option<Value> = serde_json::from_slice(value_bytes).ok();
        self.remove_key(key);
        let Some(v) = parsed else { return };
        let Some(target) = extract_path(&v, &self.path) else { return };
        if matches!(target, Value::Object(_) | Value::Array(_)) {
            return;
        }
        let ok = order_key(target);
        self.tree.entry(ok).or_default().push(key.to_vec());
    }

    pub fn remove_key(&mut self, key: &[u8]) {
        let empty_ok: Vec<u8> = Vec::new();
        let mut doomed: Vec<Vec<u8>> = Vec::new();
        for (ok, keys) in self.tree.iter_mut() {
            if let Some(pos) = keys.iter().position(|k| k == key) {
                keys.remove(pos);
                if keys.is_empty() {
                    doomed.push(ok.clone());
                }
            }
        }
        let _ = empty_ok;
        for d in doomed {
            self.tree.remove(&d);
        }
    }

    pub fn lookup(&self, value_json: &[u8]) -> Vec<Vec<u8>> {
        match order_key_from_json(value_json) {
            Some(ok) => self.tree.get(&ok).cloned().unwrap_or_default(),
            None => Vec::new(),
        }
    }


    pub fn range(&self, gte: Option<&[u8]>, lt: Option<&[u8]>, limit: usize) -> Vec<Vec<u8>> {
        use std::ops::Bound;
        let start = match gte.and_then(order_key_from_json) {
            Some(k) => Bound::Included(k),
            None => Bound::Unbounded,
        };
        let end = match lt.and_then(order_key_from_json) {
            Some(k) => Bound::Excluded(k),
            None => Bound::Unbounded,
        };
        let mut out = Vec::new();
        for (_, keys) in self.tree.range((start, end)) {
            for k in keys {
                out.push(k.clone());
                if limit > 0 && out.len() >= limit {
                    return out;
                }
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.tree.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_ordering() {
        let nine = order_key(&serde_json::json!(9));
        let ten = order_key(&serde_json::json!(10));
        let minus = order_key(&serde_json::json!(-5));
        assert!(nine < ten);
        assert!(minus < nine);
    }

    #[test]
    fn extract_nested_path() {
        let v: Value = serde_json::from_slice(br#"{"profile":{"age":30},"tags":[1,2]}"#).unwrap();
        assert_eq!(extract_path(&v, "profile.age"), Some(&serde_json::json!(30)));
        assert_eq!(extract_path(&v, "tags.1"), Some(&serde_json::json!(2)));
        assert_eq!(extract_path(&v, "profile.missing"), None);
    }

    #[test]
    fn build_lookup_remove() {
        let mut idx = SecondaryIndex::build(
            "status",
            vec![
                (b"user.1".to_vec(), br#"{"status":"active"}"#.to_vec()),
                (b"user.2".to_vec(), br#"{"status":"idle"}"#.to_vec()),
                (b"user.3".to_vec(), br#"{"status":"active"}"#.to_vec()),
            ]
            .into_iter(),
        );
        assert_eq!(idx.lookup(b"\"active\"").len(), 2);
        assert_eq!(idx.lookup(b"\"idle\""), vec![b"user.2".to_vec()]);
        idx.remove_key(b"user.1");
        assert_eq!(idx.lookup(b"\"active\""), vec![b"user.3".to_vec()]);
    }

    #[test]
    fn range_half_open() {
        let idx = SecondaryIndex::build(
            "age",
            vec![
                (b"a".to_vec(), br#"{"age":18}"#.to_vec()),
                (b"b".to_vec(), br#"{"age":21}"#.to_vec()),
                (b"c".to_vec(), br#"{"age":35}"#.to_vec()),
                (b"d".to_vec(), br#"{"age":67}"#.to_vec()),
            ]
            .into_iter(),
        );
        let r = idx.range(Some(b"20"), Some(b"40"), 0);
        assert_eq!(r, vec![b"b".to_vec(), b"c".to_vec()]);
        let r2 = idx.range(None, Some(b"21".as_slice()), 0);
        assert_eq!(r2, vec![b"a".to_vec()]);
    }
}
