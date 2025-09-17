use anyhow::Result;
use log::info;
use ntest::timeout;
use pingpong::{
    ping::{PingApi, PingBuilder},
    pong::PongBuilder,
};

#[tokio::test]
#[timeout(200)]
async fn ping_pong_wiring_test() -> Result<()> {
    // Initialize logging for the test.
    let _ = env_logger::builder().is_test(true).try_init();

    // Phase 1 (Instantiation): Create the builders for each component.
    info!("Phase 1: Instantiating Builders...");
    let ping_builder_unwired = PingBuilder::new();
    let pong_builder = PongBuilder::new(); // Already <Wired>

    // Phase 2 (Wiring): Manually connect the components by sharing the command sender.
    // Note the consumption of the unwired builder and creation of a new wired builder.
    info!("Phase 2: Wiring Components...");
    let pong_cmd_tx = pong_builder.get_command_sender();
    let ping_builder_wired = ping_builder_unwired.connect_to_pong(pong_cmd_tx);

    // Phase 3 (Activation): Start both components.
    info!("Phase 3: Activating Components...");
    let ping_handle = ping_builder_wired.start().await?;
    let pong_handle = pong_builder.start().await?;

    // Action & Assertion: Use the public API of the Ping component to trigger the interaction.
    info!("Action: Sending 'ping'...");
    let response = ping_handle.ping().await?;
    info!("Assertion: Received response: '{response}'");
    assert_eq!(response, "pong");

    // Teardown: Shut down both components gracefully.
    info!("Teardown: Shutting down components...");
    ping_handle.shutdown().await?;
    pong_handle.shutdown().await?;

    info!("Test finished successfully.");
    Ok(())
}
