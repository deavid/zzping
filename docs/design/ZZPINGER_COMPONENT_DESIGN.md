# Design doc: zzpinger component vision

- **Author:** David Martínez Martí
- **Date:** November 16, 2025
- **Status:** Draft
- **Goal:** Propose a new vision to completely rework from scratch the zzpinger component

## Motivation and background

So far AI Agents have been free to develop in the codebase and also to decide on the design on most parts as long as
they followed the vision. However this proved to be a disaster. Simple reviews of the code at a single glance reveal
that there are lots of bad decisions that lead to complexities, which in turn fill the code with unnecessary stuff,
that's needed just to get out of the mess they got into, because the design they went with was not thought through.

As a result, we will scrap zzpinger completely and start from scratch, defining manually the design to exert full
control on the end result, as "vibe-coding" proved to be completely unreliable with these decisions.

## Overall Vision and Architecture

Pinger responsibility is to execute ICMP pings to a list of targets in a constant fashion and collect the timings from
those.

It is very important that the rhythm, the interval for emitting the pings is kept as constant as possible. This means
that we need a tight control on the sleeping between the intervals, and even a fine tuned busyloop at the end if
required, to get always the ping sent with a margin of error of 500 microseconds or less (0.5ms). This is important
specially when later on MemDB component in the database will attempt to compress the data - having a tight interval
reduces drastically the amount of data to be saved.

Pinger does not use zznet tooling - it doesn't talk to other processes. This is important because the current code seems
to have tooling for this and it must be scrapped completely.

This component has two inputs from two different components:

- CState component: Sets enabled/disabled behavior. Pings are not emitted in disabled mode, but the schedule for sending
  them has to be still active.
- IntentConfig component: Sets the main config for the pinger - list of targets and pings per second to emit.

IMPORTANT:

- Timeout is not configurable. It's fixed by design at 10 seconds. The current code makes this configurable but this is
  wrong. ZZPing solves the problem of receiving late timeouts differently to other common ICMP debug apps, we do not
  need shorter timeouts.
- Ping Interval is not milliseconds. It's actually a ping speed in pings per second, where the quantity is an integer.
  So the interval can be understood as a rational number 1000ms/n where N is the number of pings per second. The
  application and component must work in pings per second as much as possible.

This component (Pinger) has ONE output to ONE component:

- MemDB: Receives the events for the pings. There are two possible events: ping emitted, and ping received. Both
  timestamped. And the event must contain the Host address that the ping was destined for. If there's no MemDB to send
  to, or it's not receiving data, pings have to stop happening.

IMPORTANT: There's no Health stats, no health component. YAGNI. It has been a pain and it's useless. All the stats that
could be computed from this component are visible from MemDB. We will stop making stuff up and stick to the original
vision of this project.

IMPORTANT: Ping frequency is not an interval, it's a system-clock aligned pinging. If IntentConfig says 2 pings per
second, then Pinger will emit its pings at HH:MM:SS.000 and HH:MM:SS.500 ; if it says 10 pings per second, it will be at
HH:MM:SS.000, HH:MM:SS.100, HH:MM:SS.200 and so on. This is non-negotiable. This method also adds the concept of a
"phase" in degrees: A 2 pings per second with 180º phase will emit pings at HH:MM:SS.250 and HH:MM:SS.750 ; for now we
will prepare the codebase for this and have this set as a constant to 0 degrees at compile time. This is something that
will be needed later but it is unclear yet in which way or who configures this. So we will set a constant to zero and
prepare the code to support it.

## Solving the Actor problem and design

Actors typically respond to events. However for this component, we need a constant background task that will perform the
pings at exact intervals. Therefore a thread is needed to act as a background task.

We can also think the act of pinging - the actual backend that will execute the ping, to be an Actor. This is something
worth exploring because:

- It conforms to the actor pattern of receiving commands and processing at the command.
- We already have a problem with the backend selection for mocking.

This would mean there would be two different Actors here:

- Pinger Actor, which acts mainly as a scheduler and collector of data
- Pinger Backend Actor, which actually performs the ping and comes in two flavors:
  - ICMPPingBackend - which executes real pings
  - MockPingBackend - which emulates pings following instructions

Then this means that the Pinger Actor instead of Addr\<T> it needs an Actix Receiver\<T> of the backend such that the
backend can be replaced and multiple backend actors can share the same role. The main purpose is to be able to use ICMP
or Mock backends.

A single backend is used to handle all pings against all targets. Backend does not schedule, the pings are sent ASAP as
they are received. With one caveat here - we might need to do some sub-millisecond wait/adjustments because the
scheduling and async task jumping probably has a time resolution bigger than 1ms. For now we will keep things simple,
but let's keep an eye on the performance and precision to see if we need something more complex.

With this, what would be remaining is the scheduler.

We can use a sort of timer as provided by Actix library to get events roughly every millisecond:
`ctx.run_interval(Duration::from_millis(1), ...`

This means that because of the resolution and timings, it's highly likely that we will have too much jitter - so the
backend definitely needs to handle the last millisecond wait. And more than one target might need to be pinged in the
same millisecond. This means that a request to the backend has to contain the desired system time for execution, which
must be <10ms away, ideally <2ms; and also contain the list of targets to ping at that time.

In turn, that does mean that we need many backends each one on its own thread, therefore we will need a `SyncArbiter`
with a pool of threads, so we can schedule many in parallel. The amount of threads in parallel we need is guided by the
expected scheduler jitter. To absorb potential scheduler jitter of up to 10ms without losing a ping slot, we need a
backend pool of at least 10 threads, allowing us to dispatch work up to 10ms ahead of its execution time. This means we
need a configuration constant of how much in advance do we want to schedule the pings - how many milliseconds. And this
advance is the minimum number of threads needed.

The scheduling itself in the Ping actor would benefit from using its own thread to avoid additional jitter from other
parts of the program. Therefore the best option here is to leverage the `PingerBuilder` pattern to ensure we spawn the
new Pinger actor using Arbiter::builder() such that we reserve one thread for itself.

Instead of passing a desired SystemTime to the Backend, we could pass a Instant + Duration. This would mark that we want
to wait until `Duration` has passed since `Instant` - which has much higher resolution. And the matching of SystemTime
vs Instant would happen in the Pinger actor itself.

## External interface

### Data from IntentConfig

- targets: Vec\<std::net::IpAddr> -> this is just a list of IP addresses, either IPv4 or IPv6. It is the list of targets
  to ping.
  - We don't expect broadcast addresses here or anything special. Just single targets in a list.
  - The quantity of targets can be 0-N. We typically expect around 10 targets configured or less, but we won't place
    limits.
- pings_per_second: u16 -> This is the amount of pings to emit per second for each target. It applies to all targets.
  - Range: 1-1000. The minimum is 1 ping per second, and the maximum is strictly 1000 pings per second hard limit.
    Technically we want a soft limt of 100pps. This is per-target, so 10 targets at 100pps will be a total of 1000pps
    exiting the network interface.
  - 0 pps (Zero pings per second) will be reseved to mean uninitialized.

This data must arrive via a message received by IntentConfig component. The Pinger actor will have something similar to:

```rust
impl Handler<UpdateIntentConfig> for PingerActor
```

Which will process the incoming messages from the IntentConfig component. There is no other way or mechanism to change
the contents of this config. The config will be initially created as empty - no targets, 0pps.

Objective / Why this exists: To be able for a GUI interface from a different program, where an admin can configure
remotely both to what targets to ping and the speed of ping. The actual behavior and internals of this is handled by the
IntentConfig component. PingerActor must only care on processing the updates correctly and timely. There are no files or
configs in disk for this as far as PingerActor is concerned.

PingerActor must obey the new config in less that 1 second.

### Data from CState

- enable: bool -> When this is false, the pings will still be scheduled but not forwarded to the backend. When true the
  scheduled pings will be forwarded to the backend.

This data must arrive via message received by CState component:

```rust
impl Handler<UpdateCState> for PingerActor
```

It will be initialized as false. And the only way to modify this is to send this message to the Pinger component.

Objective / Why this exists: To enforce and review Mastership rules or primary/secondary for when new binaries overtake
the old ones. This is to support this behavior where a process can be in watch mode without actual pinging. This is not
configurable by the user or admin. CState will take exclusive care of this. There are no configs/files in disk nor flags
to control this.

PingerActor must react to these changes in under 1 millisecond.

### Data towards MemDB

- event: Vec\<PingEvent> Where PingEvent is:
  - target_host: std::net::IPAddr
  - sent_time: SystemTime
  - state: Enum: {InFlight, TimedOut, NetworkError, ReceivedRTT(Duration)}

When sending towards MemDB, we can send in one shot many events that happened. This way we can group everything that
happened in 1-5ms roughly and reduce the amount of messages and wake-up of threads.

A ping sends two events: One with state=InFlight when the ping is sent out, and another with ReceivedRTT=Duration when
the ping is received. If it times out, the backend emits a TimedOut event using its own internal 10-second timeout
timer; MemDB never infers timeouts from missing or late data (NOTE: In practice, MemDB will have to eventually discard
InFlight or convert them into TimedOut - but this behavior is out of scope for this component). If there is a network
error, then a NetworkError event is sent instead.

### Startup/shutdown

Pinger starts scheduling as soon as the actor is created, continuously. There are no checks nor any conditions not to
start scheduling - it will always happen at the start. The caveat is, because its internal context from
IntentConfig+CState will be empty, the scheduling technically will do nothing until these are received. But this is a
subproduct of how the design works - it's not any kind of special condition at all.

On "stopped", everything will be abruptly stopped and dropped. All data that wasn't collected or sent will be lost by
design. There will be no graceful stop at all.

The backend, as it works with a SyncArbiter it cannot be stopped. Therefore the best option is to leverage its creation
to the caller itself, as if it were a different component; Or in the builder for this component. It will stop
automatically on the end of the program and it will stop sending pings as it stops receiving work to do.

## Timekeeping & Precision model

We assume the machines running the code will have some sort of NTP and that the time will be stable, meaning that while
a skew of time is acceptable, there will be no sudden jumps once the program is running.

The pings will be roughly aligned with SystemTime, and we will only assume we can get a millisecond precision from it
(In reality, precision is at least in the microsecond range).

PingActor, having a scheduler running at 1ms intervals, will then perform a SystemTime::now() call and a Instant::now()
call to align the time at that exact moment - such that the backends can use Instant and Duration to track the remaining
time with precision.

The backend therefore will need all three: SystemTime, Instant and Duration. Instant and Duration are to be used to
control and fine tune the exact moment to release the ping: i.e. at 0.05ms after Instant. But SystemTime is important
and assumed to equate the Instant, such that the backend can convert the actual Instant at which the ping was emitted to
an equivalent SystemTime.

The precision goal is jitter <0.5ms 99th percentile. We can tolerate peaks of 100ms if needed as long as they are
infrequent.

**Phase handling:** First assume that all seconds are equal, as if they were a rectangle or a range in a number line.
Then divide this range in equal parts for the configured number of pings per second. This is the base template. The
phase, that could be an angle or a percentage - it does not matter in which units as long as it works correctly,
dictates the displacement of this pattern.

If we express a phase in percentage, which could be simpler:

```rust
let pings_per_second: u16 = 2;
let ping_phase_percent: u16 = 30;
let one_second_ns: u64 = 1_000_000_000;
let ping_interval_ns: u64 = one_second_ns / pings_per_second as u64;
let ping_phase_ns: u64 = ping_interval_ns * ping_phase_percent / 100;
let mut ping_segments_ns: Vec<u64> = vec![];

for n in 0..pings_per_second {
  let ping_time_expected = ping_interval_ns * n + ping_phase_ns;
}
```

## Scheduling model & per‑target semantics

- Global vs per‑target cadence: When we refer to "pings per second" or "pps", this is per-target cadence. It's a single
  number that applies to all targets.

- Reconfiguration: When a new IntentConfig arrives, it will take effect on the next scheduled 1ms timer in PingerActor.
  There is no special handling for in-flight pings whose targets may have been removed; their results will be processed
  and reported to MemDB as usual. The natural flow of the code is correct.

- Backpressure policy: If scheduling falls behind, we skip those cycles.

Example with multiple targets: Suppose we configure 3 targets (A, B, C) at 2 pps. The second is divided into 2 slots of
500ms each, aligned to the system clock: [SS.000, SS.500). At SS.000 and SS.500 the PingerActor conceptually has 3
independent per-target ping slots, one for each of A, B and C. All targets share the same global time grid, but each
target gets its own per-second cadence mapped onto that grid, so the total emitted pings are A(SS.000, SS.500),
B(SS.000, SS.500), C(SS.000, SS.500).

## Failure modes & safety

MemDB unavailability detection: PingerActor will need an optional Receiver\<T> for the MemDB data. If this is None,
MemDB is not available. Otherwise we send data via the Actix send(M) which will give us a future that can be checked -
we will not await for it but store it for later checking in next scheduling attempts, effectively having only one
in-flight at any time. When it is processed then we send the next batch to MemDB with what it has pending to receive.

If the amount of events pending push is above 1024, we will log a warning to the console and stop sending pings until
this situation recovers.

Backend failures: ICMP errors, socket failures, permission failures - these report the packet with status NetworkError
to MemDB. The exact definition of what constitutes a "NetworkError" is left to the backend implementation (e.g., the
underlying `surge-ping` library); this design does not need to over-specify it. These errors should also be logged to
the console.

Misconfiguration: What happens with insane values (e.g. 10k pings/sec, empty target list, duplicate targets, invalid
IPs). Do we reject config, clamp, or accept and degrade? -> We accept and degrade. It's not the component task to
enforce user/usage limits.

## Backend interface

The Pinger Actor communicates with a Backend Actor through an abstraction that allows multiple implementations
(ICMPPingBackend, MockPingBackend). At a high level:

- Pinger sends scheduling commands containing: an aligned `SystemTime`, an associated `Instant`, the desired fire
  `Duration` relative to that `Instant`, and a batch of target IPs to ping.
- Backend is responsible for:
  - Emitting InFlight and final events (ReceivedRTT, TimedOut, NetworkError) based on real ICMP behavior.
  - Enforcing the fixed 10-second timeout and emitting TimedOut itself; Pinger never synthesizes TimedOut.
  - Mapping actual send/receive instants back to `SystemTime` for MemDB events.

The exact trait signatures and message types are left for the implementation work.

## Concurrency & ownership

- PingerActor runs on its own dedicated Actix Arbiter thread to minimize jitter contributions from unrelated work.
- Backend actors run in a `SyncArbiter` pool of worker threads, each handling ping execution work items.
- PingerActor holds an Actix `Receiver<T>` to send work items to the backend role; the Receiver abstraction allows the
  actual backend implementation to be swapped (ICMP vs Mock) without changing PingerActor.
- PingerActor also holds a handle to MemDB (Actix Receiver\<PingEvents>) and remains responsible for grouping and
  pushing batches of `PingEvent` toward MemDB.
- When any of these channels/addresses are dropped or fail, PingerActor follows the failure modes described earlier
  (e.g. stop sending pings when MemDB is not able to receive).

### Note on Receiver\<T> and role sharing

The use of Actix `Receiver<T>` is intended to decouple the PingerActor from a specific backend instance, so different
backend actors can adopt the same logical role ("pinger backend") over time.

And we need to be clear here - there's no intent for a program to ever run with a mix of backends. This only serves the
purpose of allowing unit tests to put a Mock backend instead of the real one, such that it works in a simple manner.

Optionally, if one desires, it could also make that the app could have a flag for dry run that would place the Mock
backend instead of the real one.

The backend is expected to be created from the main application (e.g. in `main.rs`), outside of this component. The
Pinger component, upon creation (e.g. via its builder), must receive a pre-configured `Receiver<T>` for the backend
role. The component's builder may offer facilities to simplify this, but it must not create a specific backend
implementation on its own. This ensures the main application retains full control over which backend (real, mock, etc.)
is used.

## Testing & observability

Testing strategy:

- Use `MockPingBackend` plus an injectable or controlled clock to validate scheduling logic, jitter bounds, and
  backpressure handling without real ICMP. (TODO: We need to understand how to emulate SystemTime or override in some
  way, because unit tests must finish in under 100ms. This problem should not be solved with upfront design, such as
  introducing time-related traits. The problem must be re-evaluated after a first implementation is in place to see if
  and how it manifests.)
- Cover reconfiguration behavior (changing targets and pps) and CState enable/disable transitions. These can be passed
  just by sending the messages directly to the actor ad-hoc.

Observability strategy:

- WE WILL NOT EMIT ANY counters/metrics such as: scheduled pings vs actually dispatched, skipped cycles due to
  backpressure, MemDB queue depth or pending batch size, and backend error counts, etc. Health metrics are discarded
  because they do not add anything of interest. Most of these metrics can be crafted from MemDB itself.
- Log warnings on sustained backpressure, excessive jitter beyond the 0.5ms goal, and repeated NetworkError events.
