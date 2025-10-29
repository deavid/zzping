### **Audit Plan: Final Verification and Deviation Catalog**

**Objective:** To produce a definitive list of all code locations that deviate from the "Ground Truth 3.0" vision, focusing on the two known problem areas.

**Instructions for the Agent:** Execute each task in order. For each task, provide the requested file paths, line numbers, and code snippets. Do not proceed to the next task until the current one is complete.

---

#### **Task 1: Catalog all usage of `Box<dyn RoomHandle>`**

**Objective:** Find every instance where the incorrect trait-object wiring is used instead of the actor `Recipient` pattern.

**Action:**
1.  Perform a project-wide search for the exact string `Box<dyn RoomHandle>`.
2.  For each match found, report the following:
    *   File path and line number.
    *   The line of code containing the match.
    *   The context (e.g., "struct field", "function argument", "return type").

**Expected Output:** A list of files, primarily within `zznet-router` and `zznet-room`, that need to be refactored to use `Recipient`s.

---

#### **Task 2: Analyze the `RouterActor`'s Public API and Call Sites**

**Objective:** Verify the extent of the `Router`'s incorrect runtime routing responsibilities and identify all code that uses this anti-pattern.

**Action:**
1.  **Analyze the Actor:** In `src/net/zznet-router/src/actor.rs`, confirm that the `Handler` implementations for `SendToPeer` and `BroadcastToPeers` exist.
2.  **Find the Call Sites:** Perform a project-wide search for `.send(SendToPeer` and `.send(BroadcastToPeers`.
3.  For each call site found, report:
    *   File path and line number.
    *   The component or service that is making the call.
    *   A brief description of what the code is trying to achieve with the call.

**Expected Output:** A list of components whose `NetworkManager`s are incorrectly using the `RouterActor` as a message bus instead of communicating directly with the `PeerChannels` instance.

---

#### **Task 3: Final Synthesis Report**

**Objective:** Consolidate the findings into a final report.

**Action:**
1.  Present the list of files from Task 1 under the heading "Deviations from Principle #6: Trait Object Wiring".
2.  Present the list of files from Task 2 under the heading "Deviations from Principle #2: Router's Runtime Role".
3.  Conclude with a summary statement confirming that these are the primary remaining deviations from the architectural vision.
