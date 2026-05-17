//! In-process service backends consumed by tool implementations.
//!
//! Each module here defines a `*Service` trait (the contract tools call
//! through `ctx.bus().get::<dyn …>()`) plus a default `InMemory*Store`
//! implementation that an extension's `configure()` registers on the bus.
//!
//! Naming: the trait name is what tools depend on; the in-memory struct is
//! the swappable default. Users wanting persistence (SQLite, Redis, …)
//! register their own extension with the same `name()` and `provide` a
//! different impl — the `BaseAgent` default-replacement contract handles
//! the dedup automatically.

#[cfg(feature = "task")]
pub mod task;

#[cfg(feature = "team")]
pub mod team;

#[cfg(feature = "task")]
pub use task::{InMemoryTaskStore, TaskError, TaskService, TaskUpdate};

#[cfg(feature = "team")]
pub use team::{InMemoryTeamStore, TeamError, TeamMessage, TeamService, Teammate};
