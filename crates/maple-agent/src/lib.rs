//! Transport-neutral Maple agent runtime.
//!
//! This crate is the backend half of the Maple desktop agent flow. It owns the
//! embedded Goose runtime, the Maple provider over the OpenSecret SDK,
//! developer tools, permission policy, and account-scoped session storage.
//! It has no UI and no windowing dependency; a caller composes
//! [`agent::MapleAgentService`] with its own [`agent::AgentEventSink`] and
//! drives it through [`agent::AgentRuntimeHandle`] method calls.

pub mod acp;
pub mod agent;
pub mod maple_api;
pub mod open_secret_config;
