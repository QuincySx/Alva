// Native-only: this crate manages binary tool installation (Node.js,
// Python, etc.) which has no meaning in a browser. Gated to non-wasm
// so `cargo check --workspace --target wasm32` stays green.
#![cfg(not(target_family = "wasm"))]

pub mod environment;

pub use environment::config::EnvironmentConfig;
pub use environment::installer::Installer;
pub use environment::manifest::{ArchiveFormat, ResourceManifest};
pub use environment::resolver::RuntimeResolver;
pub use environment::versions::InstalledVersions;
pub use environment::{EnvironmentError, EnvironmentManager};
