use anyhow::Result;
use async_trait::async_trait;
use ntest::timeout;
use zzchorale::{create_actor, Actor, ActorContext};

// A dummy actor for testing purposes.
struct TestActor;

// A dummy command enum for the test actor.
#[derive(Debug)]
#[allow(dead_code)] // This is a dummy for testing, so not all variants will be used.
enum TestCommand {
    DoNothing,
}

#[async_trait]
impl Actor for TestActor {
    type Command = TestCommand;

    async fn run(mut self, mut context: ActorContext<Self::Command>) -> Result<()> {
        log::info!("TestActor started and running.");
        loop {
            tokio::select! {
                // Handle commands
                Some(cmd) = context.command_rx.recv() => {
                    log::info!("TestActor received command: {cmd:?}");
                },
                // Handle shutdown signal
                _ = &mut context.shutdown_rx => {
                    log::info!("TestActor received shutdown signal. Terminating.");
                    break;
                },
                // Handle channel closure
                else => {
                    log::info!("TestActor command channel closed. Terminating.");
                    break;
                }
            }
        }
        Ok(())
    }
}

#[tokio::test]
#[timeout(100)]
async fn framework_actor_lifecycle() {
    // Initialize logging for the test.
    let _ = env_logger::builder().is_test(true).try_init();
    log::info!("Test starting: framework_actor_lifecycle");

    // 1. Create a dummy TestActor.
    log::info!("Step 1: Creating TestActor...");
    let actor = TestActor;

    // 2. Simulate a builder's .start() method.
    log::info!("Step 2: Calling create_actor to get handle and readiness future...");
    let (handle, readiness) = create_actor(actor);

    // 3. Await the readiness future.
    log::info!("Step 3: Awaiting actor readiness...");
    readiness.await.expect("Actor should become ready");
    log::info!("Actor is ready.");

    // 4. Await handle.shutdown().
    log::info!("Step 4: Shutting down actor...");
    handle
        .shutdown()
        .await
        .expect("Actor should shut down cleanly");
    log::info!("Actor shutdown complete.");

    log::info!("Test finished: framework_actor_lifecycle");
}
