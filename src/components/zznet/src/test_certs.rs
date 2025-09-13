//! This module provides an easy way to access test certificates and keys.
//! It is only compiled when running tests.

#![cfg(test)]

pub const CA_CERT: &str = include_str!("../test_certs/ca.pem");

pub const ADMIN_CERT: &str = include_str!("../test_certs/client-admin.pem");
pub const ADMIN_KEY: &str = include_str!("../test_certs/client-admin.key");

pub const RO_CERT: &str = include_str!("../test_certs/client-ro.pem");
pub const RO_KEY: &str = include_str!("../test_certs/client-ro.key");

pub const COLLECTOR_CERT: &str = include_str!("../test_certs/collector.pem");
pub const COLLECTOR_KEY: &str = include_str!("../test_certs/collector.key");

pub const DATABASE_CERT: &str = include_str!("../test_certs/database.pem");
pub const DATABASE_KEY: &str = include_str!("../test_certs/database.key");
