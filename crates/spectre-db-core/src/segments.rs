use crate::crypto;
use crate::formats::{self, Compression};
use crate::store::Store;

pub struct CompactJob {
    pub legacy: bool,
    pub full: bool,
    pub generation: u64,
    pub seg_id: u64,
    pub base: String,
    scratch: Option<Store>,
    compression: Compression,
    enc_key: Option<[u8; crypto::KEY_LEN]>,
}

pub struct CompactJobDone {
    pub legacy: bool,
    pub full: bool,
    pub generation: u64,
    pub seg_id: u64,
    pub name: String,
    pub bytes: Vec<u8>,
    pub entries: u64,
    pub crc: u32,
}

impl CompactJob {
    pub fn legacy() -> Self {
        Self {
            legacy: true,
            full: false,
            generation: 0,
            seg_id: 0,
            base: String::new(),
            scratch: None,
            compression: Compression::None,
            enc_key: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn delta(
        scratch: Store,
        compression: Compression,
        enc_key: Option<[u8; crypto::KEY_LEN]>,
        generation: u64,
        seg_id: u64,
        base: String,
    ) -> Self {
        Self {
            legacy: false,
            full: false,
            generation,
            seg_id,
            base,
            scratch: Some(scratch),
            compression,
            enc_key,
        }
    }

    pub fn full(
        scratch: Store,
        compression: Compression,
        enc_key: Option<[u8; crypto::KEY_LEN]>,
        generation: u64,
        base: String,
    ) -> Self {
        Self {
            legacy: false,
            full: true,
            generation,
            seg_id: 0,
            base,
            scratch: Some(scratch),
            compression,
            enc_key,
        }
    }


    pub fn compute(mut self) -> CompactJobDone {
        if self.legacy {
            return CompactJobDone {
                legacy: true,
                full: false,
                generation: 0,
                seg_id: 0,
                name: String::new(),
                bytes: Vec::new(),
                entries: 0,
                crc: 0,
            };
        }
        let scratch = self.scratch.take().unwrap_or_default();
        let bytes = formats::write_snapshot_ex(&scratch, self.compression, self.enc_key.as_ref());
        let crc = crc32fast::hash(&bytes);
        let name = if self.full {
            format!("{}.spseg-full-{}", self.base, self.generation)
        } else {
            format!("{}.spseg-{}", self.base, self.seg_id)
        };
        CompactJobDone {
            legacy: false,
            full: self.full,
            generation: self.generation,
            seg_id: if self.full { self.generation } else { self.seg_id },
            name,
            bytes,
            entries: scratch.len() as u64,
            crc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_compute_produces_loadable_segment() {
        let mut s = Store::new();
        s.set(b"k", b"1".to_vec().into_boxed_slice());
        let job = CompactJob::delta(s, Compression::Zstd, None, 7, 3, "db".into());
        let done = job.compute();
        assert_eq!(done.name, "db.spseg-3");
        assert_eq!(done.generation, 7);
        let mut back = formats::load_snapshot(&done.bytes, None).unwrap();
        assert_eq!(back.get(b"k"), Some(b"1".to_vec()));
    }
}
