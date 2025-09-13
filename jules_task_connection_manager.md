# Task: Implement the `Connection` Manager and Frame Loop

## Objective

Create a `Connection` struct that wraps a raw I/O stream (our `Box<dyn AsyncReadWrite>`) and manages the continuous reading and writing of length-prefixed data frames. This component is the foundational layer of our channel multiplexing system.

## File Structure

1.  Create a new file: `src/components/zznet/src/connection_manager.rs`.
2.  Update `src/components/zznet/src/lib.rs` to declare the new module: `pub mod connection_manager;`

## Requirements for `connection_manager.rs`

### `Connection` Struct

*   Define a public struct `Connection`.
*   **Docstring:** Explain that this struct represents a single, active client-server connection and is responsible for managing the multiplexing of all logical channels over this one connection.
*   It should have a `pub fn new(stream: Box<dyn AsyncReadWrite + Send + Unpin>) -> Self` method.

### `run` Method

*   Implement a `pub async fn run(self)` method that takes ownership.
*   **Docstring:** Explain that this method starts the connection's processing loops and will run until the underlying connection is closed or an error occurs.
*   **Logic:**
    1.  Split the raw stream into a read half and a write half using `tokio::io::split`.
    2.  Create a new MPSC channel for outbound frames that need to be written to the stream. The `Connection` struct will need a way to send frames into this channel.
    3.  Spawn a task for a private `read_loop` method, passing it the read half of the stream.
    4.  Spawn a task for a private `write_loop` method, passing it the write half of the stream and the `rx` end of the outbound MPSC channel.
    5.  The `run` method should then wait for these tasks to complete.

### `read_loop` (Private Method)

*   Signature: `async fn read_loop(reader: impl crate::traits::AsyncReadWrite + Unpin)`.
*   **Logic:** This function should contain a `loop` that continuously calls `crate::proto::frame::read_frame`.
*   For now, upon successfully reading a frame, it should simply `log::debug!` the received frame's contents. This confirms the read mechanism is working.
*   If `read_frame` returns an error (e.g., the connection is closed), the loop should break.

### `write_loop` (Private Method)

*   Signature: `async fn write_loop(writer: impl crate::traits::AsyncReadWrite + Unpin, mut rx: tokio::sync::mpsc::Receiver<Vec<u8>>)`.
*   **Logic:** This function should loop, waiting for a message (`Vec<u8>`) to arrive on the `rx` channel.
*   When a message is received, it should write it to the stream using `crate::proto::frame::write_frame`.
*   If the channel is closed, the loop should break.

## Conceptual Guidance

*   This `Connection` struct is the heart of `zznet`. It transforms a single raw stream into a message-oriented transport layer.
*   Separating the read and write logic into concurrent tasks is a standard and robust pattern for bidirectional communication. It allows us to be reading from the socket at the same time as we are writing to it.
*   We are deliberately keeping this simple for now. The loops just log data. In subsequent tasks, we will build on this foundation to add the actual channel management and routing logic.
