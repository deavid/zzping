use std::io::Write;
use tempfile::NamedTempFile;
// This function doesn't exist yet, so this won't compile until I refactor lib.rs
use zzping_collector::bootstrap_collector_with_port;

// Helper function to create a temporary config file for tests.
fn create_mock_config() -> (NamedTempFile, String) {
    let config_content = r#"
(
    collector_uuid: "lock-test-uuid",
    database_addr: "http://127.0.0.1:1", // Bogus addr, we won't connect.
    auth_token: "test-token",
)
"#;
    let mut config_file = NamedTempFile::new().unwrap();
    config_file.write_all(config_content.as_bytes()).unwrap();
    let config_path = config_file.path().to_str().unwrap().to_string();
    (config_file, config_path)
}

#[test]
// Removed #[timeout(100)]
fn test_port_lock_prevents_second_instance() {
    // 1. Setup mock config.
    let (_temp_file, config_path) = create_mock_config();

    // Use a specific port for testing the lock mechanism
    let test_port = 17879;

    // 2. Bootstrap once. This should succeed and hold the lock inside the service.
    let bootstrap_result1 =
        bootstrap_collector_with_port(config_path.clone(), Some(test_port), None);
    assert!(
        bootstrap_result1.is_ok(),
        "First bootstrap failed: {:?}",
        bootstrap_result1.err()
    );

    // Keep the service in scope. Its _lock field will hold the TCP lock.
    let _service1 = bootstrap_result1.unwrap();

    // 3. Attempt to bootstrap a second time.
    let bootstrap_result2 = bootstrap_collector_with_port(config_path, Some(test_port), None);

    // 4. Assert that the second attempt failed because the port is locked.
    assert!(
        bootstrap_result2.is_err(),
        "Second bootstrap should have failed to acquire lock, but it succeeded."
    );
    if let Err(e) = bootstrap_result2 {
        assert!(
            e.to_string().contains("Address already in use"),
            "Error message was not about the TCP port lock: {e}"
        );
    }

    // 5. _service1 goes out of scope here, dropping the service and releasing the lock.
}
