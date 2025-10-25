# Phase 2: Component Integration - Execution Checklist

**Date**: October 25, 2025
**Status**: Not Started
**Estimated Completion**: 3-3.5 days

---

## Pre-Execution Checklist

- [ ] Review PHASE_2_IMPLEMENTATION_PLAN.md
- [ ] Confirm architecture decisions on broadcast pattern
- [ ] Create feature branch: `feature/phase2-component-integration`
- [ ] Baseline: Confirm all 503 tests pass
- [ ] Document current line counts for before/after comparison

---

## Task 1: zzintent-config Migration

**Estimated**: 4-6 hours
**Status**: ⬜ Not Started

### Subtasks
- [ ] 1.1: Verify Room field exists in actor (30 min)
- [ ] 1.2: Replace manual serialization in ConfigUpdate broadcasting (1-2h)
- [ ] 1.3: Replace manual serialization in error messages (30 min)
- [ ] 1.4: Review and fix all other send points (1h)
- [ ] 1.5: Update builder to use auto-registration (1h)
- [ ] 1.6: Test component thoroughly (1h)

### Acceptance Criteria
- [ ] No `bincode::serde::encode_to_vec` in actor.rs (except legitimate broadcasts)
- [ ] Builder creates Room with auto-registration
- [ ] All component tests pass: `cargo nextest run -p zzintent-config`
- [ ] Manual smoke test of config updates

### Notes
_Add findings and deviations here_

---

## Task 2: zzmem-db Migration

**Estimated**: 4-6 hours
**Status**: ⬜ Not Started

### Subtasks
- [ ] 2.1: Analyze current message sending patterns (30 min)
- [ ] 2.2: Add Room field to actor state (30 min)
- [ ] 2.3: Replace serialization in SubmitBatch (1-2h)
- [ ] 2.4: Replace serialization in Acks (30 min)
- [ ] 2.5: Replace serialization in Query responses (30 min)
- [ ] 2.6: Update builder for auto-registration (1h)
- [ ] 2.7: Test component thoroughly (1h)

### Acceptance Criteria
- [ ] Room field added to MemDBActor
- [ ] No manual serialization in batch/ack/query code
- [ ] Builder creates Room with auto-registration
- [ ] All component tests pass: `cargo nextest run -p zzmem-db`
- [ ] Integration tests pass

### Notes
_Add findings and deviations here_

---

## Task 3: Update All Builders

**Estimated**: 2-3 hours
**Status**: ⬜ Not Started

### Subtasks
- [ ] 3.1: Create builder pattern template/documentation (30 min)
- [ ] 3.2: Update CStateBuilder to use auto-registration (30 min)
- [ ] 3.3: Verify IntentConfigBuilder pattern (30 min)
- [ ] 3.4: Verify MemDBBuilder pattern (30 min)
- [ ] 3.5: Test all builders (30 min)

### Acceptance Criteria
- [ ] All builders follow consistent pattern
- [ ] All builders create Room when session_manager provided
- [ ] Clear error handling when registration fails
- [ ] Documentation updated

### Notes
_Add findings and deviations here_

---

## Task 4: Verification & Testing

**Estimated**: 2-4 hours
**Status**: ⬜ Not Started

### Subtasks
- [ ] 4.1: Run all component tests individually (1h)
  - [ ] zzcollector-state
  - [ ] zzintent-config
  - [ ] zzmem-db
  - [ ] zzpinger
- [ ] 4.2: Run full integration test suite (1h)
- [ ] 4.3: Code audit for patterns (1h)
  - [ ] Grep for manual serialization
  - [ ] Verify TypedSender usage
  - [ ] Verify auto-registration usage
- [ ] 4.4: Update documentation (30 min)

### Acceptance Criteria
- [ ] All 503+ tests pass
- [ ] No manual serialization found (except legitimate cases)
- [ ] TypedSender used consistently
- [ ] Auto-registration working everywhere
- [ ] Documentation reflects new patterns

### Notes
_Add findings and deviations here_

---

## Code Metrics (Before/After)

### Manual Serialization Sites

**Before**:
- zzcollector-state: 0 (already migrated)
- zzintent-config: 2+
- zzmem-db: 3+
- **Total**: 5+

**After**:
- zzcollector-state: 0 ✅
- zzintent-config: ___ (target: 0-1)
- zzmem-db: ___ (target: 0)
- **Total**: ___ (target: 0-1)

### TypedSender Usage

**Before**:
- Components using: 1 (zzcollector-state)
- Usage sites: 4

**After**:
- Components using: ___ (target: 3+)
- Usage sites: ___ (target: 10+)

### Auto-Registration

**Before**:
- Components using: 0
- Builders supporting: 0

**After**:
- Components using: ___ (target: 3)
- Builders supporting: ___ (target: 3)

---

## Architecture Decisions Log

### Decision 1: Broadcast Pattern
- **Date**: ___________
- **Decision**: ___________
- **Rationale**: ___________

### Decision 2: Serialization in Broadcasts
- **Date**: ___________
- **Decision**: ___________
- **Rationale**: ___________

### Decision 3: Builder Consistency
- **Date**: ___________
- **Decision**: ___________
- **Rationale**: ___________

---

## Issues & Blockers

_Track any issues encountered during execution_

| Issue | Severity | Status | Resolution |
|-------|----------|--------|------------|
| | | | |

---

## Test Results

### Component Tests

```bash
# Baseline (before changes)
$ cargo nextest run
Summary: 503 tests run: 503 passed

# After zzintent-config migration
$ cargo nextest run -p zzintent-config
Summary: ___ tests run: ___ passed

# After zzmem-db migration
$ cargo nextest run -p zzmem-db
Summary: ___ tests run: ___ passed

# Final integration test
$ cargo nextest run --no-fail-fast
Summary: ___ tests run: ___ passed
```

### Code Audit

```bash
# Check for manual serialization
$ grep -r "bincode::serde::encode_to_vec" src/components/ | grep -v test
# Result: ___________

# Check TypedSender usage
$ grep -r "typed_sender" src/components/
# Result: ___________

# Check auto-registration
$ grep -r "new_with_session_manager" src/components/
# Result: ___________
```

---

## Completion Checklist

- [ ] All tasks completed
- [ ] All tests passing
- [ ] Code metrics show improvement
- [ ] Architecture decisions documented
- [ ] No outstanding issues
- [ ] Documentation updated
- [ ] Ready for Phase 3 (Application Migration)

---

## Next Phase

After completion, proceed to **Phase 3: Application Migration**
- Remove 287 lines of RoomHandlerFactory boilerplate
- Update applications to use clean builder pattern
- End-to-end verification

---

**Status Legend**:
- ⬜ Not Started
- 🔵 In Progress
- ✅ Complete
- ❌ Blocked

**Last Updated**: ___________
