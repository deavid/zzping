//! Tests for IntentConfigApi

#[cfg(test)]
mod tests {
    use crate::api::IntentConfigApi;
    use crate::builder::IntentConfigBuilder;
    use crate::messages::IntentConfigData;
    use crate::role::IntentConfigRole;
    use actix::prelude::*;
    use std::time::Duration;

    fn setup() {
        let _ = env_logger::builder()
            .is_test(true)
            .filter_level(log::LevelFilter::Debug)
            .try_init();
    }

    /// Mock actor to receive config updates
    struct MockReceiver {
        tx: tokio::sync::mpsc::Sender<IntentConfigData>,
    }

    impl Actor for MockReceiver {
        type Context = Context<Self>;
    }

    impl Handler<IntentConfigData> for MockReceiver {
        type Result = ();
        fn handle(&mut self, msg: IntentConfigData, _ctx: &mut Context<Self>) -> Self::Result {
            self.tx.try_send(msg).ok();
        }
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_api_update_config() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let addr = IntentConfigBuilder::new()
            .role(IntentConfigRole::Database {
                config_file_path: config_path,
            })
            .start()
            .expect("start failed");

        let new_config = IntentConfigData {
            targets: vec!["1.1.1.1".parse().unwrap()],
            ping_rate_pps: 42,
        };

        // ACT: Use the API to update config
        addr.update_config(new_config.clone());

        // Give actor time to process
        tokio::time::sleep(Duration::from_millis(10)).await;

        // ASSERT: No panic means success (fire-and-forget operation)
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_api_subscribe() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let addr = IntentConfigBuilder::new()
            .role(IntentConfigRole::Database {
                config_file_path: config_path,
            })
            .start()
            .expect("start failed");

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let mock = MockReceiver { tx }.start();

        // ACT: Subscribe via API
        let sub_id = addr.subscribe(mock.recipient()).await;

        // ASSERT: Should get subscription ID
        assert!(sub_id.is_ok());
        let sub_id = sub_id.unwrap();
        assert_eq!(sub_id, 0); // First subscriber gets ID 0

        // Should receive initial config
        let received = tokio::time::timeout(Duration::from_millis(10), rx.recv())
            .await
            .expect("Should receive initial config")
            .unwrap();
        assert_eq!(received, IntentConfigData::default());
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_api_subscribe_multiple() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let addr = IntentConfigBuilder::new()
            .role(IntentConfigRole::Database {
                config_file_path: config_path,
            })
            .start()
            .expect("start failed");

        let (tx1, mut rx1) = tokio::sync::mpsc::channel(10);
        let mock1 = MockReceiver { tx: tx1 }.start();

        let (tx2, mut rx2) = tokio::sync::mpsc::channel(10);
        let mock2 = MockReceiver { tx: tx2 }.start();

        // ACT: Subscribe two receivers
        let sub_id1 = addr.subscribe(mock1.recipient()).await.unwrap();
        let sub_id2 = addr.subscribe(mock2.recipient()).await.unwrap();

        // ASSERT: Different IDs
        assert_eq!(sub_id1, 0);
        assert_eq!(sub_id2, 1);

        // Both receive initial config
        rx1.recv().await.unwrap();
        rx2.recv().await.unwrap();
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_api_unsubscribe() {
        setup();
        // ARRANGE: Test that unsubscribe doesn't crash the actor
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let addr = IntentConfigBuilder::new()
            .role(IntentConfigRole::Database {
                config_file_path: config_path,
            })
            .start()
            .expect("start failed");

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let mock = MockReceiver { tx }.start();

        let sub_id = addr.subscribe(mock.recipient()).await.unwrap();

        // Drain initial config
        let _ = rx.recv().await.unwrap();

        // ACT: Unsubscribe
        addr.unsubscribe(sub_id);

        tokio::time::sleep(Duration::from_millis(10)).await;

        // ASSERT: Actor should still be responsive
        // Send another subscribe to verify actor is still working
        let (tx2, mut rx2) = tokio::sync::mpsc::channel(10);
        let mock2 = MockReceiver { tx: tx2 }.start();
        let sub_id2 = addr.subscribe(mock2.recipient()).await.unwrap();
        assert_eq!(sub_id2, 1); // Should get next ID

        // Verify new subscriber receives initial config
        let received = rx2.recv().await.unwrap();
        assert_eq!(received, IntentConfigData::default());
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_api_subscribe_receives_updates() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let addr = IntentConfigBuilder::new()
            .role(IntentConfigRole::Database {
                config_file_path: config_path,
            })
            .start()
            .expect("start failed");

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let mock = MockReceiver { tx }.start();

        addr.subscribe(mock.recipient()).await.unwrap();
        rx.recv().await.unwrap(); // Drain initial

        // ACT: Send update via API
        let new_config = IntentConfigData {
            targets: vec!["2.2.2.2".parse().unwrap()],
            ping_rate_pps: 22,
        };
        addr.update_config(new_config.clone());

        // ASSERT: Subscriber receives update
        let received = tokio::time::timeout(Duration::from_millis(10), rx.recv())
            .await
            .expect("Should receive update")
            .unwrap();
        assert_eq!(received, new_config);
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_api_multiple_updates() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let addr = IntentConfigBuilder::new()
            .role(IntentConfigRole::Database {
                config_file_path: config_path,
            })
            .start()
            .expect("start failed");

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let mock = MockReceiver { tx }.start();

        addr.subscribe(mock.recipient()).await.unwrap();
        rx.recv().await.unwrap(); // Drain initial

        // ACT: Send multiple updates
        for i in 1..=3 {
            let config = IntentConfigData {
                targets: vec![format!("{}.{}.{}.{}", i, i, i, i).parse().unwrap()],
                ping_rate_pps: i as u64 * 10,
            };
            addr.update_config(config.clone());

            // ASSERT: Receive each update
            let received = tokio::time::timeout(Duration::from_millis(10), rx.recv())
                .await
                .expect("Should receive update")
                .unwrap();
            assert_eq!(received, config);
        }
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_api_unsubscribe_wrong_id() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let addr = IntentConfigBuilder::new()
            .role(IntentConfigRole::Database {
                config_file_path: config_path,
            })
            .start()
            .expect("start failed");

        // ACT: Unsubscribe with non-existent ID
        addr.unsubscribe(999);

        // ASSERT: Should not panic (no-op for invalid ID)
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    #[actix::test]
    #[ntest::timeout(100)]
    async fn test_api_get_current_config() {
        setup();
        // ARRANGE
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.ron");
        let addr = IntentConfigBuilder::new()
            .role(IntentConfigRole::Database {
                config_file_path: config_path,
            })
            .start()
            .expect("start failed");

        let expected_config = IntentConfigData {
            targets: vec!["192.168.1.1".parse().unwrap()],
            ping_rate_pps: 100,
        };

        // Update config first
        addr.update_config(expected_config.clone());
        tokio::time::sleep(Duration::from_millis(10)).await;

        // ACT: Get current config
        let current_config = addr.get_current_config().await.unwrap();

        // ASSERT: Should return the updated config
        assert_eq!(current_config, expected_config);
    }
}
