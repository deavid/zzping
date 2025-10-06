//! Build script for generating gRPC/Prost types from proto files.
//!
//! Generates Rust sources for `proto/ingestion.proto` used by the
//! `zzping-proto` crate.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/ingestion.proto");
    tonic_prost_build::configure()
        .build_server(true) // We need the server traits
        .build_client(true) // We need the client structs
        .compile_protos(
            &["proto/ingestion.proto"], // The file to compile
            &["proto"],                 // The path to search for imports
        )?;
    Ok(())
}
