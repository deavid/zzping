# ZZNet Application Builder (`zznet-builder`)

This crate provides a complete application framework for building ZZNet applications with minimal boilerplate. It handles all the standard concerns: CLI parsing, logging, configuration, TLS, runtime setup, and graceful shutdown.

The core of the crate is the `AppBuilder`, which uses the `ZZNetService` and `ZZNetConfig` traits to manage the application lifecycle.

## Vision

The goal of `zznet-builder` is to enforce a DRY (Don't Repeat Yourself) and consistent architecture for all applications in the `zzping` workspace. By using the builder, a new service can be created with just a few lines of code in `main.rs`, delegating all the complex setup and error handling to the framework.

## Usage

To create a new application, you need to:

1.  Define a configuration struct and implement `ZZNetConfig` for it.
2.  Define a service struct and implement `ZZNetService` for it.
3.  In your `main.rs`, instantiate and run the `AppBuilder`.

### Example

Here is a complete example of a minimal `main.rs`:

```rust,ignore
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use zznet_builder::builder::AppBuilder;
use zznet_builder::traits::{ZZNetConfig, ZZNetService};

// 1. Define the configuration for the service.
#[derive(Deserialize)]
pub struct MyServiceConfig {
    pub listen_address: String,
}

// Mark it as a valid configuration object.
impl ZZNetConfig for MyServiceConfig {}

// 2. Define the service struct.
pub struct MyService;

// 3. Implement the service lifecycle.
#[async_trait]
impl ZZNetService for MyService {
    type Config = MyServiceConfig;
    type Error = anyhow::Error;

    fn new(config: Self::Config) -> Result<Self> {
        // Perform any setup based on the configuration.
        println!("Initializing service with address: {}", config.listen_address);
        Ok(MyService)
    }

    async fn run(self) -> Result<()> {
        // Start long-running tasks, listeners, etc.
        println!("Service is running...");
        // The builder will wait for a shutdown signal externally.
        Ok(())
    }
}

// 4. Use the AppBuilder to run the service.
fn main() -> Result<()> {
    AppBuilder::new("My Awesome Service", "1.0.0")
        .with_default_config("my_service.ron")
        .run_service::<MyService>()
}
```
