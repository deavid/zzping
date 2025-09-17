use pingpong::ping::PingBuilder;

#[tokio::main]
async fn main() {
    let ping_builder_unwired = PingBuilder::new();

    // This line MUST fail to compile.
    let _handle = ping_builder_unwired.start().await;
    // The error message should be similar to:
    // "no method named `start` found for struct `PingBuilder<PingBuilderUnwired>`"
}
