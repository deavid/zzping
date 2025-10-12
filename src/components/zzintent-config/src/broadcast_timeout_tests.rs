#[cfg(test)]
mod tests {
    use super::*;
    use crate::network_messages::IntentConfigMessage;
    use std::time::Duration;
    use tokio::time::{advance, pause};

    use zznet_session::session_manager::SessionManager;
    use zznet_session::types::PeerId;
    use zzping_test_utils::{MessageCaptureChannels, create_and_add_peer};

    // Ensure tests run inside Actix/tokio runtime
    #[actix::test]
    async fn test_broadcast_fast_peers_succeed_quickly() {
        // No real waiting: fast path
        let mut manager = SessionManager::<
            IntentConfigMessage,
            crate::permissions::IntentConfigPermission,
        >::new(vec!["intent-config".into()]);

        // Create two peers with ample buffer so send won't block
        let peer_a = PeerId::from("peer-a");
        let peer_b = PeerId::from("peer-b");

        // channels with buffer size 10
        let ch_a = MessageCaptureChannels::with_buffer_size(10);
        create_and_add_peer(&mut manager, &peer_a, vec!["intent-config".into()], None).unwrap();
        manager
            .connect_peer(peer_a.clone(), ch_a.tx_out, ch_a.rx_in)
            .unwrap();

        let ch_b = MessageCaptureChannels::with_buffer_size(10);
        create_and_add_peer(&mut manager, &peer_b, vec!["intent-config".into()], None).unwrap();
        manager
            .connect_peer(peer_b.clone(), ch_b.tx_out, ch_b.rx_in)
            .unwrap();

        // Broadcast with a small timeout - should succeed immediately
        let results = manager
            .broadcast_to_room(
                &"intent-config".into(),
                IntentConfigMessage::Heartbeat,
                |_role| true,
                Duration::from_millis(50),
            )
            .await;

        // Both peers should have Ok(())
        assert_eq!(results.len(), 2);
        for (_peer, res) in results {
            assert!(res.is_ok());
        }
    }

    #[actix::test]
    async fn test_broadcast_slow_peer_times_out_quickly() {
        // Simulated time so test completes fast
        pause();

        let mut manager = SessionManager::<
            IntentConfigMessage,
            crate::permissions::IntentConfigPermission,
        >::new(vec!["intent-config".into()]);

        let peer_fast = PeerId::from("peer-fast");
        let peer_slow = PeerId::from("peer-slow");

        // Fast peer: buffer size 10
        let ch_fast = MessageCaptureChannels::with_buffer_size(10);
        create_and_add_peer(&mut manager, &peer_fast, vec!["intent-config".into()], None).unwrap();
        manager
            .connect_peer(peer_fast.clone(), ch_fast.tx_out, ch_fast.rx_in)
            .unwrap();

        // Slow peer: buffer size 1, pre-fill to make send block
        let ch_slow = MessageCaptureChannels::with_buffer_size(1);
        create_and_add_peer(&mut manager, &peer_slow, vec!["intent-config".into()], None).unwrap();
        manager
            .connect_peer(peer_slow.clone(), ch_slow.tx_out.clone(), ch_slow.rx_in)
            .unwrap();

        // Fill the slow peer's outbound buffer so subsequent send will await
        ch_slow
            .tx_out
            .try_send(("intent-config".into(), IntentConfigMessage::Heartbeat))
            .ok();

        // Broadcast with small timeout
        let timeout = Duration::from_millis(20);
        let fut = manager.broadcast_to_room(
            &"intent-config".into(),
            IntentConfigMessage::Heartbeat,
            |_role| true,
            timeout,
        );

        // Drive time forward past the timeout so that blocked senders time out
        advance(timeout + Duration::from_millis(5)).await;

        let results = fut.await;

        // We should have two results: fast peer Ok, slow peer Err (SendFailed)
        assert_eq!(results.len(), 2);
        let mut saw_ok = false;
        let mut saw_err = false;
        for (_peer, res) in results {
            if res.is_ok() {
                saw_ok = true;
            } else {
                saw_err = true;
            }
        }

        assert!(saw_ok, "expected at least one successful send");
        assert!(saw_err, "expected at least one timed-out/failed send");
    }
}
