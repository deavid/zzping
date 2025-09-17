use anyhow::Result;
use async_trait::async_trait;
use ntest::timeout;
use zzchorale::{create_channel, spawn_component, Component, ComponentHandle};

/// A dummy component for testing the framework's lifecycle management.
#[derive(Default)]
struct TestComponent;

/// A dummy command enum for the test component.
#[derive(Debug)]
enum TestCommand {
    DoNothing,
}

#[async_trait]
impl Component for TestComponent {
    type Command = TestCommand;

    async fn on_start(&mut self) -> Result<()> {
        log::info!("TestComponent: on_start hook called.");
        Ok(())
    }

    async fn handle_command(&mut self, command: Self::Command) -> Result<()> {
        log::info!("TestComponent: handle_command called with {command:?}");
        Ok(())
    }

    async fn on_shutdown(&mut self) -> Result<()> {
        log::info!("TestComponent: on_shutdown hook called.");
        Ok(())
    }
}

#[tokio::test]
#[timeout(200)]
async fn component_lifecycle_test() {
    // Initialize logging for the test.
    let _ = env_logger::builder().is_test(true).try_init();
    log::info!("Test starting: component_lifecycle_test");

    // 1. Create a TestComponent.
    log::info!("Step 1: Creating TestComponent...");
    let component = TestComponent;

    // 2. Create a channel and spawn the component.
    log::info!("Step 2: Spawning component...");
    let (command_tx, command_rx) = create_channel::<TestCommand>();
    let (handle, readiness) = spawn_component(component, command_tx, command_rx);

    // 3. Await readiness.
    log::info!("Step 3: Awaiting component readiness...");
    readiness.await.expect("Component should become ready");
    log::info!("Component is ready.");

    // 4. Test that the handle is cloneable and can be used to send commands.
    log::info!("Step 4: Cloning handle and sending a command...");
    let handle_clone: ComponentHandle<TestCommand> = handle.clone();
    handle_clone
        .command_tx
        .send(TestCommand::DoNothing)
        .await
        .expect("Sending command should not fail");
    log::info!("Command sent successfully.");

    // Give a moment for the command to be processed.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // 5. Shut down the component.
    log::info!("Step 5: Shutting down component...");
    handle
        .shutdown()
        .await
        .expect("Component should shut down cleanly");
    log::info!("Component shutdown complete.");

    // 6. Subsequent shutdowns on cloned handles should be no-ops.
    log::info!("Step 6: Verifying subsequent shutdown is a no-op...");
    handle_clone
        .shutdown()
        .await
        .expect("Cloned handle shutdown should be a no-op");
    log::info!("Second shutdown call completed without error.");

    log::info!("Test finished: component_lifecycle_test");
}
