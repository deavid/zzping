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

        // ASSERT: Default role is Collector
        assert!(matches!(builder.get_role(), IntentConfigRole::Collector));
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_with_collector_role() {
        setup();
        // ARRANGE
        let role = IntentConfigRole::Collector;

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
        let config_path = PathBuf::from("/var/lib/zzping/intent.ron");
        let role = IntentConfigRole::Database {
            config_file_path: config_path.clone(),
        };

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
        let config_path = PathBuf::from("/tmp/test_db_intent.ron");
        let builder = IntentConfigBuilder::new().role(IntentConfigRole::Database {
            config_file_path: config_path,
        });

        // ACT
        let addr = builder.start().expect("start failed");

        // ASSERT: Actor should start successfully
        assert!(addr.connected());
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_starts_with_collector_role() {
        setup();
        // ARRANGE
        let builder = IntentConfigBuilder::new().role(IntentConfigRole::Collector);

        // ACT
        let addr = builder.start().expect("start failed");

        // ASSERT: Actor should start successfully
        assert!(addr.connected());
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_panics_on_invalid_database_role() {
        setup();
        // ARRANGE: Database with empty path (invalid)
        let builder = IntentConfigBuilder::new().role(IntentConfigRole::Database {
            config_file_path: PathBuf::new(),
        });

        // ACT: Should return Err for invalid role
        let res = builder.start();

        // ASSERT: Should be an error
        assert!(res.is_err(), "Builder should return Err on invalid role");
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_chaining() {
        setup();
        // ARRANGE & ACT: Test method chaining
        let config_path = PathBuf::from("/tmp/test_chain_intent.ron");
        let addr = IntentConfigBuilder::new()
            .role(IntentConfigRole::Database {
                config_file_path: config_path,
            })
            .start()
            .expect("start failed");

        // ASSERT
        assert!(addr.connected());
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_builder_default_trait() {
        setup();
        // ARRANGE & ACT
        let builder = IntentConfigBuilder::default();

        // ASSERT: Should use Collector role
        assert!(matches!(builder.get_role(), IntentConfigRole::Collector));
    }
}
