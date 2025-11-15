# ZZNet Hello Protocol (`zznet-hello`)

## Vision

This crate serves as the critical boundary between the raw byte-oriented transport layer and the typed-message-oriented session layer in the ZZNet stack. Its primary purpose is to establish a verified, version-aware communication channel.

## Concept

`zznet-hello` implements a two-phase protocol. First, it performs a mandatory "HELLO" handshake to exchange identity and negotiate protocol versions. Once the handshake is successful, it acts as a transparent serialization layer, converting all subsequent application messages between typed Rust structs and byte frames.

### Core Responsibilities

-   **HELLO Protocol:** Executes a mutual handshake to exchange peer identities and negotiate a compatible protocol version. This ensures both sides of a connection agree on how to communicate before any application data is sent.
-   **Serialization Boundary:** Provides a clear architectural layer that translates typed Rust structs into byte frames for the transport layer, and vice-versa. This isolates the rest of the application from serialization concerns.
-   **Protocol Versioning:** Embeds a version number in each frame, allowing the communication protocol to evolve over time while maintaining backward compatibility.
-   **Identity Exchange:** Confirms the peer's identity. When used with mTLS, it verifies the cryptographically-secured identity provided by the transport. Without mTLS, it provides a basic, non-verified identity from the HELLO message itself.