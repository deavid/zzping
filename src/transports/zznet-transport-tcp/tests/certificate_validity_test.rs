// This test file previously attempted to exercise a private parser function
// and used `rcgen` APIs that don't match the workspace version. The real
// unit tests live inside `src/transports/zznet-transport-tcp/src/connection.rs`
// (so they can access private helpers). Keep this placeholder to avoid
// accidental re-introduction of a fragile external test.

#[test]
fn placeholder_noop() {
    // intentionally empty
}
