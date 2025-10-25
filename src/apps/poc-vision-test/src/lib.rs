//! Proof of Concept: Vision Architecture Test
//!
//! This PoC validates that the proposed architecture works:
//! - Components auto-register with SessionManager via Room<T>
//! - No application boilerplate needed
//! - Serialization happens automatically
//! - Message roundtrip works end-to-end
//!
//! If this PoC works, we proceed with Phase 1 of the implementation plan.
//! If it fails, we need to revise the approach.

pub mod simple_component;
