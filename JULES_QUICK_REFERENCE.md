# Jules Quick Reference Card - Phase 4 Execution

**PRINT THIS AND KEEP VISIBLE WHILE WORKING**

---

## 🚨 CRITICAL RULES - DO NOT VIOLATE

### Rule #1: Verify Before Acting
```
❌ NEVER: Start coding immediately
✅ ALWAYS: Run verification commands first
```

**Example:**
```bash
# Before creating component:
ls -la src/apps/              # Does it exist?
cargo build                   # Does baseline work?
cargo test                    # How many tests passing?
```

### Rule #2: One Step, One Verification
```
❌ NEVER: Create 5 files then try to compile
✅ ALWAYS: Create 1 file → cargo check → commit → next file
```

**Example:**
```bash
# WRONG:
touch file1.rs file2.rs file3.rs file4.rs
cargo check  # 50 errors!

# RIGHT:
touch file1.rs
cargo check  # 0 errors
git commit
touch file2.rs
cargo check  # 0 errors
git commit
```

### Rule #3: Test First
```
❌ NEVER: Implement feature then add test
✅ ALWAYS: Write failing test → implement → test passes
```

**Example:**
```rust
// Step 1: Write test (SHOULD FAIL)
#[test]
fn test_config_validation() {
    let config = invalid_config();
    assert!(config.validate().is_err());
}
// Run: cargo test test_config_validation
// Expected: FAILS (validation not implemented yet)

// Step 2: Implement
impl Config {
    fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
// Run: cargo test test_config_validation
// Expected: PASSES
```

### Rule #4: Respect Checkpoints
```
❌ NEVER: Skip checkpoint items
✅ ALWAYS: Verify ALL items before proceeding
```

**When you see:** 🛑 CHECKPOINT
**You must:**
1. Stop immediately
2. Check EVERY [ ] item
3. Run EVERY verification command
4. If ANY fails → FIX IT
5. Only proceed when ALL pass

### Rule #5: Read Common Mistakes FIRST
```
❌ NEVER: Implement then discover mistake
✅ ALWAYS: Read "Common Mistakes" section before implementing
```

**Pattern:**
1. About to implement Feature X
2. Scroll to "Common Mistakes for X"
3. Read ALL mistakes
4. Keep them in mind while coding
5. When error occurs → check mistakes first

---

## 📋 Pre-flight Checklist (MUST COMPLETE)

Before writing any code for Phase 4:

```bash
# 1. Verify workspace
pwd  # Should be in zzping root

# 2. Verify baseline
cargo build  # Should succeed
cargo test   # Should pass (note count)

# 3. Verify dependencies
cargo check -p zzintent-config  # Should pass
cargo check -p zzpinger          # Should pass
cargo check -p zzmem-db          # Should pass
cargo check -p zzcollector-state # Should pass

# 4. Create branch
git checkout -b feature/collector-app

# 5. Read documents
# - PHASE4_CHECKLIST_V2.md (entire file!)
# - AGENT_CODING_STANDARDS.md
# - PHASE4_IMPROVEMENTS_SUMMARY.md
```

**DO NOT START CODING UNTIL ALL ABOVE COMPLETE**

---

## 🔄 Standard Work Loop

Repeat this for every step:

```
┌─────────────────────────────────────────┐
│ 1. Read step in checklist              │
│ 2. Read "Common Mistakes" section      │
│ 3. Write code (from checklist sample)  │
│ 4. Run verification command            │
│ 5. Check expected outcome matches      │
│    ├─ YES → Commit and continue        │
│    └─ NO → Debug and fix               │
│ 6. If checkpoint → verify ALL items    │
└─────────────────────────────────────────┘
```

**NEVER skip steps 4-5!**

---

## 🐛 When You Get an Error

### Error Response Checklist
```
[ ] 1. Read the FULL error message (don't skim)
[ ] 2. Check "Common Mistakes" section for this feature
[ ] 3. Check PHASE_CHECKLIST_IMPROVEMENTS.md for similar error
[ ] 4. Check PR25_REVIEW.md for similar issue
[ ] 5. Review working component code (zzintent-config)
[ ] 6. If still stuck after 30 min → ask for help
```

**DO NOT:**
- ❌ Try random fixes
- ❌ Batch multiple changes hoping one works
- ❌ Skip verification to "move faster"
- ❌ Ignore warnings thinking "I'll fix later"

---

## 📊 Common Error Patterns

### Error: "cannot find type X"
```rust
// FIX: Add import
use path::to::X;
```

### Error: "trait bounds not satisfied"
```rust
// FIX: Add bounds
fn func<T: Trait + Unpin + Send>() {}
```

### Error: "Rc<T> cannot be sent"
```rust
// WRONG: Rc is not Send
let sm = Rc::new(session_manager);

// RIGHT: Arc is Send
let sm = Arc::new(session_manager);
```

### Error: "method not found"
```rust
// FIX: Check you called prerequisite methods
builder
    .with_session_manager(sm)  // Must call this first!
    .start()                   // Then this works
```

---

## ✅ Verification Commands Quick Reference

```bash
# Check single package compiles
cargo check -p zzping-collector

# Build binary
cargo build --bin zzping-collector

# Run binary
./target/debug/zzping-collector --help

# Run all tests for package
cargo test -p zzping-collector

# Run specific test
cargo test -p zzping-collector test_name

# Run clippy
cargo clippy -p zzping-collector -- -D warnings

# Run formatter
cargo fmt -p zzping-collector

# Check formatting without changing
cargo fmt --check -p zzping-collector
```

---

## 📝 Commit Message Format

```
<type>(<scope>): <short description>

Types:
- feat: New feature
- fix: Bug fix
- test: Adding tests
- docs: Documentation
- chore: Maintenance
- refactor: Code restructuring

Examples:
✅ feat(collector): Implement component builder creation
✅ test(collector): Add config validation tests
✅ chore(collector): Initialize binary crate structure
✅ docs(collector): Add example configuration file

❌ "wip"
❌ "fix stuff"
❌ "update files"
```

---

## 🎯 Success Indicators

You're doing it right if:
- ✅ Never more than 5 compilation errors at once
- ✅ Every `cargo check` after a step succeeds
- ✅ Tests written before implementation
- ✅ All checkpoints pass before proceeding
- ✅ Commit after every completed step
- ✅ Clear, descriptive commit messages

You're doing it wrong if:
- ❌ Create multiple files then try to compile
- ❌ More than 10 compilation errors
- ❌ Skipping verification steps
- ❌ Marking features "done" without tests
- ❌ Proceeding past failed checkpoint
- ❌ Vague commit messages

---

## 🆘 When to Ask for Help

Ask immediately if:
1. Stuck on same error >30 minutes
2. Verification command gives unexpected result you don't understand
3. Checklist instruction doesn't make sense
4. Component API differs from checklist example
5. Checkpoint item fails and you don't know why

**DO NOT** spend hours trying random fixes. Ask early!

---

## 📚 Document Reference Priority

When you need information:

1. **PHASE4_CHECKLIST_V2.md** - Your primary source, follow it exactly
2. **Common Mistakes section** - Check before implementing each feature
3. **PHASE_CHECKLIST_IMPROVEMENTS.md** - Deep explanations and patterns
4. **AGENT_CODING_STANDARDS.md** - Code style and anti-patterns
5. **Component code** - Look at zzintent-config for examples
6. **PR25_REVIEW.md** - Learn from Phase 3 mistakes

---

## 🔢 Checklist Progress Tracking

Keep track of where you are:

```
Day 1: Application Structure
├─ Morning: Crate Structure ▶
│  ├─ Step 1: Verify [DONE]
│  ├─ Step 2: Create Cargo.toml [DONE]
│  ├─ Step 3: Create main.rs [IN PROGRESS]
│  └─ Step 4: Create lib.rs [TODO]
├─ Checkpoint 1 [TODO]
├─ Afternoon: Configuration [TODO]
└─ Evening: CLI [TODO]
```

Update after each step to track progress.

---

## 🎓 Remember the Phase 3 Lessons

**What went wrong:**
1. No verification before starting → "Phantom Component"
2. Created all files at once → 30+ compilation errors
3. Vague task descriptions → 60% completion

**What we're doing differently:**
1. ✅ Explicit verification steps with expected outcomes
2. ✅ One file at a time with compilation checks
3. ✅ Complete code samples and test-first approach

**Your job:** Follow the new process mechanically and avoid Phase 3 patterns.

---

**Print this card and keep it visible. Check it frequently. Good luck! 🚀**
