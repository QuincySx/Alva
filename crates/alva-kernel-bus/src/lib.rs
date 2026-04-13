// INPUT:  bus, caps, cell, event, handle, plugin, writer modules
// OUTPUT: Bus, Caps, StateCell, BusEvent, EventBus, BusHandle, BusPlugin, PluginRegistrar, BusWriter
// POS:    Crate root — re-exports the coordination bus (capabilities, events, state cells, plugin system).
//! alva-kernel-bus — cross-layer coordination bus.
//!
//! Provides three mechanisms for inter-crate communication:
//! - **Caps**: capability registration and discovery (typed service locator)
//! - **EventBus**: typed pub/sub via broadcast channels
//! - **StateCell**: observable shared state with change notifications
//!
//! Create a [`Bus`] at startup, distribute [`BusHandle`]s to each layer.

pub mod bus;
pub mod caps;
pub mod cell;
pub mod event;
pub mod handle;
pub mod plugin;
pub mod writer;

pub use bus::Bus;
pub use caps::Caps;
pub use cell::StateCell;
pub use event::{BusEvent, EventBus};
pub use handle::BusHandle;
pub use plugin::{BusPlugin, PluginRegistrar};
pub use writer::BusWriter;
