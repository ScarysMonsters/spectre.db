use crate::crypto;
use crate::formats::{self, Compression};
use crate::store::Store;

pub struct CompactJob {
    pub legacy: bool,
    pub full: bool,
    pub generation: u64,
    pub seg_id: u64,
    pub base: String,
    pub start_lsn: u64,
    pub dirty_keys: Vec<Vec<u8>>,
    pub dirty_all: bool,
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
    pub start_lsn: u64,
    pub dirty_keys: Vec<Vec<u8>>,
    pub dirty_all: bool,
}

impl CompactJob {
    pub fn legacy() -> Self {
        Self {
            legacy: true,
            full: false,
            generation: 0,
            seg_id: 0,
            base: String::new(),
            start_lsn: 0,
            dirty_keys: Vec::new(),
            dirty_all: false,
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
        start_lsn: u64,
        dirty_keys: Vec<Vec<u8>>,
        dirty_all: bool,
    ) -> Self {
        Self {
            legacy: false,
            full: false,
            generation,
            seg_id,
            base,
            start_lsn,
            dirty_keys,
            dirty_all,
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
        start_lsn: u64,
        dirty_keys: Vec<Vec<u8>>,
        dirty_all: bool,
    ) -> Self {
        Self {
            legacy: false,
            full: true,
            generation,
            seg_id: 0,
            base,
            start_lsn,
            dirty_keys,
            dirty_all,
            scratch: Some(scratch),
            compression,
            enc_key,
        }
    }


    pub fn compute(mut self) -> crate::error::Result<CompactJobDone> {
        if self.legacy {
            return Ok(CompactJobDone {
                legacy: true,
                full: false,
                generation: 0,
                seg_id: 0,
                name: String::new(),
                bytes: Vec::new(),
                entries: 0,
                crc: 0,
                start_lsn: 0,
                dirty_keys: Vec::new(),
                dirty_all: false,
            });
        }
        let scratch = self.scratch.take().unwrap_or_default();
        let bytes = formats::try_write_snapshot_ex(&scratch, self.compression, self.enc_key.as_ref())?;
        let crc = crc32fast::hash(&bytes);
        let name = if self.full {
            format!("{}.spseg-full-{}", self.base, self.generation)
        } else {
            format!("{}.spseg-{}", self.base, self.seg_id)
        };
        Ok(CompactJobDone {
            legacy: false,
            full: self.full,
            generation: self.generation,
            seg_id: if self.full { self.generation } else { self.seg_id },
            name,
            bytes,
            entries: scratch.len() as u64,
            crc,
            start_lsn: self.start_lsn,
            dirty_keys: self.dirty_keys,
            dirty_all: self.dirty_all,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_compute_produces_loadable_segment() {
        let mut s = Store::new();
        s.set(b"k", b"1".to_vec().into_boxed_slice());
        let job = CompactJob::delta(s, Compression::Zstd, None, 7, 3, "db".into(), 0, Vec::new(), false);
        let done = job.compute().unwrap();
        assert_eq!(done.name, "db.spseg-3");
        assert_eq!(done.generation, 7);
        let mut back = formats::load_snapshot(&done.bytes, None).unwrap();
        assert_eq!(back.get(b"k"), Some(b"1".to_vec()));
    }
}
