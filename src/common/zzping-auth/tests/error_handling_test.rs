//! Error handling tests moved to application crate.
//!
use std::io::Write;
use tempfile::NamedTempFile;
use zzping_auth::config::AclConfig;
use zzping_auth::error::AuthError;

#[test]
fn missing_acl_file_returns_error_moved() {
    let res = AclConfig::from_file("nonexistent_file.toml");
    assert!(res.is_err());
}

#[test]
fn invalid_toml_returns_error_moved() {
    let mut tf = NamedTempFile::new().unwrap();
    write!(tf, "not a toml = [}})").unwrap();
    let res = AclConfig::from_file(tf.path());
    assert!(matches!(res.unwrap_err(), AuthError::Toml(_)));
}
