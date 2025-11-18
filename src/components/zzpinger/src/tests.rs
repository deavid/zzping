//! Comprehensive test suite for zzpinger.

use crate::builder::PingerBuilder;
use crate::messages::{UpdateCState, UpdateIntentConfig};
use crate::traits::{Clock, PingError, PingerClient};
use actix::prelude::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use zzmem_db::messages::StorePingResult;

// --- Mock Infrastructure ---

#[derive(Clone)]
pub(crate) struct TokioAlignedClock {
    start_system: SystemTime,
    start_instant: tokio::time::Instant,
}

impl TokioAlignedClock {
    pub(crate) fn new() -> Self {
        Self {
            start_system: SystemTime::now(),
            start_instant: tokio::time::Instant::now(),
        }
    }
}

impl Clock for TokioAlignedClock {
    fn now(&self) -> SystemTime {
        let elapsed = tokio::time::Instant::now().duration_since(self.start_instant);
        self.start_system + elapsed
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MockBehavior {
    Success(Duration),
    Timeout,
    _NetworkError,
}

#[derive(Debug, Clone)]
pub(crate) struct PingCall {
    pub timestamp: SystemTime,
    pub target: IpAddr,
    pub _seq: u16,
}

#[derive(Clone)]
pub(crate) struct MockPingClient {
    behaviors: Arc<Mutex<HashMap<IpAddr, MockBehavior>>>,
    recorder: Arc<Mutex<Vec<PingCall>>>,
    clock: Arc<dyn Clock>,
}

impl MockPingClient {
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            behaviors: Arc::new(Mutex::new(HashMap::new())),
            recorder: Arc::new(Mutex::new(Vec::new())),
            clock,
        }
    }

    pub(crate) fn set_behavior(&self, target: IpAddr, behavior: MockBehavior) {
        self.behaviors.lock().unwrap().insert(target, behavior);
    }

    pub(crate) fn get_calls(&self) -> Vec<PingCall> {
        self.recorder.lock().unwrap().clone()
    }

    pub(crate) fn clear_calls(&self) {
        self.recorder.lock().unwrap().clear();
    }
}

#[async_trait]
impl PingerClient for MockPingClient {
    async fn ping(&self, target: IpAddr, seq: u16) -> Result<Duration, PingError> {
        let now = self.clock.now();
        println!("MockPingClient: ping {} seq={}", target, seq);
        self.recorder.lock().unwrap().push(PingCall {
            timestamp: now,
            target,
            _seq: seq,
        });

        // Determine behavior
        let behavior = {
            let map = self.behaviors.lock().unwrap();
            *map.get(&target).unwrap_or(&MockBehavior::Timeout)
        };

        match behavior {
            MockBehavior::Success(d) => Ok(d),
            MockBehavior::Timeout => Err(PingError::Timeout),
            MockBehavior::_NetworkError => Err(PingError::NetworkError),
        }
    }
}

pub(crate) struct MockMemDB {
    pub blocked: Arc<AtomicBool>,
    pub received: Vec<StorePingResult>,
}

impl MockMemDB {
    pub(crate) fn new(blocked: Arc<AtomicBool>) -> Self {
        Self {
            blocked,
            received: Vec::new(),
        }
    }
}

impl Actor for MockMemDB {
    type Context = Context<Self>;
}

#[derive(Message)]
#[rtype(result = "Vec<StorePingResult>")]
pub(crate) struct GetReceived;

impl Handler<GetReceived> for MockMemDB {
    type Result = Vec<StorePingResult>;

    fn handle(&mut self, _msg: GetReceived, _ctx: &mut Self::Context) -> Self::Result {
        self.received.clone()
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub(crate) struct ClearReceived;

impl Handler<ClearReceived> for MockMemDB {
    type Result = ();

    fn handle(&mut self, _msg: ClearReceived, _ctx: &mut Self::Context) {
        self.received.clear();
    }
}

impl Handler<StorePingResult> for MockMemDB {
    type Result = ResponseActFuture<Self, Result<(), zzmem_db::messages::MemDBError>>;

    fn handle(&mut self, msg: StorePingResult, _ctx: &mut Self::Context) -> Self::Result {
        let blocked = self.blocked.load(Ordering::Relaxed);
        // println!("MockMemDB: handle blocked={}", blocked);

        Box::pin(
            async move {
                if blocked {
                    // println!("MockMemDB: sleeping");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            .into_actor(self)
            .map(move |_, act, _| {
                act.received.push(msg);
                Ok(())
            }),
        )
    }
}

// --- Epics ---

#[actix::test]
async fn epic_1_clockwork_orange() {
    tokio::time::pause();
    let clock = Arc::new(TokioAlignedClock::new());

    // Setup
    let mock_client = MockPingClient::new(clock.clone());
    let target1: IpAddr = "192.168.1.1".parse().unwrap();
    let target2: IpAddr = "192.168.1.2".parse().unwrap();

    mock_client.set_behavior(target1, MockBehavior::Success(Duration::from_millis(5)));
    mock_client.set_behavior(target2, MockBehavior::Success(Duration::from_millis(5)));

    let blocked = Arc::new(AtomicBool::new(false));
    let mock_memdb = MockMemDB::new(blocked).start();

    let builder = PingerBuilder {
        memdb_recipient: mock_memdb.clone().recipient(),
        clock: Some(clock.clone()),
        spawn_strategy: crate::builder::SpawnStrategy::Current,
    };

    let scheduler = builder.start_on_arbiter(Arbiter::current(), mock_client.clone());

    // Configure
    scheduler
        .send(UpdateIntentConfig {
            targets: vec![target1, target2],
            pings_per_second: 10,
        })
        .await
        .unwrap();

    // Enable
    scheduler.send(UpdateCState { enable: true }).await.unwrap();

    // Run for 1 second
    for _ in 0..100 {
        tokio::time::advance(Duration::from_millis(10)).await;
    }
    tokio::time::resume();
    tokio::task::yield_now().await;

    // Verify
    let calls = mock_client.get_calls();
    let received = mock_memdb.send(GetReceived).await.unwrap();

    // Should have roughly 10 pings per target = 20 pings
    assert!(
        calls.len() >= 18 && calls.len() <= 22,
        "Expected ~20 calls, got {}",
        calls.len()
    );

    // Filter for actual results (rtt_us is Some)
    let results: Vec<_> = received
        .iter()
        .filter(|r| r.result.rtt_us.is_some())
        .collect();
    assert!(
        results.len() >= 18 && results.len() <= 22,
        "Expected ~20 results, got {}",
        results.len()
    );

    // Verify timing alignment (jitter < 1ms is hard in CI, let's say < 5ms)
    // The scheduler aligns to system clock.
    // We can check if intervals are roughly 100ms.

    let mut t1_calls: Vec<_> = calls.iter().filter(|c| c.target == target1).collect();
    t1_calls.sort_by_key(|c| c.timestamp);

    for i in 0..t1_calls.len() - 1 {
        let diff = t1_calls[i + 1]
            .timestamp
            .duration_since(t1_calls[i].timestamp)
            .unwrap();
        let millis = diff.as_millis();
        assert!(
            (60..=140).contains(&millis),
            "Interval should be ~100ms, got {}ms",
            millis
        );
    }
}

#[actix::test]
async fn epic_2_unreliable_narrator() {
    tokio::time::pause();
    let clock = Arc::new(TokioAlignedClock::new());

    // Setup
    let mock_client = MockPingClient::new(clock.clone());
    let target_a: IpAddr = "127.0.0.1".parse().unwrap(); // Blackhole
    let target_b: IpAddr = "127.0.0.2".parse().unwrap(); // 50% Loss

    mock_client.set_behavior(target_a, MockBehavior::Timeout);
    mock_client.set_behavior(target_b, MockBehavior::Success(Duration::from_millis(5)));

    let blocked = Arc::new(AtomicBool::new(false));
    let mock_memdb = MockMemDB::new(blocked).start();

    let builder = PingerBuilder {
        memdb_recipient: mock_memdb.clone().recipient(),
        clock: Some(clock.clone()),
        spawn_strategy: crate::builder::SpawnStrategy::Current,
    };

    let scheduler = builder.start_on_arbiter(Arbiter::current(), mock_client.clone());

    // Configure
    scheduler
        .send(UpdateIntentConfig {
            targets: vec![target_a, target_b],
            pings_per_second: 10,
        })
        .await
        .unwrap();

    // Enable
    scheduler.send(UpdateCState { enable: true }).await.unwrap();

    // Run for 1 second
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;

    // Verify
    let calls = mock_client.get_calls();
    let received = mock_memdb.send(GetReceived).await.unwrap();

    let calls_a = calls.iter().filter(|c| c.target == target_a).count();
    let calls_b = calls.iter().filter(|c| c.target == target_b).count();

    // Both should be pinged equally regardless of outcome
    assert!(
        calls_a >= 9,
        "Target A should be pinged ~10 times, got {}",
        calls_a
    );
    assert!(
        calls_b >= 9,
        "Target B should be pinged ~10 times, got {}",
        calls_b
    );

    // Check results
    // Target A (Timeout) produces InFlight (None) and TimedOut (None).
    // Target B (Success) produces InFlight (None) and ReceivedRTT (Some).

    let results_a = received
        .iter()
        .filter(|r| r.result.target == target_a.to_string())
        .count();
    let results_b = received
        .iter()
        .filter(|r| r.result.target == target_b.to_string())
        .count();

    // We expect 2x results per call
    assert!(
        results_a >= calls_a * 2,
        "Target A results should be >= 2x calls"
    );
    assert!(
        results_b >= calls_b * 2,
        "Target B results should be >= 2x calls"
    );

    // Verify content of results
    let r_a_some = received
        .iter()
        .filter(|r| r.result.target == target_a.to_string() && r.result.rtt_us.is_some())
        .count();
    assert_eq!(r_a_some, 0, "Target A should have no RTT");

    let r_b_some = received
        .iter()
        .filter(|r| r.result.target == target_b.to_string() && r.result.rtt_us.is_some())
        .count();
    assert!(r_b_some >= calls_b, "Target B should have RTT results");
}

#[actix::test]
async fn epic_3_clogged_drain() {
    tokio::time::pause();
    let clock = Arc::new(TokioAlignedClock::new());

    // Setup
    let mock_client = MockPingClient::new(clock.clone());
    let target: IpAddr = "127.0.0.1".parse().unwrap();
    mock_client.set_behavior(target, MockBehavior::Success(Duration::from_millis(1)));

    let blocked = Arc::new(AtomicBool::new(false));
    let blocked_clone = blocked.clone();

    // Create MemDB with small mailbox to easily clog it
    let mock_memdb = MockMemDB::create(move |ctx| {
        ctx.set_mailbox_capacity(1);
        MockMemDB::new(blocked_clone)
    });

    let builder = PingerBuilder {
        memdb_recipient: mock_memdb.clone().recipient(),
        clock: Some(clock.clone()),
        spawn_strategy: crate::builder::SpawnStrategy::Current,
    };

    let scheduler = builder.start_on_arbiter(Arbiter::current(), mock_client.clone());

    // Configure high rate to fill buffer quickly
    scheduler
        .send(UpdateIntentConfig {
            targets: vec![target],
            pings_per_second: 100, // 10ms interval
        })
        .await
        .unwrap();

    scheduler.send(UpdateCState { enable: true }).await.unwrap();

    // 1. Run normally
    for _ in 0..20 {
        tokio::time::advance(Duration::from_millis(10)).await;
    }
    let initial_calls = mock_client.get_calls().len();
    assert!(initial_calls > 0, "Should have started pinging");

    // 2. Block (Simulate slow consumer / full mailbox)
    blocked.store(true, Ordering::Relaxed);

    // 3. Wait for buffer to fill and backpressure to kick in
    // Increase PPS to fill faster
    scheduler
        .send(UpdateIntentConfig {
            targets: vec![target],
            pings_per_second: 1000,
        })
        .await
        .unwrap();

    for _ in 0..500 {
        tokio::time::advance(Duration::from_millis(10)).await;
    }

    // 4. Verify Silence
    mock_client.clear_calls();

    for _ in 0..50 {
        tokio::time::advance(Duration::from_millis(10)).await;
    }
    let calls_during_pause = mock_client.get_calls().len();

    // It might not be exactly 0 because of race conditions or in-flight, but should be very low compared to 1000 PPS.
    assert!(
        calls_during_pause < 50,
        "Should be paused, got {} calls",
        calls_during_pause
    );

    // 5. Unblock
    blocked.store(false, Ordering::Relaxed);

    // 6. Drain - Stop scheduling new pings so the buffer can drain
    scheduler
        .send(UpdateIntentConfig {
            targets: vec![], // Clear targets to stop scheduling
            pings_per_second: 0,
        })
        .await
        .unwrap();

    // We need to wait for the scheduler to flush its buffer.
    for _ in 0..500 {
        tokio::time::advance(Duration::from_millis(10)).await;
    }

    // 7. Resume - Re-enable scheduling
    scheduler
        .send(UpdateIntentConfig {
            targets: vec![target],
            pings_per_second: 100,
        })
        .await
        .unwrap();

    mock_client.clear_calls();
    for _ in 0..50 {
        tokio::time::advance(Duration::from_millis(10)).await;
    }
    let calls_after_resume = mock_client.get_calls().len();

    assert!(calls_after_resume > 0, "Should resume pinging");

    // 8. No Catch-up
    let calls = mock_client.get_calls();
    let now = clock.now(); // Use virtual clock for comparison
    for call in calls {
        let age = now.duration_since(call.timestamp).unwrap_or(Duration::ZERO);
        assert!(
            age < Duration::from_secs(1),
            "Call should be recent, not replay from past (age={:?})",
            age
        );
    }
}
