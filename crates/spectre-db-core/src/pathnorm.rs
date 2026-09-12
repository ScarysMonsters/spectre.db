use std::path::{Path, PathBuf};

pub fn strip_known_extensions(file_name: &str) -> String {
    let mut name = file_name.to_string();
    for ext in [".json.gz", ".json", ".gz", ".db", ".snapshot"] {
        if name.len() > ext.len() && name[name.len() - ext.len()..].eq_ignore_ascii_case(ext) {
            name.truncate(name.len() - ext.len());
        }
    }
    name
}

pub struct DbPaths {
    pub dir: PathBuf,
    pub base: String,
}

impl DbPaths {
    pub fn resolve(input: &Path) -> std::io::Result<Self> {
        let resolved = input.canonicalize().unwrap_or_else(|_| {

            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let p = if input.is_absolute() { input.to_path_buf() } else { cwd.join(input) };
            normalize_simple(&p)
        });
        let dir = resolved
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let file_name = resolved
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(Self { dir, base: strip_known_extensions(&file_name) })
    }

    pub fn snapshot_path(&self) -> PathBuf {
        self.dir.join(format!("{}.snapshot", self.base))
    }
    pub fn wal_path(&self) -> PathBuf {
        self.dir.join(format!("{}.wal", self.base))
    }
    pub fn spdb_path(&self) -> PathBuf {
        self.dir.join(format!("{}.spdb", self.base))
    }
    pub fn spwal_path(&self) -> PathBuf {
        self.dir.join(format!("{}.spwal", self.base))
    }
    pub fn manifest_path(&self) -> PathBuf {
        self.dir.join(format!("{}.spman", self.base))
    }
    pub fn segment_path(&self, id: u64) -> PathBuf {
        self.dir.join(format!("{}.spseg-{}", self.base, id))
    }
    pub fn lock_path(&self) -> PathBuf {

        let mut p = self.dir.join(&self.base).into_os_string();
        p.push(".lock");
        PathBuf::from(p)
    }
    pub fn backup_path(&self, gen: u32, v2: bool) -> PathBuf {
        let snap = if v2 { self.spdb_path() } else { self.snapshot_path() };
        let mut s = snap.into_os_string();
        s.push(format!(".{}.bak", gen));
        PathBuf::from(s)
    }
}

fn normalize_simple(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_extensions() {
        assert_eq!(strip_known_extensions("data.json"), "data");
        assert_eq!(strip_known_extensions("data.SNAPSHOT"), "data");
        assert_eq!(strip_known_extensions("data.db"), "data");
        assert_eq!(strip_known_extensions("data.json.gz"), "data");
        assert_eq!(strip_known_extensions("mydata"), "mydata");
        assert_eq!(strip_known_extensions("a.b.snapshot"), "a.b");
    }

    #[test]
    fn derives_paths() {
        let p = DbPaths::resolve(Path::new("/tmp/x/test.db")).unwrap();
        assert_eq!(p.snapshot_path(), PathBuf::from("/tmp/x/test.snapshot"));
        assert_eq!(p.wal_path(), PathBuf::from("/tmp/x/test.wal"));
        assert_eq!(p.spdb_path(), PathBuf::from("/tmp/x/test.spdb"));
        assert_eq!(p.spwal_path(), PathBuf::from("/tmp/x/test.spwal"));
        assert_eq!(p.lock_path(), PathBuf::from("/tmp/x/test.lock"));
        assert_eq!(p.backup_path(1, false), PathBuf::from("/tmp/x/test.snapshot.1.bak"));
        assert_eq!(p.backup_path(2, true), PathBuf::from("/tmp/x/test.spdb.2.bak"));
    }
}
