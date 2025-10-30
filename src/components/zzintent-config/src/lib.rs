//! The zzintent-config crate provides intent-based configuration management for the zzping system.
//!
//! # Architecture (Three-Actor Pattern - Phase 3 Migration)
//!
//! This component is being migrated to the three-actor pattern as part of the
//! ZZNet SOLID refactoring (see docs/zznet-solid/02_ZZNet_SOLID_Refactoring_plan.md).
//!
//! ## Actors
//! - **IntentConfigActor** (actor.rs) - Main business logic actor (config storage, subscribers)
//! - **IntentConfigNetworkManager** (network_manager.rs) - Peer lifecycle orchestration
//! - **IntentConfigTranslatorActor** (translator_actor.rs) - Per-peer protocol translation
//!
//! ## Internal Communication
//! - **internal_messages** - Messages between the three actors (not public API)

pub mod actor;
pub mod api;
pub mod builder;
pub mod config;
pub mod messages;
pub mod network_messages;

// Phase 3: Three-actor pattern modules
pub mod internal_messages;
pub mod network_manager;
pub mod translator_actor;
