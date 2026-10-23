//! App entry points share package validation with bounded core subprocesses.

pub use maple_agent::self_launch::command;
#[cfg(all(target_os = "linux", feature = "desktop"))]
pub use maple_agent::self_launch::executable;
