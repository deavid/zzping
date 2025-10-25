# October 25, 2025 - Deep Dive Investigation

This folder contains deep architectural investigations that arose during Phase 2 implementation.

---

## The Arc<Mutex<>> Question

**Document**: [ARC_MUTEX_SESSIONMANAGER_INVESTIGATION.md](ARC_MUTEX_SESSIONMANAGER_INVESTIGATION.md)

### The Core Question

**Why does the codebase use `Arc<Mutex<SessionManager>>` instead of making SessionManager an Actix actor?**

This violates the actor model's message-passing philosophy and causes:
- Mutex type mismatches (std vs tokio)
- Lock contention risks
- Complexity in async code
- Blocks Room auto-registration

### Key Findings

1. **SessionManager is NOT an actor** - it's a plain struct shared via Arc<Mutex<>>
2. **This blocks Room auto-registration** - Mutex type mismatch (std vs tokio)
3. **Actor pattern would be cleaner** - but requires 3-4 weeks of refactoring
4. **Current pattern works** - not causing production issues today

### Recommendation

**For now**: Accept Arc<Mutex<>> pattern, document it, move on to Phase 3

**For future**: Strong case for converting SessionManager to an actor:
- Pure actor model
- No locks/mutexes
- Better error handling
- Testability improvements

But needs:
- Performance benchmarking
- Detailed migration plan
- 3-4 weeks of careful refactoring

---

## Investigation Status

- ✅ Problem identified and documented
- ✅ Root causes analyzed
- ✅ Alternatives explored (actor pattern)
- ✅ Migration path outlined
- ⏸️ Decision: Defer to future work

---

## Why This Is Separate from oct22-2025

The oct22-2025 folder focuses on **vision realization** and **architectural compliance**.

This oct25-2025 folder focuses on **deep architectural questions** that need separate investigation:
- Why certain patterns exist
- Whether they should be changed
- Long-term architectural improvements

This keeps the implementation work (oct22) separate from the research/investigation work (oct25).

---

## Related Work

This investigation arose from Phase 2 implementation:
- **PHASE_2_PROGRESS_REPORT.md** (in oct22 folder): Discovered the Mutex blocker
- **PHASE_2_IMPLEMENTATION_PLAN.md** (in oct22 folder): Original plan

Decision: Accept limitations, proceed to Phase 3 (application boilerplate elimination).

---

## Future Topics for oct25 Folder

Other architectural investigations that may belong here:
- Async/await patterns in components
- Broadcasting vs point-to-point communication design
- SessionManager API design review
- Message serialization strategy review

---

**Status**: Investigation complete, documented for future reference.
**Next**: Return to oct22 folder, proceed with Phase 3.
