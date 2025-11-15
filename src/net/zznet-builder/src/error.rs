//! This module previously defined `BuilderError` for the builder crate.
//!
//! The builder crate has moved to `anyhow::Error` for public APIs and test
//! helpers. The old `BuilderError` type has been intentionally removed; if
//! code relies on specialized error variants, please reintroduce a small
//! error type local to the caller.
