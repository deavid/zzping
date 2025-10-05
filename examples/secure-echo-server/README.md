Secure Echo Server Example

This minimal example demonstrates how to wire `zzping-auth`'s ACL into a
ConnectionManager and run a TLS-secured echo server. It is intentionally
small and meant for demonstration and testing.

Usage:

1. Generate certificates (see project root):

   ./generate_certs.sh

2. Start the echo server:

   cargo run -p secure-echo-server

3. Connect with a client certificate that matches the ACL in `acl.toml`.
