use crate::error::{codes, Result, SpectreError};
use unicode_normalization::UnicodeNormalization;

pub const MAX_KEY_LENGTH: usize = 1000;
pub const MAX_VALUE_SIZE: usize = 10 * 1024 * 1024;

const FORBIDDEN_SEGMENTS: [&str; 3] = ["__proto__", "constructor", "prototype"];

pub fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    for prefix in ["password", "secret", "token", "apikey", "api_key", "private"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if rest.is_empty() || rest.starts_with('.') || rest.starts_with('_') {
                return true;
            }
        }
    }
    false
}

pub fn validate_key(key: &str) -> Result<Vec<String>> {
    if key.is_empty() {
        return Err(SpectreError::invalid_key("Key must be a non-empty string"));
    }
    if key.chars().count() > MAX_KEY_LENGTH {
        return Err(SpectreError::new(
            codes::KEY_TOO_LONG,
            format!("Key too long: {} chars (max: {})", key.chars().count(), MAX_KEY_LENGTH),
        ));
    }

    let normalized: String = key.nfc().collect();

    for ch in normalized.chars() {
        if ch <= '\u{1F}' || ch == '\u{7F}' {
            return Err(SpectreError::new(codes::KEY_CONTROL_CHARS, "Key contains control characters"));
        }
        if matches!(ch, '\u{200B}'..='\u{200D}' | '\u{FEFF}' | '\u{2028}' | '\u{2029}') {
            return Err(SpectreError::new(codes::KEY_INVISIBLE_CHARS, "Key contains invisible characters"));
        }
    }

    let mut parts = Vec::new();
    for part in normalized.split('.') {
        if part.is_empty() {
            return Err(SpectreError::new(
                codes::KEY_EMPTY_SEGMENT,
                format!("Key contains an empty segment: \"{}\"", key),
            ));
        }
        if FORBIDDEN_SEGMENTS.contains(&part) {
            return Err(SpectreError::new(
                codes::KEY_FORBIDDEN_SEGMENT,
                format!("Forbidden key segment \"{}\" in key: \"{}\"", part, key),
            ));
        }
        parts.push(part.to_string());
    }
    Ok(parts)
}

pub fn validate_value_size(serialized_len: usize) -> Result<()> {
    if serialized_len > MAX_VALUE_SIZE {
        return Err(SpectreError::new(
            codes::VALUE_TOO_LARGE,
            format!("Value too large: {} bytes (max: {})", serialized_len, MAX_VALUE_SIZE),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_validation() {
        assert!(validate_key("user.profile.name").is_ok());
        assert!(validate_key("").is_err());
        assert!(validate_key("a..b").is_err());
        assert!(validate_key("__proto__.x").is_err());
        assert!(validate_key("a\u{0000}b").is_err());
        assert!(validate_key(&"k".repeat(1001)).is_err());
    }

    #[test]
    fn sensitive_keys() {

        assert!(is_sensitive_key("password"));
        assert!(is_sensitive_key("password.holder"));
        assert!(is_sensitive_key("API_KEY.id"));
        assert!(is_sensitive_key("token"));
        assert!(!is_sensitive_key("user.password"));
        assert!(!is_sensitive_key("passwords"));
        assert!(!is_sensitive_key("username"));
        assert!(!is_sensitive_key("tokenBucket"));
    }

    #[test]
    fn nfc_normalization() {

        let parts = validate_key("caf\u{65}\u{301}.name").unwrap();
        assert_eq!(parts[0], "caf\u{E9}");
    }
}
