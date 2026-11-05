use std::fmt;

pub mod codes {

    pub const INVALID_KEY: i32 = 1000;
    pub const KEY_TOO_LONG: i32 = 1001;
    pub const KEY_EMPTY_SEGMENT: i32 = 1002;
    pub const KEY_FORBIDDEN_SEGMENT: i32 = 1003;
    pub const KEY_CONTROL_CHARS: i32 = 1004;
    pub const KEY_INVISIBLE_CHARS: i32 = 1005;

    pub const VALUE_TOO_LARGE: i32 = 1101;
    pub const CIRCULAR_REFERENCE: i32 = 1102;
    pub const UNSUPPORTED_TYPE: i32 = 1103;


    pub const PATH_TRAVERSAL: i32 = 2000;
    pub const INVALID_PATH: i32 = 2001;


    pub const SNAPSHOT_CORRUPTED: i32 = 3000;
    pub const WAL_CORRUPTED: i32 = 3001;
    pub const BACKUP_CORRUPTED: i32 = 3002;
    pub const WRITE_FAILED: i32 = 3003;
    pub const READ_FAILED: i32 = 3004;
    pub const FILE_LOCKED: i32 = 3005;


    pub const ENCRYPTION_FAILED: i32 = 4000;
    pub const DECRYPTION_FAILED: i32 = 4001;
    pub const ENC_INVALID_KEY: i32 = 4002;
    pub const KEY_DERIVATION_FAILED: i32 = 4003;


    pub const TRANSACTION_COMMIT_FAILED: i32 = 5003;


    pub const LOCK_ACQUISITION_FAILED: i32 = 7000;
    pub const LOCK_TIMEOUT: i32 = 7001;


    pub const DATABASE_CLOSED: i32 = 8000;
    pub const DATABASE_NOT_READY: i32 = 8001;


    pub const OPERATION_FAILED: i32 = 9000;
    pub const UNKNOWN_OPERATION: i32 = 9002;
}

#[derive(Debug, Clone)]
pub struct SpectreError {
    pub code: i32,
    pub message: String,
}

impl SpectreError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

    pub fn invalid_key(msg: impl Into<String>) -> Self {
        Self::new(codes::INVALID_KEY, msg)
    }
    pub fn write_failed(msg: impl Into<String>) -> Self {
        Self::new(codes::WRITE_FAILED, msg)
    }
    pub fn snapshot_corrupted(msg: impl Into<String>) -> Self {
        Self::new(codes::SNAPSHOT_CORRUPTED, msg)
    }
    pub fn wal_corrupted(msg: impl Into<String>) -> Self {
        Self::new(codes::WAL_CORRUPTED, msg)
    }
    pub fn lock_timeout(msg: impl Into<String>) -> Self {
        Self::new(codes::LOCK_TIMEOUT, msg)
    }
    pub fn lock_failed(msg: impl Into<String>) -> Self {
        Self::new(codes::LOCK_ACQUISITION_FAILED, msg)
    }
    pub fn closed() -> Self {
        Self::new(codes::DATABASE_CLOSED, "Database is closed")
    }
    pub fn operation_failed(msg: impl Into<String>) -> Self {
        Self::new(codes::OPERATION_FAILED, msg)
    }
    pub fn io(err: std::io::Error, what: &str) -> Self {
        Self::write_failed(format!("{}: {}", what, err))
    }
}

impl fmt::Display for SpectreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[spectre.db code={}]: {}", self.code, self.message)
    }
}

impl std::error::Error for SpectreError {}

pub type Result<T> = std::result::Result<T, SpectreError>;
