# ZZNet Builder Coverage Report (initial)

Date: 2025-11-01
Command: `./coverage-report.sh zznet-builder`

This report summarizes code coverage focused on the `zznet-builder` crate and adjacent code paths relevant to the builder workflow.

## Summary

- Scope: Filtered to paths containing `zznet-builder`
- Result: Below target (>95%) due primarily to TLS helpers, CLI entry-paths, default trait methods, and shutdown signals path.

### Per-file highlights (zznet-builder)

- builder.rs — 21.85% lines covered
- cli.rs — 100%
- config.rs — 97.45%
- logging.rs — 0%
- runtime.rs — 100%
- signals.rs — 48.72%
- tls.rs — 20.32%
- traits.rs — 0%

Note: Percentages summarized from llvm-cov table output (see run log for details).

## Untested hotspots (from llvm-cov text)

- tls.rs (client/server loading and validation paths):
  - load_client_tls: file I/O, cert parsing, root store aggregation
  - load_server_tls: CA paths loop, server cert/key parsing, verifier construction
  - validate_tls_paths: filesystem existence checks
- builder.rs:
  - build_and_run: end-to-end CLI flow, reading config, logging, run_actix wrapper
  - run_service: config validation path, service run, shutdown signal wait
- signals.rs:
  - ShutdownSignals::wait: SIGTERM/SIGINT select branches
- logging.rs:
  - init_logging and init_logging_from_args helpers
- traits.rs:
  - default impls for log_startup_info and service_name (trivial but currently unexercised)

## Recommendations to raise coverage

1. tls.rs (integration-style unit tests)
   - Use temporary files and the existing `test_certs/` artifacts to exercise happy paths.
   - Create small tests for error branches: missing files, empty CA lists, invalid PEM.
   - Keep tests behind `#[cfg(test)]` without network I/O.

2. builder.rs
   - Add a minimal fake service implementing `ZZNetService` whose `run()` resolves immediately. Exercise:
     - build_and_run: provide a tiny RON config via a temp file and assert success.
     - run_service: verify config validation error is surfaced; verify success path returns after service finishes.

3. signals.rs
   - Refactor `ShutdownSignals` to allow dependency injection (channels) in tests, or gate with a test-only constructor to simulate a signal without OS signals.
   - Add a unit test that triggers one branch (SIGINT or SIGTERM) and ensures `wait()` resolves.

4. logging.rs / traits.rs
   - Add straightforward unit tests that call these functions and ensure they don’t panic (smoke tests).

## How to run

```bash
# Full coverage with filter to zznet-builder
./coverage-report.sh zznet-builder

# Full repo coverage (no filter), same script
./coverage-report.sh
```

## Next steps

- Implement the targeted tests listed above, prioritizing tls.rs and builder.rs paths.
- Re-run coverage and update this document with new numbers.
- Stretch goal: add a dedicated `coverage_validation_test.rs` that asserts a minimum threshold for the builder crate to prevent regressions.
