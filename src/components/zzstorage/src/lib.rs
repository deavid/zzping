// src/components/zzstorage/src/lib.rs

#![doc = "The ZZPing storage engine component."]
#![doc = ""]
#![doc = "This crate is responsible for receiving batches of `PingResult`s,"]
#![doc = "compressing them using a v2 ANS-based codec, and writing them"]
#![doc = "to persistent storage in an append-only log format."]

/// The main actor for the storage engine.
pub mod actor;
/// The v2 data format, codec, and compression logic.
pub mod codec;
/// The file system storage format.
pub mod fs;
