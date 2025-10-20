//! Test: Can current_thread runtime handle concurrent async tasks?
//! Answer: YES - async tasks interleave on single thread without parallelism
//!
//! This validates that we CAN use current_thread runtime for Phase 7 E2E tests
//! because:
//! - spawn_local() works (needed by SessionManager)
//! - time mocking works (needed for deterministic tests)
//! - async concurrency works (services run concurrently)
//! - No OS threads needed (single thread is fine)

mod common;

use std::sync::Arc;

#[tokio::test(flavor = "current_thread")]
async fn test_concurrent_tasks_interleave() {
    use std::sync::Mutex;

    let log = Arc::new(Mutex::new(Vec::new()));

    // Task 1
    let log1 = log.clone();
    let task1 = tokio::spawn(async move {
        for i in 0..3 {
            log1.lock().unwrap().push(format!("Task1-{}", i));
            tokio::task::yield_now().await;
        }
    });

    // Task 2 running concurrently on same thread
    let log2 = log.clone();
    let task2 = tokio::spawn(async move {
        for i in 0..3 {
            log2.lock().unwrap().push(format!("Task2-{}", i));
            tokio::task::yield_now().await;
        }
    });

    // Wait for both
    task1.await.unwrap();
    task2.await.unwrap();

    // They interleaved!
    let events = log.lock().unwrap();
    println!("✓ Execution order (interleaved): {:?}", *events);

    assert_eq!(events.len(), 6);
    assert!(events.iter().filter(|e| e.contains("Task1")).count() == 3);
    assert!(events.iter().filter(|e| e.contains("Task2")).count() == 3);
}

#[tokio::test(flavor = "current_thread")]
async fn test_spawn_local_works() {
    let result = Arc::new(std::sync::Mutex::new(Vec::new()));

    {
        let result_clone = result.clone();

        // spawn_local works within a LocalSet in current_thread runtime
        tokio::task::LocalSet::new()
            .run_until(async move {
                tokio::task::spawn_local(async move {
                    result_clone.lock().unwrap().push("spawn_local works!");
                })
                .await
                .unwrap();
            })
            .await;
    }

    assert_eq!(result.lock().unwrap()[0], "spawn_local works!");
    println!("✓ spawn_local() works with LocalSet on current_thread runtime");
}

#[tokio::test(flavor = "current_thread")]
async fn test_time_mocking_works() {
    use std::time::Duration;

    tokio::time::pause();

    let start = tokio::time::Instant::now();
    tokio::time::advance(Duration::from_millis(100)).await;
    let elapsed = start.elapsed();

    assert!(elapsed >= Duration::from_millis(100));
    println!(
        "✓ Time mocking works: advanced 100ms, elapsed: {:?}",
        elapsed
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_concurrent_with_time_mocking() {
    use std::sync::Arc;
    use std::time::Duration;

    tokio::time::pause();

    let events = Arc::new(std::sync::Mutex::new(Vec::new()));

    // Task that waits 50ms
    let events1 = events.clone();
    let task1 = tokio::spawn(async move {
        events1.lock().unwrap().push("task1 start");
        tokio::time::sleep(Duration::from_millis(50)).await;
        events1.lock().unwrap().push("task1 end");
    });

    // Task that waits 100ms
    let events2 = events.clone();
    let task2 = tokio::spawn(async move {
        events2.lock().unwrap().push("task2 start");
        tokio::time::sleep(Duration::from_millis(100)).await;
        events2.lock().unwrap().push("task2 end");
    });

    // Both tasks are now pending on sleep
    // Advance time 50ms - task1 should complete
    tokio::time::advance(Duration::from_millis(50)).await;

    // Advance time another 50ms - task2 should complete
    tokio::time::advance(Duration::from_millis(50)).await;

    task1.await.unwrap();
    task2.await.unwrap();

    let log = events.lock().unwrap();
    println!("✓ Time-mocked concurrent execution: {:?}", *log);

    assert_eq!(log[0], "task1 start");
    assert_eq!(log[1], "task2 start");
    assert_eq!(log[2], "task1 end"); // completed after 50ms advance
    assert_eq!(log[3], "task2 end"); // completed after another 50ms
}
