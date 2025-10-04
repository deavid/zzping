//! Tests for IntentConfigBuilder

#[cfg(test)]
mod tests {
    use crate::builder::IntentConfigBuilder;
    use crate::role::IntentConfigRole;
    use std::path::PathBuf;

    fn setup() {
        let _ = env_logger::builder()
            .is_test(true)
            .filter_level(log::LevelFilter::Debug)
            .try_init();
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_default() {
        setup();
        // ARRANGE & ACT
        let builder = IntentConfigBuilder::new();

        // ASSERT: Default role is Database
        assert!(matches!(builder.get_role(), IntentConfigRole::Database));
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_with_collector_role() {
        setup();
        // ARRANGE
        let config_path = PathBuf::from("/etc/intent.ron");
        let role = IntentConfigRole::Collector {
            config_file_path: config_path.clone(),
        };

        // ACT
        let builder = IntentConfigBuilder::new().role(role.clone());

        // ASSERT
        assert_eq!(builder.get_role(), &role);
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_with_database_role() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Database;

        // ACT
        let builder = IntentConfigBuilder::new().role(role.clone());

        // ASSERT
        assert_eq!(builder.get_role(), &role);
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_starts_with_database_role() {
        setup();
        // ARRANGE
        let builder = IntentConfigBuilder::new().role(IntentConfigRole::Database);

        // ACT
        let addr = builder.start();

        // ASSERT: Actor should start successfully
        assert!(addr.connected());
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_starts_with_collector_role() {
        setup();
        // ARRANGE
        let builder = IntentConfigBuilder::new().role(IntentConfigRole::Collector {
            config_file_path: "/tmp/test_intent.ron".into(),
        });

        // ACT
        let addr = builder.start();

        // ASSERT: Actor should start successfully
        assert!(addr.connected());
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_panics_on_invalid_collector_role() {
        setup();
        // ARRANGE: Collector with empty path (invalid)
        let builder = IntentConfigBuilder::new().role(IntentConfigRole::Collector {
            config_file_path: PathBuf::new(),
        });

        // ACT: Should panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            builder.start();
        }));

        // ASSERT: Should have panicked
        assert!(result.is_err(), "Builder should panic on invalid role");
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_chaining() {
        setup();
        // ARRANGE & ACT: Test method chaining
        let addr = IntentConfigBuilder::new()
            .role(IntentConfigRole::Database)
            .start();

        // ASSERT
        assert!(addr.connected());
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_default_trait() {
        setup();
        // ARRANGE & ACT
        let builder = IntentConfigBuilder::default();

        // ASSERT: Should use Database role
        assert!(matches!(builder.get_role(), IntentConfigRole::Database));
    }
}
