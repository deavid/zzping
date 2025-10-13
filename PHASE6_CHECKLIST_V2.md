# Phase 6 Implementation Checklist V2: Integration, Stability & MVP

**Target:** Week 6 (Integration, performance, certificate infrastructure, and MVP acceptance)
**Status:** Use this as the authoritative Phase 6 plan (test-first, incremental, checkpoint-driven)
**Goal:** Deliver a stable end-to-end ZZPing system (collectors + database) that is production-ready for an MVP: certificate infra in place, reliable connectivity, health metrics, performance baseline, and documented failure modes.

---

## 🚨 BEFORE YOU START: Pre-flight Checklist (MANDATORY)

Follow these before touching code.

- [ ] **VERIFY WORKSPACE**: `pwd` → ensure you're in repo root
- [ ] **VERIFY BRANCH**: `git checkout -b feature/phase6-integration` (create a branch)
- [ ] **VERIFY BASELINE BUILD**: `cargo build --workspace` → should succeed
- [ ] **VERIFY PHASE 4/5 COMPLETION**:
  - `cargo test -p zzping-collector` → collector tests pass
  - `cargo test -p zzping-database` → database tests pass
- [ ] **READ & UNDERSTAND**:
  - `PHASE4_CHECKLIST_V2.md`, `PHASE5_CHECKLIST_V2.md`, `TLS_DEBUGGING_GUIDE.md`, `INTEGRATION_TESTING_GUIDE.md`
- [ ] **CREATE TEST CERTS**: `./generate_certs.sh` → verify `test_certs/` exists and contains expected files
- [ ] **SET UP CI/LOCAL ENV**: Ensure you can run tests locally and in CI (`cargo test --workspace`)

**COMMIT:** Create an initial commit after setting branch and minimal README change: `git add -A && git commit -m "chore(phase6): start Phase 6 checklist"`

---

## Success Criteria (MVP)

The Phase 6 work is successful when all of the following are true:
- [ ] End-to-end system (N collectors + 1 database) runs for 24 hours without crashes in a test harness
- [ ] TLS certificate infrastructure can produce per-instance certs and rotate them without downtime
- [ ] Heartbeat flow works: collectors send heartbeats, database acks, and stale detection is working
- [ ] Data persisted and recovered after database restarts
- [ ] Performance baseline established (e.g., 100 collectors, 1k heartbeats/sec sustained) and documented
- [ ] Integration tests cover critical flows and pass in CI
- [ ] Detailed troubleshooting and postmortem doc created for common failure modes

---

## Phase Overview — Day-by-day (test-first)

Day 1: End-to-end harness, smoke tests, and automated cert generation (CI-friendly)
Day 2: Certificate rotation & multi-CA support; automation for provisioning
Day 3: Long-running stability tests (24h smoke harness) and recovery tests
Day 4: Performance testing harness and baseline (load testing)
Day 5: Chaos testing (network partitions, process kills) and resilience improvements
Day 6: Documentation, runbook, troubleshooting, and handoff checklist
Day 7: Final polish, PR readiness, and merge to `main`

---

### Day 1: End-to-end Harness, Smoke Tests, and Cert Generation

**GOAL:** Create a reproducible e2e harness that can start multiple collectors and a single database locally (or in CI), run a smoke flow, and verify persistence. Also make certificate generation fully automated for CI.

Estimated time: 4–6 hours

#### Step 1: Test-first — E2E Smoke Test

- [ ] Create a test: `tests/e2e_smoke.rs` at repository root (integration test) that:
  - Starts a `zzping-database` instance on an ephemeral port using test certs
  - Starts N (3) collectors configured for that database using test certs
  - Waits for connection establishment and heartbeat exchange
  - Verifies the database persisted at least one record per collector
  - Tears down all processes cleanly

Example structure (high-level):
```rust
// tests/e2e_smoke.rs
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn e2e_smoke() {
    // 1. Start database service (spawn process or use in-process API)
    // 2. Start 3 collector instances
    // 3. Wait for all collectors to be connected
    // 4. Trigger a heartbeat from each collector
    // 5. Assert database received and persisted records
}
```

**VERIFY:** `cargo test --test e2e_smoke -- --nocapture` (this may require additional test harness plumbing)

#### Step 2: Certificate Automation for CI

- [ ] Ensure `./generate_certs.sh` supports non-interactive mode and can generate certs for N collectors and a database fixture.
- [ ] Add a small helper script `scripts/generate_test_certs.sh` that accepts a count and outputs a directory with named certs: `collector-01.pem`, `collector-01.key`, etc.
- [ ] Add tests that use these generated certs and verify `openssl verify -CAfile` outputs OK.

#### CHECKPOINT 1: E2E harness + cert generation
- [ ] E2E smoke test created and passes locally
- [ ] CI-friendly cert generator script present and verified

Common mistakes:
- ❌ Running e2e with production certs — always use test certs for CI harness
- ❌ Tests that require privileged ports — use ephemeral or high ports to avoid CI conflicts

---

### Day 2: Certificate Rotation, Multi-CA, and Provisioning

**GOAL:** Implement certificate rotation process and support for multiple CAs (future migration). Test-first for rotation and revocation handling.

Estimated time: 6 hours

#### Step 1: Test-first — Rotation flow tests

- [ ] Add tests: `tests/cert_rotation.rs` that simulate the following:
  - Start service with CA v1 and per-instance certs
  - Rotate to CA v2 (generate new CA and sign new server/client certs)
  - Validate that existing connections remain stable during rotation if both CA certs are trusted, and that new connections can use v2 certs
  - Validate that after retiring CA v1, old certs are rejected

**VERIFY:** Run tests locally with `cargo test --test cert_rotation`

#### Step 2: Implement rotation support

- [ ] Add config support to accept multiple CA certs in `DatabaseConfig.tls` (e.g., `trusted_ca_paths: Vec<String>`)
- [ ] Update TLS loader to accept multiple CA certs and construct a RootCertStore with all of them
- [ ] Implement a `CertificateManager` service that can swap active CA and reload server cert without downtime (graceful reload)
- [ ] Provide an admin CLI endpoint or signal (SIGHUP) to trigger reload of certificates from disk

#### CHECKPOINT 2: Certificate rotation
- [ ] Tests for rotation pass
- [ ] Admin-triggered certificate reload works without dropping active connections when configured

Common mistakes:
- ❌ Replacing root store blindly without ensuring acceptor uses updated config
- ❌ Expecting TLS to magically re-handshake existing connections — new TLS handshakes use new config; document behavior

---

### Day 3: Long-running Stability and Recovery Tests

**GOAL:** Ensure the system can run for long periods, survive restarts, and recover state.

Estimated time: 8+ hours (run time heavy)

#### Step 1: Test-first — 24h smoke harness (shortened for CI)

- [ ] Create test harness `tests/long_running.rs` that can be executed locally to run for 1 minute (fast) or 24 hours (extended). The test should:
  - Start DB and M collectors
  - Ensure continuous heartbeat exchanges
  - Simulate collector restarts and database restarts and verify persistence and reconnection

**VERIFY:** Run short mode in CI (1 minute) and long mode locally for manual validation

#### Step 2: Implement checkpointing and graceful shutdown

- [ ] Ensure database flushes in-memory state to disk periodically and on graceful shutdown
- [ ] Implement proper signal handlers for SIGTERM/SIGHUP to allow graceful drain
- [ ] Ensure collectors reconnect with exponential backoff when DB restarts

#### CHECKPOINT 3: Stability and recovery
- [ ] Short-mode long-running harness completes reliably in CI
- [ ] State recovered after database restart in tests

Common mistakes:
- ❌ Relying purely on in-memory state without periodic persistence
- ❌ Not draining pending writes before shutdown

---

### Day 4: Performance Testing (Load Harness and Baseline)

**GOAL:** Create load testing harness and measure baseline performance; identify bottlenecks and document.

Estimated time: 8+ hours (includes runs and iterations)

#### Step 1: Test-first — Performance harness tests

- [ ] Add a harness under `benches/phase6_load.rs` using Criterion or a bespoke load tool that can:
  - Simulate N collectors (configurable, start with 100)
  - Send heartbeat messages at configured rates (e.g., 10Hz each)
  - Measure throughput (requests/s), latency (ms), and resource usage (memory/CPU)

#### Step 2: Run and optimize

- [ ] Run load tests with metrics collection (top, vmstat, or `cargo flamegraph` if available)
- [ ] Identify hotspots: locking, serialization, IO, TLS handshakes
- [ ] Optimize: reduce lock contention, batch writes, use async file IO, reuse TLS session contexts where possible

#### CHECKPOINT 4: Performance baseline
- [ ] Document throughput and resource usage for baseline scenario (example: 100 collectors @1Hz = X req/s)
- [ ] Add performance regression test in CI (optional) that verifies rough throughput

Common mistakes:
- ❌ Running performance testing on CI small runners — prefer dedicated test machines for heavy loads
- ❌ Premature optimization: measure first, then optimize

---

### Day 5: Chaos Testing and Resilience Improvements

**GOAL:** Validate system resilience to network partitions, sudden process kills, and partial failures.

Estimated time: 6–8 hours

#### Step 1: Test-first — Chaos scenarios

- [ ] Add tests or scripts `tests/chaos.rs` or `scripts/chaos/run_chaos.sh` that perform:
  - Kill random collectors and verify DB remains healthy
  - Partition network between a subset of collectors and DB (use `tc`/netem in Linux)
  - Simulate disk full condition for persistence directory and verify graceful degradation

#### Step 2: Implement resilience improvements

- [ ] Add backpressure mechanisms in DB to refuse or slow incoming messages when persistence is overloaded
- [ ] Improve metrics and alerting points (expose counters for dropped messages, connection failures)
- [ ] Ensure error paths don't use `unwrap()` and are handled cleanly

#### CHECKPOINT 5: Chaos and resilience
- [ ] Chaos scripts run without causing DB crashes
- [ ] Backpressure and monitoring catch overload conditions

Common mistakes:
- ❌ Running destructive chaos in production environment — use isolated test infra
- ❌ Ignoring log saturation when many failures occur — ensure logs are rate-limited or aggregated

---

### Day 6: Documentation, Runbook, and Troubleshooting

**GOAL:** Produce runbooks, troubleshooting steps, and handoff docs so on-call engineers can operate the system.

Estimated time: 4 hours

#### Step 1: Create runbook and postmortem templates

- [ ] Add `docs/runbook.md` with:
  - How to deploy DB and collectors
  - How to rotate certificates
  - How to respond to common alerts (no heartbeats, cert failures, persistence full)
- [ ] Add `docs/postmortem_template.md` for incident reports

#### Step 2: Link troubleshooting guides

- [ ] Link to `TLS_DEBUGGING_GUIDE.md` and `TROUBLESHOOTING_GUIDE.md` from the runbook
- [ ] Create a short `docs/operations_quick_start.md` with quick commands for checking status

#### CHECKPOINT 6: Docs and handoff
- [ ] Runbook and quick-start docs present in repo
- [ ] Team readme for Phase 6 and acceptance criteria added

Common mistakes:
- ❌ Assuming runbook readers are familiar with internal code — provide explicit commands
- ❌ Leaving undocumented behaviors (e.g., what happens during cert rotate) — document deterministic behavior

---

### Day 7: Final Polish, PR Preparation, and Merge

**GOAL:** Final review, run full test suite, fix remaining issues, and prepare PR for `main`.

Estimated time: 3–4 hours

#### Step 1: Full test and lint run

- [ ] Run: `cargo test --workspace` and fix failures
- [ ] Run: `cargo clippy --workspace -- -D warnings` and address high-priority lints

#### Step 2: Create PR and checklist for reviewers

- [ ] Create PR branch: open PR against `main` with clear description and links to checklists
- [ ] Provide reviewers with concise acceptance test plan (how to run the e2e smoke, long-run, and performance harness)

#### CHECKPOINT 7: PR ready
- [ ] All tests pass
- [ ] Clippy clean (no warnings treated as errors)
- [ ] PR with description and run instructions created

---

## Cross-cutting Requirements and Patterns

- Test-first always: For each new behavior add tests first
- One-change → verify pattern: Make a small change, run `cargo check`, run tests, commit
- Use mock/session manager patterns from `INTEGRATION_TESTING_GUIDE.md`
- Prefer deterministic tests: use `tokio::time::pause()` where time matters
- Avoid heavy external dependencies in CI: mark slow or platform-specific tests as `#[ignore]` and document how to run manually

---

## Quick Commands

```bash
# Run e2e smoke test locally
cargo test --test e2e_smoke -- --nocapture

# Run cert rotation test
cargo test --test cert_rotation -- --nocapture

# Run performance harness (example)
cargo bench --bench phase6_load

# Run full workspace tests
cargo test --workspace
```

---

## Deliverables for this phase

- `tests/e2e_smoke.rs` integration test harness
- `scripts/generate_test_certs.sh` automated cert generation
- `CertificateManager` implementation and SIGHUP/CLI reload
- `benches/phase6_load.rs` (performance harness)
- `tests/long_running.rs` (short-mode and full-mode harness)
- `docs/runbook.md`, `docs/postmortem_template.md`, `docs/operations_quick_start.md`

---

## Handoff Notes

When opening the PR, include the following checklist in the description for reviewers:
- How to run the e2e smoke locally
- How to generate test certs for the CI
- Which tests are slow/ignored and how to run them manually
- Performance baseline numbers and how they were collected

---

**END OF PHASE 6 CHECKLIST V2**
