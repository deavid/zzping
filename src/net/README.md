# ZZNet: The ZZPing Network Layer

**Purpose**: This directory contains the ZZNet framework - a transport-agnostic, typed message passing system for building distributed applications.

## What is ZZNet?

ZZNet enables components to communicate across processes using **typed Rust messages** without any knowledge of how those messages are transported. The core principle is simple but powerful: **network communication should feel exactly like in-memory communication**.

When you write a component that needs to talk to another component in a different process, you don't write network code. You don't think about TCP, serialization, or connection management. You just send and receive typed Rust structs. ZZNet handles everything else.


## The Critical Insight: Same Component, Different Config

This is the most important concept in ZZNet, and it's often misunderstood.

When you have a component that needs network communication, you write **one component** that runs on **both sides** of the connection. You don't write a "client component" and a "server component". You write `MyComponent` that can be configured to act as either client or server.

Why does this matter? Because all the networking logic lives in one place. When you look at `MyComponent`'s code, you see both sides of the conversation. You see what it sends, what it expects to receive, and how the protocol works. You don't have to jump between two different files to understand the communication.

This is not just a style preference - it's a fundamental architectural principle. A component's networking code must be self-contained in that component's crate. If ComponentA talks to ComponentA across the network, all that code lives in ComponentA's implementation.

## The ZZNet Stack

Understanding the layers helps you know which crate to use:

**zznet-builder** sits at the top. This is where applications start. It provides `ClientBuilder` and `ServerBuilder` that handle all the setup complexity. Most applications only touch this crate.

**zznet-room** provides `Room<T>` - typed channels for your messages. When your component needs to send or receive typed messages, it uses Room<T>. This is what makes network communication feel like local communication.

**zznet-session** manages the SessionManager - the piece that tracks all active connections and routes messages. You rarely use this directly unless you need fine-grained control over connection lifecycle.

**zznet-hello** handles the serialization boundary - converting between typed Rust structs and bytes. This is internal plumbing. You never touch it directly.

**zznet-transport-tcp** and **zznet-api** provide the actual transport - TCP for production, mock for testing. Again, you configure this through the builder but rarely interact with it directly.

The pattern is: high-level crates for applications, low-level crates for internal plumbing.


## Which Crate Should You Use?

### Building Complete Applications: zznet-builder

When you're building an actual runnable application - something that will be compiled into a binary and started as a process - you use **zznet-builder**. This crate provides `ClientBuilder` and `ServerBuilder` that handle all the setup.

A client application connects to a server. A server application accepts connections. The builder handles creating the network stack, setting up transports, and getting everything wired together. You use the builder, call connect() or start(), and you get back a working client or server.

This is your entry point. Start here unless you have a specific reason not to.

### Building Components: zznet-room

When you're building a component - a reusable piece that will be used by applications - you use **zznet-room**. This gives you `Room<T>` - a typed channel for sending and receiving messages.

Your component holds onto Room<T> instances for each type of message it needs to send or receive. When it needs to send a message, it calls `room.send(msg)`. When it needs to receive, it calls `room.recv()`. The component doesn't know or care whether the other end is in the same process (testing) or across the network (production).

Components are given their Room instances during initialization. They use them throughout their lifetime. When the connection closes, the Room tells them.

### Advanced Control: zznet-session

If you need direct control over the session lifecycle - starting sessions, stopping them, tracking them - you can use **zznet-session** directly. This gives you the SessionManager.

Most applications don't need this. The builder creates and manages the SessionManager for you. But if you're building something that needs custom behavior - maybe you're building your own higher-level framework, or you need to track sessions in a specific way - you can use SessionManager directly.

### Testing: zznet-api Mock

For tests, you use the **mock transport** from zznet-api. This lets you create two components that communicate through in-memory channels instead of over a real network.

The beauty of mock transport is that your component code doesn't change. The same component that uses TCP in production uses mock channels in tests. You just configure it differently during setup. This is transport-agnostic design in action.

Tests run fast (no network overhead), are deterministic (no timing issues), and are isolated (no port conflicts). You test the exact same code paths that run in production.

## Complete Application Example

### Client Application

```rust
use zznet_builder::ClientBuilder;
use zznet_room::Room;

struct MyClientApp {
    request_room: Room<MyRequest>,
    response_room: Room<MyResponse>,
}

impl MyClientApp {
    pub async fn new(server_addr: &str) -> Result<Self> {
        // Build client with zznet-builder
        let client = ClientBuilder::new(server_addr)
            .with_tls(load_tls_config()?)  // Optional: TLS
            .connect()
            .await?;

        // Get typed rooms
        let request_room = client.room(RoomId(1)).unwrap();
        let response_room = client.room(RoomId(2)).unwrap();

        Ok(Self { request_room, response_room })
    }

    pub async fn do_work(&self) -> Result<()> {
        // Send typed message
        self.request_room.send(MyRequest { ... }).await?;

        // Receive typed response
        if let Some(response) = self.response_room.recv().await? {
            // Process response
        }

        Ok(())
    }
}
```

## Understanding Rooms

A room is a 1:1 typed channel between two component instances. It's NOT a broadcast mechanism. When you send a message to a Room<T>, it goes to exactly one destination - the component on the other end of that specific connection.

If you have three clients connected to a server, the server has three separate Room instances (one per connection). When the server sends to "Room 5", it means "Room 5 for this specific connection". Each connection has its own independent set of rooms.

Rooms are bidirectional. Either end can send and receive. There's no inherent "request" or "response" semantics at the framework level - that's up to your application design.

## Fire-and-Forget Semantics

ZZNet uses fire-and-forget messaging. When you call `room.send(msg)`, the framework does not provide acknowledgment that the message was received or processed.

The message goes into the network stack. It gets serialized. It travels over the transport. It arrives at the other end. It gets deserialized. But the sender doesn't get confirmation.

If you need acknowledgments, request-response patterns, or guaranteed processing, design them at your application level. Send a request message. Wait for a response message. Track timeouts. That's application logic, not framework behavior.

The framework's job is moving typed messages reliably and efficiently. Your job is deciding what those messages mean and how they relate to each other.

## The Same Component Principle

One of ZZNet's core design principles is that you write ONE component, not separate client and server components.

Your component holds Room<T> instances. In production, when you instantiate it as a client, you give it rooms configured with client behavior. When you instantiate it as a server, you give it rooms configured with server behavior. But the component code is identical.

This works because rooms are symmetric. Both ends can send and receive. The difference between "client" and "server" is just who initiates the connection. Once connected, both sides have the same capabilities.

The benefit: you test your component once with mock transport, and it works both as client and server in production. You don't duplicate logic. You don't have subtle differences between client and server code paths. You have one implementation, two configurations.



## Configuration and Environment

### Development vs Production

In development, you typically run without TLS. Your collector and database are on the same machine or trusted network. Configuration is simple.

In production, you may want TLS with client certificates for authentication. Your components may be distributed across machines. Configuration becomes more involved.

But the component code doesn't change. Only the configuration changes. The builder API lets you enable TLS, provide certificates, and set network options without touching your application logic.

### Configuration Files

ZZNet doesn't mandate a configuration format. Your application can use TOML, JSON, RON, environment variables, or whatever fits your ecosystem.

The framework just expects you to provide configuration values when you build the client or server. How you load those values is your choice.

In ZZPing, we use RON files for configuration. The collector has a config file that specifies where to connect. The database has a config file that specifies where to bind. Simple, explicit, separate concerns.

## When to Use Each Layer

### Use zznet-builder When:
- You're building a complete application (binary)
- You want the simplest possible API
- You don't need custom transport behavior
- Standard client/server patterns work for you

### Use zznet-room When:
- You're building a reusable component
- You want transport-agnostic design
- You need explicit control over message types
- You're designing for testability

### Use zznet-session When:
- You need direct session lifecycle control
- You're building a custom framework layer
- Standard builder patterns don't fit your needs
- You need advanced session tracking

### Use Mock Transport When:
- You're writing any test
- You want fast, deterministic behavior
- You're doing component development
- Production deployment isn't relevant yet



## Common Communication Patterns

### Request-Response

Your application sends a request message and expects a response message. You design two message types. One component sends the request to one room. The other component receives from that room, processes it, and sends a response to a different room. The first component waits on that second room.

The framework doesn't know this is "request-response". It just sees two independent send operations on two rooms. Your application creates the relationship between them - matching responses to requests by sequence numbers, correlation IDs, or whatever scheme you choose.

### Streaming

One component continuously sends messages to a room. The other component continuously receives from that room. There's no back-and-forth. Just a flow of data in one direction.

You might use this for telemetry, logs, or continuous data feeds. One side produces, the other side consumes. The producer doesn't wait for acknowledgment. The consumer doesn't send responses. Pure one-way flow.

### Bidirectional

Both components send and receive on multiple rooms simultaneously. There's no clear "requester" or "responder". Both sides have active behavior. Messages flow in both directions based on each component's internal logic.

This fits peer-to-peer scenarios where both ends have independent agency. Neither is just reacting to the other - both are active participants with their own state and decision-making.

## Implementation Notes

### Message Types

Your messages are Rust structs with `Serialize` and `Deserialize` traits from serde. That's it. The framework handles the rest.

Each room carries exactly one type. You can't send different message types through the same room. If you need multiple message types, use multiple rooms or design an enum that wraps your variants.

### Error Handling

When `room.send()` returns an error, the connection is probably broken. When `room.recv()` returns None, the other end closed.

The framework doesn't automatically reconnect. If you need reconnection logic, implement it at your application level. Detect the disconnect, clean up, rebuild your client, reconnect.

### Performance

ZZNet is designed for correctness and clarity, not maximum throughput. It's fire-and-forget. It's typed. It's tested with mocks. These design choices have costs.

If you need ultra-high-performance message passing, you might need a different framework. ZZNet trades some performance for safety, testability, and developer ergonomics.

That said, it's plenty fast for most distributed applications. The ZZPing use case (ping monitoring with telemetry) works fine. If you're not moving gigabytes per second, you're probably fine.


## Troubleshooting

### "My messages aren't arriving"

First, verify both sides are using the same RoomId for the message type. If Room 5 on the client sends MyMessage, Room 5 on the server must receive MyMessage. Not Room 6. Not SomeOtherMessage. Exact match.

Second, check that your message struct definitions are identical on both sides. Same fields, same types, same order. Serialization is strict. If they don't match exactly, deserialization fails silently.

Third, verify the connection is still active. If the other end disconnected, your sends go nowhere. Check connection status before assuming message delivery problems.

### "I need acknowledgments"

No, you don't. ZZNet is fire-and-forget by design. If you think you need ACKs, you're probably trying to use the framework wrong.

TCP already provides reliable delivery. Your message will arrive, or the connection will break. You don't need application-level ACKs on top of that.

If you need to know something was processed, send a response message. That's not an ACK - that's application logic. The framework supports bidirectional communication. Use it.

If you need exactly-once delivery guarantees, you need to redesign your application for idempotency. Make operations safe to repeat. Then you don't care if messages duplicate.

### "How do I broadcast to multiple connections?"

You don't. Rooms are 1:1 channels. There's no broadcast primitive.

If you need to send the same message to multiple peers, iterate over your connections and send to each one individually. The framework doesn't hide this from you - it makes the cost explicit.

Or redesign to avoid broadcast. Use a pub-sub pattern at your application level. Use a separate message broker. But don't expect the framework to do broadcast - it's not designed for that.

## Further Reading

Each zznet crate has its own README explaining its vision and requirements in detail. Start with zznet-builder if you're building an application. Read zznet-room if you're building components. The others are there when you need them.

The design documents in `docs/design/` explain the architectural philosophy and decision-making process. Read those if you want to understand why ZZNet works this way.

## Success Criteria

ZZNet succeeds when:
- Applications use only the high-level APIs (zznet-builder, zznet-room)
- The same component code works with mock in tests and TCP in production
- Networking code is simple, obvious, and maintainable
- Tests run without real network infrastructure
- Adding new transports doesn't require changing application code

If you find yourself fighting the framework, you might be using the wrong pattern. Step back and think about the design principles. Transport-agnostic. Fire-and-forget. Same component both sides. Mock-first testing. Work with those, not against them.
