//! Tests for ACL config loading and basic error handling.
//!
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use tempfile::NamedTempFile;
use zznet_auth::config::AclConfig;
use zznet_auth::error::AuthError;

#[test]
fn missing_acl_file_returns_error() {
    let res = AclConfig::from_file("nonexistent_file.toml");
    assert!(res.is_err());
}

#[test]
fn invalid_toml_returns_error() {
    let mut tf = NamedTempFile::new().unwrap();
    write!(tf, "not a toml = [}})").unwrap();
    let res = AclConfig::from_file(tf.path());
    assert!(matches!(res.unwrap_err(), AuthError::Toml(_)));
}

#[test]
fn permission_denied_acl_file_returns_error() {
    let tf = NamedTempFile::new().unwrap();
    let path = tf.path().to_path_buf();
    // Write valid content first
    fs::write(
        &path,
        r#"
allowed_peers = ["user@client-ro"]
"#,
    )
    .unwrap();

    // Remove read permission
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o200); // write only
    fs::set_permissions(&path, perms).unwrap();

    let res = AclConfig::from_file(&path);
    assert!(res.is_err());
    // Should be some IO error
    assert!(matches!(res.unwrap_err(), AuthError::Io(_)));
}

#[test]
fn malformed_acl_entries_return_error() {
    let mut tf = NamedTempFile::new().unwrap();
    write!(
        tf,
        r#"
allowed_peers = ["", "user@client-ro"]
"#
    )
    .unwrap();
    let res = AclConfig::from_file(tf.path());
    // Empty entry should cause validation error
    assert!(res.is_ok()); // from_file doesn't validate
    let config = res.unwrap();
    assert!(config.validate().is_err());
}

#[test]
fn empty_acl_config_validates() {
    let mut tf = NamedTempFile::new().unwrap();
    write!(
        tf,
        r#"
allowed_peers = []
"#
    )
    .unwrap();
    let res = AclConfig::from_file(tf.path());
    // Empty ACL should be valid (deny all)
    assert!(res.is_ok());
}

#[test]
fn duplicate_acl_entries_handled() {
    let mut tf = NamedTempFile::new().unwrap();
    write!(
        tf,
        r#"
allowed_peers = ["user@client-ro", "user@client-ro", "admin@client-admin"]
"#
    )
    .unwrap();
    let res = AclConfig::from_file(tf.path());
    // from_file loads duplicates
    assert!(res.is_ok());
    let config = res.unwrap();
    // But validate should reject duplicates
    assert!(config.validate().is_err());
}

#[test]
fn config_with_extra_unknown_fields_ignored() {
    let mut tf = NamedTempFile::new().unwrap();
    write!(
        tf,
        r#"
allowed_peers = ["user@client-ro"]
unknown_field = "ignored"
"#
    )
    .unwrap();
    let res = AclConfig::from_file(tf.path());
    // Unknown fields should be ignored
    assert!(res.is_ok());
}

#[test]
fn config_validation_missing_required_section() {
    let mut tf = NamedTempFile::new().unwrap();
    write!(
        tf,
        r#"
# Empty TOML file
"#
    )
    .unwrap();
    let res = AclConfig::from_file(tf.path());
    // Empty file should default to empty allowed_peers
    match res {
        Ok(config) => assert!(config.allowed_peers.is_empty()),
        Err(e) => panic!("Unexpected error: {:?}", e),
    }
}

#[test]
fn config_with_invalid_array_type() {
    let mut tf = NamedTempFile::new().unwrap();
    write!(
        tf,
        r#"
allowed_peers = "not_an_array"
"#
    )
    .unwrap();
    let res = AclConfig::from_file(tf.path());
    // Type mismatch should cause TOML error
    assert!(res.is_err());
    assert!(matches!(res.unwrap_err(), AuthError::Toml(_)));
}
