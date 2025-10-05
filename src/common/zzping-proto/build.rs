// File: zzping-proto/build.rs

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
