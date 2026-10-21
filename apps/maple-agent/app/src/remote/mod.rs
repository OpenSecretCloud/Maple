//! Desktop remote access is owned by the profile's local daemon.
mod network;
mod pairing;
mod store;
pub(crate) mod streams;
pub(crate) mod wire;

pub mod control;
pub mod manager;

pub(crate) use store::validate_name;
