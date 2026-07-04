// INPUT:  alva_app_core::checkpoint
// OUTPUT: re-export CheckpointManager, CheckpointMeta
// POS:    Thin shim — the manager moved to alva-app-core so CLI and Tauri
//         share ONE implementation (Tauri previously had none wired at all).

pub use alva_app_core::checkpoint::CheckpointManager;
