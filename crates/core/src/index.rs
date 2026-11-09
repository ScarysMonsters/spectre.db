use serde_json::Value;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

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

    pub tree: BTreeMap<Arc<[u8]>, Vec<Arc<[u8]>>>,
    reverse: HashMap<Arc<[u8]>, (Arc<[u8]>, usize)>,
}

impl SecondaryIndex {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            tree: BTreeMap::new(),
            reverse: HashMap::new(),
        }
    }


    pub fn build<I, K, V>(path: &str, entries: I) -> Self
    where
        I: Iterator<Item = (K, V)>,
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let mut idx = Self::new(path);
        for (k, v) in entries {
            idx.insert_key(k.as_ref(), v.as_ref());
        }
        idx
    }


    pub fn insert_key(&mut self, key: &[u8], value_bytes: &[u8]) {
        let parsed: Option<Value> = serde_json::from_slice(value_bytes).ok();
        let key: Arc<[u8]> = Arc::from(key);
        self.insert_parsed_key(&key, parsed.as_ref());
    }

    pub fn insert_parsed_key(&mut self, key: &Arc<[u8]>, root: Option<&Value>) {
        self.remove_key(key.as_ref());
        let Some(root) = root else { return };
        let Some(target) = extract_path(root, &self.path) else { return };
        if matches!(target, Value::Object(_) | Value::Array(_)) {
            return;
        }
        let encoded: Arc<[u8]> = Arc::from(order_key(target));
        let key_ref: Arc<[u8]> = Arc::clone(key);
        let (encoded, position) = match self.tree.entry(encoded) {
            Entry::Occupied(mut entry) => {
                let encoded = Arc::clone(entry.key());
                let bucket = entry.get_mut();
                let position = bucket.len();
                bucket.push(Arc::clone(&key_ref));
                (encoded, position)
            }
            Entry::Vacant(entry) => {
                let encoded = Arc::clone(entry.key());
                entry.insert(vec![Arc::clone(&key_ref)]);
                (encoded, 0)
            }
        };
        self.reverse.insert(key_ref, (encoded, position));
    }

    pub fn remove_key(&mut self, key: &[u8]) {
        let Some((encoded, position)) = self.reverse.remove(key) else {
            return;
        };
        let Some(bucket) = self.tree.get_mut(encoded.as_ref()) else {
            return;
        };
        let position = if bucket
            .get(position)
            .map(|candidate| candidate.as_ref() == key)
            .unwrap_or(false)
        {
            position
        } else {
            match bucket.iter().position(|candidate| candidate.as_ref() == key) {
                Some(found) => found,
                None => return,
            }
        };
        bucket.swap_remove(position);
        if let Some(moved) = bucket.get(position) {
            if let Some(entry) = self.reverse.get_mut(moved.as_ref()) {
                entry.1 = position;
            }
        }
        if bucket.is_empty() {
            self.tree.remove(encoded.as_ref());
        }
    }

    pub fn clear(&mut self) {
        self.tree.clear();
        self.reverse.clear();
    }

    pub fn lookup(&self, value_json: &[u8]) -> Vec<Vec<u8>> {
        match order_key_from_json(value_json) {
            Some(encoded) => {
                let encoded: Arc<[u8]> = Arc::from(encoded);
                self.tree
                    .get(encoded.as_ref())
                    .map(|keys| keys.iter().map(|key| key.as_ref().to_vec()).collect())
                    .unwrap_or_default()
            }
            None => Vec::new(),
        }
    }


    pub fn range(&self, gte: Option<&[u8]>, lt: Option<&[u8]>, limit: usize) -> Vec<Vec<u8>> {
        use std::ops::Bound;
        let start = match gte.and_then(order_key_from_json) {
            Some(k) => Bound::Included(Arc::<[u8]>::from(k)),
            None => Bound::Unbounded,
        };
        let end = match lt.and_then(order_key_from_json) {
            Some(k) => Bound::Excluded(Arc::<[u8]>::from(k)),
            None => Bound::Unbounded,
        };
        let mut out = Vec::new();
        for (_, keys) in self.tree.range((start, end)) {
            for k in keys {
                out.push(k.as_ref().to_vec());
                if limit > 0 && out.len() >= limit {
                    return out;
                }
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.reverse.len()
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

    #[test]
    fn repeated_updates_do_not_duplicate() {
        let mut idx = SecondaryIndex::new("status");
        for value in [
            br#"{"status":"idle"}"#.as_slice(),
            br#"{"status":"active"}"#.as_slice(),
            br#"{"status":"idle"}"#.as_slice(),
        ] {
            idx.insert_key(b"user.1", value);
        }
        assert_eq!(idx.len(), 1);
        assert!(idx.lookup(br#""active""#).is_empty());
        assert_eq!(idx.lookup(br#""idle""#), vec![b"user.1".to_vec()]);
    }

    #[test]
    fn shared_values_preserve_insertion_order() {
        let idx = SecondaryIndex::build(
            "status",
            vec![
                (b"user.2".to_vec(), br#"{"status":"active"}"#.to_vec()),
                (b"user.1".to_vec(), br#"{"status":"active"}"#.to_vec()),
                (b"user.3".to_vec(), br#"{"status":"active"}"#.to_vec()),
            ]
            .into_iter(),
        );
        assert_eq!(
            idx.lookup(br#""active""#),
            vec![b"user.2".to_vec(), b"user.1".to_vec(), b"user.3".to_vec()]
        );
    }

    #[test]
    fn remove_last_bucket_entry() {
        let mut idx = SecondaryIndex::build(
            "status",
            vec![(b"user.1".to_vec(), br#"{"status":"active"}"#.to_vec())].into_iter(),
        );
        idx.remove_key(b"user.1");
        assert_eq!(idx.len(), 0);
        assert!(idx.tree.is_empty());
        assert!(idx.lookup(br#""active""#).is_empty());
        idx.insert_key(b"user.1", br#"{"status":"active"}"#);
        assert_eq!(idx.lookup(br#""active""#), vec![b"user.1".to_vec()]);
    }

    #[test]
    fn scalar_and_nested_values() {
        let mut idx = SecondaryIndex::new("profile.age");
        idx.insert_key(b"a", br#"{"profile":{"age":null}}"#);
        idx.insert_key(b"b", br#"{"profile":{"age":true}}"#);
        idx.insert_key(b"c", br#"{"profile":{"age":"30"}}"#);
        idx.insert_key(b"d", br#"{"profile":{"age":30}}"#);
        assert_eq!(idx.len(), 4);
        assert_eq!(idx.lookup(br#"null"#), vec![b"a".to_vec()]);
        assert_eq!(idx.lookup(br#"true"#), vec![b"b".to_vec()]);
        assert_eq!(idx.lookup(br#""30""#), vec![b"c".to_vec()]);
        assert_eq!(idx.lookup(br#"30"#), vec![b"d".to_vec()]);
    }

    #[test]
    fn key_removed_then_reinserted() {
        let mut idx = SecondaryIndex::new("status");
        idx.insert_key(b"user.1", br#"{"status":"active"}"#);
        idx.remove_key(b"user.1");
        assert_eq!(idx.len(), 0);
        idx.insert_key(b"user.1", br#"{"status":"idle"}"#);
        assert_eq!(idx.lookup(br#""active""#), Vec::<Vec<u8>>::new());
        assert_eq!(idx.lookup(br#""idle""#), vec![b"user.1".to_vec()]);
    }
}
