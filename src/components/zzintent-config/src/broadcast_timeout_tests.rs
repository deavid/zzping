#[cfg(test)]
mod tests {
    use crate::network_messages::IntentConfigMessage;
    use std::time::Duration;
    use tokio::time::{advance, pause};

    use zznet_session::types::PeerId;
    use zznet_session::{session_manager::SessionManager, types::RoomId};
    use zzping_test_utils::MessageCaptureChannels;

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

        // Add peers and negotiate rooms
        let mut peer_session_a = zznet_session::peer_session::PeerSession::<
            IntentConfigMessage,
            crate::permissions::IntentConfigPermission,
        >::new(peer_a.clone());
        peer_session_a
            .add_room(
                "intent-config".into(),
                Box::new(zzping_test_utils::DummyRoomHandle::new(
                    "intent-config".into(),
                )),
            )
            .unwrap();
        peer_session_a.set_role(Some(
            crate::permissions::IntentConfigPermission::ReceiveConfigUpdates,
        ));
        manager.add_peer(peer_a.clone(), peer_session_a).unwrap();
        manager
            .handle_publish_rooms(&peer_a, vec!["intent-config".into()])
            .unwrap();

        let mut peer_session_b = zznet_session::peer_session::PeerSession::<
            IntentConfigMessage,
            crate::permissions::IntentConfigPermission,
        >::new(peer_b.clone());
        peer_session_b
            .add_room(
                "intent-config".into(),
                Box::new(zzping_test_utils::DummyRoomHandle::new(
                    "intent-config".into(),
                )),
            )
            .unwrap();
        peer_session_b.set_role(Some(
            crate::permissions::IntentConfigPermission::ReceiveConfigUpdates,
        ));
        manager.add_peer(peer_b.clone(), peer_session_b).unwrap();
        manager
            .handle_publish_rooms(&peer_b, vec!["intent-config".into()])
            .unwrap();

        // channels with buffer size 10
        let ch_a = MessageCaptureChannels::with_buffer_size(10);
        manager
            .connect_peer(peer_a.clone(), ch_a.tx_out, ch_a.rx_in)
            .unwrap();

        let ch_b = MessageCaptureChannels::with_buffer_size(10);
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

        // Add fast peer and negotiate rooms
        let mut peer_session_fast = zznet_session::peer_session::PeerSession::<
            IntentConfigMessage,
            crate::permissions::IntentConfigPermission,
        >::new(peer_fast.clone());
        peer_session_fast
            .add_room(
                "intent-config".into(),
                Box::new(zzping_test_utils::DummyRoomHandle::new(
                    "intent-config".into(),
                )),
            )
            .unwrap();
        peer_session_fast.set_role(Some(
            crate::permissions::IntentConfigPermission::ReceiveConfigUpdates,
        ));
        manager
            .add_peer(peer_fast.clone(), peer_session_fast)
            .unwrap();
        manager
            .handle_publish_rooms(&peer_fast, vec!["intent-config".into()])
            .unwrap();

        // Add slow peer and negotiate rooms
        let mut peer_session_slow = zznet_session::peer_session::PeerSession::<
            IntentConfigMessage,
            crate::permissions::IntentConfigPermission,
        >::new(peer_slow.clone());
        peer_session_slow
            .add_room(
                "intent-config".into(),
                Box::new(zzping_test_utils::DummyRoomHandle::new(
                    "intent-config".into(),
                )),
            )
            .unwrap();
        peer_session_slow.set_role(Some(
            crate::permissions::IntentConfigPermission::ReceiveConfigUpdates,
        ));
        manager
            .add_peer(peer_slow.clone(), peer_session_slow)
            .unwrap();
        manager
            .handle_publish_rooms(&peer_slow, vec!["intent-config".into()])
            .unwrap();

        // Fast peer: buffer size 10
        let ch_fast = MessageCaptureChannels::with_buffer_size(10);
        manager
            .connect_peer(peer_fast.clone(), ch_fast.tx_out, ch_fast.rx_in)
            .unwrap();

        // Slow peer: buffer size 1, pre-fill to make send block
        let ch_slow = MessageCaptureChannels::with_buffer_size(1);
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
        let roomid = RoomId::from("intent-config");
        let fut = manager.broadcast_to_room(
            &roomid,
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
