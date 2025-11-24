//! TCP Transport Layer Tests
//!
//! High-fidelity, narrative-driven unit tests for the TCP Transport layer.
//! These tests verify Layer 4 of the ZzNet stack, ensuring framing integrity
//! and proper bidirectional communication.

use crate::{TcpTransportClient, TcpTransportServer};
use bytes::Bytes;
use zznet_api::{TransportClient, TransportFrame, TransportServer};

/// Scenario A: "The Conversation"
///
/// Goal: Verify basic connection establishment, framing I/O loops, and bidirectional data transfer.
///
/// This test demonstrates that:
/// - A TCP server can bind and accept connections
/// - A TCP client can connect to the server
/// - Both sides can start their transport channels
/// - Messages are properly framed and delivered bidirectionally
#[actix_rt::test]
async fn test_tcp_happy_path_conversation() {
    // Server Setup: Bind to localhost on any available port
    let mut server = TcpTransportServer::new("127.0.0.1:0", None)
        .await
        .expect("Failed to create server");

    let server_addr = server.local_addr().expect("Failed to get server address");

    // Spawn server task to handle the incoming connection
    let server_handle = tokio::spawn(async move {
        // Accept the connection
        let transport = server.accept().await.expect("Failed to accept connection");

        // Start the transport to get channels
        let (tx, mut rx) = transport.start();

        // Wait for a frame from client
        let frame = rx
            .recv()
            .await
            .expect("Server: channel closed")
            .expect("Server: read error");

        // Assert frame content is "Ping"
        assert_eq!(
            frame.get_bytes(),
            b"Ping" as &[u8],
            "Server: expected 'Ping'"
        );

        // Send back "Pong"
        tx.send(TransportFrame::from(Bytes::from("Pong")))
            .await
            .expect("Server: failed to send Pong");
    });

    // Client Execution: Connect to the server
    let client =
        TcpTransportClient::new(server_addr.to_string(), None).expect("Failed to create client");

    let transport = client.connect().await.expect("Failed to connect");

    // Start the transport to get channels
    let (tx, mut rx) = transport.start();

    // Send "Ping" to server
    tx.send(TransportFrame::from(Bytes::from("Ping")))
        .await
        .expect("Client: failed to send Ping");

    // Wait for response
    let frame = rx
        .recv()
        .await
        .expect("Client: channel closed")
        .expect("Client: read error");

    // Assert received frame is "Pong"
    assert_eq!(
        frame.get_bytes(),
        b"Pong" as &[u8],
        "Client: expected 'Pong'"
    );

    // Ensure server task completed without panics
    server_handle.await.expect("Server task panicked");
}

/// Scenario B: "The Boundary"
///
/// Goal: Verify length-prefixed framing prevents message fusion (TCP stream handling).
///
/// This test demonstrates that:
/// - Multiple messages sent rapidly are properly framed
/// - Each message is received as a distinct frame (no fusion)
/// - The framing protocol maintains message boundaries over TCP streams
#[actix_rt::test]
async fn test_tcp_happy_path_boundary() {
    // Server Setup
    let mut server = TcpTransportServer::new("127.0.0.1:0", None)
        .await
        .expect("Failed to create server");

    let server_addr = server.local_addr().expect("Failed to get server address");

    // Spawn server task
    let server_handle = tokio::spawn(async move {
        let transport = server.accept().await.expect("Failed to accept connection");
        let (tx, mut rx) = transport.start();

        // Receive Frame 1
        let frame1 = rx
            .recv()
            .await
            .expect("Server: channel closed")
            .expect("Server: read error");
        assert_eq!(
            frame1.get_bytes(),
            b"Message A" as &[u8],
            "Server: expected 'Message A'"
        );

        // Receive Frame 2 (must be separate from Frame 1)
        let frame2 = rx
            .recv()
            .await
            .expect("Server: channel closed")
            .expect("Server: read error");
        assert_eq!(
            frame2.get_bytes(),
            b"Message B" as &[u8],
            "Server: expected 'Message B'"
        );

        // Send acknowledgment
        tx.send(TransportFrame::from(Bytes::from("Ack")))
            .await
            .expect("Server: failed to send Ack");
    });

    // Client Execution
    let client =
        TcpTransportClient::new(server_addr.to_string(), None).expect("Failed to create client");

    let transport = client.connect().await.expect("Failed to connect");
    let (tx, mut rx) = transport.start();

    // Send "Message A" immediately followed by "Message B"
    tx.send(TransportFrame::from(Bytes::from("Message A")))
        .await
        .expect("Client: failed to send Message A");

    tx.send(TransportFrame::from(Bytes::from("Message B")))
        .await
        .expect("Client: failed to send Message B");

    // Wait for acknowledgment
    let frame = rx
        .recv()
        .await
        .expect("Client: channel closed")
        .expect("Client: read error");
    assert_eq!(frame.get_bytes(), b"Ack" as &[u8], "Client: expected 'Ack'");

    // Ensure server task completed without panics
    server_handle.await.expect("Server task panicked");
}
