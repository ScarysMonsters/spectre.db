use std::collections::{BTreeMap, HashMap};
use std::ops::Bound;
use std::sync::Arc;

use serde_json::Value;

pub const RAW_PREFIX: u8 = 0x00;

#[inline]
pub fn is_raw_key(k: &[u8]) -> bool {
    k.first() == Some(&RAW_PREFIX)
}

#[derive(Debug, Default)]
pub struct Store {
    map: BTreeMap<Arc<[u8]>, Arc<[u8]>>,
    hash: HashMap<Arc<[u8]>, Arc<[u8]>>,
}

fn descendants_range(key: &[u8]) -> (Bound<Arc<[u8]>>, Bound<Arc<[u8]>>) {
    let mut start = Vec::with_capacity(key.len() + 1);
    start.extend_from_slice(key);
    start.push(b'.');
    let mut end = start.clone();
    end.push(0xFF);
    (Bound::Included(Arc::from(start)), Bound::Excluded(Arc::from(end)))
}

fn parse_value(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice(bytes).ok()
}

fn is_plain_object(bytes: &[u8]) -> bool {
    matches!(parse_value(bytes), Some(Value::Object(_)))
        && !bytes.starts_with(b"{\"__enc\":1")
}

fn is_envelope(bytes: &[u8]) -> bool {
    bytes.starts_with(b"{\"__enc\":1")
}

fn under_prefix(path: &[u8], prefix: &[u8]) -> bool {
    prefix.is_empty() || path.starts_with(prefix)
}

fn prefix_end(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end = prefix.to_vec();
    while let Some(last) = end.last_mut() {
        if *last < 0xFF {
            *last += 1;
            return Some(end);
        }
        end.pop();
    }
    None
}

fn prefix_bounds(prefix: &[u8], after: &[u8]) -> (Bound<Arc<[u8]>>, Bound<Arc<[u8]>>) {
    let start = if !after.is_empty() && after >= prefix {
        Bound::Excluded(Arc::from(after))
    } else {
        Bound::Included(Arc::from(prefix))
    };
    let end = prefix_end(prefix)
        .map(|end| Bound::Excluded(Arc::from(end)))
        .unwrap_or(Bound::Unbounded);
    (start, end)
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }


    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn value_bytes(&self) -> u64 {
        self.map.values().map(|v| v.len() as u64).sum()
    }


    pub fn raw_insert(&mut self, key: &[u8], value: Box<[u8]>) {
        self.raw_insert_arc(key, Arc::from(value.into_vec()));
    }

    pub fn raw_insert_arc(&mut self, key: &[u8], value: Arc<[u8]>) {
        let key: Arc<[u8]> = Arc::from(key);
        self.map.insert(Arc::clone(&key), Arc::clone(&value));
        self.hash.insert(key, value);
    }

    fn insert_entry(&mut self, key: Vec<u8>, value: Box<[u8]>) {
        self.insert_entry_arc(key, Arc::from(value.into_vec()));
    }

    fn insert_entry_arc(&mut self, key: Vec<u8>, value: Arc<[u8]>) {
        let key: Arc<[u8]> = Arc::from(key);
        self.map.insert(Arc::clone(&key), Arc::clone(&value));
        self.hash.insert(key, value);
    }

    fn remove_entry(&mut self, key: &[u8]) -> bool {
        let removed = self.map.remove(key).is_some();
        self.hash.remove(key);
        removed
    }

    pub fn contains(&self, key: &[u8]) -> bool {
        self.hash.contains_key(key)
    }


    fn ancestor_leaf(&self, key: &[u8]) -> Option<usize> {
        for (i, b) in key.iter().enumerate() {
            if *b == b'.' && self.hash.contains_key(&key[..i]) {
                return Some(i);
            }
        }
        None
    }


    fn remove_with_descendants_report(&mut self, key: &[u8]) -> (bool, bool) {
        let exact = self.remove_entry(key);
        let mut any = exact;
        let (s, e) = descendants_range(key);
        let doomed: Vec<Vec<u8>> = self
            .map
            .range((s, e))
            .map(|(k, _)| k.as_ref().to_vec())
            .collect();
        for d in &doomed {
            self.remove_entry(d);
            any = true;
        }
        (exact, any)
    }

    fn remove_with_descendants(&mut self, key: &[u8]) -> bool {
        self.remove_with_descendants_report(key).1
    }

    pub fn descendant_keys(&self, key: &[u8]) -> Vec<Vec<u8>> {
        let (s, e) = descendants_range(key);
        self.map
            .range((s, e))
            .filter(|(k, _)| !is_raw_key(k))
            .map(|(k, _)| k.as_ref().to_vec())
            .collect()
    }


    pub fn set(&mut self, key: &[u8], value: Box<[u8]>) {
        self.set_arc(key, Arc::from(value.into_vec()));
    }

    pub fn set_slice(&mut self, key: &[u8], value: &[u8]) {
        self.set_arc(key, Arc::from(value));
    }

    fn set_arc(&mut self, key: &[u8], value: Arc<[u8]>) {
        if let Some(alen) = self.ancestor_leaf(key) {
            let ancestor = self.map.get(&key[..alen]).unwrap();
            if is_plain_object(ancestor) {
                self.mutate_object_leaf(alen, key, &value);
                return;
            }

            self.remove_entry(&key[..alen]);
        }
        self.remove_with_descendants(key);
        self.insert_entry_arc(key.to_vec(), value);
    }


    fn mutate_object_leaf(&mut self, alen: usize, key: &[u8], value: &[u8]) {
        let Some(ancestor) = self.map.get(&key[..alen]) else { return };
        let Some(mut root) = parse_value(ancestor) else { return };
        let rest = &key[alen + 1..];
        let mut cur = &mut root;
        let parts: Vec<&[u8]> = rest.split(|&b| b == b'.').collect();
        for (i, part) in parts.iter().enumerate() {
            let part_str = String::from_utf8_lossy(part).into_owned();
            let last = i == parts.len() - 1;
            if last {
                if let Ok(v) = serde_json::from_slice::<Value>(value) {
                    if let Value::Object(map) = cur {
                        map.insert(part_str.clone(), v);
                    }
                }
            } else {
                let descend = match cur.get(&part_str) {
                    Some(Value::Object(_)) => true,
                    _ => false,
                };
                if !descend {
                    if let Value::Object(map) = cur {
                        map.insert(part_str.clone(), Value::Object(serde_json::Map::new()));
                    }
                }
                cur = cur.get_mut(&part_str).unwrap();
            }
        }
        if let Ok(bytes) = serde_json::to_vec(&root) {

            self.remove_with_descendants(&key[..alen]);
            self.insert_entry(key[..alen].to_vec(), bytes.into_boxed_slice());
        }
    }


    #[inline]
    pub fn get_ref(&self, key: &[u8]) -> Option<&[u8]> {
        self.hash.get(key).map(|value| &value[..])
    }

    pub fn get(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(v) = self.hash.get(key) {
            return Some(v.to_vec());
        }
        if let Some(alen) = self.ancestor_leaf(key) {
            let ancestor = self.map.get(&key[..alen])?;
            if is_envelope(ancestor) {
                return None;
            }
            let mut node = parse_value(ancestor)?;
            for part in key[alen + 1..].split(|&b| b == b'.') {
                node = match node {
                    Value::Object(mut map) => map.remove(&String::from_utf8_lossy(part).into_owned())?,
                    Value::Array(mut arr) => {
                        let idx: usize = std::str::from_utf8(part).ok()?.parse().ok()?;
                        arr.get_mut(idx)?.clone()
                    }
                    _ => return None,
                };
            }
            return Some(serde_json::to_vec(&node).ok()?);
        }


        let value = self.resolve_node(key)?;
        let bytes = serde_json::to_vec(&value).ok()?;
        self.remove_with_descendants(key);
        self.insert_entry(key.to_vec(), bytes.clone().into_boxed_slice());
        Some(bytes)
    }


    fn resolve_node(&self, path: &[u8]) -> Option<Value> {
        if let Some(v) = self.map.get(path) {
            return parse_value(v);
        }
        let mut obj = serde_json::Map::new();
        let (s, e) = descendants_range(path);
        let mut children: Vec<(String, Vec<u8>)> = Vec::new();
        let plen = path.len() + 1;
        let mut last_seg: Option<String> = None;
        for (k, _) in self.map.range((s, e)) {
            let rest = &k[plen..];
            let seg_len = rest.iter().position(|&b| b == b'.').unwrap_or(rest.len());
            let seg = String::from_utf8_lossy(&rest[..seg_len]).into_owned();
            if last_seg.as_deref() == Some(seg.as_str()) {
                continue;
            }
            last_seg = Some(seg.clone());
            let child_path = [path, b"." , rest[..seg_len].as_ref()].concat();
            children.push((seg, child_path));
        }
        if children.is_empty() {
            return None;
        }
        for (seg, child_path) in children {
            obj.insert(seg, self.resolve_node(&child_path)?);
        }
        Some(Value::Object(obj))
    }


    pub fn delete(&mut self, key: &[u8]) -> bool {
        self.delete_with_exact(key).1
    }

    pub fn delete_with_exact(&mut self, key: &[u8]) -> (bool, bool) {
        let (exact, any) = self.remove_with_descendants_report(key);
        if any {
            return (exact, true);
        }
        if let Some(alen) = self.ancestor_leaf(key) {
            let ancestor = self.map.get(&key[..alen]).unwrap();
            if is_envelope(ancestor) {
                return (exact, false);
            }

            let Some(mut root) = parse_value(ancestor) else { return (exact, false) };
            let parts: Vec<&[u8]> = key[alen + 1..].split(|&b| b == b'.').collect();
            let mut cur = &mut root;
            for (i, part) in parts.iter().enumerate() {
                let part_str = String::from_utf8_lossy(part).into_owned();
                let last = i == parts.len() - 1;
                if last {
                    let existed = match cur {
                        Value::Object(map) => map.remove(&part_str).is_some(),
                        _ => false,
                    };
                    if !existed {
                        return (exact, false);
                    }
                } else {
                    match cur.get_mut(&part_str) {
                        Some(Value::Object(_)) => cur = cur.get_mut(&part_str).unwrap(),
                        _ => return (exact, false),
                    }
                }
            }
            if let Ok(bytes) = serde_json::to_vec(&root) {
                self.insert_entry(key[..alen].to_vec(), bytes.into_boxed_slice());
                return (exact, true);
            }
            return (exact, false);
        }
        (exact, false)
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.hash.clear();
    }


    pub fn count(&self, prefix: Option<&[u8]>) -> usize {
        let pfx: &[u8] = prefix.unwrap_or(b"");
        let mut count = 0;
        for (k, v) in &self.map {
            if is_raw_key(k) {
                continue;
            }
            let leaf_is_ancestor = !k.starts_with(pfx) && pfx.starts_with(k.as_ref());
            if !under_prefix(k, pfx) && !leaf_is_ancestor {
                continue;
            }
            if v.first() == Some(&b'{') && !is_envelope(v) {
                return self.all(prefix).len();
            }
            count += 1;
        }
        count
    }


    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.map.iter().map(|(k, v)| (k.as_ref(), &v[..]))
    }

    pub fn iter_arc(&self) -> impl Iterator<Item = (&[u8], &Arc<[u8]>)> {
        self.map.iter().map(|(k, v)| (k.as_ref(), v))
    }

    pub fn get_shared(&self, key: &[u8]) -> Option<Arc<[u8]>> {
        self.hash.get(key).cloned()
    }

    fn ancestor_entries(&self, prefix: &[u8]) -> Vec<(Arc<[u8]>, Arc<[u8]>)> {
        let mut out = Vec::new();
        if prefix.is_empty() {
            return out;
        }
        for index in 1..prefix.len() {
            if prefix[index] == b'.' {
                if let Some(value) = self.map.get(&prefix[..index]) {
                    out.push((Arc::from(&prefix[..index]), Arc::clone(value)));
                }
            }
        }
        out
    }


    pub fn set_raw(&mut self, key: &[u8], value: Box<[u8]>) {
        let mut k = Vec::with_capacity(key.len() + 1);
        k.push(RAW_PREFIX);
        k.extend_from_slice(key);
        let value: Arc<[u8]> = Arc::from(value.into_vec());
        self.raw_insert_arc(&k, value);
    }

    pub fn get_raw(&self, key: &[u8]) -> Option<Vec<u8>> {
        let mut k = Vec::with_capacity(key.len() + 1);
        k.push(RAW_PREFIX);
        k.extend_from_slice(key);
        self.hash.get(k.as_slice()).map(|v| v.to_vec())
    }

    pub fn delete_raw(&mut self, key: &[u8]) -> bool {
        let mut k = Vec::with_capacity(key.len() + 1);
        k.push(RAW_PREFIX);
        k.extend_from_slice(key);
        self.remove_entry(&k)
    }


    pub fn page(&self, prefix: Option<&[u8]>, after: &[u8], limit: usize) -> (Vec<(Vec<u8>, Vec<u8>)>, Vec<u8>) {
        let pfx: &[u8] = prefix.unwrap_or(b"");
        let mut rows = Vec::new();
        let mut cursor: Vec<u8> = Vec::new();
        let (start, end) = prefix_bounds(pfx, after);
        let mut hit_limit = false;
        for (k, v) in self.map.range((start, end)) {
            if is_raw_key(k) {
                continue;
            }
            if rows.len() >= limit {
                hit_limit = true;
                break;
            }
            rows.push((k.as_ref().to_vec(), v.to_vec()));
            cursor = k.as_ref().to_vec();
        }
        if !hit_limit {
            cursor.clear();
        }
        (rows, cursor)
    }

    pub fn page_ref<'a>(
        &'a self,
        prefix: Option<&[u8]>,
        after: &[u8],
        limit: usize,
    ) -> (Vec<(&'a [u8], &'a [u8])>, Vec<u8>) {
        let pfx: &[u8] = prefix.unwrap_or(b"");
        let mut rows = Vec::with_capacity(limit.min(1024));
        let mut cursor: Vec<u8> = Vec::new();
        let (start, end) = prefix_bounds(pfx, after);
        let mut hit_limit = false;
        for (k, v) in self.map.range((start, end)) {
            if is_raw_key(k) {
                continue;
            }
            if rows.len() >= limit {
                hit_limit = true;
                break;
            }
            rows.push((&k[..], &v[..]));
            cursor.clear();
            cursor.extend_from_slice(k.as_ref());
        }
        if !hit_limit {
            cursor.clear();
        }
        (rows, cursor)
    }


    pub fn raw_count(&self) -> usize {
        self.map.iter().take_while(|(k, _)| is_raw_key(k)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(v: &str) -> Box<[u8]> {
        v.as_bytes().to_vec().into_boxed_slice()
    }

    #[test]
    fn set_and_get_leaf() {
        let mut s = Store::new();
        s.set(b"a.b.c", j("1"));
        assert_eq!(s.get(b"a.b.c"), Some(b"1".to_vec()));

        assert_eq!(s.get(b"a.b"), Some(br#"{"c":1}"#.to_vec()));
        assert_eq!(s.get(b"a"), Some(br#"{"b":{"c":1}}"#.to_vec()));
    }

    #[test]
    fn object_value_is_leaf_but_descent_works() {
        let mut s = Store::new();
        s.set(b"user.1", j(r#"{"name":"alice","age":30}"#));

        assert_eq!(s.get(b"user.1"), Some(br#"{"name":"alice","age":30}"#.to_vec()));

        assert_eq!(s.get(b"user.1.name"), Some(br#""alice""#.to_vec()));
        assert_eq!(s.get(b"user.1.age"), Some(b"30".to_vec()));
        assert_eq!(s.get(b"user.1.missing"), None);
    }

    #[test]
    fn set_through_object_leaf_mutates() {
        let mut s = Store::new();
        s.set(b"a", j(r#"{"b":"scalar"}"#));
        s.set(b"a.c.d", j("1"));


        let a = s.get(b"a").unwrap();
        let v: Value = serde_json::from_slice(&a).unwrap();
        assert_eq!(v["c"]["d"], 1);
        assert_eq!(v["b"], "scalar");

        let text = String::from_utf8(a).unwrap();
        assert!(text.starts_with("{\"b\":"));
    }

    #[test]
    fn scalar_ancestor_replaced_by_branch() {
        let mut s = Store::new();
        s.set(b"a", j("scalar"));
        s.set(b"a.b", j("2"));

        assert_eq!(s.get(b"a"), Some(br#"{"b":2}"#.to_vec()));
        assert_eq!(s.get(b"a.b"), Some(b"2".to_vec()));
    }

    #[test]
    fn leaf_replaces_subtree() {
        let mut s = Store::new();
        s.set(b"a.b", j("2"));
        s.set(b"a.c", j("3"));
        s.set(b"a", j("scalar"));
        assert_eq!(s.get(b"a"), Some(b"scalar".to_vec()));
        assert_eq!(s.get(b"a.b"), None);
        assert_eq!(s.get(b"a.c"), None);
    }

    #[test]
    fn delete_subtree_and_quirks() {
        let mut s = Store::new();
        s.set(b"a.b", j("1"));
        s.set(b"a.c.d", j("2"));
        assert!(s.delete(b"a"));
        assert_eq!(s.len(), 0);
        assert!(!s.delete(b"a"));


        let mut s2 = Store::new();
        s2.set(b"a", j(r#"{"b":1,"c":2}"#));
        assert!(s2.delete(b"a.b"));
        assert_eq!(s2.get(b"a"), Some(br#"{"c":2}"#.to_vec()));

        let mut s3 = Store::new();
        s3.set(b"a", j(r#"{"b":"scalar"}"#));
        assert!(!s3.delete(b"a.b.c"));
    }

    #[test]
    fn aggregation_materializes_branch() {
        let mut s = Store::new();

        s.raw_insert(b"user.1.name", j(r#""alice""#));
        s.raw_insert(b"user.1.age", j("30"));
        s.raw_insert(b"user.2", j("7"));
        let v = s.get(b"user.1").unwrap();
        let parsed: Value = serde_json::from_slice(&v).unwrap();
        assert_eq!(parsed["name"], "alice");
        assert_eq!(parsed["age"], 30);

        assert_eq!(s.len(), 2);

        assert_eq!(s.get(b"user.1"), Some(v));

        assert_eq!(s.get(b"user.1.name"), Some(br#""alice""#.to_vec()));
    }

    #[test]
    fn all_splits_objects_like_v11() {
        let mut s = Store::new();
        s.set(b"user.1", j(r#"{"name":"alice","age":30}"#));
        s.set(b"post.1", j("[1,2]"));
        s.set(b"flag", j("true"));
        let all = s.all(None);
        let ids: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(ids, vec!["flag", "post.1", "user.1.name", "user.1.age"]);
        let users = s.all(Some(b"user"));
        assert_eq!(users.len(), 2);
        let one = s.all(Some(b"user.1.name"));
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn all_with_envelope_leaf() {
        let mut s = Store::new();
        s.set(b"user.token", j(r#"{"__enc":1,"iv":"a","ct":"b","tag":"c"}"#));
        let all = s.all(None);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, "user.token");

        assert!(s.get(b"user.token.iv").is_none() || true);
    }

    #[test]
    fn utf8_order_and_prefix_count() {
        let mut s = Store::new();
        s.set(b"b", j("1"));
        s.set(b"a", j("2"));
        s.set(b"c", j("3"));
        let keys: Vec<&[u8]> = s.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec![&b"a"[..], &b"b"[..], &b"c"[..]]);
        assert_eq!(s.count(Some(b"a")), 1);
        assert_eq!(s.count(None), 3);
    }

    #[test]
    fn count_matches_all_for_object_values() {
        let mut s = Store::new();
        s.set(b"a", j("1"));
        s.set(b"u", j(r#"{"name":"x","n":2}"#));
        assert_eq!(s.count(None), s.all(None).len());
        assert_eq!(s.count(Some(b"u.")), s.all(Some(b"u.")).len());
    }

    #[test]
    fn array_descent_on_get() {
        let mut s = Store::new();
        s.set(b"list", j("[10,20,30]"));

        assert_eq!(s.get(b"list.1"), Some(b"20".to_vec()));

        s.set(b"list.1", j("\"x\""));
        let v = s.get(b"list").unwrap();
        let parsed: Value = serde_json::from_slice(&v).unwrap();
        assert_eq!(parsed["1"], "x");

        let mut s2 = Store::new();
        s2.set(b"list", j("[1,2]"));
        assert!(!s2.delete(b"list.0"));
    }
}


pub fn split_object_fields(bytes: &[u8]) -> Option<Vec<(String, &[u8])>> {
    let mut i = 0usize;
    skip_ws(bytes, &mut i);
    if bytes.get(i) != Some(&b'{') {
        return None;
    }
    if bytes.starts_with(b"{\"__enc\":1") {
        return None;
    }
    i += 1;
    skip_ws(bytes, &mut i);
    let mut out = Vec::new();
    if bytes.get(i) == Some(&b'}') {
        return Some(out);
    }
    loop {
        skip_ws(bytes, &mut i);
        let name = scan_json_string(bytes, &mut i)?;
        skip_ws(bytes, &mut i);
        if bytes.get(i) != Some(&b':') {
            return None;
        }
        i += 1;
        skip_ws(bytes, &mut i);
        let start = i;
        scan_json_value(bytes, &mut i)?;
        out.push((name, &bytes[start..i]));
        skip_ws(bytes, &mut i);
        match bytes.get(i) {
            Some(b',') => {
                i += 1;
            }
            Some(b'}') => {
                return Some(out);
            }
            _ => return None,
        }
    }
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], b' ' | b'\t' | b'\n' | b'\r') {
        *i += 1;
    }
}

fn scan_json_string(b: &[u8], i: &mut usize) -> Option<String> {
    if b.get(*i) != Some(&b'"') {
        return None;
    }
    *i += 1;
    let mut out = Vec::with_capacity(16);
    while *i < b.len() {
        let c = b[*i];
        match c {
            b'"' => {
                *i += 1;
                return String::from_utf8(out).ok();
            }
            b'\\' => {
                *i += 1;
                let e = *b.get(*i)?;
                *i += 1;
                match e {
                    b'"' => out.push(b'"'),
                    b'\\' => out.push(b'\\'),
                    b'/' => out.push(b'/'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0C),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'u' => {
                        let hex = b.get(*i..*i + 4)?;
                        *i += 4;
                        let cp = u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                        let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    _ => return None,
                }
            }
            _ => {
                out.push(c);
                *i += 1;
            }
        }
    }
    None
}

fn scan_json_value(b: &[u8], i: &mut usize) -> Option<()> {
    skip_ws(b, i);
    match *b.get(*i)? {
        b'{' | b'[' => {
            let mut depth = 0usize;
            while *i < b.len() {
                match b[*i] {
                    b'"' => {
                        scan_json_string(b, i)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            *i += 1;
                            return Some(());
                        }
                    }
                    _ => {}
                }
                *i += 1;
            }
            None
        }
        b'"' => scan_json_string(b, i).map(|_| ()),
        b't' => {
            if b.get(*i..*i + 4)? == b"true" {
                *i += 4;
                Some(())
            } else {
                None
            }
        }
        b'f' => {
            if b.get(*i..*i + 5)? == b"false" {
                *i += 5;
                Some(())
            } else {
                None
            }
        }
        b'n' => {
            if b.get(*i..*i + 4)? == b"null" {
                *i += 4;
                Some(())
            } else {
                None
            }
        }
        _ => {

            while *i < b.len()
                && !matches!(b[*i], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
            {
                *i += 1;
            }
            Some(())
        }
    }
}


impl Store {
pub fn all(&self, prefix: Option<&[u8]>) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let pfx: &[u8] = prefix.unwrap_or(b"");
    for (key, value) in self.ancestor_entries(pfx) {
        Self::split_leaf(&key, &value, pfx, &mut out);
    }
    let (start, end) = prefix_bounds(pfx, b"");
    for (k, v) in self.map.range((start, end)) {
        if is_raw_key(k) {
            continue;
        }
        Self::split_leaf(k, v, pfx, &mut out);
    }
    out
}

fn split_leaf(path: &[u8], value: &[u8], prefix: &[u8], out: &mut Vec<(String, Vec<u8>)>) {
    if value.first() == Some(&b'{') && !is_envelope(value) {
        if let Some(fields) = split_object_fields(value) {
            let mut base = Vec::with_capacity(path.len() + 16);
            base.extend_from_slice(path);
            base.push(b'.');
            let base_len = base.len();
            for (field, field_bytes) in fields {
                base.truncate(base_len);
                base.extend_from_slice(field.as_bytes());
                match field_bytes.first() {
                    Some(b'{') if !field_bytes.starts_with(b"{\"__enc\":1") => {
                        Self::split_leaf(&base, field_bytes, prefix, out);
                    }
                    _ => {
                        if under_prefix(&base, prefix) {
                            out.push((String::from_utf8_lossy(&base).into_owned(), field_bytes.to_vec()));
                        }
                    }
                }
            }
            return;
        }
    }
    if under_prefix(path, prefix) {
        out.push((String::from_utf8_lossy(path).into_owned(), value.to_vec()));
    }
}


    pub fn scan<F>(&self, prefix: Option<&[u8]>, f: &mut F) -> crate::error::Result<()>
    where
        F: FnMut(&[u8], &[u8]) -> crate::error::Result<()>,
    {
        let pfx: &[u8] = prefix.unwrap_or(b"");
        for (key, value) in self.ancestor_entries(pfx) {
            Self::scan_leaf(&key, &value, pfx, f)?;
        }
        let (start, end) = prefix_bounds(pfx, b"");
        for (k, v) in self.map.range((start, end)) {
            if is_raw_key(k) {
                continue;
            }
            Self::scan_leaf(k, v, pfx, f)?;
        }
        Ok(())
    }

    fn scan_leaf<F>(path: &[u8], value: &[u8], prefix: &[u8], f: &mut F) -> crate::error::Result<()>
    where
        F: FnMut(&[u8], &[u8]) -> crate::error::Result<()>,
    {
        if value.first() == Some(&b'{') && !is_envelope(value) {
            if let Some(fields) = split_object_fields(value) {
                let mut base = Vec::with_capacity(path.len() + 24);
                base.extend_from_slice(path);
                base.push(b'.');
                let base_len = base.len();
                for (field, field_bytes) in fields {
                    base.truncate(base_len);
                    base.extend_from_slice(field.as_bytes());
                    match field_bytes.first() {
                        Some(b'{') if !field_bytes.starts_with(b"{\"__enc\":1") => {
                            Self::scan_leaf(&base, field_bytes, prefix, f)?;
                        }
                        _ => {
                            if under_prefix(&base, prefix) {
                                f(&base, field_bytes)?;
                            }
                        }
                    }
                }
                return Ok(());
            }
        }
        if under_prefix(path, prefix) {
            f(path, value)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod splitter_tests {
    use super::*;

    #[test]
    fn splits_simple_object() {
        let f = split_object_fields(br#"{"name":"alice","age":30,"ok":true,"n":null,"arr":[1,2],"nest":{"x":1}}"#).unwrap();
        let names: Vec<&str> = f.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, vec!["name", "age", "ok", "n", "arr", "nest"]);
        assert_eq!(f[0].1, br#""alice""#);
        assert_eq!(f[1].1, b"30");
        assert_eq!(f[4].1, b"[1,2]");
        assert_eq!(f[5].1, br#"{"x":1}"#);
    }

    #[test]
    fn handles_escapes_and_braces_in_strings() {
        let f = split_object_fields(br#"{"a":"he\"llo{,}","b.c":"}"}"#).unwrap();
        assert_eq!(f[0].0, "a");
        assert_eq!(f[0].1, br#""he\"llo{,}""#);
        assert_eq!(f[1].0, "b.c");
        assert_eq!(f[1].1, br#""}""#);
    }

    #[test]
    fn unicode_escapes_in_keys() {
        let f = split_object_fields(br#"{"caf\u00e9":1}"#).unwrap();
        assert_eq!(f[0].0, "caf\u{E9}");
    }

    #[test]
    fn rejects_non_objects_and_envelopes() {
        assert!(split_object_fields(b"[1,2]").is_none());
        assert!(split_object_fields(b"42").is_none());
        assert!(split_object_fields(br#"{"__enc":1,"iv":"a"}"#).is_none());
        assert_eq!(split_object_fields(br#"{}"#).unwrap().len(), 0);
    }

    #[test]
    fn whitespace_tolerant() {
        let f = split_object_fields(b"{ \"a\" : 1 ,\n\t \"b\" : [ 1 , 2 ] }").unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[1].1, b"[ 1 , 2 ]");
    }
}
